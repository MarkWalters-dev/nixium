use std::path::PathBuf;

use axum::{
    extract::Path,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::ApiError;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
pub struct ExtManifest {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub version: String,
    pub description: String,
    pub main: String,
}

// ---------------------------------------------------------------------------
// Directory helpers
// ---------------------------------------------------------------------------

/// Returns the path to the user's Nixium extensions directory.
///
/// Order of precedence:
///   1. `$NIXIUM_EXTENSIONS_DIR` environment variable (dev / CI override)
///   2. `$HOME/.config/nixium/extensions/`  (default user install)
pub fn extensions_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NIXIUM_EXTENSIONS_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".config")
        .join("nixium")
        .join("extensions")
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/extensions
/// Returns a JSON array of `ExtManifest` for every extension directory that
/// contains a valid `manifest.json`.
pub async fn api_extensions_list() -> Response {
    let dir = extensions_dir();
    info!("EXT LIST {:?}", dir);
    if !dir.exists() {
        info!("EXT LIST dir not found");
        return Json(Vec::<ExtManifest>::new()).into_response();
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Json(Vec::<ExtManifest>::new()).into_response();
    };
    let mut manifests = Vec::new();
    for entry in entries.flatten() {
        if !entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        let Ok(text) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        info!("EXT FOUND {}", name);
        manifests.push(ExtManifest {
            name: name.clone(),
            display_name: v["displayName"].as_str().unwrap_or(&name).to_string(),
            version: v["version"].as_str().unwrap_or("0.0.0").to_string(),
            description: v["description"].as_str().unwrap_or("").to_string(),
            main: v["main"].as_str().unwrap_or("index.js").to_string(),
        });
    }
    Json(manifests).into_response()
}

/// DELETE /api/extensions/:name  – permanently remove an installed extension.
pub async fn api_extension_delete(Path(name): Path<String>) -> Response {
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.starts_with('.') {
        return (StatusCode::BAD_REQUEST, "Invalid extension name").into_response();
    }
    let ext_dir = extensions_dir().join(&name);
    if !ext_dir.exists() {
        return ApiError::response(StatusCode::NOT_FOUND, format!("Extension '{name}' not found"));
    }
    match std::fs::remove_dir_all(&ext_dir) {
        Ok(_) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// GET /api/extensions/:name/readme
/// Serves the extension's README file as plain text (the UI renders it as Markdown).
pub async fn api_extension_readme(Path(name): Path<String>) -> Response {
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.starts_with('.') {
        return (StatusCode::BAD_REQUEST, "Invalid extension name").into_response();
    }
    let ext_dir = extensions_dir().join(&name);
    for filename in &["README.md", "readme.md", "CHANGELOG.md", "README", "readme"] {
        let p = ext_dir.join(filename);
        if let Ok(content) = std::fs::read_to_string(&p) {
            return (
                [(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"))],
                content,
            )
                .into_response();
        }
    }
    (StatusCode::NOT_FOUND, "").into_response()
}

/// GET /api/extensions/:name/script
/// Serves the extension's entry-point JS file with Content-Type: application/javascript.
pub async fn api_extension_script(Path(name): Path<String>) -> Response {
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.starts_with('.') {
        return (StatusCode::BAD_REQUEST, "Invalid extension name").into_response();
    }
    let ext_dir = extensions_dir().join(&name);
    let main_file = std::fs::read_to_string(ext_dir.join("manifest.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["main"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "index.js".to_string());
    match std::fs::read_to_string(ext_dir.join(&main_file)) {
        Ok(script) => (
            [(header::CONTENT_TYPE, HeaderValue::from_static("application/javascript"))],
            script,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "Extension script not found").into_response(),
    }
}
