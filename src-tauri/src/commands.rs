use crate::network::NetworkEntry;
use crate::server::{start_local_server, ServerHandle};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

pub struct AppManagedState {
    pub server: Arc<Mutex<Option<ServerHandle>>>,
}

#[derive(Serialize, Deserialize)]
pub struct StartServerResult {
    pub ok: bool,
    pub entries: Vec<NetworkEntry>,
    pub port: u16,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SimpleResult {
    pub ok: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_default_folder() -> String {
    let base = dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Local Sharer").to_string_lossy().to_string()
}

#[tauri::command]
pub async fn select_folder() -> Result<Option<String>, String> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Select Save Folder")
        .pick_folder()
        .await;

    Ok(folder.map(|f| f.path().to_string_lossy().to_string()))
}

#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    open::that(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_server(
    save_path: String,
    state: State<'_, AppManagedState>,
    app_handle: tauri::AppHandle,
) -> Result<StartServerResult, String> {
    let mut guard = state.server.lock().await;

    if let Some(existing) = guard.as_ref() {
        return Ok(StartServerResult {
            ok: true,
            entries: existing.entries.clone(),
            port: existing.port,
            error: None,
        });
    }

    match start_local_server(3000, PathBuf::from(save_path), app_handle).await {
        Ok(handle) => {
            let entries = handle.entries.clone();
            let port = handle.port;
            *guard = Some(handle);
            Ok(StartServerResult {
                ok: true,
                entries,
                port,
                error: None,
            })
        }
        Err(err) => Ok(StartServerResult {
            ok: false,
            entries: Vec::new(),
            port: 4000,
            error: Some(err),
        }),
    }
}

#[tauri::command]
pub async fn stop_server(state: State<'_, AppManagedState>) -> Result<SimpleResult, String> {
    let mut guard = state.server.lock().await;
    if let Some(handle) = guard.take() {
        let _ = handle.shutdown_tx.send(());
    }
    Ok(SimpleResult {
        ok: true,
        error: None,
    })
}

