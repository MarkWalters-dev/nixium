use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::info;

use crate::{error::ApiError, state::AppState};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A configured external MCP server that communicates over stdio.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ExternalMcpServer {
    pub id: String,
    /// Human-readable label shown in the UI.
    pub name: String,
    /// Executable to run (e.g. `npx`, `node`, `/usr/bin/python3`).
    pub command: String,
    /// Arguments passed to the command (e.g. `["-y", "@sveltejs/mcp"]`).
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional extra environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Whether this server is active. Disabled servers are hidden from the AI.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

/// Cached tool metadata fetched from a running external server.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExtToolMeta {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Wire type: an individual tool from an external server, as returned by the API.
#[derive(Serialize, Clone)]
pub struct ExternalToolInfo {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub description: String,
    pub enabled: bool,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
    #[serde(rename = "serverId")]
    pub server_id: String,
    #[serde(rename = "serverName")]
    pub server_name: String,
}

/// In-memory state for all external MCP servers.
#[derive(Default, Debug)]
pub struct ExternalMcpState {
    pub servers: Vec<ExternalMcpServer>,
    /// Per-server cached tool list  (server_id → tools).
    pub tool_cache: HashMap<String, Vec<ExtToolMeta>>,
    /// Fast dispatch table: tool_name → server_id.
    pub tool_index: HashMap<String, String>,
    /// Set of enabled external tool keys: "server_id::tool_name".
    pub enabled_tools: std::collections::HashSet<String>,
}

impl ExternalMcpState {
    pub fn rebuild_index(&mut self) {
        self.tool_index.clear();
        for server in &self.servers {
            if !server.enabled { continue; }
            if let Some(tools) = self.tool_cache.get(&server.id) {
                for tool in tools {
                    // Last-registered wins on name collision
                    self.tool_index.insert(tool.name.clone(), server.id.clone());
                }
            }
        }
    }

    fn tool_key(server_id: &str, tool_name: &str) -> String {
        format!("{server_id}::{tool_name}")
    }

    pub fn is_tool_enabled(&self, server_id: &str, tool_name: &str) -> bool {
        self.enabled_tools.contains(&Self::tool_key(server_id, tool_name))
    }

    pub fn set_tool_enabled(&mut self, server_id: &str, tool_name: &str, enabled: bool) {
        let key = Self::tool_key(server_id, tool_name);
        if enabled { self.enabled_tools.insert(key); } else { self.enabled_tools.remove(&key); }
    }

    /// When a server's tools are freshly cached, enable all new tools by default.
    pub fn enable_all_tools_for(&mut self, server_id: &str) {
        if let Some(tools) = self.tool_cache.get(server_id) {
            for tool in tools.clone() {
                let key = Self::tool_key(server_id, &tool.name);
                self.enabled_tools.insert(key);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn servers_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("external_mcp.json")
}

pub fn load_servers(data_dir: &Path) -> Vec<ExternalMcpServer> {
    let path = servers_path(data_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_servers(data_dir: &Path, servers: &[ExternalMcpServer]) {
    let _ = std::fs::create_dir_all(data_dir);
    if let Ok(json) = serde_json::to_string_pretty(servers) {
        let _ = std::fs::write(servers_path(data_dir), json);
    }
}

// ---------------------------------------------------------------------------
// MCP stdio JSON-RPC core
// ---------------------------------------------------------------------------

/// Spawn the external MCP server, send `messages`, and collect JSON-RPC
/// responses for every `id` in `wait_for_ids`.  Returns within `timeout_secs`
/// even if not all responses arrive.
async fn run_mcp_session(
    config: &ExternalMcpServer,
    messages: &[serde_json::Value],
    wait_for_ids: &[u64],
    timeout_secs: u64,
) -> Result<HashMap<u64, serde_json::Value>, String> {
    let mut child = Command::new(&config.command)
        .args(&config.args)
        .envs(&config.env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn '{}': {e}", config.command))?;

    let mut stdin = child.stdin.take().ok_or("no stdin pipe")?;
    let stdout = child.stdout.take().ok_or("no stdout pipe")?;

    // Write all messages (one JSON object per line as per the MCP stdio spec).
    for msg in messages {
        let mut line = serde_json::to_string(msg)
            .map_err(|e| format!("serialize: {e}"))?;
        line.push('\n');
        stdin.write_all(line.as_bytes()).await.map_err(|e| format!("stdin write: {e}"))?;
    }
    stdin.flush().await.ok();
    drop(stdin); // Close stdin → signals EOF to the server

    // Read responses with an overall deadline.
    let mut reader = BufReader::new(stdout);
    let mut responses: HashMap<u64, serde_json::Value> = HashMap::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        if responses.len() >= wait_for_ids.len() { break; }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            let _ = child.kill().await;
            return Err(format!(
                "Timeout after {timeout_secs}s waiting for MCP server '{}'. \
                 Check that the command is correct and the server starts up successfully.",
                config.name
            ));
        }

        let mut line = String::new();
        match tokio::time::timeout(remaining, reader.read_line(&mut line)).await {
            Ok(Ok(0)) => break,           // EOF
            Ok(Err(e)) => { let _ = child.kill().await; return Err(format!("read: {e}")); }
            Err(_) => { let _ = child.kill().await; return Err("Timeout reading response.".to_string()); }
            Ok(Ok(_)) => {}
        }

        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            // Only collect responses that have a numeric id we're waiting for.
            if let Some(id) = val.get("id").and_then(|v| v.as_u64()) {
                if wait_for_ids.contains(&id) {
                    responses.insert(id, val);
                }
            }
        }
    }

    let _ = child.kill().await;
    Ok(responses)
}

fn init_messages() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "nixium", "version": "1.0" }
            }
        }),
        // Notification (no id, no response expected)
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
    ]
}

