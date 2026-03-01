pub mod rust;
pub mod weather;

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{error::ApiError, state::AppState};

// ---------------------------------------------------------------------------
// Built-in tool registry
// ---------------------------------------------------------------------------

/// Static metadata for every built-in MCP skill.
pub struct BuiltinMcpMeta {
    pub name: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    /// JSON Schema for the tool's input parameters (OpenAI function-calling format).
    pub input_schema: &'static str,
    /// Markdown readme shown in the detail view.
    pub readme: &'static str,
}

pub const BUILTIN_MCP_TOOLS: &[BuiltinMcpMeta] = &[
    BuiltinMcpMeta {
        name: "lookup_rust_crate",
        display_name: "Rust Crate Lookup (crates.io)",
        description: "Fetches metadata for a Rust crate from crates.io: latest version, description, \
                      download count, docs link, and a ready-to-paste Cargo.toml snippet.",
        input_schema: r#"{
            "type": "object",
            "properties": {
                "crate_name": {
                    "type": "string",
                    "description": "The exact crate name to look up on crates.io."
                }
            },
            "required": ["crate_name"]
        }"#,
        readme: rust::README_LOOKUP,
    },
    BuiltinMcpMeta {
        name: "search_rust_crates",
        display_name: "Rust Crate Search (crates.io)",
        description: "Searches crates.io by keyword and returns a ranked list of matching crates \
                      with name, version, description, and download count.",
        input_schema: r#"{
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search keywords, e.g. \"async http client\"."
                },
                "per_page": {
                    "type": "number",
                    "description": "Number of results to return (1–10, default 5)."
                }
            },
            "required": ["query"]
        }"#,
        readme: rust::README_SEARCH,
    },
    BuiltinMcpMeta {
        name: "lookup_rustc_error",
        display_name: "Rustc Error Lookup",
        description: "Fetches the official Rust compiler error explanation for a given error code \
                      (e.g. E0308) from doc.rust-lang.org.",
        input_schema: r#"{
            "type": "object",
            "properties": {
                "error_code": {
                    "type": "string",
                    "description": "Rust compiler error code, e.g. E0308."
                }
            },
            "required": ["error_code"]
        }"#,
        readme: rust::README_ERROR,
    },
    BuiltinMcpMeta {
        name: "get_crate_dependencies",
        display_name: "Crate Dependencies",
        description: "Fetches the dependency list for a Rust crate version from crates.io, \
                      grouped by runtime, dev, and build dependencies.",
        input_schema: r#"{
            "type": "object",
            "properties": {
                "crate_name": {
                    "type": "string",
                    "description": "The crate name."
                },
                "version": {
                    "type": "string",
                    "description": "Specific version to inspect. Defaults to latest."
                }
            },
            "required": ["crate_name"]
        }"#,
        readme: rust::README_DEPS,
    },
    BuiltinMcpMeta {
        name: "get_crate_versions",
        display_name: "Crate Version History",
        description: "Lists all published versions of a Rust crate from crates.io, including \
                      publish date, downloads, and yanked status.",
        input_schema: r#"{
            "type": "object",
            "properties": {
                "crate_name": {
                    "type": "string",
                    "description": "The crate name."
                }
            },
            "required": ["crate_name"]
        }"#,
        readme: rust::README_VERSIONS,
    },
    BuiltinMcpMeta {
        name: "lookup_docs_rs",
        display_name: "docs.rs Lookup",
        description: "Returns the docs.rs documentation URL for a Rust crate and optionally \
                      searches for a specific item (struct, trait, fn, etc.) within it.",
        input_schema: r#"{
            "type": "object",
            "properties": {
                "crate_name": {
                    "type": "string",
                    "description": "The crate name."
                },
                "item": {
                    "type": "string",
                    "description": "Optional item to search for, e.g. \"spawn\" or \"Serialize\"."
                }
            },
            "required": ["crate_name"]
        }"#,
        readme: rust::README_DOCS,
    },
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
        readme: weather::README,
    },
];

// ---------------------------------------------------------------------------
// Wire-format types
// ---------------------------------------------------------------------------

/// Wire-format tool info returned by GET /api/mcp/tools.
#[derive(Serialize, Clone)]
pub struct McpToolInfo {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub description: String,
    pub enabled: bool,
    /// JSON Schema object (OpenAI parameters format).
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

#[derive(Deserialize)]
pub struct McpCallRequest {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Serialize)]
pub struct McpCallResponse {
    pub content: String,
    pub is_error: bool,
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch a call to a named built-in MCP skill and return its text response.
pub async fn dispatch_mcp_call(
    name: &str,
    args: &serde_json::Value,
    client: &reqwest::Client,
) -> McpCallResponse {
    match name {
        "lookup_rust_crate"      => rust::call_lookup(args, client).await,
        "search_rust_crates"     => rust::call_search(args, client).await,
        "lookup_rustc_error"     => rust::call_error(args, client).await,
        "get_crate_dependencies" => rust::call_deps(args, client).await,
        "get_crate_versions"     => rust::call_versions(args, client).await,
        "lookup_docs_rs"         => rust::call_docs(args, client).await,
        "get_current_temperature" => weather::call(args, client).await,
        other => McpCallResponse {
            content: format!("Unknown MCP tool: {other}"),
            is_error: true,
        },
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/mcp/tools  – list all built-in MCP tools with their enabled state.
pub async fn api_mcp_list_tools(State(state): State<Arc<AppState>>) -> Response {
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
pub async fn api_mcp_toggle_tool(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
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

/// GET /api/mcp/tools/:name/readme  – serve the tool's Markdown readme.
pub async fn api_mcp_tool_readme(Path(name): Path<String>) -> Response {
    let Some(meta) = BUILTIN_MCP_TOOLS.iter().find(|t| t.name == name) else {
        return (StatusCode::NOT_FOUND, "").into_response();
    };
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"))],
        meta.readme,
    )
        .into_response()
}

/// POST /api/mcp/call  – invoke a named MCP skill by the AI or the user.
pub async fn api_mcp_call(
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
