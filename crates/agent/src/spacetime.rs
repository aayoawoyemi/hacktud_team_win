//! CONCEPT: the agent's control-plane loop over SpacetimeDB (D-13; CONTRACTS.md "Agent pattern"). Probes the machine,
//!          registers it, then every heartbeat interval: claims its pending commands (start/stop rpc-server, probe),
//!          does them with the WP-3 supervisor, completes them, and sends a heartbeat with the rpc-server state.
//! WHY: the coordinator (WP-4) only talks to tables: it issues start_rpc_server and waits for the helper's heartbeat
//!      to say running + port_accepting with an rpc_port. This loop is what answers. One identity per node_id (the
//!      module binds them), so the login token is saved per (db, node_id) and reused after a restart.
//! DEPENDS ON: contracts::control bindings + control_glue::node_info; supervisor.rs (RpcServer/RpcLaunch);
//!             probe.rs; config.rs (spacetime_uri, spacetime_db, role, rpc_*); O-26 heartbeat 1000 ms.
//! AFFECTS: WP-4's cycle (rpc_port, port_accepting, command results); what `spacetime sql ... FROM node` shows.
//!
//! Rejected: implementing control.rs's ControlPlane trait. It only covers register + heartbeat; commands need the
//! subscribed client cache anyway, so a plain loop over the bindings is shorter and has one fewer layer.

use crate::config::AgentConfig;
use crate::probe::{self, NodeProbe};
use crate::supervisor::{RpcLaunch, RpcServer, RpcState};
use contracts::control::*;
use contracts::control_glue::node_info;
use spacetimedb_sdk::{DbContext, Table, credentials};
use std::time::Duration;
use tokio::sync::oneshot;

