//! CONCEPT: end-to-end test of the coordinator on one Mac (PRD §5.8 single-machine dev mode): two helper agents
//!          register in SpacetimeDB, the coordinator plans, commands them to start rpc-server, launches llama-server,
//!          gets an answer, then a third helper joins and the pool re-plans to three, answers again, and stops clean.
//! WHY: WP-4 acceptance (cold to SERVING with one command; stop leaves no llama.cpp process) and D-14 (the demo
//!      shows a join). Proves the control logic only: every rpc-server shares one GPU and localhost, so no speed claims.
//! DEPENDS ON: a local SpacetimeDB with crates/control published as `pool-smoke`, the publisher token, llama.cpp b8665,
//!             a small GGUF. Ignored by default (Aries' Windows run has none of these). Run on a Mac:
//!               spacetime publish -s local -p crates/control pool-smoke -y --delete-data
//!               SPACETIME_TOKEN=... LLAMA_BIN_DIR=~/llama-b8665/llama-b8665 MODEL=/path/small.gguf \
//!                 cargo test -p coordinator --test full_cycle -- --ignored --nocapture
//! AFFECTS: nothing at runtime.
//!
//! The helper agents here are STAND-INS that follow the CONTRACTS.md agent pattern (register, subscribe to own
//! commands, claim -> act -> complete, 1 s heartbeat). The real WP-3 agent's SpacetimeDB wiring isn't merged yet;
//! when it is, this test should start real `agent` processes instead.

use contracts::control::*;
use coordinator::cycle::{Config, Coordinator};
use coordinator::launch::accepting;
use spacetimedb_sdk::{DbContext, Table};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

const URL: &str = "http://127.0.0.1:3000";
const DB: &str = "pool-smoke";

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("set {k} (see the header of this file)"))
}

