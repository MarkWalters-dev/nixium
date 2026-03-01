use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::{stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc;
use tracing::info;

use crate::{error::ApiError, mcp, state::AppState};

const MAX_AGENT_TURNS: usize = 8;

// ---------------------------------------------------------------------------
// Legacy types – used by the raw /api/ai/chat proxy (kept for compatibility)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct OllamaModelsQuery {
    #[serde(rename = "baseUrl", default)]
    pub base_url: String,
}

#[derive(Deserialize)]
pub struct AiChatRequest {
    pub provider: String,
    #[serde(rename = "apiKey", default)]
    pub api_key: String,
    pub model: String,
    #[serde(rename = "baseUrl", default)]
    pub base_url: String,
    pub messages: Vec<serde_json::Value>,
    #[serde(rename = "systemPrompt", default)]
    pub system_prompt: String,
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    #[serde(rename = "toolChoice", default)]
    pub tool_choice: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Agent request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ContextFile {
    pub name: String,
    pub content: String,
}

/// All fields the UI posts to POST /api/ai/agent.
#[derive(Deserialize)]
pub struct AgentRequest {
    pub provider: String,
    #[serde(rename = "apiKey", default)]
    pub api_key: String,
    pub model: String,
    #[serde(rename = "baseUrl", default)]
    pub base_url: String,
    /// Full conversation history, including the latest user message.
    pub messages: Vec<serde_json::Value>,
    /// "ask" | "plan" | "agent"
    #[serde(default)]
    pub mode: String,
    /// Absolute path to the project root (for resolving relative agent paths).
    #[serde(rename = "rootPath", default)]
    pub root_path: String,
    /// Currently open file sent as context when the user enables "use context".
    #[serde(rename = "contextFile")]
    pub context_file: Option<ContextFile>,
}

// ---------------------------------------------------------------------------
// SSE events streamed back to the client
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AgentEvent {
    /// A new assistant turn is starting; push an empty assistant message in the UI.
    TurnStart,
    /// Abort the turn that was just started and retry in a different mode.
    TurnAbort,
    /// A text chunk from the model.
    Text { content: String },
    /// Replace the full content of the current assistant message (strips XML tags).
    TextSet { content: String },
    /// The model is calling a tool.
    ToolCall { id: String, name: String, args: serde_json::Value },
    /// A tool finished executing.
    ToolResult {
        id: String,
        name: String,
        content: String,
        is_error: bool,
        /// Present when a file was written; lets the UI refresh open editor tabs.
        #[serde(skip_serializing_if = "Option::is_none")]
        file_written: Option<FileWritten>,
    },
    /// An unrecoverable error occurred.
    Error { message: String },
    /// The conversation is fully complete.
    Done,
}

#[derive(Serialize)]
struct FileWritten {
    /// Absolute resolved path so the editor can match open tabs.
    path: String,
    content: String,
}

#[inline]
fn sse(event: &AgentEvent) -> Vec<u8> {
    format!("data: {}\n\n", serde_json::to_string(event).unwrap_or_default()).into_bytes()
}

// ---------------------------------------------------------------------------
// System-prompt construction
// ---------------------------------------------------------------------------

fn build_system_prompt(
    mode: &str,
    provider: &str,
    mcp_tools: &[mcp::McpToolInfo],
    context_file: Option<&ContextFile>,
) -> String {
    let is_anthropic = provider == "anthropic";
    let is_agent = mode == "agent";
    let enabled: Vec<&mcp::McpToolInfo> = mcp_tools.iter().filter(|t| t.enabled).collect();

    let mut prompt =
        "You are a helpful coding assistant built into Nixium, a local code editor. \
         Be concise and practical. Format code in markdown triple-backtick blocks."
            .to_string();

    if !enabled.is_empty() {
        let tool_lines = enabled
            .iter()
            .map(|t| format!("- {}: {}", t.name, t.description))
            .collect::<Vec<_>>()
            .join("\n");
        if !is_anthropic {
            prompt.push_str(&format!(
                "\n\nYou have access to the following tools. Use them when they would \
                 genuinely help answer the user's question — otherwise just reply normally:\n{tool_lines}"
            ));
        } else {
            prompt.push_str(&format!(
                "\n\nYou have access to the following MCP tools. Emit:\n\
                 <mcp_call name=\"TOOL_NAME\">{{\"arg\":\"value\"}}</mcp_call>\n\
                 Otherwise just reply normally. Available tools:\n{tool_lines}"
            ));
        }
    }

    if mode == "plan" {
        prompt.push_str(
            "\n\nThe user wants a PLAN: outline the approach in numbered steps before writing any code.",
        );
    }

    if is_agent {
        if is_anthropic {
            let mcp_section = if !enabled.is_empty() {
                let names = enabled.iter().map(|t| format!("- {}", t.name)).collect::<Vec<_>>().join("\n");
                format!("\n\nReminder — call MCP skills via XML too:\n{names}\n\nSyntax: <mcp_call name=\"SKILL_NAME\">{{\"arg\": \"value\"}}</mcp_call>")
            } else {
                String::new()
            };
            prompt.push_str(&format!(
                "\n\nYou are in AGENT mode. You can read, write, and list files directly.\n\n\
                 <write_file path=\"path/file.py\">\nfull contents\n</write_file>\n\n\
                 <read_file path=\"path/file.py\" />\n\n\
                 <list_directory path=\".\" />\
                 {mcp_section}\n\n\
                 RULES: Always use <write_file> to save files. Paths relative to project root. \
                 Multiple commands per response are fine."
            ));
        } else {
            prompt.push_str(
                "\n\nYou are in AGENT mode. Use the tools provided to read, write, and list files \
                 and call MCP skills. Complete tasks directly — do not just show code.",
            );
        }
    }

    if let Some(ctx) = context_file {
        let preview = &ctx.content[..ctx.content.len().min(8000)];
        prompt.push_str(&format!(
            "\n\nThe user has this file open ({}):\n```\n{preview}\n```",
            ctx.name
        ));
    }

    prompt
}

// ---------------------------------------------------------------------------
// Message-array preparation
// ---------------------------------------------------------------------------

fn build_api_messages(messages: &[serde_json::Value], xml_mode: bool) -> Vec<serde_json::Value> {
    if xml_mode {
        let mut out: Vec<serde_json::Value> = Vec::new();
        for m in messages {
            if m["role"].as_str() == Some("tool") {
                let name = m["tool_name"].as_str().unwrap_or("tool");
                let content = m["content"].as_str().unwrap_or("");
                out.push(serde_json::json!({ "role": "user", "content": format!("Tool result for {name}:\n{content}") }));
            } else if m.get("tool_calls").and_then(|v| v.as_array()).map(|a| a.is_empty()).unwrap_or(true) {
                out.push(serde_json::json!({ "role": m["role"], "content": m["content"] }));
            }
        }
        return out;
    }

    let mut native_ids = std::collections::HashSet::<String>::new();
    for m in messages {
        if let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tcs {
                if let Some(id) = tc["id"].as_str() { native_ids.insert(id.to_string()); }
            }
        }
    }

