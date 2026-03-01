//! Single-binary self-hosted code editor.
//!
//! Architecture:
//!   - The SvelteKit SPA is embedded at compile time via rust-embed from
//!     `frontend/build/`.
//!   - Axum serves the SPA and REST + WebSocket endpoints:
//!       GET  /api/fs/read?path=<abs-path>   – read a file
//!       POST /api/fs/write  { path, content } – write a file
//!       GET  /api/fs/list?path=<abs-path>   – list a directory
//!       GET  /api/terminal/ws              – WebSocket PTY terminal
//!   - Path resolution honours the `$PREFIX` environment variable so that
//!     absolute paths are correctly rooted inside a nix-on-droid / Termux
//!     proot environment.

use std::{
    collections::HashSet,
    env,
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use mime_guess::from_path;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tower_http::cors::{Any, CorsLayer};
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Embedded frontend assets
// ---------------------------------------------------------------------------

/// All files produced by `npm run build` inside `frontend/` are embedded into
/// the binary.  The path is relative to the workspace root (where Cargo.toml
/// lives), so build the frontend first with:
///   cd frontend && npm ci && npm run build
#[derive(RustEmbed)]
#[folder = "frontend/build/"]
struct Assets;

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

/// Immutable server configuration derived from the environment at startup.
#[derive(Clone, Debug)]
struct AppState {
    /// When running inside nix-on-droid / Termux the `$PREFIX` env-var points
    /// to the proot root (e.g. `/data/data/com.termux/files/usr`).  Any
    /// absolute path supplied by the client is re-rooted under this prefix so
    /// file I/O lands in the correct location.
    prefix: Option<String>,
    /// Set of MCP tool names that are currently enabled (toggled by the UI).
    mcp_enabled: Arc<RwLock<HashSet<String>>>,
}

impl AppState {
    fn from_env() -> Self {
        let prefix = env::var("PREFIX").ok();
        if let Some(ref p) = prefix {
            info!("nix-on-droid / Termux mode: $PREFIX = {}", p);
        } else {
            info!("Standard Linux mode (no $PREFIX)");
        }
        // All built-in MCP tools are enabled by default.
        let mcp_enabled = Arc::new(RwLock::new(
            BUILTIN_MCP_TOOLS.iter().map(|t| t.name.to_string()).collect::<HashSet<_>>(),
        ));
        Self { prefix, mcp_enabled }
    }

    /// Resolve a client-supplied path to an absolute [`PathBuf`] on the host.
    ///
    /// Rules:
    ///   1. Never allow path traversal (`..` segments are stripped after
    ///      canonicalisation attempts).
    ///   2. If `$PREFIX` is set **and** the requested path is absolute,
    ///      prepend the prefix so the path is rooted inside the proot
    ///      environment.
    ///   3. Relative paths are rejected.
    fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        let p = PathBuf::from(raw);

        if p.is_relative() {
            return Err("Relative paths are not allowed".into());
        }

        // Strip the leading `/` so we can join cleanly.
        let stripped = p
            .strip_prefix("/")
            .map_err(|_| "Failed to strip root prefix".to_string())?;

        // Guard against traversal: reject any `..` component.
        for component in stripped.components() {
            use std::path::Component;
            if matches!(component, Component::ParentDir) {
                return Err("Path traversal detected".into());
            }
        }

        let resolved = match &self.prefix {
            Some(prefix) => PathBuf::from(prefix).join(stripped),
            None => p,
        };

        Ok(resolved)
    }
}

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ReadQuery {
    path: String,
}

#[derive(Deserialize)]
struct TerminalQuery {
    cwd: Option<String>,
}