/// Stand-in agent: one SpacetimeDB identity + one rpc-server on `port`.
struct Agent {
    conn: Arc<DbConnection>,
    rpc: Arc<Mutex<Option<Child>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Agent {
    fn spawn(node_id: &str, port: u16, rpc_server: String) -> Agent {
        let id = node_id.to_string();
        let sub = format!("SELECT * FROM command WHERE node_id = '{id}'");
        let conn = DbConnection::builder()
            .with_uri(URL)
            .with_database_name(DB)
            .on_connect(move |c, _, _| {
                c.subscription_builder().subscribe([sub.clone()]);
            })
            .build()
            .expect("agent connects");
        conn.run_threaded();
        let info = NodeInfo {
            node_id: id.clone(),
            role: "helper".into(),
            hostname: id.clone(),
            os_family: "macos".into(),
            arch: "aarch64".into(),
            cpu_model: "Apple M5".into(),
            logical_cores: Some(10),
            total_memory_bytes: 24 << 30,
            available_memory_bytes: 12 << 30,
            // What --list-devices reports on an M5 with b8665: Metal plus the BLAS device that must be excluded.
            devices: vec![
                Device {
                    name: "MTL0".into(),
                    backend: "Metal".into(),
                    total_mib: 18186,
                    free_mib: 18185,
                    unified_memory: Some(true),
                },
                Device {
                    name: "BLAS".into(),
                    backend: "BLAS".into(),
                    total_mib: 0,
                    free_mib: 0,
                    unified_memory: Some(true),
                },
            ],
            llama_build: Some(8665),
            llama_commit: Some("b8635075f".into()),
            rpc_protocol: Some("3.6.1".into()),
            rpc_server_devices: vec!["MTL0".into()],
            addresses: vec!["127.0.0.1".into()],
        };
        conn.reducers
            .register_node(info, "{}".into())
            .expect("register_node");

        let conn = Arc::new(conn);
        let rpc: Arc<Mutex<Option<Child>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (c, r, s) = (conn.clone(), rpc.clone(), stop.clone());
        let thread = thread::spawn(move || {
            let mut seq = 0u64;
            while !s.load(Ordering::SeqCst) {
                for cmd in
                    c.db.command()
                        .iter()
                        .filter(|x| x.node_id == id && x.status == "pending")
                {
                    let (tx, rx) = mpsc::channel();
                    c.reducers
                        .claim_command_then(cmd.command_id, move |_, res| {
                            let _ = tx.send(matches!(res, Ok(Ok(()))));
                        })
                        .unwrap();
                    if !rx.recv_timeout(Duration::from_secs(5)).unwrap_or(false) {
                        continue; // already claimed
                    }
                    let (ok, err) = act(&cmd, port, &rpc_server, &r);
                    c.reducers
                        .complete_command(
                            cmd.command_id,
                            ok,
                            "{}".into(),
                            err.as_ref().map(|_| "E_RPC_SERVER_START".into()),
                            err,
                        )
                        .unwrap();
                }
                seq += 1;
                let running = r
                    .lock()
                    .unwrap()
                    .as_mut()
                    .is_some_and(|ch| ch.try_wait().ok().flatten().is_none());
                let hb = HeartbeatInfo {
                    seq,
                    rpc_state: if running { "running" } else { "not_started" }.into(),
                    rpc_pid: r.lock().unwrap().as_ref().map(Child::id),
                    rpc_port: Some(port.into()),
                    port_accepting: running && accepting(&format!("127.0.0.1:{port}")),
                    cpu_util_pct: None,
                    mem_used_bytes: None,
                    relay_bytes_in: None,
                    relay_bytes_out: None,
                    relay_graph_computes: None,
                    on_ac: None,
                    battery_pct: None,
                    last_error_code: None,
                    last_error_message: None,
                };
                let _ = c.reducers.send_heartbeat(hb, "{}".into());
                sleep(Duration::from_millis(
                    contracts::defaults::HEARTBEAT_INTERVAL_MS / 2,
                ));
            }
        });
        Agent {
            conn,
            rpc,
            stop,
            thread: Some(thread),
        }
    }
}

/// Do one command: start or stop rpc-server. Returns (ok, error message).
fn act(
    cmd: &CommandRow,
    port: u16,
    bin: &str,
    rpc: &Mutex<Option<Child>>,
) -> (bool, Option<String>) {
    match cmd.kind.as_str() {
        "start_rpc_server" => {
            let args: serde_json::Value = serde_json::from_str(&cmd.args_json).unwrap_or_default();
            let mut c = Command::new(bin);
            c.args(["-H", "127.0.0.1", "-p", &port.to_string(), "-c"]);
            if let Some(d) = args["devices"].as_array() {
                let d: Vec<&str> = d.iter().filter_map(|v| v.as_str()).collect();
                c.args(["-d", &d.join(",")]);
            }
            let child = match c.stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                Ok(ch) => ch,
                Err(e) => return (false, Some(e.to_string())),
            };
            *rpc.lock().unwrap() = Some(child);
            let t = Instant::now();
            while !accepting(&format!("127.0.0.1:{port}")) {
                if t.elapsed() > Duration::from_secs(15) {
                    return (false, Some("rpc-server not listening in 15 s".into()));
                }
                sleep(Duration::from_millis(100));
            }
            (true, None)
        }
        "stop_rpc_server" => {
            if let Some(mut ch) = rpc.lock().unwrap().take() {
                let _ = ch.kill(); // rpc-server ignores SIGINT; kill = SIGKILL
                let _ = ch.wait();
            }
            (true, None)
        }
        other => (
            false,
            Some(format!("stand-in agent: {other} not supported")),
        ),
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some(mut ch) = self.rpc.lock().unwrap().take() {
            let _ = ch.kill();
            let _ = ch.wait();
        }
        let _ = self.conn.disconnect();
    }
}

/// One chat request to the pool; returns the answer text.
fn ask(port: u16, prompt: &str) -> String {
    let body = serde_json::json!({ "messages": [{ "role": "user", "content": prompt }], "max_tokens": 24 });
    let out = Command::new("curl")
        .args([
            "-s",
            "-m",
            "60",
            &format!("http://127.0.0.1:{port}/v1/chat/completions"),
            "-H",
            "Content-Type: application/json",
            "-d",
        ])
        .arg(body.to_string())
        .output()
        .expect("curl");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let tps = v["timings"]["predicted_per_second"].as_f64().unwrap_or(0.0);
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    println!("  answer: {text:?} ({tps:.1} tok/s)");
    text
}

fn wait_until(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let t = Instant::now();
    while !ok() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timed out waiting for {what}"
        );
        sleep(Duration::from_millis(200));
    }
}

fn node_states(c: &Coordinator) -> Vec<(String, String)> {
    let mut v: Vec<_> = c
        .conn
        .db
        .node()
        .iter()
        .map(|n| (n.node_id, n.state))
        .collect();
    v.sort();
    v
}

fn rpc_devices_in_log(path: &std::path::Path) -> Vec<String> {
    let log = std::fs::read_to_string(path).unwrap_or_default();
    let mut d: Vec<String> = log
        .lines()
        .filter(|l| l.contains("model buffer size"))
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .collect();
    d.dedup();
    d
}

