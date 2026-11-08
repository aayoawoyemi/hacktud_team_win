//! CONCEPT: the agent's config, one TOML file per role (O-24). Key names are the agent rows of PRD §6.12,
//!          flat at the top level, so a config file reads like the table:
//! ```toml
//! node_id = "dell"
//! llama_bin_dir = 'C:\llama.cpp'   # optional; else rpc-server / llama-server are looked up on PATH
//! rpc_bind = "0.0.0.0"
//! rpc_port = 50052
//! rpc_cache = true
//! ```
//! WHY: O-24 proposes "TOML file per role"; §6.12 fixes the key names so WP-4 and WP-12 example configs agree.
//!      Every key has a default, so an empty file (or no file) is a valid config for a local test.
//! DEPENDS ON: serde, toml.
//! AFFECTS: supervisor (`RpcLaunch::from_config`), probe (binary paths), WP-12 R12.8 example configs.
//!
//! ASSUMPTION: unknown keys are ignored (§6.0 "readers ignore unknown fields"), so one shared file can carry
//! keys a newer agent understands. Rejected: `deny_unknown_fields` (catches typos, but breaks that rule).
//! `rpc_server_path` is NOT a §6.12 key: added here as the explicit "binary path" override (mirrors the
//! coordinator's `llama_server_path`). Keys for decided-later items (heartbeat O-26, coordinator O-04) are
//! optional and unused by this crate until the control plane lands (D-13).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// llama.cpp's own default rpc-server port (§6.12 `rpc_port`).
pub const DEFAULT_RPC_PORT: u16 = 50052;
/// llama.cpp's own default bind address. Loopback: a helper must opt in to `0.0.0.0` / a LAN address.
pub const DEFAULT_RPC_BIND: &str = "127.0.0.1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// Node identity (O-16). None = use the hostname.
    pub node_id: Option<String>,
    /// Where to register (O-04). Unused until the D-13 control plane lands.
    pub coordinator_addr: Option<String>,
    /// Folder with the pinned llama.cpp binaries. None = look them up on PATH.
    pub llama_bin_dir: Option<PathBuf>,
    /// Explicit rpc-server binary. Wins over `llama_bin_dir`.
    pub rpc_server_path: Option<PathBuf>,
    /// `-H`: address rpc-server binds to (O-05).
    pub rpc_bind: String,
    /// `-p`: rpc-server port.
    pub rpc_port: u16,
    /// `-d`: comma-separated device list. None = rpc-server default.
    pub rpc_devices: Option<String>,
    /// `-t`: CPU threads. None = rpc-server default.
    pub rpc_threads: Option<u32>,
    /// `-c`: local tensor cache (faster reloads).
    pub rpc_cache: bool,
    /// `GGML_RPC_DEBUG` (0 to 3, O-08). None = unset.
    pub rpc_debug_level: Option<u8>,
    /// `LLAMA_CACHE` for rpc-server. None = llama.cpp default.
    pub llama_cache_dir: Option<PathBuf>,
    /// Heartbeat period (O-26 = 1000 ms).
    pub heartbeat_interval_ms: u64,
    /// Where logs go (O-24). TODO(R3.4/R3.14): mirror the rpc-server ring into a file here.
    pub log_dir: Option<PathBuf>,
    /// SpacetimeDB on main (D-13), e.g. "http://192.168.8.10:3000". Key name from CONTRACTS.md "Not in the contract yet".
    pub spacetime_uri: String,
    /// Database name (O-27 = "pool"; tests use "pool-smoke").
    pub spacetime_db: String,
    /// §5.1 role: "helper" (lends its GPU via rpc-server) or "main" (the machine running llama-server).
    pub role: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            node_id: None,
            coordinator_addr: None,
            llama_bin_dir: None,
            rpc_server_path: None,
            rpc_bind: DEFAULT_RPC_BIND.to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            rpc_devices: None,
            rpc_threads: None,
            rpc_cache: false,
            rpc_debug_level: None,
            llama_cache_dir: None,
            // O-26 (Aayo 7:55pm): 1000 ms; same value as contracts::defaults::HEARTBEAT_INTERVAL_MS.
            heartbeat_interval_ms: 1000,
            log_dir: None,
            spacetime_uri: "http://127.0.0.1:3000".into(),
            spacetime_db: "pool".into(),
            role: "helper".into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading config {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parsing config {path}: {source}")]
    Toml {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl AgentConfig {
    /// Parses TOML text. Missing keys take their defaults.
    pub fn from_toml_str(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Reads and parses a config file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&text).map_err(|source| ConfigError::Toml {
            path: path.to_path_buf(),
            source,
        })
    }

    /// The rpc-server binary: `rpc_server_path`, else `llama_bin_dir/rpc-server[.exe]`, else bare name (PATH).
    pub fn rpc_server_binary(&self) -> PathBuf {
        if let Some(p) = &self.rpc_server_path {
            return p.clone();
        }
        llama_binary(self.llama_bin_dir.as_deref(), "rpc-server")
    }

    /// The llama-server binary, used only by the probe (`--version`, `--list-devices`).
    pub fn llama_server_binary(&self) -> PathBuf {
        llama_binary(self.llama_bin_dir.as_deref(), "llama-server")
    }
}

/// `dir/name.exe` on Windows, `dir/name` elsewhere; bare `name` when there is no dir (resolved on PATH).
fn llama_binary(dir: Option<&Path>, name: &str) -> PathBuf {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    match dir {
        Some(d) => d.join(file),
        None => PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_all_defaults() {
        let c = AgentConfig::from_toml_str("").unwrap();
        assert_eq!(c, AgentConfig::default());
        assert_eq!(c.rpc_port, 50052);
        assert_eq!(c.rpc_bind, "127.0.0.1");
        assert!(!c.rpc_cache);
    }

    #[test]
    fn helper_config_parses_and_ignores_unknown_keys() {
        let c = AgentConfig::from_toml_str(
            r#"
            node_id = "dell"
            rpc_bind = "0.0.0.0"
            rpc_port = 50100
            rpc_cache = true
            rpc_server_path = 'C:\llama\rpc-server.exe'
            some_future_key = 3
            "#,
        )
        .unwrap();
        assert_eq!(c.node_id.as_deref(), Some("dell"));
        assert_eq!(c.rpc_bind, "0.0.0.0");
        assert_eq!(c.rpc_port, 50100);
        assert!(c.rpc_cache);
        assert_eq!(
            c.rpc_server_binary(),
            PathBuf::from(r"C:\llama\rpc-server.exe")
        );
    }

    #[test]
    fn binary_resolution() {
        let mut c = AgentConfig::default();
        assert_eq!(c.rpc_server_binary(), PathBuf::from("rpc-server"));
        c.llama_bin_dir = Some(PathBuf::from("bin"));
        let expect =
            PathBuf::from("bin").join(format!("rpc-server{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(c.rpc_server_binary(), expect);
    }

    #[test]
    fn bad_type_is_an_error() {
        assert!(AgentConfig::from_toml_str("rpc_port = \"x\"").is_err());
    }
}
