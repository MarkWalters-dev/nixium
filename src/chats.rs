use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use tracing::info;

use crate::state::AppState;

pub async fn api_chats_load(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let path = state.data_dir.join("chats.json");
    match tokio::fs::read_to_string(&path).await {
        Ok(data) => {
            let val: serde_json::Value =
                serde_json::from_str(&data).unwrap_or(serde_json::json!([]));
            (StatusCode::OK, Json(val)).into_response()
        }
        // File not yet created → return empty array (not an error)
        Err(_) => (StatusCode::OK, Json(serde_json::json!([]))).into_response(),
    }
}

pub async fn api_chats_save(
    State(state): State<Arc<AppState>>,
    Json(threads): Json<serde_json::Value>,
) -> impl IntoResponse {
    let path = state.data_dir.join("chats.json");
    if let Err(e) = tokio::fs::create_dir_all(&state.data_dir).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    let data = serde_json::to_string(&threads).unwrap_or_else(|_| "[]".to_string());
    match tokio::fs::write(&path, data).await {
        Ok(_) => {
            info!("Chats persisted to {:?}", path);
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn api_chats_delete(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let path = state.data_dir.join("chats.json");
    let data = match tokio::fs::read_to_string(&path).await {
        Ok(d) => d,
        Err(_) => return StatusCode::OK.into_response(), // nothing to delete
    };
    let mut threads: Vec<serde_json::Value> =
        serde_json::from_str(&data).unwrap_or_default();
    let before = threads.len();
    threads.retain(|t| t["id"].as_str() != Some(&id));
    if threads.len() == before {
        return StatusCode::OK.into_response(); // id not found, no-op
    }
    let json = serde_json::to_string(&threads).unwrap_or_else(|_| "[]".to_string());
    match tokio::fs::write(&path, json).await {
        Ok(_) => {
            info!("Deleted chat thread {:?}", id);
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