/// Layers landed on the helper at `port`. llama.cpp labels every remote device by its index ON THAT SERVER
/// ("RPC0[127.0.0.1:50073]" for the second helper too), so match the endpoint, not the RPCn name.
fn on_helper(devs: &[String], port: u16) -> bool {
    devs.iter().any(|d| d.ends_with(&format!(":{port}]")))
}

#[test]
#[ignore = "needs local SpacetimeDB (pool-smoke), llama.cpp b8665 and a GGUF; see file header"]
fn full_cycle_two_helpers_then_join() {
    let bin = env("LLAMA_BIN_DIR");
    let rpc_server = format!("{bin}/rpc-server");
    let port = 8089u16;
    let log_dir = std::env::temp_dir().join(format!("wp4-full-cycle-{}", std::process::id()));
    std::fs::create_dir_all(&log_dir).unwrap();
    let cfg = Config {
        spacetime_url: URL.into(),
        spacetime_db: DB.into(),
        token: Some(env("SPACETIME_TOKEN")),
        llama_server_path: format!("{bin}/llama-server"),
        model_path: env("MODEL"),
        model_id: "test-model".into(),
        host: "127.0.0.1".into(),
        port,
        log_dir: log_dir.clone(),
        ..Default::default()
    };

    println!("1. register: helper-a, helper-b");
    let a = Agent::spawn("helper-a", 50072, rpc_server.clone());
    let b = Agent::spawn("helper-b", 50073, rpc_server.clone());
    let mut c = Coordinator::connect(cfg).expect("coordinator connects");
    wait_until("2 helpers online", 15, || c.helpers().len() == 2);
    println!("  nodes: {:?}", node_states(&c));

    println!("2-4. plan, start helpers, launch");
    let t0 = Instant::now();
    c.bring_up().expect("bring_up");
    let cold = t0.elapsed();
    println!(
        "  cluster {} with {:?} in {:.1} s",
        c.cluster_state(),
        c.active(),
        cold.as_secs_f64()
    );
    assert_eq!(c.cluster_state(), "SERVING");
    wait_until("both helpers ACTIVE", 5, || {
        node_states(&c).iter().all(|(_, s)| s == "ACTIVE")
    });
    println!("  nodes: {:?}", node_states(&c));
    let devs = rpc_devices_in_log(&log_dir.join("llama-server-plan-0001.log"));
    println!("  layers on devices: {devs:?}");
    assert!(
        on_helper(&devs, 50072) && on_helper(&devs, 50073),
        "{devs:?}"
    );

    println!("5. answer");
    assert!(!ask(port, "Say hi in five words.").is_empty());

    println!("6. join: helper-c");
    let cc = Agent::spawn("helper-c", 50074, rpc_server.clone());
    let t1 = Instant::now();
    wait_until("re-plan to 3 helpers", 180, || {
        c.tick().expect("tick");
        c.active().len() == 3
    });
    let rejoin = t1.elapsed();
    println!(
        "  cluster {} with {:?} in {:.1} s",
        c.cluster_state(),
        c.active(),
        rejoin.as_secs_f64()
    );
    assert_eq!(c.cluster_state(), "SERVING");
    let devs = rpc_devices_in_log(&log_dir.join("llama-server-plan-0002.log"));
    println!("  layers on devices: {devs:?}");
    assert!(
        [50072, 50073, 50074].iter().all(|p| on_helper(&devs, *p)),
        "{devs:?}"
    );
    assert!(!ask(port, "Name one planet.").is_empty());

    println!("7. stop");
    c.shutdown().expect("shutdown");
    assert_eq!(c.cluster_state(), "STOPPED");
    wait_until("rpc-servers stopped", 15, || {
        [&a, &b, &cc]
            .iter()
            .all(|ag| ag.rpc.lock().unwrap().is_none())
    });
    println!("  nodes: {:?}", node_states(&c));
    let events: Vec<String> = c
        .conn
        .db
        .cluster_status()
        .iter()
        .map(|r| r.cluster_state)
        .collect();
    println!("  final cluster_status: {events:?}");
    drop((a, b, cc));
    assert!(
        !accepting(&format!("127.0.0.1:{port}")),
        "llama-server still listening"
    );
    for p in [50072, 50073, 50074] {
        assert!(
            !accepting(&format!("127.0.0.1:{p}")),
            "rpc-server on {p} still listening"
        );
    }
    println!(
        "RESULT: cold start {:.1} s, join re-plan {:.1} s, no llama.cpp processes left",
        cold.as_secs_f64(),
        rejoin.as_secs_f64()
    );
}
