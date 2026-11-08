//! CONCEPT: integration test: start the real rpc-server on 127.0.0.1 at a free port, see Listening, stop it,
//!          see the port close. Plus the two failure paths that need no binary (port in use, bad binary).
//! WHY: the supervisor's whole job is lifecycle against the real binary (R3.4 to R3.6, R3.9); unit tests
//!      can't show that rpc-server 8665 actually binds, accepts a probe and dies on stop.
//! DEPENDS ON: `rpc-server` on PATH (or `AGENT_TEST_RPC_SERVER=<path>`). Skips with a message if absent,
//!      so `cargo test` stays green on a machine without llama.cpp.
//! AFFECTS: nothing; gate for supervisor.rs changes.

use agent::port::tcp_accepting;
use agent::supervisor::SupervisorError;
use agent::{RpcLaunch, RpcServer, RpcState};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn rpc_server_binary() -> PathBuf {
    std::env::var_os("AGENT_TEST_RPC_SERVER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rpc-server"))
}

/// Free port right now (bind :0, read, release). Racy in theory; fine for a test.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn rpc_server_available(bin: &Path) -> bool {
    agent::probe::run_capture(bin, &["--help"], Duration::from_secs(20))
        .await
        .is_ok()
}

#[tokio::test]
async fn real_rpc_server_listens_then_stops() {
    let bin = rpc_server_binary();
    if !rpc_server_available(&bin).await {
        eprintln!(
            "SKIPPED real_rpc_server_listens_then_stops: `{}` not runnable (put rpc-server on PATH or set AGENT_TEST_RPC_SERVER)",
            bin.display()
        );
        return;
    }
    let port = free_port();
    let mut server = RpcServer::start(RpcLaunch::new(&bin, "127.0.0.1", port))
        .await
        .expect("spawn rpc-server");
    assert!(server.pid().is_some());

    let reached = server.wait_listening(Duration::from_secs(30)).await;
    if reached.is_err() {
        for l in server.logs() {
            eprintln!("  [{:?}] {}", l.stream, l.text);
        }
    }
    assert_eq!(reached, Ok(()), "rpc-server never accepted on {port}");
    assert_eq!(server.state(), RpcState::Listening);
    // Informational: on 8665 the banner is stdout-buffered, so it may be None until exit (supervisor.rs doc).
    eprintln!(
        "banner while running: {}",
        if server.banner().is_some() {
            "present"
        } else {
            "absent"
        }
    );

    // A second probe must not kill it (port.rs assumption: rpc-server survives connect-then-close).
    let status = server.status().await;
    assert!(status.port_accepting);
    assert_eq!(status.state, "running");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        server.state(),
        RpcState::Listening,
        "probe killed rpc-server"
    );

    let final_state = server.stop(Duration::from_secs(10)).await;
    assert!(
        matches!(
            final_state,
            RpcState::Exited {
                stopped_by_agent: true,
                ..
            }
        ),
        "{final_state:?}"
    );
    // Idempotent stop (R3.7).
    assert_eq!(server.stop(Duration::from_secs(1)).await, final_state);
    assert!(!tcp_accepting("127.0.0.1", port, Duration::from_millis(500)).await);

    eprintln!("final: {final_state:?}, banner: {:?}", server.banner());
    eprintln!("captured {} lines, e.g.:", server.logs().len());
    for l in server.log_tail(5) {
        eprintln!("  [{:?}] {}", l.stream, l.text);
    }
}

#[tokio::test]
async fn port_in_use_is_reported_before_spawn() {
    let holder = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = holder.local_addr().unwrap().port();
    // Binary needn't exist: the check runs first.
    let err = RpcServer::start(RpcLaunch::new("no-such-binary", "127.0.0.1", port))
        .await
        .err()
        .expect("must fail");
    assert!(
        matches!(err, SupervisorError::PortInUse { port: p, .. } if p == port),
        "{err}"
    );
}

#[tokio::test]
async fn missing_binary_is_a_spawn_error() {
    let err = RpcServer::start(RpcLaunch::new(
        "definitely-not-rpc-server-xyz",
        "127.0.0.1",
        free_port(),
    ))
    .await
    .err()
    .expect("must fail");
    assert!(matches!(err, SupervisorError::Spawn { .. }), "{err}");
}
