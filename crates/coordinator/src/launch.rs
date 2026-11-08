//! CONCEPT: start, watch and stop llama-server on the main Mac (WP-4 R4.6, R4.7, R4.9). Turns a LaunchSpec into
//!          llama-server's command line, refuses to launch when a helper's rpc-server is unreachable or the port
//!          is taken, captures the load log to a file, and polls /health until the model is loaded.
//! WHY: O-01 option A (Rust supervises the official llama.cpp binaries as child processes). The probe before launch
//!      exists because llama-server aborts outright when an --rpc address is unreachable (ggml-rpc.cpp:653, from
//!      pi-research in #11), so a dead helper must be caught here with a readable error, not as a crash.
//! DEPENDS ON: PRD §6.6 LaunchSpec field names (mirrored locally until WP-2's contracts crate lands, PR #18);
//!             llama-server flags of build 8665 (O-14); §6.11 error codes.
//! AFFECTS: WP-9 recovery (relaunch uses start/stop), WP-7 status (reads the log file and readiness), WP-8 bench
//!          (needs the endpoint up). If a flag mapping here changes, the §6.6 table in the PRD must change too.
//!
//! Rejected: tokio + reqwest. Everything here is one child process and one localhost GET, which std does in a
//! few lines; an async runtime can come with the SpacetimeDB client when the coordinator needs one.

use std::fs::File;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

// CONCEPT: local mirror of PRD §6.6 LaunchSpec, same field names.
// WHY: WP-2's shared type is still in review (PR #18, being reworked for D-13). Same names means swapping to
//      `contracts::LaunchSpec` later is a one-line import change.
// shortcut: `speculative` (WP-S1 stretch) is left out; add it with the contracts type if O-15 is ever decided.
#[derive(Debug, Clone, Default)]
pub struct LaunchSpec {
    pub plan_id: String,
    pub llama_server_path: String,
    pub model_path: String,
    /// Helper rpc-server addresses, `host:port`, in the order llama-server will number them (RPC0, RPC1, ...).
    pub rpc: Vec<String>,
    pub n_gpu_layers: Option<String>,
    pub split_mode: Option<String>,
    /// `-ts` shares. llama.cpp reads these as shares of the LAYER COUNT, not bytes, RPC devices first and main's
    /// own device last (llama-model.cpp:1733-1789). The planner should pass integer layer counts.
    pub tensor_split: Option<Vec<f64>>,
    pub devices: Option<Vec<String>>,
    pub ctx_size: Option<u32>,
    pub parallel: Option<u32>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub metrics: bool,
    pub flash_attn: Option<String>,
    pub extra_args: Vec<String>,
    pub env: Vec<(String, String)>,
}

// CONCEPT: the §6.11 codes this module can raise. `code()` gives the wire string for events and status.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("E_RPC_UNREACHABLE: helper rpc-server {0} is not accepting connections")]
    RpcUnreachable(String),
    #[error("E_PORT_IN_USE: {0} is already taken (leftover llama-server?)")]
    PortInUse(String),
    #[error("E_LAUNCH_FAILED: {0}")]
    LaunchFailed(String),
    #[error("E_NOT_READY_TIMEOUT: llama-server not ready after {0:?}")]
    NotReadyTimeout(Duration),
}

impl LaunchError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::RpcUnreachable(_) => "E_RPC_UNREACHABLE",
            Self::PortInUse(_) => "E_PORT_IN_USE",
            Self::LaunchFailed(_) => "E_LAUNCH_FAILED",
            Self::NotReadyTimeout(_) => "E_NOT_READY_TIMEOUT",
        }
    }
}

const DEFAULT_PORT: u16 = 8080; // llama-server's own default
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// R4.6: the llama-server arguments for a spec, following the §6.6 mapping table. `None` = flag not passed,
/// so llama.cpp's own default applies.
pub fn args(spec: &LaunchSpec) -> Vec<String> {
    let mut a = vec!["-m".to_string(), spec.model_path.clone()];
    let mut flag = |f: &str, v: String| {
        a.push(f.to_string());
        a.push(v);
    };
    if !spec.rpc.is_empty() {
        flag("--rpc", spec.rpc.join(","));
    }
    if let Some(v) = &spec.n_gpu_layers {
        flag("-ngl", v.clone());
    }
    if let Some(v) = &spec.split_mode {
        flag("-sm", v.clone());
    }
    if let Some(v) = &spec.tensor_split {
        // f64 Display prints 12.0 as "12", so integer layer counts stay integers on the command line.
        flag(
            "-ts",
            v.iter().map(f64::to_string).collect::<Vec<_>>().join(","),
        );
    }
    if let Some(v) = &spec.devices {
        flag("-dev", v.join(","));
    }
    // Careful: on llama-server `-c` is context size; on rpc-server `-c` means cache (PRD §4.3).
    if let Some(v) = spec.ctx_size {
        flag("-c", v.to_string());
    }
    if let Some(v) = spec.parallel {
        flag("-np", v.to_string());
    }
    if let Some(v) = &spec.host {
        flag("--host", v.clone());
    }
    if let Some(v) = spec.port {
        flag("--port", v.to_string());
    }
    if let Some(v) = &spec.flash_attn {
        flag("-fa", v.clone());
    }
    if spec.metrics {
        a.push("--metrics".to_string());
    }
    a.extend(spec.extra_args.iter().cloned());
    a
}

