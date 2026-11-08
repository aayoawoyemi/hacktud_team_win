//! CONCEPT: end-to-end test of the coordinator on one Mac (PRD §5.8 single-machine dev mode): two helper agents
//!          register in SpacetimeDB, the coordinator plans, commands them to start rpc-server, launches llama-server,
//!          gets an answer, then a third helper joins and the pool re-plans to three, answers again, and stops clean.
//! WHY: WP-4 acceptance (cold to SERVING with one command; stop leaves no llama.cpp process) and D-14 (the demo
//!      shows a join). Proves the control logic only: every rpc-server shares one GPU and localhost, so no speed claims.
//! DEPENDS ON: a local SpacetimeDB with crates/control published as `pool-smoke`, the publisher token, llama.cpp b8665,
//!             a small GGUF. Ignored by default (Aries' Windows run has none of these). Run on a Mac:
//!               spacetime publish -s local -p crates/control pool-smoke -y --delete-data
//!             (another server or db: set SPACETIME_URI / SPACETIME_DB; the live `pool` db is refused)
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

/// SpacetimeDB to test against: `SPACETIME_URI` / `SPACETIME_DB` from the environment, else a local scratch db
/// (`http://127.0.0.1:<contracts::defaults::SPACETIME_PORT>`, `pool-smoke`). Never point this at the live `pool`:
/// the test publishes nodes, plans and commands into whatever db it is given.
fn spacetime() -> (String, String) {
    let uri = std::env::var("SPACETIME_URI").unwrap_or_else(|_| {
        format!(
            "http://127.0.0.1:{}",
            contracts::defaults::SPACETIME_PORT
        )
    });
    let db = std::env::var("SPACETIME_DB").unwrap_or_else(|_| "pool-smoke".into());
    assert_ne!(
        db,
        contracts::defaults::DB_NAME,
        "full_cycle must not run against the live pool db"
    );
    (uri, db)
}

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
        let (uri, db) = spacetime();
        let toml = format!(
            "node_id = \"{node_id}\"\nrole = \"helper\"\nspacetime_uri = \"{uri}\"\nspacetime_db = \"{db}\"\n\
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

/// Ask for a model exactly as the dashboard does: an anonymous HTTP call of the request_model reducer.
/// One request_model call; returns (HTTP status, body).
fn try_request_model(id: &str) -> (String, String) {
    let (uri, db) = spacetime();
    let out = Command::new("curl")
        .args(["-s", "-w", "\n%{http_code}", "-X", "POST"])
        .arg(format!("{uri}/v1/database/{db}/call/request_model"))
        .args(["-H", "Content-Type: application/json", "-d"])
        .arg(serde_json::json!([id]).to_string())
        .output()
        .expect("curl");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    (code.to_string(), body.to_string())
}

/// request_model, waiting out the module's one-switch-per-30-s cooldown (Aries #51 review item 3).
fn request_model(id: &str) {
    let t = Instant::now();
    loop {
        let (code, body) = try_request_model(id);
        if code == "200" {
            return;
        }
        assert!(
            body.contains("one model switch per") && t.elapsed() < Duration::from_secs(45),
            "request_model {id}: {code} {body}"
        );
        sleep(Duration::from_secs(1));
    }
}

/// Event messages from the db (the coordinator's cache doesn't subscribe to `event`).
fn event_messages() -> Vec<String> {
    let (uri, db) = spacetime();
    let out = Command::new("curl")
        .args(["-s", "-X", "POST"])
        .arg(format!("{uri}/v1/database/{db}/sql"))
        .args(["-d", "SELECT message FROM event"])
        .output()
        .expect("curl");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    v[0]["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| r[0].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
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

/// The newest llama-server log in `dir` (plan ids are timestamps now, e.g. plan-1794123373-1, not plan-0001).
fn latest_plan_log(dir: &Path) -> std::path::PathBuf {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().contains("llama-server-plan-"))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .expect("no llama-server log")
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
        spacetime_uri: spacetime().0,
        spacetime_db: spacetime().1,
        token: Some(env("SPACETIME_TOKEN")),
        llama_server_path: format!("{bin}/llama-server"),
        model_path: env("MODEL"),
        model_id: "test-model".into(),
        host: "127.0.0.1".into(),
        port,
        log_dir: log_dir.clone(),
        // Model switching (6c-6e): the same small GGUF under a second id, and an id whose file doesn't exist.
        models: [
            ("alt".to_string(), env("MODEL")),
            ("broken".to_string(), "/nonexistent/broken.gguf".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    println!("1. register: helper-a, helper-b");
    let a = Agent::spawn("helper-a", 50072, &bin, &log_dir);
    let b = Agent::spawn("helper-b", 50073, &bin, &log_dir);
    let mut c = Coordinator::connect(cfg.clone()).expect("coordinator connects");
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
    let devs = rpc_devices_in_log(&latest_plan_log(&log_dir));
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
    let devs = rpc_devices_in_log(&latest_plan_log(&log_dir));
    println!("  layers on devices: {devs:?}");
    assert!(
        [50072, 50073, 50074].iter().all(|p| on_helper(&devs, *p)),
        "{devs:?}"
    );
    assert!(!ask(port, "Name one planet.").is_empty());

    // The demo's live-join moment is "stop a helper's agent, start it again" (#11 07:27Z / 07:30Z): the returning
    // helper must take the normal join path and bring the pool back to SERVING, not leave it DEGRADED.
    println!("6b. leave + rejoin: helper-c");
    drop(cc);
    wait_until("helper-c LOST", 30, || {
        c.tick().expect("tick");
        node_states(&c)
            .iter()
            .any(|(id, st)| id == "helper-c" && st == "LOST")
    });
    let cc = Agent::spawn("helper-c", 50074, &bin, &log_dir);
    let t2 = Instant::now();
    wait_until("rejoin re-plan back to SERVING with 3 helpers", 180, || {
        c.tick().expect("tick");
        c.cluster_state() == "SERVING" && c.active().len() == 3
    });
    println!("  rejoin re-plan in {:.1} s", t2.elapsed().as_secs_f64());
    assert!(!ask(port, "Name one ocean.").is_empty());

    // Preston (Sun 2am): switch the active model from the dashboard; Aries 07:50Z: a failed load restores the old one.
    println!("6c. model switch: test-model -> alt");
    request_model("alt");
    let (code, body) = try_request_model("broken");
    assert!(
        code != "200" && body.contains("one model switch per"),
        "a second request right away must hit the cooldown: {code} {body}"
    );
    wait_until("SERVING on alt", 180, || {
        c.tick().expect("tick");
        c.cfg.model_id == "alt" && c.cluster_state() == "SERVING"
    });
    let row = c.conn.db.cluster_status().iter().next().unwrap();
    assert_eq!(row.model_id, "alt");
    assert!(row.status_json.contains("\"broken\""), "{}", row.status_json);
    assert!(!ask(port, "Name one color.").is_empty());

    println!("6d. switch to a model that can't load -> previous model comes back");
    request_model("broken");
    wait_until("restore after failed switch", 180, || {
        c.tick().expect("tick");
        event_messages()
            .iter()
            .any(|m| m.starts_with("switch to broken failed"))
            && c.cluster_state() == "SERVING"
    });
    assert_eq!(c.cfg.model_id, "alt");
    assert!(!ask(port, "Name one fruit.").is_empty());

    println!("6e. unknown model id is ignored");
    request_model("not-configured");
    wait_until("unknown id logged", 30, || {
        c.tick().expect("tick");
        event_messages()
            .iter()
            .any(|m| m.contains("\"not-configured\" is not in coordinator.toml"))
    });
    assert_eq!(c.cfg.model_id, "alt");
    assert_eq!(c.cluster_state(), "SERVING");

    println!("6f. switch back to the default model (it must stay in the list after a switch)");
    let row = c.conn.db.cluster_status().iter().next().unwrap();
    assert!(row.status_json.contains("\"test-model\""), "{}", row.status_json);
    request_model("test-model");
    wait_until("SERVING on test-model", 180, || {
        c.tick().expect("tick");
        c.cfg.model_id == "test-model" && c.cluster_state() == "SERVING"
    });
    assert_eq!(c.cfg.model_path, env("MODEL"));
    assert!(!ask(port, "Name one animal.").is_empty());

    // Aries/P2 #51 review item 2: a request left in the db from before a restart must not override the config.
    println!("6h. restart: a stale model request is ignored, the configured default loads");
    request_model("alt");
    c.shutdown().expect("shutdown before restart");
    drop(c);
    let mut c = Coordinator::connect(cfg.clone()).expect("coordinator reconnects");
    c.bring_up().expect("bring-up after restart");
    assert_eq!(c.cfg.model_id, "test-model");
    assert_eq!(c.cluster_state(), "SERVING");
    assert!(!ask(port, "Name one tree.").is_empty());

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
