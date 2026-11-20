use crate::db::Database;
use axum::{extract::State, http::StatusCode, routing::{get, post}, Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tower_http::cors::CorsLayer;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNotifyPayload {
    pub agent: String,
    #[serde(default = "default_status")]
    pub status: String, // "working" | "needs_input" | "done" | "error"
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_accent")]
    pub accent: String,
}

fn default_status() -> String {
    "working".to_string()
}

fn default_accent() -> String {
    "#ffffff".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionState {
    pub agent: String,
    pub status: String,
    pub title: String,
    pub body: String,
    pub accent: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTilePayload {
    pub id: String,
    pub label: String,
    pub value: String,
    pub progress: Option<f32>,
}

pub struct ApiStore {
    pub agents: Mutex<HashMap<String, AgentSessionState>>,
    pub tiles: Mutex<HashMap<String, LiveTilePayload>>,
}

impl ApiStore {
    pub fn new() -> Self {
        Self {
            agents: Mutex::new(HashMap::new()),
            tiles: Mutex::new(HashMap::new()),
        }
    }

    pub fn list_agents(&self) -> Vec<AgentSessionState> {
        let map = self.agents.lock().unwrap();
        let mut items: Vec<_> = map.values().cloned().collect();
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        items
    }

    pub fn list_tiles(&self) -> Vec<LiveTilePayload> {
        let map = self.tiles.lock().unwrap();
        map.values().cloned().collect()
    }
}

#[derive(Clone)]
struct ServerState {
    app: AppHandle,
    db: Arc<Database>,
    store: Arc<ApiStore>,
}

async fn health() -> &'static str {
    "ok"
}

async fn handle_notify(
    State(state): State<ServerState>,
    Json(payload): Json<AgentNotifyPayload>,
) -> StatusCode {
    let session = AgentSessionState {
        agent: payload.agent.clone(),
        status: payload.status.clone(),
        title: payload.title.clone(),
        body: payload.body.clone(),
        accent: payload.accent.clone(),
        updated_at: Utc::now().timestamp(),
    };
    {
        let mut map = state.store.agents.lock().unwrap();
        map.insert(payload.agent.clone(), session.clone());
    }
    let _ = state
        .db
        .add_notification(&payload.agent, &payload.title, &payload.body, &payload.accent);
    let _ = state.app.emit("agent-notify", session);
    StatusCode::OK
}

async fn handle_tile(
    State(state): State<ServerState>,
    Json(payload): Json<LiveTilePayload>,
) -> StatusCode {
    {
        let mut map = state.store.tiles.lock().unwrap();
        map.insert(payload.id.clone(), payload.clone());
    }
    let _ = state.app.emit("live-tile", payload);
    StatusCode::OK
}

pub fn spawn_loopback_api(app: AppHandle, db: Arc<Database>, store: Arc<ApiStore>) {
    tauri::async_runtime::spawn(async move {
        let state = ServerState { app, db, store };
        let router = Router::new()
            .route("/health", get(health))
            .route("/notify", post(handle_notify))
            .route("/tile", post(handle_tile))
            .layer(CorsLayer::permissive())
            .with_state(state);

        if let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:47823").await {
            let _ = axum::serve(listener, router).await;
        }
    });
}
