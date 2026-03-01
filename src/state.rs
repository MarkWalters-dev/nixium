use std::{collections::HashSet, env, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tracing::info;

use crate::mcp::BUILTIN_MCP_TOOLS;

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
    /// Directory used for all persistent nixium data (chats, etc.).
    /// Resolved from: $NIXIUM_DATA_DIR > $XDG_CONFIG_HOME/nixium > ~/.config/nixium
    pub data_dir: PathBuf,
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

        Self { prefix, mcp_enabled, no_tools_models: Arc::new(RwLock::new(HashSet::new())), data_dir }
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
