//! CONCEPT: supervise ONE rpc-server child: spawn it with the configured flags, capture its stdout/stderr
//!          into a bounded ring, report its state (Starting -> Listening -> Exited), stop it on request.
//! WHY: O-01 = A: the agent runs the official rpc-server binary (D-04, build 8665) as a child process.
//!      R3.4 start with args, R3.5 health (alive + port accepting), R3.6 stop leaving no orphan,
//!      R3.9 port already in use -> PortInUse before spawning.
//!      D-14 (joins-only demo): NO auto-restart, backoff, recovery or re-plan. A crash just becomes
//!      `Exited { stopped_by_agent: false }` and stays there; the caller decides (R3.8 is deferred, WP-9).
//! DEPENDS ON: tokio process/io/sync/time, port.rs (Listening signal), ring.rs, parse.rs (banner),
//!      config.rs (`RpcLaunch::from_config`).
//! AFFECTS: the heartbeat's `rpc_server` block (§6.2) via `RpcServer::status`; WP-6 6A reads the log ring;
//!      the D-13 command handler (start/stop_rpc_server, §6.3) will own one `RpcServer`.
//!
//! How "Listening" is decided: a TCP connect to the port succeeds (port.rs). The banner is NOT the signal:
//! rpc-server prints it with printf to stdout, and C stdio may buffer a piped stdout, so its timing isn't
//! guaranteed (a file-redirect run on the Dell showed no banner after 4 s; through the agent's pipe on the
//! Dell, build 8665, it did arrive while running). The banner is parsed from whatever arrives on either
//! stream, so `banner()` fills in when it lands; the readers are drained after exit so it is never lost.
//! Stop: `start_kill` (TerminateProcess on Windows, SIGKILL on Unix), then reap. rpc-server holds no state
//! worth flushing (the `-c` cache is written per tensor as it arrives), so a hard kill is a clean stop.
//! Rejected: SIGTERM-then-wait: needs a signal crate (nix/libc) and buys nothing for rpc-server.
//! Orphans: `kill_on_drop(true)` and the monitor task own the child, so dropping `RpcServer` or shutting
//! down the runtime kills it. TODO(R3.6): an agent CRASH on Windows still orphans the child; add a Job
//! Object with KILL_ON_JOB_CLOSE (`win32job` / `windows` crate). On macOS, check the port at startup (the
//! PortInUse pre-check already reports a leftover server).

use crate::config::AgentConfig;
use crate::parse::{self, RpcBanner};
use crate::port::{probe_host_for_bind, tcp_accepting};
use crate::ring::{LogLine, LogRing, Stream};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

/// Everything needed to launch rpc-server. Built from config (§6.12 keys) or by hand (tests, §6.3 args).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RpcLaunch {
    pub binary: PathBuf,
    /// `-H`
    pub host: String,
    /// `-p`
    pub port: u16,
    /// `-c`
    pub cache: bool,
    /// `-d`
    pub devices: Option<String>,
    /// `-t`
    pub threads: Option<u32>,
    /// env `GGML_RPC_DEBUG`
    pub debug_level: Option<u8>,
    /// env `LLAMA_CACHE`
    pub cache_dir: Option<PathBuf>,
    /// Extra env vars (§6.3 `args.env`).
    pub env: Vec<(String, String)>,
    /// Ring size in lines. 4096 lines of ~150 bytes is ~600 KB per server.
    pub log_capacity: usize,
    /// How often the port is probed while Starting.
    pub probe_interval: Duration,
    /// Per-probe connect timeout.
    pub probe_timeout: Duration,
}

impl RpcLaunch {
    pub fn new(binary: impl Into<PathBuf>, host: impl Into<String>, port: u16) -> Self {
        Self {
            binary: binary.into(),
            host: host.into(),
            port,
            cache: false,
            devices: None,
            threads: None,
            debug_level: None,
            cache_dir: None,
            env: Vec::new(),
            log_capacity: 4096,
            probe_interval: Duration::from_millis(100),
            probe_timeout: Duration::from_millis(500),
        }
    }

    pub fn from_config(cfg: &AgentConfig) -> Self {
        Self {
            cache: cfg.rpc_cache,
            devices: cfg.rpc_devices.clone(),
            threads: cfg.rpc_threads,
            debug_level: cfg.rpc_debug_level,
            cache_dir: cfg.llama_cache_dir.clone(),
            ..Self::new(cfg.rpc_server_binary(), cfg.rpc_bind.clone(), cfg.rpc_port)
        }
    }

