use std::{env, io::{Read, Write}, sync::Arc};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::warn;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct TerminalQuery {
    pub cwd: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/terminal/ws — upgrades to a WebSocket that bridges a PTY shell.
///
/// Message protocol (from client):
///   - Text starting with `\x00resize:COLS:ROWS` — resize the PTY
///   - Any other text — raw input forwarded to the shell's stdin
///
/// Message protocol (to client):
///   - Binary — raw PTY stdout/stderr bytes (consumed directly by xterm.js)
pub async fn terminal_ws(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TerminalQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_terminal_socket(socket, state, params.cwd))
        .into_response()
}

pub async fn handle_terminal_socket(
    socket: WebSocket,
    state: Arc<AppState>,
    requested_cwd: Option<String>,
) {
    // Determine the shell binary.
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());

    // Default working directory: requested cwd > $HOME > prefix root > /.
    let cwd = requested_cwd
        .and_then(|p| state.resolve(&p).ok())
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| std::path::Path::new(p).is_dir())
        .unwrap_or_else(|| {
            env::var("HOME").unwrap_or_else(|_| {
                state.prefix.clone().unwrap_or_else(|| "/".into())
            })
        });

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
        Err(e) => {
            warn!("PTY try_clone_reader: {}", e);
            return;
        }
    };
    let mut pty_writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            warn!("PTY take_writer: {}", e);
            return;
        }
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