// ---------------------------------------------------------------------------
// Public interface: list tools from a server
// ---------------------------------------------------------------------------

pub async fn list_tools(config: &ExternalMcpServer) -> Result<Vec<ExtToolMeta>, String> {
    let mut messages = init_messages();
    messages.push(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }));

    // Wait for init ack (id=1) and tools/list result (id=2).
    let responses = run_mcp_session(config, &messages, &[1, 2], 60).await?;

    if let Some(init) = responses.get(&1) {
        if let Some(err) = init.get("error") {
            return Err(format!(
                "Server '{}' init failed: {}",
                config.name,
                err["message"].as_str().unwrap_or("unknown error")
            ));
        }
    }

    let tools_resp = responses
        .get(&2)
        .ok_or_else(|| format!("Server '{}' did not respond to tools/list.", config.name))?;

    if let Some(err) = tools_resp.get("error") {
        return Err(format!(
            "tools/list error from '{}': {}",
            config.name,
            err["message"].as_str().unwrap_or("unknown")
        ));
    }

    let tools_arr = tools_resp["result"]["tools"]
        .as_array()
        .ok_or_else(|| format!("Server '{}' returned no tools array.", config.name))?;

    let metas: Vec<ExtToolMeta> = tools_arr
        .iter()
        .map(|t| ExtToolMeta {
            name: t["name"].as_str().unwrap_or("unknown").to_string(),
            description: t["description"].as_str().unwrap_or("").to_string(),
            input_schema: t.get("inputSchema")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"type":"object","properties":{}})),
        })
        .collect();

    Ok(metas)
}

// ---------------------------------------------------------------------------
// Public interface: call a tool on an external server
// ---------------------------------------------------------------------------