    /// The command line after the binary, in rpc-server 8665's flag names (`rpc-server --help`).
    pub fn args(&self) -> Vec<String> {
        let mut a = vec![
            "-H".to_string(),
            self.host.clone(),
            "-p".to_string(),
            self.port.to_string(),
        ];
        if let Some(d) = &self.devices {
            a.extend(["-d".to_string(), d.clone()]);
        }
        if let Some(t) = self.threads {
            a.extend(["-t".to_string(), t.to_string()]);
        }
        if self.cache {
            a.push("-c".to_string());
        }
        a
    }
}

/// Lifecycle of one rpc-server child. There is no NotStarted: an `RpcServer` exists only after a spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcState {
    /// Spawned; port not accepting yet.
    Starting,
    /// Process alive and a TCP connect to the port succeeded.
    Listening,
    /// Process gone. `code` is None when killed without one (Unix signal). `stopped_by_agent` separates
    /// `stop()` from a crash or a failed start (e.g. bad flag, port taken between the check and the bind).
    Exited {
        code: Option<i32>,
        stopped_by_agent: bool,
    },
}

impl RpcState {
    /// §6.2 `rpc_server.state` value: `starting`, `running`, `exited` (stopped by us) or `failed`.
    pub fn heartbeat_state(&self) -> &'static str {
        match self {
            RpcState::Starting => "starting",
            RpcState::Listening => "running",
            RpcState::Exited {
                stopped_by_agent: true,
                ..
            } => "exited",
            RpcState::Exited { .. } => "failed",
        }
    }

    pub fn is_exited(&self) -> bool {
        matches!(self, RpcState::Exited { .. })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    /// R3.9 / §6.11 `E_PORT_IN_USE`: something already accepts on the port (often a leftover rpc-server).
    #[error("port {host}:{port} already accepts connections (E_PORT_IN_USE)")]
    PortInUse { host: String, port: u16 },
    #[error("spawning {binary}: {source}")]
    Spawn {
        binary: PathBuf,
        source: std::io::Error,
    },
}

/// What the heartbeat needs about the server (§6.2 `rpc_server`, minus fields owned elsewhere).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RpcStatus {
    /// §6.2 value (`starting`, `running`, `exited`, `failed`), from `RpcState::heartbeat_state`.
    pub state: &'static str,
    pub pid: Option<u32>,
    pub bind: String,
    pub port: u16,
    /// Seconds since the Unix epoch. TODO(D-13): RFC 3339 once a time crate is agreed (§6.0).
    pub started_at_unix_s: u64,
    pub last_exit_code: Option<i32>,
    pub port_accepting: bool,
}

/// A running (or finished) rpc-server child. Drop = stop (the monitor kills the child).
pub struct RpcServer {
    launch: RpcLaunch,
    pid: Option<u32>,
    started_at: SystemTime,
    state_rx: watch::Receiver<RpcState>,
    logs: Arc<Mutex<LogRing>>,
    banner: Arc<Mutex<Option<RpcBanner>>>,
    stop_tx: Option<oneshot::Sender<()>>,
    monitor: Option<JoinHandle<()>>,
}

/// Lines of output kept for banner parsing. The banner is ~10 lines after ~6 lines of backend noise.
const BANNER_SCAN_LINES: usize = 200;
/// After the child exits, how long to wait for the pipe readers to drain (buffered stdout lands here).
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

