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
//! The helpers are real `agent` processes (crates/agent, built first with `cargo build -p agent`), each with its own
//! TOML config, SpacetimeDB identity and rpc-server port, stopped with SIGINT like Ctrl-C.

use contracts::control::*;
use coordinator::cycle::{Config, Coordinator};
use coordinator::launch::accepting;
use spacetimedb_sdk::Table;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const URL: &str = "http://127.0.0.1:3000";
const DB: &str = "pool-smoke";

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("set {k} (see the header of this file)"))
}

/// A real `agent` process (WP-3, crates/agent) with its own config: one SpacetimeDB identity + one rpc-server port.
struct Agent {
    child: Child,
    port: u16,
}

impl Agent {
    fn spawn(node_id: &str, port: u16, llama_bin_dir: &str, dir: &Path) -> Agent {
        let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/agent");
        assert!(bin.exists(), "build the agent first: cargo build -p agent");
        let cfg = dir.join(format!("{node_id}.toml"));
        let toml = format!(
            "node_id = \"{node_id}\"\nrole = \"helper\"\nspacetime_uri = \"{URL}\"\nspacetime_db = \"{DB}\"\n\
             llama_bin_dir = \"{llama_bin_dir}\"\nrpc_bind = \"127.0.0.1\"\nrpc_port = {port}\nrpc_cache = true\n"
        );
        std::fs::write(&cfg, toml).unwrap();
        let log = std::fs::File::create(dir.join(format!("{node_id}.log"))).unwrap();
        let child = Command::new(bin)
            .arg(&cfg)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("spawn agent");
        Agent { child, port }
    }
}

impl Drop for Agent {
    /// Ctrl-C, like a person stopping it: the agent stops its rpc-server and disconnects. SIGKILL only as fallback.
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-INT", &self.child.id().to_string()])
            .status();
        let t = Instant::now();
        while self.child.try_wait().ok().flatten().is_none()
            && t.elapsed() < Duration::from_secs(10)
        {
            sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
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
    let port = 8089u16;
    let log_dir = std::env::temp_dir().join(format!("wp4-full-cycle-{}", std::process::id()));
    std::fs::create_dir_all(&log_dir).unwrap();
    let cfg = Config {
        spacetime_uri: URL.into(),
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
    let a = Agent::spawn("helper-a", 50072, &bin, &log_dir);
    let b = Agent::spawn("helper-b", 50073, &bin, &log_dir);
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
    let cc = Agent::spawn("helper-c", 50074, &bin, &log_dir);
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
    // The coordinator's stop_rpc_server commands (not the agents exiting) must close every helper port.
    wait_until("rpc-servers stopped by command", 15, || {
        [&a, &b, &cc]
            .iter()
            .all(|ag| !accepting(&format!("127.0.0.1:{}", ag.port)))
    });
    let mut cmds: Vec<(u64, String, String, String)> = c
        .conn
        .db
        .command()
        .iter()
        .map(|r| (r.command_id, r.node_id, r.kind, r.status))
        .collect();
    cmds.sort();
    println!("  commands: {cmds:?}");
    assert!(cmds.iter().all(|(_, _, _, st)| st == "ok"), "{cmds:?}");
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
