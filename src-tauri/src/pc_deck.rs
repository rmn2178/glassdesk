use crate::media::media_command;
use crate::win32;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use chrono::Utc;
use qrcode::{render::svg, QrCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PcDeckStatus {
    pub active: bool,
    pub url: String,
    pub qr_svg: String,
    pub expires_at: i64,
    pub pending_device: Option<String>,
    pub paired_device: Option<String>,
}

struct DeckShared {
    pair_token: String,
    expires_at: i64,
    pending_device: Option<String>,
    approved: bool,
    paired_device: Option<String>,
    app: AppHandle,
}

pub struct PcDeckManager {
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    shared: Mutex<Option<Arc<Mutex<DeckShared>>>>,
    last_url: Mutex<String>,
    last_qr: Mutex<String>,
}

impl PcDeckManager {
    pub fn new() -> Self {
        Self {
            shutdown_tx: Mutex::new(None),
            shared: Mutex::new(None),
            last_url: Mutex::new(String::new()),
            last_qr: Mutex::new(String::new()),
        }
    }

    pub fn stop(&self) {
        if let Some(tx) = self.shutdown_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
        *self.shared.lock().unwrap() = None;
    }

    pub fn approve(&self, allow: bool) -> PcDeckStatus {
        if let Some(shared) = self.shared.lock().unwrap().as_ref() {
            let mut s = shared.lock().unwrap();
            if allow {
                s.approved = true;
                s.paired_device = s.pending_device.take().or(Some("Phone Browser".to_string()));
            } else {
                s.approved = false;
                s.pending_device = None;
                s.paired_device = None;
            }
        }
        self.status()
    }

    pub fn status(&self) -> PcDeckStatus {
        let url = self.last_url.lock().unwrap().clone();
        let qr_svg = self.last_qr.lock().unwrap().clone();
        if let Some(shared) = self.shared.lock().unwrap().as_ref() {
            let s = shared.lock().unwrap();
            PcDeckStatus {
                active: true,
                url,
                qr_svg,
                expires_at: s.expires_at,
                pending_device: s.pending_device.clone(),
                paired_device: s.paired_device.clone(),
            }
        } else {
            PcDeckStatus {
                active: false,
                url: String::new(),
                qr_svg: String::new(),
                expires_at: 0,
                pending_device: None,
                paired_device: None,
            }
        }
    }

    pub async fn start(&self, app: AppHandle) -> Result<PcDeckStatus, String> {
        self.stop();

        let pair_token = Uuid::new_v4().simple().to_string();
        let expires_at = Utc::now().timestamp() + 120; // 2-minute pairing window

        let ip = local_ip_address::local_ip()
            .map(|i| i.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
            .await
            .map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let url = format!("http://{}:{}/?token={}", ip, port, pair_token);

        let qr_svg = QrCode::new(url.as_bytes())
            .map_err(|e| e.to_string())?
            .render::<svg::Color>()
            .min_dimensions(156, 156)
            .dark_color(svg::Color("#ffffff"))
            .light_color(svg::Color("transparent"))
            .build();

        *self.last_url.lock().unwrap() = url.clone();
        *self.last_qr.lock().unwrap() = qr_svg.clone();

        let shared = Arc::new(Mutex::new(DeckShared {
            pair_token,
            expires_at,
            pending_device: None,
            approved: false,
            paired_device: None,
            app,
        }));
        *self.shared.lock().unwrap() = Some(shared.clone());

        let router = Router::new()
            .route("/", get(deck_ui))
            .route("/ws", get(deck_ws))
            .with_state(shared);

        let (tx, rx) = oneshot::channel::<()>();
        *self.shutdown_tx.lock().unwrap() = Some(tx);

        tauri::async_runtime::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });

        Ok(self.status())
    }
}