    messages.iter().map(|m| {
        let role = m["role"].as_str().unwrap_or("user");
        if role == "tool" {
            let tcid = m["tool_call_id"].as_str().unwrap_or("");
            if !tcid.is_empty() && native_ids.contains(tcid) {
                return serde_json::json!({ "role": "tool", "tool_call_id": tcid, "content": m["content"] });
            }
            let name = m["tool_name"].as_str().unwrap_or("tool");
            let content = m["content"].as_str().unwrap_or("");
            return serde_json::json!({ "role": "user", "content": format!("Tool result for {name}:\n{content}") });
        }
        if let Some(tcs) = m.get("tool_calls") {
            return serde_json::json!({ "role": "assistant", "content": m.get("content").cloned().unwrap_or(serde_json::Value::Null), "tool_calls": tcs });
        }
        serde_json::json!({ "role": role, "content": m["content"] })
    }).collect()
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

fn agent_file_tools() -> serde_json::Value {
    serde_json::json!([
        { "type": "function", "function": { "name": "write_file", "description": "Write (create or overwrite) a file on disk.",
            "parameters": { "type": "object", "required": ["path","content"], "properties": {
                "path": { "type": "string", "description": "Relative to project root or absolute." },
                "content": { "type": "string", "description": "Full text content to write." } } } } },
        { "type": "function", "function": { "name": "read_file", "description": "Read the text content of a file.",
            "parameters": { "type": "object", "required": ["path"], "properties": { "path": { "type": "string" } } } } },
        { "type": "function", "function": { "name": "list_directory", "description": "List files and directories.",
            "parameters": { "type": "object", "required": ["path"], "properties": { "path": { "type": "string" } } } } },
    ])
}

fn build_tools_array(include_file_tools: bool, mcp_tools: &[mcp::McpToolInfo]) -> serde_json::Value {
    let mut tools: Vec<serde_json::Value> = Vec::new();
    if include_file_tools {
        if let Some(arr) = agent_file_tools().as_array() { tools.extend(arr.iter().cloned()); }
    }
    for t in mcp_tools.iter().filter(|t| t.enabled) {
        tools.push(serde_json::json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.input_schema } }));
    }
    serde_json::Value::Array(tools)
}

