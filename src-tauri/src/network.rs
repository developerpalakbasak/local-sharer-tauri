use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use qrcode::render::svg;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub name: String,
    pub address: String,
    pub url: String,
    pub qr_data_url: String,
}

pub fn generate_qr_svg_base64(url: &str) -> Result<String, String> {
    let code = QrCode::new(url.as_bytes()).map_err(|e| e.to_string())?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    let b64 = BASE64.encode(svg.as_bytes());
    Ok(format!("data:image/svg+xml;base64,{}", b64))
}

pub fn get_network_ips(port: u16) -> Vec<NetworkEntry> {
    let mut entries = Vec::new();

    if let Ok(interfaces) = NetworkInterface::show() {
        for iface in interfaces {
            let lower_name = iface.name.to_lowercase();
            // Filter virtual / VPN / container interfaces
            if lower_name.contains("vmware")
                || lower_name.contains("virtualbox")
                || lower_name.contains("vethernet")
                || lower_name.contains("tailscale")
                || lower_name.contains("zerotier")
                || lower_name.contains("docker")
                || lower_name.contains("veth")
                || lower_name.contains("br-")
            {
                continue;
            }

            for addr in iface.addr {
                if let IpAddr::V4(ipv4) = addr.ip() {
                    if !ipv4.is_loopback() && !ipv4.is_link_local() {
                        let ip_str = ipv4.to_string();
                        let url = format!("http://{}:{}", ip_str, port);
                        let qr_data_url = generate_qr_svg_base64(&url).unwrap_or_default();
                        entries.push(NetworkEntry {
                            name: iface.name.clone(),
                            address: ip_str,
                            url,
                            qr_data_url,
                        });
                    }
                }
            }
        }
    }

    entries
}

