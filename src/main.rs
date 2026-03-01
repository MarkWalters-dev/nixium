//! Single-binary self-hosted code editor.
//!
//! Architecture:
//!   - The SvelteKit SPA is embedded at compile time via rust-embed from
//!     `frontend/build/`.
//!   - Axum serves the SPA and REST + WebSocket endpoints:
//!       GET  /api/fs/read?path=<abs-path>      – read a file
//!       POST /api/fs/write  { path, content }  – write a file
//!       GET  /api/fs/list?path=<abs-path>      – list a directory
//!       GET  /api/terminal/ws                  – WebSocket PTY terminal
//!   - Path resolution honours the `$PREFIX` environment variable so that
//!     absolute paths are correctly rooted inside a nix-on-droid / Termux
//!     proot environment.

mod ai;
mod assets;
mod chats;
mod error;
mod extensions;
mod fs;
mod mcp;
mod state;
mod store;
mod terminal;

use std::{env, sync::Arc};

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use assets::static_handler;
use state::AppState;

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
        // ── File system ──────────────────────────────────────────────────
        .route("/fs/read",   get(fs::api_read))
        .route("/fs/write",  post(fs::api_write))
        .route("/fs/list",   get(fs::api_list))
        .route("/fs/search", get(fs::api_search))
        // ── Installed extensions ─────────────────────────────────────────
        .route("/extensions",               get(extensions::api_extensions_list))
        .route("/extensions/:name",         axum::routing::delete(extensions::api_extension_delete))
        .route("/extensions/:name/readme",  get(extensions::api_extension_readme))
        .route("/extensions/:name/script",  get(extensions::api_extension_script))
        // ── Extension store ──────────────────────────────────────────────
        .route("/extensions/store/search",  get(store::api_ext_store_search))
        .route("/extensions/store/install", post(store::api_ext_store_install))
        // ── Terminal ─────────────────────────────────────────────────────
        .route("/terminal/ws", get(terminal::terminal_ws))
        // ── AI proxy ─────────────────────────────────────────────────────
        .route("/ai/agent",         post(ai::api_ai_agent))
        .route("/ai/chat",          post(ai::api_ai_chat))
        .route("/ai/ollama-models", get(ai::api_ollama_models))
        // ── Chat persistence ─────────────────────────────────────────────
        .route("/chats",     get(chats::api_chats_load))
        .route("/chats",     post(chats::api_chats_save))
        .route("/chats/:id", axum::routing::delete(chats::api_chats_delete))
        // ── MCP skills ───────────────────────────────────────────────────
        .route("/mcp/tools",                                      get(mcp::api_mcp_list_tools))
        .route("/mcp/tools/:name/toggle",                         post(mcp::api_mcp_toggle_tool))
        .route("/mcp/tools/:name/readme",                         get(mcp::api_mcp_tool_readme))
        .route("/mcp/call",                                       post(mcp::api_mcp_call))
        // ── External stdio MCP servers ───────────────────────────────────────
        .route("/mcp/external",                                   get(mcp::external::api_list_servers).post(mcp::external::api_add_server))
        .route("/mcp/external/:id",                               axum::routing::delete(mcp::external::api_delete_server))
        .route("/mcp/external/:id/toggle",                        post(mcp::external::api_toggle_server))
        .route("/mcp/external/:id/tools",                         get(mcp::external::api_list_server_tools))
        .route("/mcp/external/:id/tools/:tool/toggle",            post(mcp::external::api_toggle_server_tool));

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
    info!(
        "Open your browser at http://localhost:{}",
        listener.local_addr().unwrap().port()
    );

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
