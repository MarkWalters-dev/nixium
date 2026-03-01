use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use std::{path::PathBuf, sync::Arc};
use tracing::info;

use crate::state::AppState;

fn chats_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".nixium").join("chats.json")
}

pub async fn api_chats_load(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let path = chats_path();
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
    State(_state): State<Arc<AppState>>,
    Json(threads): Json<serde_json::Value>,
) -> impl IntoResponse {
    let path = chats_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
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