async fn deck_ui(
    Query(params): Query<HashMap<String, String>>,
    State(shared): State<Arc<Mutex<DeckShared>>>,
) -> impl IntoResponse {
    let token = params.get("token").cloned().unwrap_or_default();
    let valid = {
        let mut s = shared.lock().unwrap();
        let ok = token == s.pair_token && (s.approved || Utc::now().timestamp() <= s.expires_at);
        if ok && !s.approved && s.pending_device.is_none() {
            s.pending_device = Some("Mobile Browser".to_string());
            let _ = s.app.emit("deck-pair-request", "Mobile Browser");
        }
        ok
    };
    if !valid {
        return (
            StatusCode::FORBIDDEN,
            Html("<h3>Pairing link expired or invalid. Generate a new QR code in NotchGlass.</h3>".to_string()),
        );
    }

    let html = format!(
        r#"<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1,user-scalable=no">
<title>NotchGlass PC Deck</title>
<style>
body{{margin:0;background:#09090b;color:#f5f5f7;font-family:-apple-system,Segoe UI,sans-serif;padding:16px;user-select:none}}
h2{{margin:0 0 12px;font-size:17px;font-weight:600}}
.pad{{height:220px;background:#141418;border:1px solid #2b2b33;border-radius:18px;display:flex;align-items:center;justify-content:center;color:#71717a;touch-action:none;margin-bottom:12px}}
.row{{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin-bottom:10px}}
button{{background:#1c1c22;color:#fff;border:1px solid #2e2e38;border-radius:12px;padding:13px 8px;font-size:13px;font-weight:500}}
button:active{{background:#30303a}}
input{{width:100%;box-sizing:border-box;background:#141418;color:#fff;border:1px solid #2e2e38;border-radius:12px;padding:12px;margin-bottom:10px;font-size:14px}}
</style></head><body>
<h2>NotchGlass PC Deck</h2>
<div id="pad" class="pad">Touchpad (Drag to move, Tap to click)</div>
<div class="row">
  <button onclick="send({{t:'click',right:false}})">Left Click</button>
  <button onclick="send({{t:'click',right:true}})">Right Click</button>
  <button onclick="send({{t:'mic'}})">Mic Mute</button>
</div>
<div class="row">
  <button onclick="send({{t:'media',cmd:'prev'}})">Prev</button>
  <button onclick="send({{t:'media',cmd:'play_pause'}})">Play / Pause</button>
  <button onclick="send({{t:'media',cmd:'next'}})">Next</button>
</div>
<div class="row">
  <button onclick="send({{t:'vk',vk:37}})">Slide Back</button>
  <button onclick="send({{t:'vk',vk:39}})">Slide Next</button>
  <button onclick="send({{t:'vk',vk:70}})">Fullscreen (F)</button>
</div>
<input id="kb" placeholder="Type text and press Enter to send to PC..." />
<script>
const ws = new WebSocket((location.protocol==='https:'?'wss://':'ws://')+location.host+'/ws?token={token}');
function send(obj){{ if(ws.readyState===1) ws.send(JSON.stringify(obj)); }}
const pad = document.getElementById('pad');
let lx=null, ly=null, moved=false;
pad.addEventListener('touchstart', e=>{{ lx=e.touches[0].clientX; ly=e.touches[0].clientY; moved=false; }});
pad.addEventListener('touchmove', e=>{{
  e.preventDefault();
  const x=e.touches[0].clientX, y=e.touches[0].clientY;
  const dx=Math.round((x-lx)*1.8), dy=Math.round((y-ly)*1.8);
  lx=x; ly=y; if(Math.abs(dx)+Math.abs(dy)>1){{ moved=true; send({{t:'move',dx,dy}}); }}
}});
pad.addEventListener('touchend', ()=>{{ if(!moved) send({{t:'click',right:false}}); }});
const kb=document.getElementById('kb');
kb.addEventListener('keydown', e=>{{
  if(e.key==='Enter' && kb.value){{ send({{t:'text',text:kb.value}}); kb.value=''; }}
}});
</script></body></html>"#,
        token = token
    );
    (StatusCode::OK, Html(html))
}

#[derive(Deserialize)]
#[serde(tag = "t")]
enum DeckCommand {
    #[serde(rename = "move")]
    Move { dx: i32, dy: i32 },
    #[serde(rename = "click")]
    Click { right: bool },
    #[serde(rename = "scroll")]
    Scroll { delta: i32 },
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "vk")]
    Vk { vk: u16 },
    #[serde(rename = "media")]
    Media { cmd: String },
    #[serde(rename = "mic")]
    Mic,
}

async fn deck_ws(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    State(shared): State<Arc<Mutex<DeckShared>>>,
) -> impl IntoResponse {
    let token = params.get("token").cloned().unwrap_or_default();
    let ok = {
        let s = shared.lock().unwrap();
        token == s.pair_token
    };
    if !ok {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, shared))
}

async fn handle_socket(mut socket: WebSocket, shared: Arc<Mutex<DeckShared>>) {
    while let Some(Ok(Message::Text(txt))) = socket.recv().await {
        let approved = shared.lock().unwrap().approved;
        if !approved {
            continue;
        }
        if let Ok(cmd) = serde_json::from_str::<DeckCommand>(&txt) {
            match cmd {
                DeckCommand::Move { dx, dy } => win32::send_mouse_move(dx, dy),
                DeckCommand::Click { right } => win32::send_mouse_click(right),
                DeckCommand::Scroll { delta } => win32::send_mouse_scroll(delta),
                DeckCommand::Text { text } => win32::send_unicode_text(&text),
                DeckCommand::Vk { vk } => win32::send_virtual_key(vk),
                DeckCommand::Media { cmd } => {
                    let _ = media_command(&cmd);
                }
                DeckCommand::Mic => {
                    let _ = win32::toggle_mic_mute();
                }
            }
        }
    }
}