// ---------------------------------------------------------------------------
// XML command parsing (Anthropic / Ollama fallback protocol)
// ---------------------------------------------------------------------------

struct XmlCmd {
    name: String,
    path: String,
    body: String,
    mcp_name: Option<String>,
    mcp_args: Option<String>,
}

fn get_attr(tag_text: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = tag_text.find(&needle)? + needle.len();
    let end = start + tag_text[start..].find('"')?;
    Some(tag_text[start..end].to_string())
}

fn parse_xml_commands(text: &str) -> Vec<XmlCmd> {
    let mut cmds: Vec<XmlCmd> = Vec::new();
    let mut pos = 0;
    while pos < text.len() {
        let Some(lt) = text[pos..].find('<') else { break };
        let tag_start = pos + lt;
        let inner = &text[tag_start + 1..];

        if inner.starts_with("write_file ") || inner.starts_with("write_file\n") {
            if let Some(path) = get_attr(inner, "path") {
                if let Some(gt) = inner.find('>') {
                    let body_start = tag_start + 1 + gt + 1;
                    if let Some(cp) = text[body_start..].find("</write_file>") {
                        cmds.push(XmlCmd { name: "write_file".into(), path, body: text[body_start..body_start+cp].to_string(), mcp_name: None, mcp_args: None });
                        pos = body_start + cp + "</write_file>".len();
                        continue;
                    }
                }
            }
        } else if inner.starts_with("read_file ") {
            if let Some(path) = get_attr(inner, "path") {
                let off = inner.find("/>").map(|i| i+2).or_else(|| inner.find('>').map(|i| i+1)).unwrap_or(2);
                cmds.push(XmlCmd { name: "read_file".into(), path, body: String::new(), mcp_name: None, mcp_args: None });
                pos = tag_start + 1 + off;
                continue;
            }
        } else if inner.starts_with("list_directory ") {
            if let Some(path) = get_attr(inner, "path") {
                let off = inner.find("/>").map(|i| i+2).or_else(|| inner.find('>').map(|i| i+1)).unwrap_or(2);
                cmds.push(XmlCmd { name: "list_directory".into(), path, body: String::new(), mcp_name: None, mcp_args: None });
                pos = tag_start + 1 + off;
                continue;
            }
        } else if inner.starts_with("mcp_call ") {
            if let Some(mcp_name) = get_attr(inner, "name") {
                if let Some(gt) = inner.find('>') {
                    let body_start = tag_start + 1 + gt + 1;
                    if let Some(cp) = text[body_start..].find("</mcp_call>") {
                        cmds.push(XmlCmd { name: "mcp_call".into(), path: String::new(), body: String::new(), mcp_name: Some(mcp_name), mcp_args: Some(text[body_start..body_start+cp].trim().to_string()) });
                        pos = body_start + cp + "</mcp_call>".len();
                        continue;
                    }
                }
            }
        }
        pos = tag_start + 1;
    }
    cmds
}

fn strip_xml_commands(text: &str) -> String {
    let mut s = text.to_string();
    for (open, close) in &[("<write_file ", "</write_file>"), ("<mcp_call ", "</mcp_call>")] {
        loop {
            let Some(start) = s.find(open) else { break };
            let Some(cp) = s[start..].find(close) else { break };
            s.drain(start..start + cp + close.len());
        }
    }
    for open in &["<read_file ", "<list_directory "] {
        loop {
            let Some(start) = s.find(open) else { break };
            let rest = &s[start..].to_string();
            let len = rest.find("/>").map(|i| i+2).or_else(|| rest.find('>').map(|i| i+1));
            let Some(len) = len else { break };
            s.drain(start..start + len);
        }
    }
    s.trim().to_string()
}

