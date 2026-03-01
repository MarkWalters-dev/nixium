use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::TryStreamExt;
use serde::Deserialize;

use crate::error::ApiError;

// ---------------------------------------------------------------------------
// Request types
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
    /// Optional tool definitions forwarded to the upstream API (OpenAI function-calling format).
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    /// Optional tool_choice value forwarded to the upstream API.
    #[serde(rename = "toolChoice", default)]
    pub tool_choice: Option<serde_json::Value>,
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
