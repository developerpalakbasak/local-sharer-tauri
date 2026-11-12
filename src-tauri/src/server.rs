use crate::network::{get_network_ips, NetworkEntry};
use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tauri::Emitter;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

const INDEX_HTML: &str = include_str!("../../public/index.html");
const STYLE_CSS: &str = include_str!("../../public/style.css");
const FOLDERS: [&str; 5] = ["audio", "video", "pictures", "documents", "files"];

#[derive(Clone)]
pub struct ServerState {
    pub uploads_dir: PathBuf,
    pub app_handle: tauri::AppHandle,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub size: String,
    pub date: String,
    pub r#type: String,
}

#[derive(Debug, Deserialize)]
pub struct UploadParams {
    pub filename: Option<String>,
    #[serde(rename = "uploadId")]
    pub upload_id: Option<String>,
}

pub struct ServerHandle {
    pub shutdown_tx: oneshot::Sender<()>,
    pub port: u16,
    pub entries: Vec<NetworkEntry>,
}

pub fn get_file_type(filename: &str) -> &'static str {
    let ext = Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "mp4" | "mkv" | "3gp" | "avi" | "mov" => "video",
        "mp3" | "wav" | "m4a" | "flac" => "audio",
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => "pictures",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "txt" => "documents",
        _ => "files",
    }
}

fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 Bytes".to_string();
    }
    const SIZES: &[&str] = &["Bytes", "KB", "MB", "GB", "TB"];
    let i = ((bytes as f64).log(1024.0).floor() as usize).min(SIZES.len() - 1);
    let val = (bytes as f64) / 1024_f64.powi(i as i32);
    format!("{:.2} {}", val, SIZES[i])
}

fn format_date(modified: SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = modified.into();
    datetime.format("%m/%d/%Y, %I:%M:%S %p").to_string()
}

// Endpoints
async fn index_handler() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], INDEX_HTML)
}

async fn style_handler() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], STYLE_CSS)
}

async fn files_handler(State(state): State<Arc<ServerState>>) -> Result<Json<Vec<FileInfo>>, (StatusCode, String)> {
    let mut all_files = Vec::new();

    for folder_name in FOLDERS {
        let folder_path = state.uploads_dir.join(folder_name);
        if folder_path.exists() {
            if let Ok(mut dir) = tokio::fs::read_dir(&folder_path).await {
                while let Ok(Some(entry)) = dir.next_entry().await {
                    if let Ok(meta) = entry.metadata().await {
                        if meta.is_file() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            let size = format_size(meta.len());
                            let date = meta
                                .modified()
                                .map(format_date)
                                .unwrap_or_else(|_| "Unknown".to_string());

                            all_files.push(FileInfo {
                                name,
                                size,
                                date,
                                r#type: folder_name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(Json(all_files))
}

async fn upload_handler(
    Query(params): Query<UploadParams>,
    headers: HeaderMap,
    State(state): State<Arc<ServerState>>,
    body: Body,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let raw_filename = params.filename.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, "Filename query parameter required".to_string())
    })?;

    // Extract base filename to prevent path traversal
    let clean_filename = Path::new(&raw_filename)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unnamed_file")
        .to_string();

    let folder_type = get_file_type(&clean_filename);
    let target_folder = state.uploads_dir.join(folder_type);

    if let Err(e) = tokio::fs::create_dir_all(&target_folder).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create directory: {}", e)));
    }

    // Resolve duplicate filename by appending counter
    let stem = Path::new(&clean_filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = Path::new(&clean_filename)
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();

    let mut dest_path = target_folder.join(&clean_filename);
    let mut counter = 1;
    while dest_path.exists() {
        dest_path = target_folder.join(format!("{}_{}{}", stem, counter, ext));
        counter += 1;
    }

    let final_filename = dest_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&clean_filename)
        .to_string();

    let total_bytes: u64 = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut file = tokio::fs::File::create(&dest_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create destination file: {}", e))
    })?;

    let mut stream = body.into_data_stream();
    let mut uploaded_bytes: u64 = 0;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Body stream error: {}", e))
        })?;
        uploaded_bytes += chunk.len() as u64;

        file.write_all(&chunk).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Write error: {}", e))
        })?;

        if total_bytes > 0 {
            let progress = ((uploaded_bytes as f64 / total_bytes as f64) * 100.0).min(100.0) as u32;
            let _ = state.app_handle.emit(
                "upload-progress",
                serde_json::json!({
                    "filename": final_filename,
                    "progress": progress
                }),
            );
        }
    }

    file.flush().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("Flush error: {}", e))
    })?;

    // Signal completion
    let _ = state.app_handle.emit(
        "upload-progress",
        serde_json::json!({
            "filename": final_filename,
            "progress": 100
        }),
    );

    Ok(Json(serde_json::json!({
        "success": true,
        "folder": folder_type,
        "filename": final_filename
    })))
}

pub async fn start_local_server(
    port: u16,
    uploads_dir: PathBuf,
    app_handle: tauri::AppHandle,
) -> Result<ServerHandle, String> {
    // Ensure root and category folders exist
    tokio::fs::create_dir_all(&uploads_dir)
        .await
        .map_err(|e| format!("Failed to create uploads directory: {}", e))?;

    for f in FOLDERS {
        let p = uploads_dir.join(f);
        tokio::fs::create_dir_all(&p)
            .await
            .map_err(|e| format!("Failed to create folder {}: {}", f, e))?;
    }

    let state = Arc::new(ServerState {
        uploads_dir: uploads_dir.clone(),
        app_handle,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Download directory served with full HTTP Range / streaming support
    let serve_dir = ServeDir::new(&uploads_dir);

    let router = Router::new()
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/style.css", get(style_handler))
        .route("/api/files", get(files_handler))
        .route("/upload", post(upload_handler))
        .nest_service("/download", serve_dir)
        .layer(cors)
        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Could not bind port {}: {}", port, e))?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        let server = axum::serve(listener, router);
        let graceful = server.with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });

        if let Err(e) = graceful.await {
            eprintln!("Local Sharer server error: {}", e);
        }
    });

    let entries = get_network_ips(port);

    Ok(ServerHandle {
        shutdown_tx,
        port,
        entries,
    })
}