// ---------------------------------------------------------------------------
// Tool execution
// ---------------------------------------------------------------------------

struct ToolOutput {
    content: String,
    is_error: bool,
    file_written: Option<FileWritten>,
}

async fn exec_tool(
    name: &str,
    args: &serde_json::Value,
    root_path: &str,
    state: &Arc<AppState>,
    client: &reqwest::Client,
) -> ToolOutput {
    fn resolve(path: &str, root: &str) -> String {
        if path.starts_with('/') { path.to_string() }
        else { format!("{}/{}", root.trim_end_matches('/'), path) }
    }

    match name {
        "write_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            let abs = resolve(path, root_path);
            let resolved = match state.resolve(&abs) {
                Ok(p) => p,
                Err(e) => return ToolOutput { content: format!("Path error: {e}"), is_error: true, file_written: None },
            };
            if let Some(parent) = resolved.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return ToolOutput { content: format!("Error creating dirs: {e}"), is_error: true, file_written: None };
                }
            }
            match std::fs::write(&resolved, content) {
                Ok(_) => ToolOutput { content: format!("Wrote {path}"), is_error: false, file_written: Some(FileWritten { path: abs, content: content.to_string() }) },
                Err(e) => ToolOutput { content: format!("Error writing {path}: {e}"), is_error: true, file_written: None },
            }
        }
        "read_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let resolved = match state.resolve(&resolve(path, root_path)) {
                Ok(p) => p,
                Err(e) => return ToolOutput { content: format!("Path error: {e}"), is_error: true, file_written: None },
            };
            match std::fs::read_to_string(&resolved) {
                Ok(c) => ToolOutput { content: c, is_error: false, file_written: None },
                Err(e) => ToolOutput { content: format!("Error reading {path}: {e}"), is_error: true, file_written: None },
            }
        }
        "list_directory" => {
            let path = args["path"].as_str().unwrap_or(".");
            let resolved = match state.resolve(&resolve(path, root_path)) {
                Ok(p) => p,
                Err(e) => return ToolOutput { content: format!("Path error: {e}"), is_error: true, file_written: None },
            };
            match std::fs::read_dir(&resolved) {
                Ok(rd) => {
                    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
                    entries.sort_by(|a, b| {
                        let ad = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        let bd = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        bd.cmp(&ad).then(a.file_name().cmp(&b.file_name()))
                    });
                    let lines: Vec<String> = entries.iter().map(|e| {
                        let d = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        format!("{} {}", if d { "d" } else { "f" }, e.file_name().to_string_lossy())
                    }).collect();
                    ToolOutput { content: if lines.is_empty() { "(empty)".into() } else { lines.join("\n") }, is_error: false, file_written: None }
                }
                Err(e) => ToolOutput { content: format!("Error listing {path}: {e}"), is_error: true, file_written: None },
            }
        }
        other => {
            // Try built-in tools first.
            if mcp::BUILTIN_MCP_TOOLS.iter().any(|t| t.name == other) {
                let r = mcp::dispatch_mcp_call(other, args, client).await;
                return ToolOutput { content: r.content, is_error: r.is_error, file_written: None };
            }
            // Try external stdio MCP servers.
            let ext_server: Option<mcp::external::ExternalMcpServer> = {
                let ext = state.external_mcp.read().await;
                if let Some(srv_id) = ext.tool_index.get(other).cloned() {
                    if ext.is_tool_enabled(&srv_id, other) {
                        ext.servers.iter().find(|s| s.id == srv_id && s.enabled).cloned()
                    } else { None }
                } else { None }
            };
            if let Some(srv) = ext_server {
                let r = mcp::external::call_tool(&srv, other, args).await;
                return ToolOutput { content: r.content, is_error: r.is_error, file_written: None };
            }
            ToolOutput { content: format!("Unknown MCP tool: {other}"), is_error: true, file_written: None }
        }
    }
}

// ---------------------------------------------------------------------------
// OpenAI-compatible single turn – streams text tokens, collects tool calls
// ---------------------------------------------------------------------------

struct TurnOutput {
    text: String,
    tool_calls: Vec<PendingCall>,
    function_confusion: bool,
    tools_not_supported: bool,
    error: Option<String>,
}