pub async fn call_tool(
    config: &ExternalMcpServer,
    tool_name: &str,
    args: &serde_json::Value,
) -> super::McpCallResponse {
    let mut messages = init_messages();
    messages.push(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": tool_name, "arguments": args }
    }));

    let responses = match run_mcp_session(config, &messages, &[1, 2], 60).await {
        Ok(r) => r,
        Err(e) => return super::McpCallResponse { content: e, is_error: true },
    };

    let call_resp = match responses.get(&2) {
        Some(r) => r,
        None => return super::McpCallResponse {
            content: format!("Server '{}' did not return a tools/call response.", config.name),
            is_error: true,
        },
    };

    if let Some(err) = call_resp.get("error") {
        return super::McpCallResponse {
            content: format!(
                "MCP error from '{}': {}",
                config.name,
                err["message"].as_str().unwrap_or("unknown")
            ),
            is_error: true,
        };
    }

    // Extract text from the content array (standard MCP result format).
    let content = if let Some(arr) = call_resp["result"]["content"].as_array() {
        arr.iter()
            .filter_map(|c| {
                if c["type"].as_str() == Some("text") { c["text"].as_str() } else { None }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else if let Some(s) = call_resp["result"]["content"].as_str() {
        s.to_string()
    } else {
        "(empty response)".to_string()
    };

    let is_error = call_resp["result"]["isError"].as_bool().unwrap_or(false);
    super::McpCallResponse { content, is_error }
}

// ---------------------------------------------------------------------------
// Helpers used by ai.rs to build the tool list for the agent
// ---------------------------------------------------------------------------

/// Returns McpToolInfo entries for all enabled external tools (from the cache).
pub fn get_enabled_tools_for_agent(ext: &ExternalMcpState) -> Vec<super::McpToolInfo> {
    let mut tools = Vec::new();
    for server in &ext.servers {
        if !server.enabled { continue; }
        if let Some(cached) = ext.tool_cache.get(&server.id) {
            for tool in cached {
                if !ext.is_tool_enabled(&server.id, &tool.name) { continue; }
                tools.push(super::McpToolInfo {
                    name: tool.name.clone(),
                    display_name: format!("{} ({})", tool.name, server.name),
                    description: tool.description.clone(),
                    enabled: true,
                    input_schema: tool.input_schema.clone(),
                });
            }
        }
    }
    tools
}

/// Ensure at least one attempt was made to cache tools for all enabled servers.
/// Called by the agent loop before building the tool list.
pub async fn ensure_cache(state: &Arc<AppState>) {
    let servers: Vec<ExternalMcpServer> = {
        let ext = state.external_mcp.read().await;
        ext.servers.iter().filter(|s| s.enabled).cloned().collect()
    };

    for server in servers {
        let already_cached = {
            let ext = state.external_mcp.read().await;
            ext.tool_cache.contains_key(&server.id)
        };
        if already_cached { continue; }

        info!("External MCP: auto-caching tools for '{}'", server.name);
        match list_tools(&server).await {
            Ok(tools) => {
                let mut ext = state.external_mcp.write().await;
                ext.tool_cache.insert(server.id.clone(), tools);
                ext.enable_all_tools_for(&server.id);
                ext.rebuild_index();
            }
            Err(e) => {
                info!("External MCP: could not cache '{}' — {e}", server.name);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// API request/response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AddServerRequest {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// API Handlers
// ---------------------------------------------------------------------------

/// GET /api/mcp/external — list all configured external servers.
pub async fn api_list_servers(State(state): State<Arc<AppState>>) -> Response {
    let ext = state.external_mcp.read().await;
    Json(ext.servers.clone()).into_response()
}

/// POST /api/mcp/external — add a new external server.
pub async fn api_add_server(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddServerRequest>,
) -> Response {
    if req.name.trim().is_empty() || req.command.trim().is_empty() {
        return ApiError::response(StatusCode::BAD_REQUEST, "name and command are required");
    }

    let id = format!("{}", uuid_simple());

    let server = ExternalMcpServer {
        id: id.clone(),
        name: req.name.trim().to_string(),
        command: req.command.trim().to_string(),
        args: req.args,
        env: req.env,
        enabled: true,
    };

    {
        let mut ext = state.external_mcp.write().await;
        ext.servers.push(server.clone());
        save_servers(&state.data_dir, &ext.servers);
    }

    info!("External MCP: added server '{}' ({})", server.name, server.command);
    Json(server).into_response()
}

/// DELETE /api/mcp/external/:id — remove a server.
pub async fn api_delete_server(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let mut ext = state.external_mcp.write().await;
    let before = ext.servers.len();
    ext.servers.retain(|s| s.id != id);
    if ext.servers.len() == before {
        return ApiError::response(StatusCode::NOT_FOUND, format!("Server '{id}' not found"));
    }
    ext.tool_cache.remove(&id);
    ext.rebuild_index();
    save_servers(&state.data_dir, &ext.servers);
    info!("External MCP: removed server {id}");
    StatusCode::NO_CONTENT.into_response()
}

/// POST /api/mcp/external/:id/toggle — enable or disable a server.
pub async fn api_toggle_server(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let mut ext = state.external_mcp.write().await;
    let Some(server) = ext.servers.iter_mut().find(|s| s.id == id) else {
        return ApiError::response(StatusCode::NOT_FOUND, format!("Server '{id}' not found"));
    };
    server.enabled = !server.enabled;
    let enabled = server.enabled;
    let server_clone = server.clone();
    ext.rebuild_index();
    save_servers(&state.data_dir, &ext.servers);
    info!("External MCP: server '{}' enabled={enabled}", server_clone.name);
    Json(server_clone).into_response()
}

/// GET /api/mcp/external/:id/tools — list tools from the server (spawns process, caches result).
pub async fn api_list_server_tools(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let server = {
        let ext = state.external_mcp.read().await;
        ext.servers.iter().find(|s| s.id == id).cloned()
    };
    let Some(server) = server else {
        return ApiError::response(StatusCode::NOT_FOUND, format!("Server '{id}' not found"));
    };

    info!("External MCP: fetching tools from '{}'", server.name);

    match list_tools(&server).await {
        Err(e) => {
            ApiError::response(StatusCode::BAD_GATEWAY, format!("Failed to list tools: {e}"))
        }
        Ok(tools) => {
            let mut ext = state.external_mcp.write().await;
            // Enable any newly discovered tools by default.
            for tool in &tools {
                let key = ExternalMcpState::tool_key(&id, &tool.name);
                ext.enabled_tools.insert(key);
            }
            ext.tool_cache.insert(id.clone(), tools);
            ext.rebuild_index();

            let result: Vec<ExternalToolInfo> = ext
                .tool_cache
                .get(&id)
                .map(|tools| {
                    tools.iter().map(|t| ExternalToolInfo {
                        name: t.name.clone(),
                        display_name: t.name.clone(),
                        description: t.description.clone(),
                        enabled: ext.is_tool_enabled(&id, &t.name),
                        input_schema: t.input_schema.clone(),
                        server_id: id.clone(),
                        server_name: server.name.clone(),
                    }).collect()
                })
                .unwrap_or_default();

            Json(result).into_response()
        }
    }
}

/// POST /api/mcp/external/:id/tools/:tool/toggle — enable/disable an individual tool.
pub async fn api_toggle_server_tool(
    State(state): State<Arc<AppState>>,
    AxumPath((server_id, tool_name)): AxumPath<(String, String)>,
) -> Response {
    let mut ext = state.external_mcp.write().await;
    let server = match ext.servers.iter().find(|s| s.id == server_id) {
        Some(s) => s.clone(),
        None => return ApiError::response(StatusCode::NOT_FOUND, "Server not found"),
    };

    let currently = ext.is_tool_enabled(&server_id, &tool_name);
    ext.set_tool_enabled(&server_id, &tool_name, !currently);
    ext.rebuild_index();

    let enabled = !currently;
    Json(serde_json::json!({ "name": tool_name, "enabled": enabled, "serverId": server_id, "serverName": server.name })).into_response()
}

// ---------------------------------------------------------------------------
// Tiny UUID-like ID generator (no extra crate needed)
// ---------------------------------------------------------------------------

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let r: u64 = (t ^ (t >> 32)) as u64
        ^ (t.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) as u128) as u64;
    format!("{:016x}{:016x}", t as u64, r)[..24].to_string()
}