const START_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs until Ctrl-C, then stops rpc-server and disconnects (Ctrl-C alone would orphan it: rpc-server ignores SIGINT).
pub async fn run(cfg: AgentConfig) -> Result<(), String> {
    // Ponytail #5: a helper bound to loopback would advertise 127.0.0.1, and main's llama-server would dial its own
    // loopback ("E_RPC_UNREACHABLE" pointing at the wrong machine). Refuse it up front.
    if cfg.role == "helper"
        && is_loopback_host(&cfg.rpc_bind)
        && !is_loopback_host(uri_host(&cfg.spacetime_uri))
    {
        return Err(format!(
            "E_CONFIG: helper rpc_bind is {}; main can't reach it. Set rpc_bind = \"0.0.0.0\" in the agent config",
            cfg.rpc_bind
        ));
    }
    let probe = probe::probe(&cfg).await;
    for e in &probe.errors {
        eprintln!("agent: probe: {e}");
    }
    let node_id = cfg
        .node_id
        .clone()
        .unwrap_or_else(|| probe.hostname.clone());
    let caps = capabilities(&node_id, &probe, &advertise_addr(&cfg));
    let caps_json = serde_json::to_string(&caps).map_err(|e| e.to_string())?;

    let key = format!("pool-agent-{}-{}", cfg.spacetime_db, node_id);
    let token = credentials::File::new(key.clone()).load().ok().flatten();
    let (tx, rx) = oneshot::channel();
    let sub = format!("SELECT * FROM command WHERE node_id = '{node_id}'");
    let conn = DbConnection::builder()
        .with_uri(cfg.spacetime_uri.clone())
        .with_database_name(cfg.spacetime_db.clone())
        .with_token(token)
        .on_connect(move |c, _, token| {
            let _ = credentials::File::new(key).save(token);
            c.subscription_builder()
                .on_applied(move |_| {
                    let _ = tx.send(());
                })
                .subscribe([sub]);
        })
        .build()
        .map_err(|e| format!("E_TRANSPORT: connect {}: {e}", cfg.spacetime_uri))?;
    conn.run_threaded();
    tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .map_err(|_| "E_TRANSPORT: subscription not applied in 10 s".to_string())?
        .map_err(|_| "E_TRANSPORT: disconnected".to_string())?;

    let (cb, rx) = done();
    conn.reducers
        .register_node_then(node_info(&caps, &cfg.role), caps_json.clone(), cb)
        .map_err(|e| e.to_string())?;
    wait(rx).await?;
    eprintln!(
        "agent: {node_id} registered with {} ({})",
        cfg.spacetime_uri, cfg.spacetime_db
    );

    let mut server: Option<RpcServer> = None;
    // Unix ms, not 0: a restarted agent's heartbeats must sort after the old process's (the module drops seq <= stored).
    let mut seq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut tick = tokio::time::interval(Duration::from_millis(cfg.heartbeat_interval_ms));
    // Ponytail #13: one Ctrl-C future for the whole run, so a press during a long start command isn't lost.
    let stop = tokio::signal::ctrl_c();
    tokio::pin!(stop);
    // Ponytail #8: the loop runs in an inner block so any error still reaches the rpc-server stop below
    // (an early `?` used to skip it, and process::exit could orphan rpc-server holding the port).
    let result: Result<(), String> = async {
        loop {
            tokio::select! {
                _ = &mut stop => break,
                _ = tick.tick() => {}
            }
            let pending: Vec<CommandRow> = conn
                .db
                .command()
                .iter()
                .filter(|c| c.node_id == node_id && c.status == "pending")
                .collect();
            for cmd in pending {
                let (cb, rx) = done();
                conn.reducers
                    .claim_command_then(cmd.command_id, cb)
                    .map_err(|e| e.to_string())?;
                if wait(rx).await.is_err() {
                    continue; // no longer pending
                }
                let r = execute(&cmd, &cfg, &probe, &caps_json, &mut server).await;
                eprintln!(
                    "agent: {} #{} -> {}",
                    cmd.kind,
                    cmd.command_id,
                    if r.ok { "ok" } else { "failed" }
                );
                conn.reducers
                    .complete_command(cmd.command_id, r.ok, r.result, r.code, r.message)
                    .map_err(|e| e.to_string())?;
            }
            seq += 1;
            let (hb, json) = heartbeat(seq, server.as_ref());
            conn.reducers
                .send_heartbeat(hb, json)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    .await;
    if let Some(mut s) = server {
        s.stop(STOP_TIMEOUT).await;
    }
    let _ = conn.disconnect();
    eprintln!("agent: stopped");
    result
}

/// Host part of "http://host:port/..." (or the input if it has no scheme).
fn uri_host(uri: &str) -> &str {
    let rest = uri.split("://").last().unwrap_or_default();
    let hostport = rest.split('/').next().unwrap_or_default();
    hostport
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(hostport)
}

fn is_loopback_host(h: &str) -> bool {
    let h = h.trim_start_matches('[').trim_end_matches(']');
    h == "localhost"
        || h.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

struct Outcome {
    ok: bool,
    result: String,
    code: Option<String>,
    message: Option<String>,
}

fn ok(result: serde_json::Value) -> Outcome {
    Outcome {
        ok: true,
        result: result.to_string(),
        code: None,
        message: None,
    }
}

fn failed(code: &str, message: String) -> Outcome {
    Outcome {
        ok: false,
        result: String::new(),
        code: Some(code.into()),
        message: Some(message),
    }
}

/// One §6.3 command. start/stop are idempotent (R3.7).
async fn execute(
    cmd: &CommandRow,
    cfg: &AgentConfig,
    probe: &NodeProbe,
    caps_json: &str,
    server: &mut Option<RpcServer>,
) -> Outcome {
    match cmd.kind.as_str() {
        "start_rpc_server" => {
            let args: contracts::StartRpcServerArgs = match serde_json::from_str(&cmd.args_json) {
                Ok(a) => a,
                Err(e) => return failed("E_CONFIG", format!("start_rpc_server args: {e}")),
            };
            let launch = launch_for(cfg, probe, &args);
            if let Some(s) = server.as_ref()
                && s.launch() == &launch
                && s.state() == RpcState::Listening
            {
                return ok(serde_json::json!({ "pid": s.pid() }));
            }
            if let Some(mut old) = server.take() {
                old.stop(STOP_TIMEOUT).await;
            }
            let s = match RpcServer::start(launch).await {
                Ok(s) => s,
                Err(e @ crate::supervisor::SupervisorError::PortInUse { .. }) => {
                    return failed("E_PORT_IN_USE", e.to_string());
                }
                Err(e) => return failed("E_RPC_SERVER_START", e.to_string()),
            };
            if let Err(state) = s.wait_listening(START_TIMEOUT).await {
                let tail: Vec<String> = s.log_tail(5).into_iter().map(|l| l.text).collect();
                return failed(
                    "E_RPC_SERVER_START",
                    format!("{state:?}; last output: {tail:?}"),
                );
            }
            let pid = s.pid();
            *server = Some(s);
            ok(serde_json::json!({ "pid": pid }))
        }
        "stop_rpc_server" => {
            let code = match server.take() {
                Some(mut s) => match s.stop(STOP_TIMEOUT).await {
                    RpcState::Exited { code, .. } => code,
                    _ => None,
                },
                None => None,
            };
            ok(serde_json::json!({ "exit_code": code }))
        }
        "probe" => ok(serde_json::from_str(caps_json).unwrap_or_default()),
        other => failed("E_INTERNAL", format!("agent: {other} not implemented yet")),
    }
}

/// Config defaults, overridden by the command's args. With no devices given anywhere, pass only GPU devices:
/// on macOS rpc-server otherwise also offers BLAS, and layers placed there crash it (found by Agent-P1 on b8665).
fn launch_for(
    cfg: &AgentConfig,
    probe: &NodeProbe,
    args: &contracts::StartRpcServerArgs,
) -> RpcLaunch {
    let mut l = RpcLaunch::from_config(cfg);
    if let Some(b) = &args.bind {
        l.host = b.clone();
    }
    if let Some(p) = args.port.and_then(|p| u16::try_from(p).ok()) {
        l.port = p;
    }
    if !args.devices.is_empty() {
        l.devices = Some(args.devices.join(","));
    }
    if l.devices.is_none() {
        let gpus = gpu_devices(probe);
        l.devices = (!gpus.is_empty()).then(|| gpus.join(","));
    }
    if let Some(t) = args.threads {
        l.threads = Some(t);
    }
    if let Some(c) = args.cache {
        l.cache = c;
    }
    if let Some(d) = args.debug_level.and_then(|d| u8::try_from(d).ok()) {
        l.debug_level = Some(d);
    }
    l.env.extend(args.env.clone());
    l
}

fn gpu_devices(probe: &NodeProbe) -> Vec<String> {
    probe
        .devices
        .iter()
        .filter(|d| d.backend != "BLAS" && d.backend != "CPU")
        .map(|d| d.name.clone())
        .collect()
}

/// §6.2 heartbeat. port_accepting comes from the supervisor's state (it saw the port accept before Listening),
/// not from a fresh TCP connect: rpc-server serves one client, and while llama-server holds it every probe
/// connect would sit unaccepted in the listen backlog, ~1 per second, until it fills.
fn heartbeat(seq: u64, server: Option<&RpcServer>) -> (HeartbeatInfo, String) {
    let (state, pid, port, accepting, exit_code) = match server {
        None => ("not_started", None, None, false, None),
        Some(s) => {
            let st = s.state();
            let code = match st {
                RpcState::Exited { code, .. } => code,
                _ => None,
            };
            (
                st.heartbeat_state(),
                s.pid(),
                Some(u32::from(s.launch().port)),
                st == RpcState::Listening,
                code,
            )
        }
    };
    let error = (state == "failed").then(|| format!("rpc-server exited (code {exit_code:?})"));
    let hb = HeartbeatInfo {
        seq,
        rpc_state: state.into(),
        rpc_pid: pid,
        rpc_port: port,
        port_accepting: accepting,
        cpu_util_pct: None,
        mem_used_bytes: None,
        relay_bytes_in: None,
        relay_bytes_out: None,
        relay_graph_computes: None,
        on_ac: None,
        battery_pct: None,
        last_error_code: error.as_ref().map(|_| "E_RPC_SERVER_START".into()),
        last_error_message: error,
    };
    let json = serde_json::json!({
        "schema_version": contracts::SCHEMA_VERSION,
        "seq": seq,
        "rpc_server": { "state": state, "pid": pid, "port": port, "port_accepting": accepting, "last_exit_code": exit_code },
    })
    .to_string();
    (hb, json)
}

/// §6.1 from the probe. shortcut: captured_at, disk, speed and power stay empty until something reads them.
fn capabilities(node_id: &str, p: &NodeProbe, addr: &str) -> contracts::NodeCapabilities {
    contracts::NodeCapabilities {
        schema_version: contracts::SCHEMA_VERSION,
        node_id: node_id.into(),
        hostname: p.hostname.clone(),
        agent_version: p.agent_version.clone(),
        os: Some(contracts::OsInfo {
            family: p.os.family.clone(),
            version: p.os.version.clone(),
            arch: p.os.arch.clone(),
        }),
        cpu: Some(contracts::CpuInfo {
            model: p.cpu.model.clone(),
            physical_cores: p.cpu.physical_cores.map(|n| n as u32),
            logical_cores: Some(p.cpu.logical_cores as u32),
        }),
        memory: Some(contracts::MemoryInfo {
            total_bytes: p.memory.total_bytes,
            available_bytes: p.memory.available_bytes,
        }),
        llama_cpp: p.llama_cpp.as_ref().map(|l| contracts::LlamaCppInfo {
            bin_dir: l
                .bin_dir
                .as_ref()
                .map(|d| d.display().to_string())
                .unwrap_or_default(),
            build: Some(l.build),
            commit: Some(l.commit.clone()),
            rpc_protocol: None, // only in the banner, which rpc-server buffers when piped
            backends: l.backends.clone(),
        }),
        devices: p
            .devices
            .iter()
            .map(|d| contracts::DeviceInfo {
                name: d.name.clone(),
                description: d.description.clone(),
                backend: d.backend.clone(),
                total_mib: d.total_mib,
                free_mib: d.free_mib,
                unified_memory: d.unified_memory,
            })
            .collect(),
        rpc_server_devices: gpu_devices(p),
        network: Some(contracts::NetworkInfo {
            addresses: vec![addr.into()],
            interface: None,
        }),
        ..Default::default()
    }
}

/// The address the coordinator should dial for our rpc-server: the bind address, or for a wildcard bind the local
/// address this machine uses to reach SpacetimeDB on main (= our address on the pool's network). No packet is sent.
fn advertise_addr(cfg: &AgentConfig) -> String {
    if !matches!(cfg.rpc_bind.as_str(), "0.0.0.0" | "::") {
        return cfg.rpc_bind.clone();
    }
    let host = cfg.spacetime_uri.split("://").last().unwrap_or_default();
    let host = host.split('/').next().unwrap_or_default();
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect(host)?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

type ReducerResult = Result<Result<(), String>, spacetimedb_sdk::__codegen::InternalError>;

fn done() -> (
    impl FnOnce(&ReducerEventContext, ReducerResult) + Send + 'static,
    oneshot::Receiver<Result<(), String>>,
) {
    let (tx, rx) = oneshot::channel();
    let cb = move |_: &ReducerEventContext, r: ReducerResult| {
        let _ = tx.send(r.unwrap_or_else(|e| Err(format!("E_TRANSPORT: {e:?}"))));
    };
    (cb, rx)
}

async fn wait(rx: oneshot::Receiver<Result<(), String>>) -> Result<(), String> {
    match tokio::time::timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(r)) => r,
        _ => Err("E_TRANSPORT: no reducer reply in 10 s".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::DeviceInfo;
    use crate::probe::{CpuInfo, MemoryInfo, OsInfo};

    fn mac_probe() -> NodeProbe {
        let dev = |name: &str, backend: &str| DeviceInfo {
            name: name.into(),
            description: String::new(),
            backend: backend.into(),
            total_mib: 0,
            free_mib: 0,
            unified_memory: Some(true),
        };
        NodeProbe {
            hostname: "mac".into(),
            agent_version: "0".into(),
            os: OsInfo {
                family: "macos".into(),
                version: None,
                arch: "aarch64".into(),
            },
            cpu: CpuInfo {
                model: "Apple M5".into(),
                physical_cores: Some(10),
                logical_cores: 10,
            },
            memory: MemoryInfo {
                total_bytes: 1,
                available_bytes: 1,
            },
            llama_cpp: None,
            devices: vec![dev("MTL0", "MTL"), dev("BLAS", "BLAS")],
            errors: vec![],
        }
    }

    #[test]
    fn mac_default_launch_excludes_blas() {
        let l = launch_for(&AgentConfig::default(), &mac_probe(), &Default::default());
        assert_eq!(l.devices.as_deref(), Some("MTL0"));
    }

    #[test]
    fn command_args_override_config() {
        let args: contracts::StartRpcServerArgs = serde_json::from_str(
            r#"{"bind":"0.0.0.0","port":50100,"devices":["MTL0"],"cache":true}"#,
        )
        .unwrap();
        let l = launch_for(&AgentConfig::default(), &mac_probe(), &args);
        assert_eq!((l.host.as_str(), l.port, l.cache), ("0.0.0.0", 50100, true));
        assert_eq!(l.devices.as_deref(), Some("MTL0"));
    }

    #[test]
    fn null_args_keep_config() {
        let args: contracts::StartRpcServerArgs =
            serde_json::from_str(r#"{"bind":null,"port":null,"devices":null,"cache":true}"#)
                .unwrap();
        let cfg = AgentConfig {
            rpc_port: 50072,
            ..Default::default()
        };
        let l = launch_for(&cfg, &mac_probe(), &args);
        assert_eq!((l.host.as_str(), l.port), ("127.0.0.1", 50072));
    }

    #[test]
    fn idle_heartbeat_is_not_started() {
        let (hb, json) = heartbeat(1, None);
        assert_eq!(
            (hb.rpc_state.as_str(), hb.port_accepting, hb.rpc_port),
            ("not_started", false, None)
        );
        assert!(json.contains("\"not_started\""));
    }
}

#[cfg(test)]
mod loopback_tests {
    use super::{is_loopback_host, uri_host};

    #[test]
    fn uri_host_strips_scheme_port_and_path() {
        assert_eq!(uri_host("http://192.168.8.174:3000"), "192.168.8.174");
        assert_eq!(uri_host("http://localhost:3000/x"), "localhost");
        assert_eq!(uri_host("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("::1"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.8.113"));
    }
}
