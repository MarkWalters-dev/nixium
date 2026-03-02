use std::{
    collections::{HashMap, HashSet},
    env,
    path::PathBuf,
    sync::{atomic::{AtomicBool, Ordering}, Arc},
};
use tokio::sync::{watch, RwLock};
use tracing::info;

use crate::mcp::{external, BUILTIN_MCP_TOOLS};

// ---------------------------------------------------------------------------
// Agent session – shared between the agent task and streaming clients
// ---------------------------------------------------------------------------

/// Buffered state for a single agent run.  Lives in AppState.sessions so that
/// the SSE stream can be resumed after a client reconnect without restarting
/// the AI generation.
#[derive(Debug)]
pub struct AgentSession {
    /// Every SSE line ever sent (indexed, e.g. `"id: 3\ndata: {...}\n\n"`).
    pub events: std::sync::Mutex<Vec<String>>,
    /// Set to `true` once the agent has finished (Done / Error emitted).
    pub done: AtomicBool,
    /// Set to `true` by a DELETE request – stops the agent at the next
    /// safe cancellation point.
    pub cancelled: AtomicBool,
    /// Value = number of buffered events (or `usize::MAX` when done).
    /// Clients subscribe to be notified of new events.
    pub notify: watch::Sender<usize>,
}

impl AgentSession {
    pub fn new() -> Arc<Self> {
        let (notify, _) = watch::channel(0usize);
        Arc::new(Self {
            events: std::sync::Mutex::new(Vec::new()),
            done: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            notify,
        })
    }

    /// Buffer an SSE line and wake watchers.  Returns `false` if cancelled.
    pub fn push(&self, line: String) -> bool {
        if self.cancelled.load(Ordering::Relaxed) { return false; }
        let mut events = self.events.lock().unwrap();
        let idx = events.len();
        events.push(line);
        let _ = self.notify.send_replace(idx + 1);
        true
    }

    /// Mark as done and send the sentinel value so cleanup/stream tasks wake up.
    pub fn finish(&self) {
        self.done.store(true, Ordering::Release);
        let _ = self.notify.send_replace(usize::MAX);
    }
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

/// Immutable server configuration derived from the environment at startup.
#[derive(Clone, Debug)]
pub struct AppState {
    /// When running inside nix-on-droid / Termux the `$PREFIX` env-var points
    /// to the proot root (e.g. `/data/data/com.termux/files/usr`).  Any
    /// absolute path supplied by the client is re-rooted under this prefix so
    /// file I/O lands in the correct location.
    pub prefix: Option<String>,
    /// Set of MCP tool names that are currently enabled (toggled by the UI).
    pub mcp_enabled: Arc<RwLock<HashSet<String>>>,
    /// Models that responded with "does not support tools"; we skip sending
    /// tool definitions to them for the lifetime of the server process.
    pub no_tools_models: Arc<RwLock<HashSet<String>>>,
    /// External stdio MCP server configuration and runtime cache.
    pub external_mcp: Arc<RwLock<external::ExternalMcpState>>,
    /// Directory used for all persistent nixium data (chats, etc.).
    /// Resolved from: $NIXIUM_DATA_DIR > $XDG_CONFIG_HOME/nixium > ~/.config/nixium
    pub data_dir: PathBuf,
    /// Live agent sessions keyed by session ID.  Each session buffers all SSE
    /// events so clients can resume after a network interruption.
    pub sessions: Arc<tokio::sync::Mutex<HashMap<String, Arc<AgentSession>>>>,
}

impl AppState {
    pub fn from_env() -> Self {
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

        let data_dir = if let Ok(d) = env::var("NIXIUM_DATA_DIR") {
            PathBuf::from(d)
        } else {
            let config_home = env::var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                    PathBuf::from(home).join(".config")
                });
            config_home.join("nixium")
        };
        info!("Data directory: {:?} (override via $NIXIUM_DATA_DIR)", data_dir);

        // Load persisted external MCP server configs.
        let ext_servers = external::load_servers(&data_dir);
        info!("External MCP servers loaded: {}", ext_servers.len());
        let external_mcp = Arc::new(RwLock::new(external::ExternalMcpState {
            servers: ext_servers,
            ..Default::default()
        }));

        Self {
            prefix,
            mcp_enabled,
            no_tools_models: Arc::new(RwLock::new(HashSet::new())),
            external_mcp,
            data_dir,
            sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
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
    pub fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
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
