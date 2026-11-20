use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use qrcode::{render::svg, QrCode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneTransferSession {
    pub active: bool,
    pub url: String,
    pub qr_svg: String,
    pub file_count: usize,
}

#[derive(Clone)]
struct TransferState {
    token: String,
    files: Vec<PathBuf>,
}

pub struct PhoneTransferManager {
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl PhoneTransferManager {
    pub fn new() -> Self {
        Self {
            shutdown_tx: Mutex::new(None),
        }
    }

    pub fn stop(&self) {
        if let Some(tx) = self.shutdown_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }

    pub async fn start(&self, file_paths: Vec<String>) -> Result<PhoneTransferSession, String> {
        self.stop();

        let files: Vec<PathBuf> = file_paths
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.exists() && p.is_file())
            .collect();

        if files.is_empty() {
            return Err("No valid files on the shelf to share.".to_string());
        }

        let token = Uuid::new_v4().simple().to_string();
        let ip = local_ip_address::local_ip()
            .map(|i| i.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());

        let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
            .await
            .map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let url = format!("http://{}:{}/t/{}", ip, port, token);

        let qr_svg = QrCode::new(url.as_bytes())
            .map_err(|e| e.to_string())?
            .render::<svg::Color>()
            .min_dimensions(160, 160)
            .dark_color(svg::Color("#ffffff"))
            .light_color(svg::Color("transparent"))
            .build();

        let file_count = files.len();
        let state = Arc::new(TransferState { token, files });

        let router = Router::new()
            .route("/t/:token", get(index_page))
            .route("/d/:token/:idx", get(download_file))
            .with_state(state);

        let (tx, rx) = oneshot::channel::<()>();
        *self.shutdown_tx.lock().unwrap() = Some(tx);

        tauri::async_runtime::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });

        Ok(PhoneTransferSession {
            active: true,
            url,
            qr_svg,
            file_count,
        })
    }
}

async fn index_page(
    Path(token): Path<String>,
    State(state): State<Arc<TransferState>>,
) -> impl IntoResponse {
    if token != state.token {
        return (StatusCode::FORBIDDEN, Html("<h3>Invalid or expired link.</h3>".to_string()));
    }
    let mut items_html = String::new();
    for (idx, path) in state.files.iter().enumerate() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("file_{}", idx));
        let size_kb = std::fs::metadata(path).map(|m| m.len() / 1024).unwrap_or(0);
        items_html.push_str(&format!(
            "<a class='item' href='/d/{}/{}'><span>{}</span><small>{} KB</small></a>",
            token, idx, name, size_kb
        ));
    }

    let html = format!(
        "<!doctype html><html><head><meta name='viewport' content='width=device-width,initial-scale=1'>\
        <title>NotchGlass Transfer</title>\
        <style>body{{margin:0;background:#0a0a0c;color:#f5f5f7;font-family:-apple-system,Segoe UI,sans-serif;padding:24px}}\
        h2{{font-weight:600;margin-bottom:16px}}.item{{display:flex;justify-content:space-between;align-items:center;\
        background:#16161a;border:1px solid #2c2c32;border-radius:14px;padding:14px 16px;margin-bottom:10px;\
        color:#fff;text-decoration:none}}small{{color:#8e8e93}}</style></head>\
        <body><h2>NotchGlass File Transfer</h2>{}</body></html>",
        items_html
    );
    (StatusCode::OK, Html(html))
}

async fn download_file(
    Path((token, idx)): Path<(String, usize)>,
    State(state): State<Arc<TransferState>>,
) -> impl IntoResponse {
    if token != state.token {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(path) = state.files.get(idx) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download.bin".to_string());
    match tokio::fs::read(path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", name),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