struct PendingCall { id: String, name: String, arguments: String }

async fn do_openai_turn(
    client: &reqwest::Client,
    provider: &str,
    api_key: &str,
    model: &str,
    base_url: &str,
    system_prompt: &str,
    messages: Vec<serde_json::Value>,
    tools: Option<serde_json::Value>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
) -> TurnOutput {
    let base = if !base_url.is_empty() { base_url.to_string() }
               else if provider == "ollama" { "http://localhost:11434".to_string() }
               else { "https://api.openai.com".to_string() };

    let mut all_msgs: Vec<serde_json::Value> = Vec::new();
    if !system_prompt.is_empty() {
        all_msgs.push(serde_json::json!({ "role": "system", "content": system_prompt }));
    }
    all_msgs.extend(messages);

    let mut body = serde_json::json!({ "model": model, "stream": true, "messages": all_msgs });
    if let Some(ref t) = tools {
        body["tools"] = t.clone();
        body["tool_choice"] = serde_json::json!("auto");
    }

    let mut req = client.post(format!("{base}/v1/chat/completions")).json(&body);
    if !api_key.is_empty() { req = req.header("authorization", format!("Bearer {api_key}")); }

    let upstream = match req.send().await {
        Err(e) => return TurnOutput { text: String::new(), tool_calls: vec![], function_confusion: false, tools_not_supported: false, error: Some(format!("Cannot reach {base}: {e}")) },
        Ok(r) => r,
    };

    if !upstream.status().is_success() {
        let status = upstream.status().as_u16();
        let body_text = upstream.text().await.unwrap_or_default();
        info!("AI upstream HTTP {status}: {:.200}", body_text);
        let tns = tools.is_some() && body_text.to_lowercase().contains("does not support tools");
        return TurnOutput { text: String::new(), tool_calls: vec![], function_confusion: false, tools_not_supported: tns, error: Some(format_upstream_error(&body_text, status)) };
    }

    let mut byte_stream = upstream.bytes_stream();
    let mut buf = String::new();
    let mut full_text = String::new();
    let mut pending: HashMap<usize, PendingCall> = HashMap::new();
    let mut has_tool_calls_finish = false;

    loop {
        let chunk = match byte_stream.try_next().await {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => return TurnOutput { text: full_text, tool_calls: vec![], function_confusion: false, tools_not_supported: false, error: Some(e.to_string()) },
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(nl) = buf.find('\n') {
            let line = buf[..nl].trim().to_string();
            buf = buf[nl + 1..].to_string();
            if !line.starts_with("data: ") { continue; }
            let data = &line[6..];
            if data == "[DONE]" { continue; }
            let Ok(p) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            let Some(choice) = p["choices"].as_array().and_then(|a| a.first()).cloned() else { continue };
            let delta = &choice["delta"];
            if let Some(chunk_text) = delta["content"].as_str() {
                if !chunk_text.is_empty() {
                    full_text.push_str(chunk_text);
                    let _ = tx.send(sse(&AgentEvent::Text { content: chunk_text.to_string() }));
                }
            }
            if let Some(tcs) = delta["tool_calls"].as_array() {
                for tc in tcs {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    let e = pending.entry(idx).or_insert(PendingCall { id: String::new(), name: String::new(), arguments: String::new() });
                    if let Some(id) = tc["id"].as_str() { e.id.push_str(id); }
                    if let Some(nm) = tc["function"]["name"].as_str() { e.name.push_str(nm); }
                    if let Some(args) = tc["function"]["arguments"].as_str() { e.arguments.push_str(args); }
                }
            }
            if choice["finish_reason"].as_str() == Some("tool_calls") { has_tool_calls_finish = true; }
        }
    }

    let mut sorted: Vec<(usize, PendingCall)> = pending.into_iter().collect();
    sorted.sort_by_key(|(i, _)| *i);
    let tool_calls: Vec<PendingCall> = sorted.into_iter().map(|(_, tc)| tc).collect();

    let function_confusion = tools.is_some()
        && !has_tool_calls_finish
        && tool_calls.is_empty()
        && (full_text.contains("\"name\"") && full_text.contains("\"parameters\"")
            || full_text.to_lowercase().contains("function call"));

    TurnOutput { text: full_text, tool_calls, function_confusion, tools_not_supported: false, error: None }
}

fn format_upstream_error(body: &str, status: u16) -> String {
    if body.is_empty() { return format!("HTTP {status}"); }
    if let Ok(outer) = serde_json::from_str::<serde_json::Value>(body) {
        let err = &outer["error"];
        if let Some(s) = err.as_str() {
            if let Ok(inner) = serde_json::from_str::<serde_json::Value>(s) {
                if let Some(msg) = inner["error"]["message"].as_str() { return msg.to_string(); }
            }
            return s.to_string();
        }
        if let Some(msg) = err["message"].as_str() { return msg.to_string(); }
    }
    body.to_string()
}

// ---------------------------------------------------------------------------
// Anthropic single turn – XML protocol only (streams text, no native tools)
// ---------------------------------------------------------------------------

async fn do_anthropic_turn(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    base_url: &str,
    system_prompt: &str,
    messages: Vec<serde_json::Value>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
) -> TurnOutput {
    let base = if base_url.is_empty() { "https://api.anthropic.com" } else { base_url };
    let body = serde_json::json!({ "model": model, "max_tokens": 8096, "stream": true, "system": system_prompt, "messages": messages });

    let upstream = match client.post(format!("{base}/v1/messages")).json(&body)
        .header("x-api-key", api_key).header("anthropic-version", "2023-06-01").send().await
    {
        Err(e) => return TurnOutput { text: String::new(), tool_calls: vec![], function_confusion: false, tools_not_supported: false, error: Some(format!("Cannot reach Anthropic: {e}")) },
        Ok(r) => r,
    };

    if !upstream.status().is_success() {
        let status = upstream.status().as_u16();
        let body_text = upstream.text().await.unwrap_or_default();
        return TurnOutput { text: String::new(), tool_calls: vec![], function_confusion: false, tools_not_supported: false, error: Some(format_upstream_error(&body_text, status)) };
    }

    let mut byte_stream = upstream.bytes_stream();
    let mut buf = String::new();
    let mut full_text = String::new();

    loop {
        let chunk = match byte_stream.try_next().await { Ok(Some(b)) => b, _ => break };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(nl) = buf.find('\n') {
            let line = buf[..nl].trim().to_string();
            buf = buf[nl + 1..].to_string();
            if !line.starts_with("data: ") { continue; }
            let Ok(p) = serde_json::from_str::<serde_json::Value>(&line[6..]) else { continue };
            if p["type"] == "content_block_delta" && p["delta"]["type"] == "text_delta" {
                if let Some(text) = p["delta"]["text"].as_str() {
                    if !text.is_empty() {
                        full_text.push_str(text);
                        let _ = tx.send(sse(&AgentEvent::Text { content: text.to_string() }));
                    }
                }
            }
        }
    }

    TurnOutput { text: full_text, tool_calls: vec![], function_confusion: false, tools_not_supported: false, error: None }
}

// ---------------------------------------------------------------------------
// The agentic loop (runs in a spawned task, sends SSE events over the channel)
// ---------------------------------------------------------------------------

async fn run_agent_loop(
    req: AgentRequest,
    state: Arc<AppState>,
    client: reqwest::Client,
    tx: mpsc::UnboundedSender<Vec<u8>>,
) {
    let is_anthropic = req.provider == "anthropic";
    let is_agent = req.mode == "agent";

    // Kick off external-server tool caching in the background so it never blocks
    // the agent turn.  Tools will be available from the *next* request onward if
    // the cache was cold; that is acceptable for first-use latency.
    {
        let state_for_cache = state.clone();
        tokio::spawn(async move { mcp::external::ensure_cache(&state_for_cache).await; });
    }

    let mcp_tools: Vec<mcp::McpToolInfo> = {
        let enabled = state.mcp_enabled.read().await;
        let mut tools: Vec<mcp::McpToolInfo> = mcp::BUILTIN_MCP_TOOLS.iter().map(|t| {
            let schema = serde_json::from_str(t.input_schema).unwrap_or(serde_json::json!({}));
            mcp::McpToolInfo {
                name: t.name.to_string(), display_name: t.display_name.to_string(),
                description: t.description.to_string(), enabled: enabled.contains(t.name),
                input_schema: schema,
            }
        }).collect();
        drop(enabled);
        // Append enabled external tools (already cached by ensure_cache above).
        let ext = state.external_mcp.read().await;
        tools.extend(mcp::external::get_enabled_tools_for_agent(&ext));
        tools
    };

    let system_prompt = build_system_prompt(&req.mode, &req.provider, &mcp_tools, req.context_file.as_ref());

    let mut history: Vec<serde_json::Value> = req.messages.clone();

    // If we already learned this model doesn't support tools, skip native tools from the start.
    let model_known_no_tools = state.no_tools_models.read().await.contains(&req.model);
    let mut force_xml = model_known_no_tools;
    if model_known_no_tools {
        info!("AI skipping tools for {} (known unsupported)", req.model);
    }

    for depth in 0..MAX_AGENT_TURNS {
        let use_native = !force_xml && !is_anthropic && (is_agent || mcp_tools.iter().any(|t| t.enabled));
        let xml_msgs = !use_native;

        // Exit immediately if the client has disconnected (stop button / navigation).
        if tx.send(sse(&AgentEvent::TurnStart)).is_err() { return; }

        let api_msgs = build_api_messages(&history, xml_msgs);

        let turn = if is_anthropic {
            do_anthropic_turn(&client, &req.api_key, &req.model, &req.base_url, &system_prompt, api_msgs, &tx).await
        } else {
            let tools = use_native.then(|| build_tools_array(is_agent, &mcp_tools));
            do_openai_turn(&client, &req.provider, &req.api_key, &req.model, &req.base_url, &system_prompt, api_msgs, tools, &tx).await
        };

        // Recoverable: retry in XML mode
        if turn.tools_not_supported || turn.function_confusion {
            let _ = tx.send(sse(&AgentEvent::TurnAbort));
            force_xml = true;
            if turn.tools_not_supported {
                // Persist so future requests to this model skip tools immediately.
                info!("AI marking {} as no-tools model", req.model);
                state.no_tools_models.write().await.insert(req.model.clone());
            }
            continue;
        }

        if let Some(err) = turn.error {
            let _ = tx.send(sse(&AgentEvent::Error { message: err }));
            return;
        }

        // ── Native tool calls ────────────────────────────────────────────────
        if !turn.tool_calls.is_empty() {
            let calls_json: Vec<serde_json::Value> = turn.tool_calls.iter().map(|tc| serde_json::json!({
                "id": if tc.id.is_empty() { small_id() } else { tc.id.clone() },
                "type": "function",
                "function": { "name": tc.name, "arguments": tc.arguments },
            })).collect();

            history.push(serde_json::json!({ "role": "assistant", "content": turn.text, "tool_calls": calls_json }));

            for tc_json in &calls_json {
                let id   = tc_json["id"].as_str().unwrap_or("").to_string();
                let name = tc_json["function"]["name"].as_str().unwrap_or("").to_string();
                let args: serde_json::Value = serde_json::from_str(
                    tc_json["function"]["arguments"].as_str().unwrap_or("{}")
                ).unwrap_or(serde_json::json!({}));

                // Skip expensive tool execution if the client has already disconnected.
                if tx.send(sse(&AgentEvent::ToolCall { id: id.clone(), name: name.clone(), args: args.clone() })).is_err() { return; }
                let out = exec_tool(&name, &args, &req.root_path, &state, &client).await;
                if tx.send(sse(&AgentEvent::ToolResult { id: id.clone(), name: name.clone(), content: out.content.clone(), is_error: out.is_error, file_written: out.file_written })).is_err() { return; }
                history.push(serde_json::json!({ "role": "tool", "tool_call_id": id, "content": out.content }));
            }
            continue; // next AI turn to incorporate tool results
        }

        // ── XML command protocol (Anthropic or force_xml) ────────────────────
        if xml_msgs || is_anthropic {
            let cmds: Vec<XmlCmd> = if is_agent {
                parse_xml_commands(&turn.text)
            } else {
                parse_xml_commands(&turn.text).into_iter().filter(|c| c.name == "mcp_call").collect()
            };

            if !cmds.is_empty() {
                let clean = strip_xml_commands(&turn.text);
                let _ = tx.send(sse(&AgentEvent::TextSet { content: clean.clone() }));
                history.push(serde_json::json!({ "role": "assistant", "content": clean }));

                let mut needs_followup = false;
                for cmd in cmds {
                    let id = format!("xml_{}", small_id());
                    let (name, args) = if cmd.name == "mcp_call" {
                        let n = cmd.mcp_name.as_deref().unwrap_or("").to_string();
                        let a: serde_json::Value = serde_json::from_str(cmd.mcp_args.as_deref().unwrap_or("{}")).unwrap_or(serde_json::json!({}));
                        (n, a)
                    } else {
                        (cmd.name.clone(), serde_json::json!({ "path": cmd.path, "content": cmd.body }))
                    };

                    if tx.send(sse(&AgentEvent::ToolCall { id: id.clone(), name: name.clone(), args: args.clone() })).is_err() { return; }
                    let out = exec_tool(&name, &args, &req.root_path, &state, &client).await;
                    if !out.is_error { needs_followup = true; }
                    if tx.send(sse(&AgentEvent::ToolResult { id: id.clone(), name: name.clone(), content: out.content.clone(), is_error: out.is_error, file_written: out.file_written })).is_err() { return; }
                    history.push(serde_json::json!({ "role": "tool", "tool_call_id": id, "content": out.content }));
                }

                if needs_followup && depth + 1 < MAX_AGENT_TURNS { continue; }
            } else {
                history.push(serde_json::json!({ "role": "assistant", "content": turn.text }));
            }
        } else {
            history.push(serde_json::json!({ "role": "assistant", "content": turn.text }));
        }

        break;
    }

    let _ = tx.send(sse(&AgentEvent::Done));
}

fn small_id() -> String {
    use std::time::UNIX_EPOCH;
    let ns = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos();
    format!("{ns:08x}")
}

// ---------------------------------------------------------------------------
// POST /api/ai/agent  – new agentic endpoint
// ---------------------------------------------------------------------------

pub async fn api_ai_agent(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentRequest>,
) -> Response {
    info!(
        "AI AGENT provider={} model={} mode={} messages={}",
        req.provider, req.model, req.mode, req.messages.len()
    );

    let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build() {
        Ok(c) => c,
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    tokio::spawn(run_agent_loop(req, state, client, tx));

    let s = stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|bytes| (Ok::<Vec<u8>, std::io::Error>(bytes), rx))
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(Body::from_stream(s))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/ai/ollama-models?baseUrl=  – list models from a running Ollama instance
pub async fn api_ollama_models(
    axum::extract::Query(q): axum::extract::Query<OllamaModelsQuery>,
) -> Response {
    let base = if q.base_url.is_empty() {
        "http://localhost:11434".to_string()
    } else {
        q.base_url
    };
    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let res = match client.get(format!("{}/api/tags", base)).send().await {
        Ok(r) => r,
        Err(e) => return ApiError::response(StatusCode::BAD_GATEWAY, e.to_string()),
    };
    if !res.status().is_success() {
        return ApiError::response(
            StatusCode::BAD_GATEWAY,
            format!("Ollama returned {}", res.status()),
        );
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

/// POST /api/ai/chat  – proxy to an AI provider and stream the SSE response
pub async fn api_ai_chat(Json(req): Json<AiChatRequest>) -> Response {
    info!(
        "AI CHAT provider={} model={} messages={} tools={}",
        req.provider,
        req.model,
        req.messages.len(),
        req.tools.is_some(),
    );
    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let (url, body) = match req.provider.as_str() {
        "anthropic" => {
            let base = if req.base_url.is_empty() {
                "https://api.anthropic.com"
            } else {
                &req.base_url
            };
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

    let mut builder = client
        .post(&url)
        .json(&body)
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
        Err(e) => {
            return ApiError::response(
                StatusCode::BAD_GATEWAY,
                format!("Cannot reach {} — {}", url, e),
            )
        }
        Ok(r) => r,
    };

    if !upstream.status().is_success() {
        let status = upstream.status().as_u16();
        let text = upstream.text().await.unwrap_or_default();
        info!("AI CHAT upstream error HTTP {}: {:.200}", status, text);
        let msg = if text.is_empty() {
            format!("Upstream {} returned HTTP {}", url, status)
        } else {
            text
        };
        return ApiError::response(
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            msg,
        );
    }
    info!("AI CHAT streaming from {}", url);

    let stream = upstream
        .bytes_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
        .header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"))
        .body(Body::from_stream(stream))
        .unwrap()
}
