use axum::{
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::{RwLock, Notify};
use crate::state::SyncState;

const HTML_CONTENT: &str = include_str!("web_ui.html");

pub async fn start_web_server(
    state: Arc<RwLock<SyncState>>,
    trigger: Arc<Notify>,
    host: String,
    port: u16,
) -> Result<(), String> {
    let app = Router::new()
        .route("/", get(serve_ui))
        .route("/api/status", get(get_status))
        .route("/api/config", post(update_config))
        .route("/api/sync", post(trigger_sync))
        .layer(axum::Extension(state))
        .layer(axum::Extension(trigger));

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Axum server error: {}", e))?;

    Ok(())
}

async fn serve_ui() -> axum::response::Html<&'static str> {
    axum::response::Html(HTML_CONTENT)
}

async fn get_status(
    axum::Extension(state): axum::Extension<Arc<RwLock<SyncState>>>,
) -> Json<serde_json::Value> {
    let s = state.read().await;
    Json(serde_json::to_value(&*s).unwrap())
}

#[derive(serde::Deserialize)]
struct ConfigPayload {
    github_username: String,
    github_token: String,
    local_path: String,
    sync_interval_secs: u64,
    web_port: u16,
}

async fn update_config(
    axum::Extension(state): axum::Extension<Arc<RwLock<SyncState>>>,
    axum::Extension(trigger): axum::Extension<Arc<Notify>>,
    Json(payload): Json<ConfigPayload>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let mut s = state.write().await;
    let old_port = s.config.web_port;
    
    s.config.github_username = payload.github_username;
    s.config.github_token = payload.github_token;
    s.config.local_path = payload.local_path;
    s.config.sync_interval_secs = payload.sync_interval_secs;
    s.config.web_port = payload.web_port;

    if let Err(e) = s.config.save() {
        s.add_log("ERROR", &format!("Failed to save config: {}", e));
        return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    s.add_log("INFO", "Configuration updated successfully via Web UI.");
    trigger.notify_one();

    let mut response = serde_json::Map::new();
    response.insert("success".to_string(), serde_json::Value::Bool(true));
    if old_port != payload.web_port {
        response.insert(
            "message".to_string(),
            serde_json::Value::String("Config updated. Port changed; please restart daemon to bind to new port.".to_string())
        );
    }
    
    Ok(Json(serde_json::Value::Object(response)))
}

#[derive(serde::Deserialize)]
struct SyncQuery {
    mode: Option<crate::state::SyncMode>,
}

async fn trigger_sync(
    axum::Extension(state): axum::Extension<Arc<RwLock<SyncState>>>,
    axum::Extension(trigger): axum::Extension<Arc<Notify>>,
    axum::extract::Query(query): axum::extract::Query<SyncQuery>,
) -> Json<serde_json::Value> {
    let mut s = state.write().await;
    if s.status == "Syncing" {
        let mut response = serde_json::Map::new();
        response.insert("success".to_string(), serde_json::Value::Bool(false));
        response.insert("message".to_string(), serde_json::Value::String("Sync is already running.".to_string()));
        return Json(serde_json::Value::Object(response));
    }

    let mode = query.mode.unwrap_or(crate::state::SyncMode::Full);
    s.next_sync_mode = mode;
    s.add_log("INFO", &format!("Manual sync requested via Web API. Mode: {:?}", mode));
    trigger.notify_one();

    let mut response = serde_json::Map::new();
    response.insert("success".to_string(), serde_json::Value::Bool(true));
    Json(serde_json::Value::Object(response))
}