impl RpcServer {
    /// Spawns rpc-server. Returns as soon as the process exists (state Starting); use `wait_listening`.
    /// Must be called inside a tokio runtime.
    pub async fn start(launch: RpcLaunch) -> Result<Self, SupervisorError> {
        let probe_host = probe_host_for_bind(&launch.host).to_string();
        if tcp_accepting(&probe_host, launch.port, launch.probe_timeout).await {
            return Err(SupervisorError::PortInUse {
                host: launch.host.clone(),
                port: launch.port,
            });
        }

        let mut cmd = tokio::process::Command::new(&launch.binary);
        cmd.args(launch.args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(level) = launch.debug_level {
            cmd.env("GGML_RPC_DEBUG", level.to_string());
        }
        if let Some(dir) = &launch.cache_dir {
            cmd.env("LLAMA_CACHE", dir);
        }
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|source| SupervisorError::Spawn {
            binary: launch.binary.clone(),
            source,
        })?;
        let started_at = SystemTime::now();
        let pid = child.id();

        let logs = Arc::new(Mutex::new(LogRing::new(launch.log_capacity)));
        let banner = Arc::new(Mutex::new(None));
        let banner_buf = Arc::new(Mutex::new(String::new()));
        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            readers.push(spawn_reader(
                out,
                Stream::Stdout,
                &logs,
                &banner,
                &banner_buf,
            ));
        }
        if let Some(err) = child.stderr.take() {
            readers.push(spawn_reader(
                err,
                Stream::Stderr,
                &logs,
                &banner,
                &banner_buf,
            ));
        }

        let (state_tx, state_rx) = watch::channel(RpcState::Starting);
        let (stop_tx, stop_rx) = oneshot::channel();
        let monitor = tokio::spawn(monitor(
            child,
            stop_rx,
            state_tx,
            readers,
            probe_host,
            launch.port,
            launch.probe_interval,
            launch.probe_timeout,
        ));

        Ok(Self {
            launch,
            pid,
            started_at,
            state_rx,
            logs,
            banner,
            stop_tx: Some(stop_tx),
            monitor: Some(monitor),
        })
    }

    pub fn state(&self) -> RpcState {
        *self.state_rx.borrow()
    }

    /// A receiver that sees every state change (for the heartbeat loop or a UI).
    pub fn subscribe(&self) -> watch::Receiver<RpcState> {
        self.state_rx.clone()
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn launch(&self) -> &RpcLaunch {
        &self.launch
    }

    pub fn started_at(&self) -> SystemTime {
        self.started_at
    }

    /// Waits until Listening (Ok) or Exited / timeout (Err with the state at that moment).
    pub async fn wait_listening(&self, timeout: Duration) -> Result<(), RpcState> {
        let mut rx = self.state_rx.clone();
        let waited = tokio::time::timeout(timeout, rx.wait_for(|s| *s != RpcState::Starting)).await;
        match waited {
            Ok(Ok(s)) if *s == RpcState::Listening => Ok(()),
            _ => Err(self.state()),
        }
    }

    /// Live TCP check of the port (§6.2 `port_accepting`). Don't call in a tight loop (see port.rs).
    pub async fn port_accepting(&self) -> bool {
        tcp_accepting(
            probe_host_for_bind(&self.launch.host),
            self.launch.port,
            self.launch.probe_timeout,
        )
        .await
    }

    /// Heartbeat-ready snapshot. Probes the port only while the process is alive.
    pub async fn status(&self) -> RpcStatus {
        let state = self.state();
        let port_accepting = !state.is_exited() && self.port_accepting().await;
        RpcStatus {
            state: state.heartbeat_state(),
            pid: self.pid,
            bind: self.launch.host.clone(),
            port: self.launch.port,
            started_at_unix_s: self
                .started_at
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            last_exit_code: match state {
                RpcState::Exited { code, .. } => code,
                _ => None,
            },
            port_accepting,
        }
    }

    /// Copy of the captured output, oldest first.
    pub fn logs(&self) -> Vec<LogLine> {
        lock(&self.logs).snapshot()
    }

    pub fn log_tail(&self, n: usize) -> Vec<LogLine> {
        lock(&self.logs).tail(n)
    }

    /// Lines lost off the front of the ring.
    pub fn logs_dropped(&self) -> u64 {
        lock(&self.logs).dropped()
    }

    /// The parsed start banner, once it has arrived (timing not guaranteed; see module doc).
    pub fn banner(&self) -> Option<RpcBanner> {
        lock(&self.banner).clone()
    }

    /// Kills the child and waits for it to be reaped and its output drained. Idempotent (R3.7): calling it
    /// again, or after a crash, just returns the final state. Returns the state at return time, which is
    /// Exited unless the monitor didn't finish within `timeout`.
    pub async fn stop(&mut self, timeout: Duration) -> RpcState {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(()); // Err = monitor already finished (child exited by itself): fine.
        }
        if let Some(handle) = self.monitor.take() {
            let mut handle = handle;
            if tokio::time::timeout(timeout, &mut handle).await.is_err() {
                // Keep the handle so a later stop() can wait again.
                self.monitor = Some(handle);
            }
        }
        self.state()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A reader task panicking mid-push must not take the supervisor down with it.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Reads one pipe line by line into the ring; feeds the first BANNER_SCAN_LINES lines (both streams) to the
/// banner parser. Bytes, not `lines()`: a non-UTF-8 device name must not end the reader.
fn spawn_reader<R: AsyncRead + Unpin + Send + 'static>(
    pipe: R,
    stream: Stream,
    logs: &Arc<Mutex<LogRing>>,
    banner: &Arc<Mutex<Option<RpcBanner>>>,
    banner_buf: &Arc<Mutex<String>>,
) -> JoinHandle<()> {
    let (logs, banner, banner_buf) = (logs.clone(), banner.clone(), banner_buf.clone());
    tokio::spawn(async move {
        let mut reader = BufReader::new(pipe);
        let mut buf = Vec::new();
        let mut scanned = 0usize;
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&buf);
            let text = text.trim_end_matches(['\r', '\n']).to_string();
            if scanned < BANNER_SCAN_LINES {
                scanned += 1;
                let mut b = lock(&banner_buf);
                b.push_str(&text);
                b.push('\n');
                if let Ok(parsed) = parse::parse_rpc_server_banner(&b) {
                    *lock(&banner) = Some(parsed);
                }
            }
            lock(&logs).push(stream, text);
        }
    })
}