#[derive(Deserialize)]
struct WriteBody {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct OllamaModelsQuery {
    #[serde(rename = "baseUrl", default)]
    base_url: String,
}

/// GET /api/ai/ollama-models?baseUrl=  – list models from a running Ollama instance
async fn api_ollama_models(Query(q): Query<OllamaModelsQuery>) -> Response {
    let base = if q.base_url.is_empty() { "http://localhost:11434".to_string() } else { q.base_url };
    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let res = match client.get(format!("{}/api/tags", base)).send().await {
        Ok(r) => r,
        Err(e) => return ApiError::response(StatusCode::BAD_GATEWAY, e.to_string()),
    };
    if !res.status().is_success() {
        return ApiError::response(StatusCode::BAD_GATEWAY, format!("Ollama returned {}", res.status()));
    }
    let json: serde_json::Value = match res.json().await {
        Ok(v) => v,
        Err(e) => return ApiError::response(StatusCode::BAD_GATEWAY, e.to_string()),
    };
    let names: Vec<String> = json["models"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
        .collect();
    Json(names).into_response()
}

#[derive(Deserialize)]
struct AiChatRequest {
    provider: String,
    #[serde(rename = "apiKey", default)]
    api_key: String,
    model: String,
    #[serde(rename = "baseUrl", default)]
    base_url: String,
    messages: Vec<serde_json::Value>,
    #[serde(rename = "systemPrompt", default)]
    system_prompt: String,
    /// Optional tool definitions forwarded to the upstream API (OpenAI function-calling format).
    #[serde(default)]
    tools: Option<serde_json::Value>,
    /// Optional tool_choice value forwarded to the upstream API.
    #[serde(rename = "toolChoice", default)]
    tool_choice: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct FsEntry {
    name: String,
    path: String,
    is_dir: bool,
}

#[derive(Deserialize)]
struct SearchQuery {
    path: String,
    query: String,
    #[serde(rename = "caseSensitive", default)]
    case_sensitive: bool,
}

#[derive(Serialize)]
struct SearchMatch {
    /// Client-visible (virtual) path, e.g. "/home/user/project/src/main.rs"
    path: String,
    /// 1-based line number
    line: usize,
    /// 0-based column (byte offset) of the match within the line
    col: usize,
    /// The full trimmed line text for display as a snippet
    text: String,
}

// Directories that are never searched.
const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", "target", ".cache", "__pycache__",
    ".next", ".svelte-kit", "dist", "build",
];

/// Walk `dir` recursively, collecting lines matching `query` into `out`.
/// `client_base` is the virtual path prefix for constructing result paths.
fn search_dir(
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
// MCP (Model Context Protocol) server
// ---------------------------------------------------------------------------

/// Static metadata for every built-in MCP skill.
struct BuiltinMcpMeta {
    name: &'static str,
    display_name: &'static str,
    description: &'static str,
    /// JSON Schema for the tool's input parameters (OpenAI function-calling format).
    input_schema: &'static str,
}

const BUILTIN_MCP_TOOLS: &[BuiltinMcpMeta] = &[
    BuiltinMcpMeta {
        name: "get_current_temperature",
        display_name: "Current Temperature — Blackfoot, Idaho",
        description: "Fetches the current temperature, humidity, wind speed and sky conditions \
                      in Blackfoot, Idaho using the Open-Meteo free weather API. No API key required. \
                      Returns Fahrenheit by default unless unit=celsius is specified.",
        input_schema: r#"{
            "type": "object",
            "properties": {
                "unit": {
                    "type": "string",
                    "enum": ["fahrenheit", "celsius"],
                    "description": "Temperature unit. Defaults to fahrenheit."
                }
            }
        }"#,
    },
];

/// Wire-format tool info returned by GET /api/mcp/tools.
#[derive(Serialize, Clone)]
struct McpToolInfo {
    name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    description: String,
    enabled: bool,
    /// JSON Schema object (OpenAI parameters format).
    #[serde(rename = "inputSchema")]
    input_schema: serde_json::Value,
}

#[derive(Deserialize)]
struct McpCallRequest {
    name: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

#[derive(Serialize)]
struct McpCallResponse {
    content: String,
    is_error: bool,
}

/// GET /api/mcp/tools  – list all built-in MCP tools with their enabled state.
async fn api_mcp_list_tools(State(state): State<Arc<AppState>>) -> Response {
    let enabled = state.mcp_enabled.read().await;
    let tools: Vec<McpToolInfo> = BUILTIN_MCP_TOOLS
        .iter()
        .map(|t| {
            let schema = serde_json::from_str(t.input_schema).unwrap_or(serde_json::json!({}));
            McpToolInfo {
                name: t.name.to_string(),
                display_name: t.display_name.to_string(),
                description: t.description.to_string(),
                enabled: enabled.contains(t.name),
                input_schema: schema,
            }
        })
        .collect();
    Json(tools).into_response()
}

/// POST /api/mcp/tools/:name/toggle  – enable or disable an MCP skill.
async fn api_mcp_toggle_tool(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    // Reject unknown tools.
    let Some(meta) = BUILTIN_MCP_TOOLS.iter().find(|t| t.name == name) else {
        return ApiError::response(StatusCode::NOT_FOUND, format!("Unknown MCP tool: {name}"));
    };

    let mut enabled = state.mcp_enabled.write().await;
    let now_enabled = if enabled.contains(&name) {
        enabled.remove(&name);
        false
    } else {
        enabled.insert(name.clone());
        true
    };
    drop(enabled);

    let schema = serde_json::from_str(meta.input_schema).unwrap_or(serde_json::json!({}));
    Json(McpToolInfo {
        name: meta.name.to_string(),
        display_name: meta.display_name.to_string(),
        description: meta.description.to_string(),
        enabled: now_enabled,
        input_schema: schema,
    })
    .into_response()
}

/// WMO weather interpretation code → human-readable description.
fn weather_code_desc(code: i64) -> &'static str {
    match code {
        0  => "Clear sky",
        1  => "Mainly clear",
        2  => "Partly cloudy",
        3  => "Overcast",
        45 | 48 => "Foggy",
        51 | 53 | 55 => "Drizzle",
        61 | 63 | 65 => "Rain",
        71 | 73 | 75 => "Snowfall",
        77  => "Snow grains",
        80 | 81 | 82 => "Rain showers",
        85 | 86 => "Snow showers",
        95  => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _  => "Unknown conditions",
    }
}

/// Dispatch a call to a named built-in MCP skill and return its text response.
async fn dispatch_mcp_call(
    name: &str,
    args: &serde_json::Value,
    client: &reqwest::Client,
) -> McpCallResponse {
    match name {
        "get_current_temperature" => {
            let unit   = args.get("unit").and_then(|v| v.as_str()).unwrap_or("fahrenheit");
            let temp_unit = if unit == "celsius" { "celsius" } else { "fahrenheit" };
            let symbol    = if unit == "celsius" { "°C" } else { "°F" };

            let url = format!(
                "https://api.open-meteo.com/v1/forecast\
                 ?latitude=43.1935&longitude=-112.3490\
                 &current=temperature_2m,weather_code,relative_humidity_2m,wind_speed_10m\
                 &temperature_unit={temp_unit}&wind_speed_unit=mph&timezone=America%2FDenver"
            );

            match client.get(&url).send().await {
                Err(e) => McpCallResponse {
                    content: format!("Failed to reach Open-Meteo API: {e}"),
                    is_error: true,
                },
                Ok(resp) => {
                    if !resp.status().is_success() {
                        return McpCallResponse {
                            content: format!("Open-Meteo returned HTTP {}", resp.status()),
                            is_error: true,
                        };
                    }
                    match resp.json::<serde_json::Value>().await {
                        Err(e) => McpCallResponse {
                            content: format!("Failed to parse weather response: {e}"),
                            is_error: true,
                        },
                        Ok(json) => {
                            let cur   = &json["current"];
                            let temp  = cur["temperature_2m"].as_f64().unwrap_or(0.0);
                            let humid = cur["relative_humidity_2m"].as_f64().unwrap_or(0.0);
                            let wind  = cur["wind_speed_10m"].as_f64().unwrap_or(0.0);
                            let code  = cur["weather_code"].as_i64().unwrap_or(0);
                            // Use the unit name the API confirmed it used, not just the symbol,
                            // so the LLM cannot confuse the scale with the other unit.
                            let unit_label = if unit == "celsius" { "Celsius" } else { "Fahrenheit" };
                            McpCallResponse {
                                content: format!(
                                    "Current weather in Blackfoot, Idaho (temperatures in {unit_label}):\n\
                                     Temperature : {temp:.1} {symbol}\n\
                                     Conditions  : {}\n\
                                     Humidity    : {humid:.0}%\n\
                                     Wind Speed  : {wind:.1} mph",
                                    weather_code_desc(code),
                                ),
                                is_error: false,
                            }
                        }
                    }
                }
            }
        }
        other => McpCallResponse {
            content: format!("Unknown MCP tool: {other}"),
            is_error: true,
        },
    }
}

/// POST /api/mcp/call  – invoke a named MCP skill by the AI or the user.
async fn api_mcp_call(
    State(state): State<Arc<AppState>>,
    Json(req): Json<McpCallRequest>,
) -> Response {
    // Reject disabled tools.
    {
        let enabled = state.mcp_enabled.read().await;
        if !enabled.contains(&req.name) {
            return ApiError::response(
                StatusCode::FORBIDDEN,
                format!("MCP tool '{}' is disabled", req.name),
            );
        }
    }

    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let resp = dispatch_mcp_call(&req.name, &req.arguments, &client).await;
    Json(resp).into_response()
}

// ---------------------------------------------------------------------------
// Extension system
// ---------------------------------------------------------------------------

/// Returns the path to the user's Nixium extensions directory.
/// Order of precedence:
///   1. `$NIXIUM_EXTENSIONS_DIR` environment variable (dev / CI override)
///   2. `$HOME/.config/nixium/extensions/`  (default user install)
fn extensions_dir() -> PathBuf {
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

#[derive(Serialize, Clone)]
struct ExtManifest {
    name:         String,
    #[serde(rename = "displayName")]
    display_name: String,
    version:      String,
    description:  String,
    main:         String,
}

/// GET /api/extensions
/// Returns a JSON array of `ExtManifest` for every extension directory that
/// contains a valid `manifest.json`.
async fn api_extensions_list() -> Response {
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
        let Ok(text) = std::fs::read_to_string(&manifest_path) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        info!("EXT FOUND {}", name);
        manifests.push(ExtManifest {
            name:         name.clone(),
            display_name: v["displayName"].as_str().unwrap_or(&name).to_string(),
            version:      v["version"].as_str().unwrap_or("0.0.0").to_string(),
            description:  v["description"].as_str().unwrap_or("").to_string(),
            main:         v["main"].as_str().unwrap_or("index.js").to_string(),
        });
    }
    Json(manifests).into_response()
}

/// DELETE /api/extensions/:name  – permanently remove an installed extension.
async fn api_extension_delete(Path(name): Path<String>) -> Response {
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

// ---------------------------------------------------------------------------
// Extension store
// ---------------------------------------------------------------------------

/// Default public registry URL – override with `NIXIUM_EXT_REGISTRY` env var.
const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/MarkWalters-dev/nixium-extensions/main/registry.json";

/// A single entry in the remote extension registry.
#[derive(Serialize, Deserialize, Clone, Default)]
struct ExtStoreEntry {
    name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    version: String,
    description: String,
    #[serde(default)]
    author: String,
    /// URL of a `.zip` or `.tar.gz` containing the extension files.
    download_url: String,
}

#[derive(Deserialize)]
struct StoreSearchQuery {
    #[serde(default)]
    q: String,
}

/// GET /api/extensions/store/search?q=<text>
/// Fetches the remote registry and returns entries whose name/description
/// contain the query (case-insensitive).  Returns an empty array on network
/// or parse failure so the UI degrades gracefully.
async fn api_ext_store_search(Query(params): Query<StoreSearchQuery>) -> Response {
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

#[derive(Deserialize)]
struct ExtInstallRequest {
    name: String,
    download_url: String,
}

/// POST /api/extensions/store/install
/// Downloads the archive at `download_url` (`.zip` or `.tar.gz`) and
/// extracts it into `~/.config/nixium/extensions/<name>/`.
/// A common top-level directory is stripped automatically (GitHub-style
/// archives like `repo-main/` are handled transparently).
async fn api_ext_store_install(Json(req): Json<ExtInstallRequest>) -> Response {
    // --- Validate name ---------------------------------------------------
    if req.name.is_empty()
        || req.name.contains('/')
        || req.name.contains('\\')
        || req.name.contains("..")
        || req.name.starts_with('.')
    {
        return ApiError::response(StatusCode::BAD_REQUEST, "Invalid extension name");
    }

    let ext_dir = extensions_dir().join(&req.name);

    // --- Download --------------------------------------------------------
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
    {
        Ok(c) => c,
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let bytes = match client.get(&req.download_url).send().await {
        Ok(r) => match r.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return ApiError::response(
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to read download body: {e}"),
                )
            }
        },
        Err(e) => {
            return ApiError::response(
                StatusCode::BAD_GATEWAY,
                format!("Download failed: {e}"),
            )
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
        // ── ZIP ──────────────────────────────────────────────────────────
        let cursor = std::io::Cursor::new(&bytes[..]);
        let mut archive = match zip::ZipArchive::new(cursor) {
            Ok(a) => a,
            Err(e) => return ApiError::response(StatusCode::BAD_REQUEST, format!("Invalid zip: {e}")),
        };

        // Detect a common top-level directory to strip.
        let prefix: Option<String> = (0..archive.len())
            .find_map(|i| {
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

        for i in 0..archive.len() {
            let mut file = match archive.by_index(i) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let raw_name = file.name().to_string();
            let rel_name = match &prefix {
                Some(pfx) => raw_name.strip_prefix(pfx).unwrap_or(&raw_name).to_string(),
                None => raw_name.clone(),
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
    } else {
        // ── TAR.GZ (default) ─────────────────────────────────────────────
        let cursor = std::io::Cursor::new(&bytes[..]);
        let gz = flate2::read::GzDecoder::new(cursor);
        let mut archive = tar::Archive::new(gz);

        // Collect all entries into memory so we can do a two-pass prefix detection.
        let entries_data: Vec<(String, Vec<u8>)> = archive
            .entries()
            .map(|iter| {
                iter.filter_map(|entry| {
                    let mut e = entry.ok()?;
                    let path = e.path().ok()?.to_string_lossy().to_string();
                    let mut data = Vec::new();
                    std::io::Read::read_to_end(&mut e, &mut data).ok()?;
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

        for (raw_name, data) in &entries_data {
            let rel_name: String = match &prefix {
                Some(pfx) => raw_name.strip_prefix(pfx).unwrap_or(raw_name).to_string(),
                None => raw_name.clone(),
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
        version:      v["version"].as_str().unwrap_or("0.0.0").to_string(),
        description:  v["description"].as_str().unwrap_or("").to_string(),
        main:         v["main"].as_str().unwrap_or("index.js").to_string(),
    })
    .into_response()
}

/// GET /api/extensions/:name/readme
/// Serves the extension's README file as plain text (the UI renders it as Markdown).
/// Tries README.md, readme.md, README, readme in order.
async fn api_extension_readme(Path(name): Path<String>) -> Response {
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
            ).into_response();
        }
    }
    (StatusCode::NOT_FOUND, "").into_response()
}

/// GET /api/extensions/:name/script
/// Serves the extension's entry-point JS file with Content-Type: application/javascript.
async fn api_extension_script(Path(name): Path<String>) -> Response {
    // Basic sanitization — reject anything that looks like path traversal
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

/// GET /api/fs/search?path=<dir>&query=<text>&caseSensitive=<bool>
async fn api_search(
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

#[derive(Serialize)]
struct ApiError {
    error: String,
}

impl ApiError {
    fn response(status: StatusCode, msg: impl Into<String>) -> Response {
        (status, Json(ApiError { error: msg.into() })).into_response()
    }
}

// ---------------------------------------------------------------------------
// API handlers
// ---------------------------------------------------------------------------

/// POST /api/ai/chat  – proxy to an AI provider and stream the SSE response
async fn api_ai_chat(Json(req): Json<AiChatRequest>) -> Response {
    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let (url, body) = match req.provider.as_str() {
        "anthropic" => {
            let base = if req.base_url.is_empty() { "https://api.anthropic.com" } else { &req.base_url };
            let body = serde_json::json!({
                "model": req.model,
                "max_tokens": 8096,
                "stream": true,
                "system": req.system_prompt,
                "messages": req.messages,
            });
            (format!("{}/v1/messages", base), body)
        }
        _ => {
            // OpenAI-compatible: openai, ollama, custom
            let base = if !req.base_url.is_empty() {
                req.base_url.clone()
            } else if req.provider == "ollama" {
                "http://localhost:11434".to_string()
            } else {
                "https://api.openai.com".to_string()
            };
            let mut messages: Vec<serde_json::Value> = Vec::new();
            if !req.system_prompt.is_empty() {
                messages.push(serde_json::json!({"role": "system", "content": req.system_prompt}));
            }
            messages.extend(req.messages.iter().cloned());
            let mut body = serde_json::json!({
                "model": req.model,
                "stream": true,
                "messages": messages,
            });
            if let Some(tools) = &req.tools {
                body["tools"] = tools.clone();
            }
            if let Some(tc) = &req.tool_choice {
                body["tool_choice"] = tc.clone();
            }
            (format!("{}/v1/chat/completions", base), body)
        }
    };

    let mut builder = client.post(&url).json(&body)
        .header("content-type", "application/json");

    match req.provider.as_str() {
        "anthropic" => {
            builder = builder
                .header("x-api-key", &req.api_key)
                .header("anthropic-version", "2023-06-01");
        }
        _ => {
            if !req.api_key.is_empty() {
                builder = builder.header("authorization", format!("Bearer {}", req.api_key));
            }
        }
    }

    let upstream = match builder.send().await {
        Err(e) => return ApiError::response(
            StatusCode::BAD_GATEWAY,
            format!("Cannot reach {} — {}", url, e),
        ),
        Ok(r) => r,
    };

    if !upstream.status().is_success() {
        let status = upstream.status().as_u16();
        let text = upstream.text().await.unwrap_or_default();
        let msg = if text.is_empty() { format!("Upstream {} returned HTTP {}", url, status) } else { text };
        return ApiError::response(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), msg);
    }

    let stream = upstream.bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(Body::from_stream(stream))
        .unwrap()
}

/// GET /api/fs/read?path=/absolute/path/to/file
async fn api_read(
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
        Err(e) => {
            ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    }
}

/// GET /api/fs/list?path=/absolute/dir
async fn api_list(
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
            return ApiError::response(StatusCode::FORBIDDEN, format!("Permission denied: {}", resolved.display()));
        }
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let mut entries: Vec<FsEntry> = read_dir
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let name = e.file_name().to_string_lossy().to_string();
            // Reconstruct the canonical client-side path (always uses '/').
            let base = params.path.trim_end_matches('/');
            let path = format!("{}/{}", base, name);
            Some(FsEntry { name, path, is_dir })
        })
        .collect();

    // Directories first, then alphabetical within each group.
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));

    (StatusCode::OK, Json(entries)).into_response()
}

/// POST /api/fs/write   body: { "path": "...", "content": "..." }
async fn api_write(
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
        Err(e) => {
            ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal WebSocket handler
// ---------------------------------------------------------------------------

/// GET /api/terminal/ws — upgrades to a WebSocket that bridges a PTY shell.
///
/// Message protocol (from client):
///   - Text starting with `\x00resize:COLS:ROWS` — resize the PTY
///   - Any other text — raw input forwarded to the shell's stdin
///
/// Message protocol (to client):
///   - Binary — raw PTY stdout/stderr bytes (consumed directly by xterm.js)
async fn terminal_ws(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TerminalQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_terminal_socket(socket, state, params.cwd))
}

async fn handle_terminal_socket(socket: WebSocket, state: Arc<AppState>, requested_cwd: Option<String>) {
    // Determine the shell binary.
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());

    // Default working directory: requested cwd > $HOME > prefix root > /.
    let cwd = requested_cwd
        .and_then(|p| state.resolve(&p).ok())
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| std::path::Path::new(p).is_dir())
        .unwrap_or_else(|| env::var("HOME").unwrap_or_else(|_| {
            state.prefix.clone().unwrap_or_else(|| "/".into())
        }));

    // Open a PTY pair.
    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to open PTY: {}", e);
            return;
        }
    };

    // Build and spawn the shell command.
    let mut cmd = CommandBuilder::new(&shell);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.cwd(&cwd);

    let _child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to spawn shell '{}': {}", shell, e);
            return;
        }
    };

    // Clone handles for the PTY master.
    let mut pty_reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => { warn!("PTY try_clone_reader: {}", e); return; }
    };
    let mut pty_writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(e) => { warn!("PTY take_writer: {}", e); return; }
    };
    // Keep master alive for resize calls.
    let master = pair.master;

    // Channel: PTY reader thread → async WS sender task.
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(128);

    // Spawn a blocking thread to read PTY output and forward via channel.
    let read_tx = tx.clone();
    tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 4096];
        loop {
            match pty_reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if read_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Split the WebSocket.
    let (mut ws_tx, mut ws_rx) = socket.split();

    loop {
        tokio::select! {
            // PTY output → client
            Some(data) = rx.recv() => {
                if ws_tx.send(Message::Binary(data)).await.is_err() {
                    break;
                }
            }
            // Client input → PTY
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(rest) = text.strip_prefix("\x00resize:") {
                            // Resize: "\x00resize:COLS:ROWS"
                            let parts: Vec<&str> = rest.splitn(2, ':').collect();
                            if parts.len() == 2 {
                                if let (Ok(cols), Ok(rows)) = (
                                    parts[0].parse::<u16>(),
                                    parts[1].parse::<u16>(),
                                ) {
                                    let _ = master.resize(PtySize {
                                        rows,
                                        cols,
                                        pixel_width: 0,
                                        pixel_height: 0,
                                    });
                                }
                            }
                        } else if pty_writer.write_all(text.as_bytes()).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        if pty_writer.write_all(&data).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Static file / SPA fallback handler
// ---------------------------------------------------------------------------

/// Serve an embedded asset by URI path.  Falls back to `index.html` for any
/// path that does not match a real asset – this is essential for client-side
/// SvelteKit routing.
async fn static_handler(uri: Uri) -> Response {
    let raw_path = uri.path().trim_start_matches('/');

    // Try exact match first.
    if let Some(content) = Assets::get(raw_path) {
        return serve_asset(raw_path, content.data);
    }

    // If the path has no extension it is likely a client-side route; serve
    // the SPA shell so the router can take over.
    if !raw_path.contains('.') {
        if let Some(index) = Assets::get("index.html") {
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"))],
                Body::from(index.data),
            )
                .into_response();
        }
    }

    (StatusCode::NOT_FOUND, "404 Not Found").into_response()
}

fn serve_asset(path: &str, data: std::borrow::Cow<'static, [u8]>) -> Response {
    let mime = from_path(path).first_or_octet_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_str(mime.as_ref()).unwrap_or(HeaderValue::from_static("application/octet-stream")),
        )
        // Cache immutable hashed assets aggressively; HTML must revalidate.
        .header(
            header::CACHE_CONTROL,
            if path.ends_with(".html") {
                HeaderValue::from_static("no-cache")
            } else {
                HeaderValue::from_static("public, max-age=31536000, immutable")
            },
        )
        .body(Body::from(data))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Initialise logging.  Set RUST_LOG=debug for verbose output.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nixium=info,tower_http=info".parse().unwrap()),
        )
        .init();

    let state = Arc::new(AppState::from_env());

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_router = Router::new()
        .route("/fs/read", get(api_read))
        .route("/fs/write", post(api_write))
        .route("/fs/list", get(api_list))
        .route("/fs/search", get(api_search))
        .route("/extensions", get(api_extensions_list))
        .route("/extensions/store/search", get(api_ext_store_search))
        .route("/extensions/store/install", post(api_ext_store_install))
        .route("/extensions/:name", axum::routing::delete(api_extension_delete))
        .route("/extensions/:name/readme", get(api_extension_readme))
        .route("/extensions/:name/script", get(api_extension_script))
        .route("/terminal/ws", get(terminal_ws))
        .route("/ai/chat", post(api_ai_chat))
        .route("/ai/ollama-models", get(api_ollama_models))
        .route("/mcp/tools", get(api_mcp_list_tools))
        .route("/mcp/tools/:name/toggle", post(api_mcp_toggle_tool))
        .route("/mcp/call", post(api_mcp_call));

    let app = Router::new()
        .nest("/api", api_router)
        // Catch-all: serve the embedded SPA for every other request.
        .fallback(static_handler)
        .layer(cors)
        .with_state(state);

    let bind_addr = env::var("NIXIUM_ADDR").unwrap_or_else(|_| "0.0.0.0:8123".into());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind TCP listener");

    info!("Listening on http://{}", bind_addr);
    info!("Open your browser at http://localhost:{}", listener.local_addr().unwrap().port());

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
