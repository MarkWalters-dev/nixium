use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{error::ApiError, state::AppState};

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ReadQuery {
    pub path: String,
}

#[derive(Deserialize)]
pub struct WriteBody {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub path: String,
    pub query: String,
    #[serde(rename = "caseSensitive", default)]
    pub case_sensitive: bool,
}

#[derive(Serialize)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Serialize)]
pub struct SearchMatch {
    /// Client-visible (virtual) path, e.g. "/home/user/project/src/main.rs"
    pub path: String,
    /// 1-based line number
    pub line: usize,
    /// 0-based column (byte offset) of the match within the line
    pub col: usize,
    /// The full trimmed line text for display as a snippet
    pub text: String,
}

// ---------------------------------------------------------------------------
// Directory search helper
// ---------------------------------------------------------------------------

/// Directories that are never searched.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", "target", ".cache", "__pycache__",
    ".next", ".svelte-kit", "dist", "build",
];

/// Walk `dir` recursively, collecting lines matching `query` into `out`.
/// `client_base` is the virtual path prefix for constructing result paths.
pub fn search_dir(
    dir: &std::path::Path,
    client_base: &str,
    query: &str,
    case_sensitive: bool,
    out: &mut Vec<SearchMatch>,
    max: usize,
) {
    if out.len() >= max {
        return;
    }

    let rd = match std::fs::read_dir(dir) {
        Ok(x) => x,
        Err(_) => return,
    };

    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    // Sort: dirs first, then alphabetical.
    entries.sort_by(|a, b| {
        let ad = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let bd = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        bd.cmp(&ad).then(a.file_name().cmp(&b.file_name()))
    });

    for entry in entries {
        if out.len() >= max {
            break;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let client_path = format!("{}/{}", client_base.trim_end_matches('/'), &name);

        if meta.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            search_dir(&entry.path(), &client_path, query, case_sensitive, out, max);
        } else if meta.is_file() {
            // Skip files larger than 1 MB to avoid reading huge binaries.
            if meta.len() > 1_000_000 {
                continue;
            }
            let content = match std::fs::read_to_string(entry.path()) {
                Ok(c) => c,
                Err(_) => continue, // binary or permission error - skip
            };

            let needle = if case_sensitive {
                query.to_string()
            } else {
                query.to_lowercase()
            };

            for (idx, line) in content.lines().enumerate() {
                if out.len() >= max {
                    break;
                }
                let haystack = if case_sensitive {
                    line.to_string()
                } else {
                    line.to_lowercase()
                };
                if let Some(col) = haystack.find(&needle) {
                    out.push(SearchMatch {
                        path: client_path.clone(),
                        line: idx + 1,
                        col,
                        text: line.to_string(),
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/fs/read?path=/absolute/path/to/file
pub async fn api_read(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ReadQuery>,
) -> Response {
    let resolved = match state.resolve(&params.path) {
        Ok(p) => p,
        Err(e) => return ApiError::response(StatusCode::BAD_REQUEST, e),
    };

    info!("READ {:?}", resolved);

    match std::fs::read_to_string(&resolved) {
        Ok(contents) => (StatusCode::OK, contents).into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            ApiError::response(StatusCode::NOT_FOUND, format!("File not found: {}", resolved.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            ApiError::response(StatusCode::FORBIDDEN, format!("Permission denied: {}", resolved.display()))
        }
        Err(e) => ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// GET /api/fs/list?path=/absolute/dir
pub async fn api_list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ReadQuery>,
) -> Response {
    let resolved = match state.resolve(&params.path) {
        Ok(p) => p,
        Err(e) => return ApiError::response(StatusCode::BAD_REQUEST, e),
    };

    info!("LIST {:?}", resolved);

    let read_dir = match std::fs::read_dir(&resolved) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return ApiError::response(StatusCode::NOT_FOUND, format!("Not found: {}", resolved.display()));
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return ApiError::response(
                StatusCode::FORBIDDEN,
                format!("Permission denied: {}", resolved.display()),
            );
        }
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let mut entries: Vec<FsEntry> = read_dir
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let name = e.file_name().to_string_lossy().to_string();
            let base = params.path.trim_end_matches('/');
            let path = format!("{}/{}", base, name);
            Some(FsEntry { name, path, is_dir })
        })
        .collect();

    // Directories first, then alphabetical within each group.
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    (StatusCode::OK, Json(entries)).into_response()
}

/// POST /api/fs/write   body: { "path": "...", "content": "..." }
pub async fn api_write(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WriteBody>,
) -> Response {
    let resolved = match state.resolve(&body.path) {
        Ok(p) => p,
        Err(e) => return ApiError::response(StatusCode::BAD_REQUEST, e),
    };

    info!("WRITE {:?}", resolved);

    // Create parent directories if they do not exist.
    if let Some(parent) = resolved.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ApiError::response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create directories: {e}"),
            );
        }
    }

    match std::fs::write(&resolved, body.content.as_bytes()) {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            ApiError::response(StatusCode::FORBIDDEN, format!("Permission denied: {}", resolved.display()))
        }
        Err(e) => ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// GET /api/fs/search?path=<dir>&query=<text>&caseSensitive=<bool>
pub async fn api_search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchQuery>,
) -> Response {
    if params.query.is_empty() {
        return (StatusCode::OK, Json(Vec::<SearchMatch>::new())).into_response();
    }

    let resolved = match state.resolve(&params.path) {
        Ok(p) => p,
        Err(e) => return ApiError::response(StatusCode::BAD_REQUEST, e),
    };

    if !resolved.is_dir() {
        return ApiError::response(StatusCode::BAD_REQUEST, "Path is not a directory");
    }

    let client_base = params.path.trim_end_matches('/').to_string();
    let mut matches: Vec<SearchMatch> = Vec::new();
    search_dir(
        &resolved,
        &client_base,
        &params.query,
        params.case_sensitive,
        &mut matches,
        500,
    );

    (StatusCode::OK, Json(matches)).into_response()
}
