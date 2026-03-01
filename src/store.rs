use std::{env, io::Read, time::Duration};

use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{error::ApiError, extensions::{extensions_dir, ExtManifest}};

/// Default public registry URL – override with `NIXIUM_EXT_REGISTRY` env var.
const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/MarkWalters-dev/nixium-extensions/master/registry.json";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single entry in the remote extension registry.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ExtStoreEntry {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
    /// URL of a `.zip` or `.tar.gz` containing the extension files.
    pub download_url: String,
}

#[derive(Deserialize)]
pub struct StoreSearchQuery {
    #[serde(default)]
    pub q: String,
}

#[derive(Deserialize)]
pub struct ExtInstallRequest {
    pub name: String,
    pub download_url: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/extensions/store/search?q=<text>
/// Fetches the remote registry and returns entries whose name/description
/// contain the query (case-insensitive).  Returns an empty array on network
/// or parse failure so the UI degrades gracefully.
pub async fn api_ext_store_search(Query(params): Query<StoreSearchQuery>) -> Response {
    let registry_url = env::var("NIXIUM_EXT_REGISTRY")
        .unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string());
    info!("EXT STORE SEARCH q={:?} registry={}", params.q, registry_url);

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let entries: Vec<ExtStoreEntry> = match client.get(&registry_url).send().await {
        Ok(res) if res.status().is_success() => {
            let v: Vec<ExtStoreEntry> = res.json().await.unwrap_or_default();
            info!("EXT STORE registry returned {} entries", v.len());
            v
        }
        Ok(res) => {
            info!("EXT STORE registry fetch failed: HTTP {}", res.status());
            vec![]
        }
        Err(e) => {
            info!("EXT STORE registry fetch error: {e}");
            vec![]
        }
    };

    let q = params.q.to_lowercase();
    let filtered: Vec<&ExtStoreEntry> = if q.is_empty() {
        entries.iter().collect()
    } else {
        entries
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&q)
                    || e.display_name.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
                    || e.author.to_lowercase().contains(&q)
            })
            .collect()
    };

    Json(filtered).into_response()
}