/// True when something accepts TCP connections at `addr` within the probe timeout.
pub fn accepting(addr: &str) -> bool {
    let Ok(addrs) = addr.to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|a: SocketAddr| TcpStream::connect_timeout(&a, PROBE_TIMEOUT).is_ok())
}

/// Where we reach llama-server ourselves: its bind address, except a wildcard bind is reached via loopback.
fn local_addr(spec: &LaunchSpec) -> String {
    let host = match spec.host.as_deref() {
        None | Some("0.0.0.0") | Some("::") => "127.0.0.1",
        Some(h) => h,
    };
    format!("{host}:{}", spec.port.unwrap_or(DEFAULT_PORT))
}

/// A running llama-server. Dropping it kills the process, so the coordinator exiting (or panicking) never leaves
/// an orphan holding the port and the helpers (R4.9; macOS rule from R3.6: kill-on-drop plus a port check at start).
pub struct LlamaServer {
    child: Child,
    addr: String,
}

/// Probe every helper, check our port is free, then spawn llama-server with stdout and stderr in `log_path`
/// (R4.8 parses that load log for actual placement later).
pub fn start(spec: &LaunchSpec, log_path: &Path) -> Result<LlamaServer, LaunchError> {
    // An unreachable --rpc address makes llama-server abort, and rpc-server serves only one client, so check first.
    if let Some(dead) = spec.rpc.iter().find(|a| !accepting(a)) {
        return Err(LaunchError::RpcUnreachable(dead.clone()));
    }
    let addr = local_addr(spec);
    if accepting(&addr) {
        return Err(LaunchError::PortInUse(addr));
    }
    let log =
        File::create(log_path).map_err(|e| LaunchError::LaunchFailed(format!("log file: {e}")))?;
    let log2 = log
        .try_clone()
        .map_err(|e| LaunchError::LaunchFailed(format!("log file: {e}")))?;
    let child = Command::new(&spec.llama_server_path)
        .args(args(spec))
        .envs(spec.env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log2)
        .spawn()
        .map_err(|e| LaunchError::LaunchFailed(format!("{}: {e}", spec.llama_server_path)))?;
    Ok(LlamaServer { child, addr })
}

impl LlamaServer {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// `Some(exit description)` once the process has exited. WP-9 polls this: a helper dying mid-run makes
    /// llama-server abort (ggml-rpc.cpp:31), so an exit is how the coordinator learns the pool broke.
    pub fn exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            Ok(None) => None,
            Err(e) => Some(format!("wait failed: {e}")),
        }
    }

    /// R4.7: poll GET /health until 200 or `timeout`. llama-server answers 503 while the model loads.
    /// Fails fast if the process exits during load (bad model path, out of memory, helper refused).
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<(), LaunchError> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if let Some(status) = self.exited() {
                return Err(LaunchError::LaunchFailed(format!(
                    "llama-server exited during load: {status}"
                )));
            }
            if health_ok(&self.addr) {
                return Ok(());
            }
            sleep(Duration::from_millis(500));
        }
        Err(LaunchError::NotReadyTimeout(timeout))
    }

    /// R4.9: stop and reap the process. Idempotent with Drop.
    pub fn stop(mut self) {
        self.kill();
    }

    fn kill(&mut self) {
        let _ = self.child.kill(); // Err just means it already exited.
        let _ = self.child.wait();
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        self.kill();
    }
}

/// One HTTP/1.0 GET /health; true on a 200 status line.
/// shortcut: hand-written HTTP for one localhost endpoint; switch to an HTTP client crate if the coordinator
/// starts reading /metrics or /slots (WP-7).
pub fn health_ok(addr: &str) -> bool {
    let Ok(a) = addr.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut s) = TcpStream::connect_timeout(&a, PROBE_TIMEOUT) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(PROBE_TIMEOUT));
    if s.write_all(format!("GET /health HTTP/1.0\r\nHost: {addr}\r\n\r\n").as_bytes())
        .is_err()
    {
        return false;
    }
    let mut head = [0u8; 16];
    let n = s.read(&mut head).unwrap_or(0);
    // Status line: "HTTP/1.1 200 OK"; the code sits at bytes 9..12.
    head[..n].get(9..12) == Some(b"200".as_slice())
}
