//! CONCEPT: dev tool: print this machine's probe as JSON (R3.2 stand-in), optionally run rpc-server for N
//!          seconds and print its status and captured output.
//! WHY: O-02 (CLI shape) is open, so this is an example, not a shipped binary. Lets a teammate check a Mac
//!      or the Dell in one command before the control plane (D-13) exists.
//! DEPENDS ON: the agent library; llama.cpp binaries on PATH or `llama_bin_dir` in the config.
//! AFFECTS: nothing (dev only).
//!
//! Usage:
//!   cargo run -p agent --example probe                          # probe only
//!   cargo run -p agent --example probe -- --config agent.toml   # with a config file (O-24)
//!   cargo run -p agent --example probe -- --serve 10            # also run rpc-server for 10 s

use agent::control::{ControlPlane, HeartbeatBody, LogOnly};
use agent::{AgentConfig, RpcLaunch, RpcServer};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut cfg = AgentConfig::default();
    let mut serve_secs: Option<u64> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => {
                cfg = AgentConfig::load(args.next().ok_or("--config needs a path")?.as_ref())?
            }
            "--serve" => serve_secs = Some(args.next().ok_or("--serve needs seconds")?.parse()?),
            other => return Err(format!("unknown argument {other}").into()),
        }
    }

    let probe = agent::probe::probe(&cfg).await;
    println!("{}", serde_json::to_string_pretty(&probe)?);
    let node_id = cfg
        .node_id
        .clone()
        .unwrap_or_else(|| probe.hostname.clone());
    LogOnly.register_node(&node_id, &probe).await?;

    let Some(secs) = serve_secs else {
        return Ok(());
    };
    let mut server = RpcServer::start(RpcLaunch::from_config(&cfg)).await?;
    match server.wait_listening(Duration::from_secs(30)).await {
        Ok(()) => eprintln!("rpc-server listening on {}:{}", cfg.rpc_bind, cfg.rpc_port),
        Err(s) => eprintln!("rpc-server did not reach Listening: {s:?}"),
    }
    for seq in 0..secs {
        let body = HeartbeatBody {
            node_id: node_id.clone(),
            seq,
            rpc_server: Some(server.status().await),
            mem_available_bytes: agent::probe::probe_host().3.available_bytes,
            last_error: None,
        };
        LogOnly.heartbeat(&body).await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let final_state = server.stop(Duration::from_secs(10)).await;
    eprintln!("stopped: {final_state:?}; banner: {:?}", server.banner());
    for line in server.log_tail(40) {
        eprintln!("  [{:?}] {}", line.stream, line.text);
    }
    Ok(())
}
