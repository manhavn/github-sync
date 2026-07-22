use axum::{
    routing::{get, post, delete},
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::{RwLock, Notify};
use crate::state::{SyncState, ProfileSyncState};
use crate::config::SyncProfile;

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
        .route("/api/profiles", post(add_or_update_profile))
        .route("/api/profiles/:id", delete(delete_profile))
        .route("/api/select_profile", post(select_profile))
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
    
    // Mask tokens
    let mut profiles_masked = Vec::new();
    for p in &s.profiles {
        let mut p_masked = p.clone();
        p_masked.token = if p.token.is_empty() { "".to_string() } else { "********".to_string() };
        profiles_masked.push(p_masked);
    }
    
    let active_id = s.active_profile_id.clone();
    let active_state = s.profile_states.get(&active_id).cloned().unwrap_or_else(ProfileSyncState::new);
    
    let val = serde_json::json!({
        "profiles": profiles_masked,
        "active_profile_id": s.active_profile_id,
        "status": active_state.status,
        "last_sync_time": active_state.last_sync_time,
        "repos": active_state.repos,
        "logs": active_state.logs,
        "web_host": s.web_host,
        "web_port": s.web_port,
    });
    
    Json(val)
}

#[derive(serde::Deserialize)]
struct ProfilePayload {
    id: Option<String>,
    name: String,
    provider: String,
    domain: String,
    username: String,
    token: String,
    local_path: String,
    sync_interval_secs: u64,
}

async fn add_or_update_profile(
    axum::Extension(state): axum::Extension<Arc<RwLock<SyncState>>>,
    Json(payload): Json<ProfilePayload>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let mut s = state.write().await;
    let mut config = crate::config::Config::load().map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let profile_id = match payload.id {
        Some(ref id) if !id.is_empty() => {
            // Update existing
            if let Some(p) = config.profiles.iter_mut().find(|p| p.id == *id) {
                p.name = payload.name.clone();
                p.provider = payload.provider.clone();
                p.domain = payload.domain.clone();
                p.username = payload.username.clone();
                // Only overwrite token if the user typed a new one (not masked placeholder)
                if payload.token != "********" {
                    p.token = payload.token.clone();
                }
                p.local_path = payload.local_path.clone();
                p.sync_interval_secs = payload.sync_interval_secs;
            }
            
            // Sync state in memory
            if let Some(p) = s.profiles.iter_mut().find(|p| p.id == *id) {
                p.name = payload.name;
                p.provider = payload.provider;
                p.domain = payload.domain;
                p.username = payload.username;
                if payload.token != "********" {
                    p.token = payload.token;
                }
                p.local_path = payload.local_path;
                p.sync_interval_secs = payload.sync_interval_secs;
            }
            id.clone()
        }
        _ => {
            // Create new profile
            let new_id = format!("profile-{}", chrono::Utc::now().timestamp_millis());
            let new_profile = SyncProfile {
                id: new_id.clone(),
                name: payload.name,
                provider: payload.provider,
                domain: payload.domain,
                username: payload.username,
                token: payload.token,
                local_path: payload.local_path,
                sync_interval_secs: payload.sync_interval_secs,
            };
            
            config.profiles.push(new_profile.clone());
            s.profiles.push(new_profile);
            s.profile_states.insert(new_id.clone(), ProfileSyncState::new());
            
            if s.active_profile_id.is_empty() {
                config.active_profile_id = new_id.clone();
                s.active_profile_id = new_id.clone();
            }
            new_id
        }
    };

    config.save().map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let mut response = serde_json::Map::new();
    response.insert("success".to_string(), serde_json::Value::Bool(true));
    response.insert("profile_id".to_string(), serde_json::Value::String(profile_id));
    Ok(Json(serde_json::Value::Object(response)))
}

async fn delete_profile(
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Extension(state): axum::Extension<Arc<RwLock<SyncState>>>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let mut s = state.write().await;
    let mut config = crate::config::Config::load().map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    config.profiles.retain(|p| p.id != id);
    s.profiles.retain(|p| p.id != id);
    s.profile_states.remove(&id);

    if s.active_profile_id == id {
        let first_id = config.profiles.first().map(|p| p.id.clone()).unwrap_or_default();
        config.active_profile_id = first_id.clone();
        s.active_profile_id = first_id;
    }

    config.save().map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut response = serde_json::Map::new();
    response.insert("success".to_string(), serde_json::Value::Bool(true));
    Ok(Json(serde_json::Value::Object(response)))
}

#[derive(serde::Deserialize)]
struct SelectProfilePayload {
    id: String,
}

async fn select_profile(
    axum::Extension(state): axum::Extension<Arc<RwLock<SyncState>>>,
    axum::Extension(trigger): axum::Extension<Arc<Notify>>,
    Json(payload): Json<SelectProfilePayload>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let mut s = state.write().await;
    let mut config = crate::config::Config::load().map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    if !config.profiles.iter().any(|p| p.id == payload.id) {
        return Err(axum::http::StatusCode::NOT_FOUND);
    }

    config.active_profile_id = payload.id.clone();
    s.active_profile_id = payload.id;

    config.save().map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;

    s.add_log_to_active("INFO", "Switched active profile via Web UI.");
    
    // Trigger sync cycle on switch
    trigger.notify_one();

    let mut response = serde_json::Map::new();
    response.insert("success".to_string(), serde_json::Value::Bool(true));
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
    let active_id = s.active_profile_id.clone();
    
    if active_id.is_empty() {
        let mut response = serde_json::Map::new();
        response.insert("success".to_string(), serde_json::Value::Bool(false));
        response.insert("message".to_string(), serde_json::Value::String("No active profile configured to sync.".to_string()));
        return Json(serde_json::Value::Object(response));
    }

    let active_status = s.profile_states.get(&active_id).map(|st| st.status.clone()).unwrap_or_else(|| "Idle".to_string());
    if active_status == "Syncing" {
        let mut response = serde_json::Map::new();
        response.insert("success".to_string(), serde_json::Value::Bool(false));
        response.insert("message".to_string(), serde_json::Value::String("Sync is already running for this profile.".to_string()));
        return Json(serde_json::Value::Object(response));
    }

    let mode = query.mode.unwrap_or(crate::state::SyncMode::Full);
    s.next_sync_mode = mode;
    s.add_log_to_active("INFO", &format!("Manual sync requested via Web API. Mode: {:?}", mode));
    trigger.notify_one();

    let mut response = serde_json::Map::new();
    response.insert("success".to_string(), serde_json::Value::Bool(true));
    Json(serde_json::Value::Object(response))
}
