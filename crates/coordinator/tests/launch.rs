//! CONCEPT: tests for the llama-server lifecycle (WP-4 slice 1) that run anywhere, with no llama.cpp installed.
//! WHY: Aries builds and tests every Rust PR on Windows; real llama-server runs only on the main Mac, so these use
//!      local TCP listeners as stand-ins for helpers and for a ready llama-server.
//! DEPENDS ON: coordinator::launch.
//! AFFECTS: nothing at runtime.

use coordinator::launch::{LaunchError, LaunchSpec, args, health_ok, start};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

fn spec() -> LaunchSpec {
    // Mirrors the §6.6 example (fixtures/json/launch_spec.json), plus a tensor split.
    LaunchSpec {
        plan_id: "plan-0003".into(),
        llama_server_path: "llama-server".into(),
        model_path: "models/example-8b-q4_k_m.gguf".into(),
        rpc: vec!["192.0.2.11:50052".into(), "192.0.2.12:50052".into()],
        n_gpu_layers: Some("99".into()),
        split_mode: Some("layer".into()),
        tensor_split: vec![12.0, 12.0, 9.0],
        ctx_size: Some(8192),
        host: Some("127.0.0.1".into()),
        port: Some(8080),
        metrics: Some(true),
        ..Default::default()
    }
}

#[test]
fn args_follow_the_6_6_mapping() {
    let want = "-m models/example-8b-q4_k_m.gguf --rpc 192.0.2.11:50052,192.0.2.12:50052 -ngl 99 -sm layer \
                -ts 12,12,9 -c 8192 --host 127.0.0.1 --port 8080 --metrics";
    assert_eq!(args(&spec()).join(" "), want);
}

#[test]
fn unset_fields_pass_no_flags() {
    let s = LaunchSpec {
        model_path: "m.gguf".into(),
        ..Default::default()
    };
    assert_eq!(args(&s), ["-m", "m.gguf"]);
}

#[test]
fn unreachable_helper_is_refused_before_launch() {
    // Bind then drop a listener: that port is now (almost certainly) closed.
    let dead = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let s = LaunchSpec {
        rpc: vec![dead.clone()],
        ..spec()
    };
    let err = start(&s, &std::env::temp_dir().join("wp4-test-unreach.log"))
        .err()
        .unwrap();
    assert!(matches!(err, LaunchError::RpcUnreachable(a) if a == dead));
}

#[test]
fn taken_port_is_refused() {
    let helper = TcpListener::bind("127.0.0.1:0").unwrap();
    let busy = TcpListener::bind("127.0.0.1:0").unwrap();
    let s = LaunchSpec {
        rpc: vec![helper.local_addr().unwrap().to_string()],
        port: Some(busy.local_addr().unwrap().port().into()),
        ..spec()
    };
    let err = start(&s, &std::env::temp_dir().join("wp4-test-busy.log"))
        .err()
        .unwrap();
    assert_eq!(err.code().as_str_name(), "E_PORT_IN_USE");
}

#[test]
fn missing_binary_is_launch_failed() {
    let free = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let s = LaunchSpec {
        llama_server_path: "definitely-not-llama-server".into(),
        rpc: vec![],
        port: Some(free.into()),
        ..spec()
    };
    let err = start(&s, &std::env::temp_dir().join("wp4-test-missing.log"))
        .err()
        .unwrap();
    assert_eq!(err.code().as_str_name(), "E_LAUNCH_FAILED");
}

/// Serves one HTTP response with the given status, like llama-server's /health.
fn one_response(status: &'static str) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    thread::spawn(move || {
        let (mut c, _) = l.accept().unwrap();
        let mut buf = [0u8; 256];
        let _ = c.read(&mut buf);
        let _ = c.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").as_bytes());
    });
    addr
}

#[test]
fn health_200_is_ready_503_is_loading() {
    assert!(health_ok(&one_response("200 OK")));
    assert!(!health_ok(&one_response("503 Service Unavailable")));
}

#[test]
fn health_on_closed_port_is_not_ready() {
    let dead = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    thread::sleep(Duration::from_millis(10));
    assert!(!health_ok(&dead));
}

#[test]
fn health_resolves_hostnames() {
    // Aries' note on #22: a host name (mac-main.local) must work, not only a literal IP.
    let addr = one_response("200 OK");
    let port = addr.rsplit(':').next().unwrap();
    assert!(health_ok(&format!("localhost:{port}")));
}