/// Owns the child. Probes the port while Starting; ends on exit or on a stop request (or the RpcServer
/// being dropped, which drops `stop_rx`'s sender).
#[allow(clippy::too_many_arguments)]
async fn monitor(
    mut child: tokio::process::Child,
    mut stop_rx: oneshot::Receiver<()>,
    state_tx: watch::Sender<RpcState>,
    readers: Vec<JoinHandle<()>>,
    probe_host: String,
    port: u16,
    probe_interval: Duration,
    probe_timeout: Duration,
) {
    let mut ticker = tokio::time::interval(probe_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let final_state = loop {
        let starting = *state_tx.borrow() == RpcState::Starting;
        tokio::select! {
            status = child.wait() => {
                break RpcState::Exited {
                    code: status.ok().and_then(|s| s.code()),
                    stopped_by_agent: false,
                };
            }
            _ = &mut stop_rx => {
                // Either stop() or the RpcServer was dropped: both mean "kill it".
                let _ = child.start_kill();
                let status = child.wait().await;
                break RpcState::Exited {
                    code: status.ok().and_then(|s| s.code()),
                    stopped_by_agent: true,
                };
            }
            _ = ticker.tick(), if starting => {
                // Only probe while Starting: once Listening, probes are on demand (port.rs note).
                if tcp_accepting(&probe_host, port, probe_timeout).await {
                    state_tx.send_replace(RpcState::Listening);
                }
            }
        }
    };
    // Let buffered stdout (the banner on 8665) land in the ring before publishing Exited.
    for r in readers {
        let _ = tokio::time::timeout(DRAIN_TIMEOUT, r).await;
    }
    state_tx.send_replace(final_state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_follow_rpc_server_flags() {
        let mut l = RpcLaunch::new("rpc-server", "0.0.0.0", 50052);
        assert_eq!(l.args(), ["-H", "0.0.0.0", "-p", "50052"]);
        l.cache = true;
        l.devices = Some("Vulkan1".into());
        l.threads = Some(4);
        assert_eq!(
            l.args(),
            [
                "-H", "0.0.0.0", "-p", "50052", "-d", "Vulkan1", "-t", "4", "-c"
            ]
        );
    }

    #[test]
    fn from_config_uses_defaults() {
        let l = RpcLaunch::from_config(&AgentConfig::default());
        assert_eq!(l.port, 50052);
        assert_eq!(l.host, "127.0.0.1");
        assert!(!l.cache);
    }

    #[test]
    fn heartbeat_state_names() {
        assert_eq!(RpcState::Listening.heartbeat_state(), "running");
        let crashed = RpcState::Exited {
            code: Some(1),
            stopped_by_agent: false,
        };
        assert_eq!(crashed.heartbeat_state(), "failed");
    }
}