/// POST /api/extensions/store/install
/// Downloads the archive at `download_url` (`.zip` or `.tar.gz`) and
/// extracts it into `~/.config/nixium/extensions/<name>/`.
/// A common top-level directory is stripped automatically (GitHub-style
/// archives like `repo-main/` are handled transparently).
pub async fn api_ext_store_install(Json(req): Json<ExtInstallRequest>) -> Response {
    // --- Validate name ---------------------------------------------------
    if req.name.is_empty()
        || req.name.contains('/')
        || req.name.contains('\\')
        || req.name.contains("..")
        || req.name.starts_with('.')
    {
        return ApiError::response(StatusCode::BAD_REQUEST, "Invalid extension name");
    }

    info!("EXT INSTALL {} from {}", req.name, req.download_url);
    let ext_dir = extensions_dir().join(&req.name);

    // --- Download --------------------------------------------------------
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let bytes = match client.get(&req.download_url).send().await {
        Ok(r) => {
            let status = r.status();
            info!("EXT INSTALL download HTTP {status}");
            if !status.is_success() {
                return ApiError::response(
                    StatusCode::BAD_GATEWAY,
                    format!("Download returned HTTP {status}"),
                );
            }
            match r.bytes().await {
                Ok(b) => {
                    info!("EXT INSTALL downloaded {} bytes", b.len());
                    b
                }
                Err(e) => {
                    return ApiError::response(
                        StatusCode::BAD_GATEWAY,
                        format!("Failed to read download body: {e}"),
                    )
                }
            }
        }
        Err(e) => {
            info!("EXT INSTALL download error: {e}");
            return ApiError::response(StatusCode::BAD_GATEWAY, format!("Download failed: {e}"));
        }
    };

    // --- Clear / create destination directory ----------------------------
    if ext_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&ext_dir) {
            return ApiError::response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to clear existing extension: {e}"),
            );
        }
    }
    if let Err(e) = std::fs::create_dir_all(&ext_dir) {
        return ApiError::response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create extension directory: {e}"),
        );
    }

    // --- Extract ---------------------------------------------------------
    let url_lower = req.download_url.to_lowercase();

    if url_lower.ends_with(".zip") {
        if let Err(e) = extract_zip(&bytes, &req.name, &ext_dir) {
            return e;
        }
    } else {
        if let Err(e) = extract_targz(&bytes, &req.name, &ext_dir) {
            return e;
        }
    }

    // --- Verify manifest -------------------------------------------------
    let manifest_path = ext_dir.join("manifest.json");
    if !manifest_path.exists() {
        let _ = std::fs::remove_dir_all(&ext_dir);
        return ApiError::response(
            StatusCode::BAD_REQUEST,
            "Extracted archive does not contain manifest.json",
        );
    }

    // Return the installed manifest so the UI can update immediately.
    let text = std::fs::read_to_string(&manifest_path).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
    let name = req.name.clone();
    Json(ExtManifest {
        name: name.clone(),
        display_name: v["displayName"].as_str().unwrap_or(&name).to_string(),
        version: v["version"].as_str().unwrap_or("0.0.0").to_string(),
        description: v["description"].as_str().unwrap_or("").to_string(),
        main: v["main"].as_str().unwrap_or("index.js").to_string(),
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Archive extraction helpers
// ---------------------------------------------------------------------------

fn extract_zip(
    bytes: &[u8],
    ext_name: &str,
    ext_dir: &std::path::Path,
) -> Result<(), Response> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| {
        ApiError::response(StatusCode::BAD_REQUEST, format!("Invalid zip: {e}"))
    })?;

    // Detect a common top-level directory to strip.
    let prefix: Option<String> = (0..archive.len()).find_map(|i| {
        archive.by_index(i).ok().and_then(|f| {
            let n = f.name().to_string();
            let mut parts = n.splitn(2, '/');
            let dir = parts.next()?;
            if parts.next().is_some() && !dir.is_empty() {
                Some(format!("{dir}/"))
            } else {
                None
            }
        })
    });

    let ext_prefix = format!("{}/", ext_name);
    for i in 0..archive.len() {
        let mut file = match archive.by_index(i) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let raw_name = file.name().to_string();
        let after_archive = match &prefix {
            Some(pfx) => raw_name.strip_prefix(pfx).unwrap_or(&raw_name),
            None => &raw_name,
        };
        let rel_name = if let Some(r) = after_archive.strip_prefix(&ext_prefix) {
            r.to_string()
        } else if !after_archive.contains('/') {
            after_archive.to_string()
        } else {
            continue;
        };
        if rel_name.is_empty() || rel_name.ends_with('/') || rel_name.contains("..") {
            continue;
        }
        let out_path = ext_dir.join(&rel_name);
        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut content = Vec::new();
        if std::io::Read::read_to_end(&mut file, &mut content).is_ok() {
            let _ = std::fs::write(&out_path, content);
        }
    }
    Ok(())
}

fn extract_targz(
    bytes: &[u8],
    ext_name: &str,
    ext_dir: &std::path::Path,
) -> Result<(), Response> {
    let cursor = std::io::Cursor::new(bytes);
    let gz = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(gz);

    let entries_data: Vec<(String, Vec<u8>)> = archive
        .entries()
        .map(|iter| {
            iter.filter_map(|entry| {
                let mut e = entry.ok()?;
                let path = e.path().ok()?.to_string_lossy().to_string();
                let mut data = Vec::new();
                e.read_to_end(&mut data).ok()?;
                Some((path, data))
            })
            .collect()
        })
        .unwrap_or_default();

    // Detect common top-level directory.
    let prefix: Option<String> = entries_data.iter().find_map(|(name, _)| {
        let mut parts = name.splitn(2, '/');
        let dir = parts.next()?;
        if parts.next().is_some() && !dir.is_empty() {
            Some(format!("{dir}/"))
        } else {
            None
        }
    });

    let ext_prefix = format!("{}/", ext_name);
    for (raw_name, data) in &entries_data {
        let after_archive: &str = match &prefix {
            Some(pfx) => raw_name.strip_prefix(pfx).unwrap_or(raw_name),
            None => raw_name,
        };
        let rel_name = if let Some(r) = after_archive.strip_prefix(&ext_prefix) {
            r.to_string()
        } else if !after_archive.contains('/') {
            after_archive.to_string()
        } else {
            continue;
        };
        if rel_name.is_empty() || rel_name.ends_with('/') || rel_name.contains("..") {
            continue;
        }
        let out_path = ext_dir.join(&rel_name);
        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&out_path, data);
    }
    Ok(())
}
