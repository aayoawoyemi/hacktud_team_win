//! CONCEPT: the coordinator's control loop on the main Mac (WP-4 R4.1-R4.7, R4.13; PRD §5.5 startup sequence).
//!          Reads the pool from SpacetimeDB, records a plan, commands each helper's agent to start rpc-server,
//!          launches llama-server over the ready helpers, and moves the cluster through §5.6:
//!          PLANNED -> STARTING_HELPERS -> LAUNCHING -> SERVING. When a new helper joins, it stops llama-server
//!          (SERVING -> STOPPED) and runs the same sequence again with every helper (D-14: the demo shows joins).
//! WHY: D-13 (SpacetimeDB is the control plane): registry, commands, plan and status live in tables, so this file
//!      only reads rows and calls reducers. Module code can't start processes or reach the LAN, so launching
//!      llama-server stays here (launch.rs).
//! DEPENDS ON: crates/control reducers via `contracts::control` bindings (set_split_plan, set_cluster_status,
//!             issue_command, record_event; admin-only, so the coordinator connects with the publisher's token or
//!             an identity added with add_admin); agents (WP-3) that claim start_rpc_server/stop_rpc_server and
//!             report rpc_port + port_accepting in heartbeats; launch.rs; contracts::defaults (O-26, O-17b).
//! AFFECTS: what `spacetime subscribe` shows during the demo (cluster_status, node states, events); WP-9 (llama-server
//!          exit is recorded here, recovery itself is stretch under D-14); WP-5 (plan_for() is where the planner plugs in).
//!
//! Rejected: callbacks/async for the cycle. Each step waits on the subscribed client cache with a short poll, which is
//! easy to read top to bottom and fast enough for a few nodes (the SDK keeps the cache current on its own thread).

use crate::launch::{self, LaunchSpec, LlamaServer};
use contracts::control::*;
use spacetimedb_sdk::{DbContext, Identity, Table};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Coordinator settings (O-24: one TOML per role). Defaults come from `contracts::defaults`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// SpacetimeDB on main, e.g. "http://127.0.0.1:3000".
    pub spacetime_uri: String,
    pub spacetime_db: String,
    /// Publisher token (admin). Never commit it; the binary reads SPACETIME_TOKEN.
    #[serde(skip)]
    pub token: Option<String>,
    pub llama_server_path: String,
    pub model_path: String,
    pub model_id: String,
    pub ctx_size: u32,
    /// llama-server bind address: 0.0.0.0 so other machines can send prompts (it's our private router, R-09).
    pub host: String,
    pub port: u16,
    pub log_dir: PathBuf,
    pub ready_timeout_s: u64,
    /// Wait for this many helpers before the first bring-up, so startup doesn't load the model twice.
    pub min_helpers: usize,
    /// Checkpoint S speed flags, passed to llama-server as-is. `flash_attn`: "on" | "off" | "auto" (-fa).
    pub flash_attn: Option<String>,
    /// Extra llama-server arguments, e.g. ["-ctk","q8_0","-ctv","q8_0"] or ["--spec-type","ngram-simple"].
    pub extra_args: Vec<String>,
    /// -ts layer shares, RPC devices first, main last (empty = llama.cpp splits by free memory).
    pub tensor_split: Vec<f64>,
    /// Address other machines use for llama-server, shown as the endpoint in cluster_status (Ponytail #6).
    /// None: `host`, or main's LAN IP toward the first helper when `host` is a wildcard.
    pub public_host: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        use contracts::defaults as d;
        Self {
            spacetime_uri: format!("http://127.0.0.1:{}", d::SPACETIME_PORT),
            spacetime_db: d::DB_NAME.into(),
            token: None,
            llama_server_path: "llama-server".into(),
            model_path: String::new(),
            model_id: String::new(),
            ctx_size: 4096,
            host: "0.0.0.0".into(),
            port: d::LLAMA_SERVER_PORT,
            log_dir: std::env::temp_dir(),
            ready_timeout_s: d::READY_TIMEOUT_S,
            public_host: None,
            min_helpers: 1,
            flash_attn: None,
            extra_args: vec![],
            tensor_split: vec![],
        }
    }
}

const STEP_TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(200);

pub struct Coordinator {
    pub cfg: Config,
    pub conn: DbConnection,
    server: Option<LlamaServer>,
    /// Helpers holding layers in the running plan (become ACTIVE at SERVING).
    active: Vec<String>,
    plan_id: String,
    plans: u32,
    /// Helpers whose join failed this run; left out until their agent restarts (goes LOST, then registers again).
    skipped: Vec<String>,
    /// Joined helpers already sent start_rpc_server, and when that join began (command watermark + clock).
    starting: Vec<String>,
    join: Option<(u64, Instant)>,
}

impl Coordinator {
    /// Connect, subscribe to the tables the cycle reads, and wait until the cache holds them.
    pub fn connect(cfg: Config) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let conn = DbConnection::builder()
            .with_uri(cfg.spacetime_uri.clone())
            .with_database_name(cfg.spacetime_db.clone())
            .with_token(cfg.token.clone())
            .on_connect(move |conn, _, _| {
                conn.subscription_builder()
                    .on_applied(move |_| {
                        let _ = tx.send(());
                    })
                    .subscribe([
                        "SELECT * FROM node",
                        "SELECT * FROM heartbeat",
                        "SELECT * FROM command",
                        "SELECT * FROM cluster_status",
                    ]);
            })
            .build()
            .map_err(|e| format!("E_TRANSPORT: connect {}: {e}", cfg.spacetime_uri))?;
        conn.run_threaded();
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "E_TRANSPORT: subscription not applied in 10 s".to_string())?;
        let cluster = conn.db.cluster_status().iter().next();
        Ok(Self {
            cfg,
            conn,
            server: None,
            active: vec![],
            plan_id: cluster.map(|c| c.plan_id).unwrap_or_default(),
            plans: 0,
            skipped: vec![],
            starting: vec![],
            join: None,
        })
    }

    pub fn identity(&self) -> Option<Identity> {
        self.conn.try_identity()
    }

    pub fn active(&self) -> &[String] {
        &self.active
    }

    /// llama-server is up (SERVING).
    pub fn is_serving(&self) -> bool {
        self.server.is_some()
    }

    pub fn cluster_state(&self) -> String {
        self.conn
            .db
            .cluster_status()
            .iter()
            .next()
            .map(|c| c.cluster_state)
            .unwrap_or_default()
    }

    /// Online helpers that aren't LOST or skipped, in node_id order (= the --rpc order, so RPC0 is the first id).
    pub fn helpers(&self) -> Vec<NodeRow> {
        let mut h: Vec<NodeRow> = self
            .online_helpers()
            .into_iter()
            .filter(|n| !self.skipped.contains(&n.node_id))
            .collect();
        h.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        h
    }

    fn online_helpers(&self) -> Vec<NodeRow> {
        self.conn
            .db
            .node()
            .iter()
            .filter(|n| n.role == "helper" && n.online && n.state != "LOST")
            .collect()
    }

    /// PRD §5.5 steps 2-6: plan, start helpers, launch, wait for /health, SERVING.
    pub fn bring_up(&mut self) -> Result<(), String> {
        // Ponytail #2: a coordinator that was killed (or crashed mid-bring-up) leaves the row in SERVING,
        // STARTING_HELPERS, ... and the module only allows PLANNED from EMPTY/STOPPED/FAILED. STOPPED is legal from
        // any state, so reset first; the module also moves ACTIVE nodes back to READY.
        if !matches!(
            self.cluster_state().as_str(),
            "" | "EMPTY" | "STOPPED" | "FAILED" | "SERVING" | "DEGRADED"
        ) {
            self.set_state("STOPPED", &[], "")?;
        }
        // Ponytail #3: only commands issued from here on count as this bring-up's failures.
        let since = self.max_command_id();
        let helpers = self.helpers();
        if helpers.is_empty() {
            return Err("E_MODEL_DOES_NOT_FIT: no helpers online".into());
        }
        let ids: Vec<String> = helpers.iter().map(|n| n.node_id.clone()).collect();

        // 2. Plan. Time-based id, so each run gets its own plan row and llama-server log (Ponytail #14).
        self.plans += 1;
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.plan_id = format!("plan-{secs}-{}", self.plans);
        let plan_json = plan_for(&self.plan_id, &self.cfg, &ids);
        let (cb, rx) = done();
        self.conn
            .reducers
            .set_split_plan_then(
                self.plan_id.clone(),
                self.cfg.model_id.clone(),
                "llama_default".into(),
                true,
                plan_json,
                cb,
            )
            .map_err(|e| e.to_string())?;
        wait(rx)?;
        // SERVING -> PLANNED is the join re-plan; a module published before it allowed that rejects it, so fall
        // back to STOPPED first (STOPPED is legal from any state).
        if self.set_state("PLANNED", &[], "").is_err() {
            self.set_state("STOPPED", &[], "")?;
            self.set_state("PLANNED", &[], "")?;
        }

        // 3. Start rpc-server on every helper that isn't already serving.
        self.set_state("STARTING_HELPERS", &[], "")?;
        for n in &helpers {
            if self.rpc_addr(n).is_none() {
                let (cb, rx) = done();
                self.conn
                    .reducers
                    .issue_command_then(
                        n.node_id.clone(),
                        "start_rpc_server".into(),
                        start_args(n),
                        cb,
                    )
                    .map_err(|e| e.to_string())?;
                wait(rx)?;
            }
        }
        let deadline = Instant::now() + STEP_TIMEOUT;
        // A helper whose rpc-server fails (or isn't up in 60 s) is skipped, and the pool comes up on the others;
        // only when none is left does the bring-up fail (one bad laptop must not block the demo).
        let mut helpers = helpers;
        let rpc = loop {
            for n in helpers.clone() {
                if let Some(f) = self.failed_command(std::slice::from_ref(&n.node_id), since) {
                    self.skip(&n.node_id, &f);
                    helpers.retain(|h| h.node_id != n.node_id);
                }
            }
            if helpers.is_empty() {
                return self.fail("E_RPC_SERVER_START: no helper's rpc-server started");
            }
            let addrs: Vec<Option<String>> = helpers.iter().map(|n| self.rpc_addr(n)).collect();
            if addrs.iter().all(Option::is_some) {
                break addrs.into_iter().flatten().collect::<Vec<_>>();
            }
            if Instant::now() > deadline {
                let slow: Vec<String> = helpers
                    .iter()
                    .filter(|n| self.rpc_addr(n).is_none())
                    .map(|n| n.node_id.clone())
                    .collect();
                for id in &slow {
                    self.skip(id, "rpc-server not up in 60 s");
                }
                helpers.retain(|h| !slow.contains(&h.node_id));
                if helpers.is_empty() {
                    return self.fail("E_RPC_SERVER_START: helpers not ready in 60 s");
                }
            }
            sleep(POLL);
        };
        let ids: Vec<String> = helpers.iter().map(|n| n.node_id.clone()).collect();

        // 4-6. Launch llama-server over the helpers (RPC devices first, main's own GPU last: O-23) and wait.
        self.set_state("LAUNCHING", &[], "")?;
        let spec = LaunchSpec {
            schema_version: contracts::SCHEMA_VERSION,
            plan_id: self.plan_id.clone(),
            llama_server_path: self.cfg.llama_server_path.clone(),
            model_path: self.cfg.model_path.clone(),
            rpc,
            n_gpu_layers: Some("99".into()),
            ctx_size: Some(self.cfg.ctx_size),
            host: Some(self.cfg.host.clone()),
            port: Some(self.cfg.port.into()),
            metrics: Some(true),
            flash_attn: self.cfg.flash_attn.clone(),
            extra_args: self.cfg.extra_args.clone(),
            tensor_split: self.cfg.tensor_split.clone(),
            ..Default::default()
        };
        let log = self
            .cfg
            .log_dir
            .join(format!("llama-server-{}.log", self.plan_id));
        let mut server = match launch::start(&spec, &log) {
            Ok(s) => s,
            Err(e) => return self.fail(&e.to_string()),
        };
        if let Err(e) = server.wait_ready(Duration::from_secs(self.cfg.ready_timeout_s)) {
            return self.fail(&format!("{e} (log: {})", log.display()));
        }
        self.server = Some(server);
        self.active = ids;
        let endpoint = format!("http://{}:{}/v1", self.public_host(), self.cfg.port);
        let active = self.active.clone();
        self.set_state("SERVING", &active, &endpoint)
    }

    /// One pass of the watch loop (call every ~500 ms). Records a llama-server exit; re-plans when a helper joins.
    pub fn tick(&mut self) -> Result<(), String> {
        if let Some(status) = self.server.as_mut().and_then(LlamaServer::exited) {
            self.server = None;
            self.event("llama_server_exited", &status);
            // D-14: no automatic recovery in the demo; the pool goes to STOPPED (never FAILED on screen, Aayo
            // 11:20pm) and an operator restarts. The reason is in the event log.
            self.active.clear();
            let _ = self.set_state("STOPPED", &[], "");
            return Err(format!("llama-server exited: {status}"));
        }
        // A skipped helper whose agent restarted (LOST, then back) gets another chance as a fresh join.
        let online: Vec<String> = self
            .online_helpers()
            .into_iter()
            .map(|n| n.node_id)
            .collect();
        self.skipped.retain(|id| online.contains(id));
        // A helper that went LOST leaves the active set, so when its agent comes back it takes the normal join path
        // below and the pool re-plans back to SERVING (#11 07:27Z/07:30Z: before, it stayed DEGRADED forever).
        // Leaving itself still triggers no re-plan (D-14): llama-server keeps serving on the old plan meanwhile.
        self.active.retain(|id| online.contains(id));
        if self.server.is_none() {
            return Ok(());
        }
        let joined: Vec<NodeRow> = self
            .helpers()
            .into_iter()
            .filter(|n| !self.active.contains(&n.node_id))
            .collect();
        if joined.is_empty() {
            self.join = None;
            self.starting.clear();
            return Ok(());
        }

        // Safe join (Ponytail #4): the pool keeps SERVING while each joined helper starts rpc-server. Re-plan only
        // when all of them are up; a helper that fails or isn't up in 60 s is skipped and the pool keeps serving.
        let since = match self.join {
            Some((s, _)) => s,
            None => {
                let s = self.max_command_id();
                self.join = Some((s, Instant::now()));
                s
            }
        };
        for n in &joined {
            if self.rpc_addr(n).is_none() && !self.starting.contains(&n.node_id) {
                let (cb, rx) = done();
                self.conn
                    .reducers
                    .issue_command_then(
                        n.node_id.clone(),
                        "start_rpc_server".into(),
                        start_args(n),
                        cb,
                    )
                    .map_err(|e| e.to_string())?;
                wait(rx)?;
                self.starting.push(n.node_id.clone());
            }
        }
        let timed_out = self.join.is_some_and(|(_, t)| t.elapsed() > STEP_TIMEOUT);
        let mut ready = vec![];
        let mut waiting = false;
        for n in &joined {
            let id = n.node_id.clone();
            if let Some(f) = self.failed_command(std::slice::from_ref(&id), since) {
                self.skip(&id, &f);
            } else if self.rpc_addr(n).is_some() {
                ready.push(id);
            } else if timed_out {
                self.skip(&id, "rpc-server not up in 60 s");
            } else {
                waiting = true;
            }
        }
        if waiting || ready.is_empty() {
            return Ok(());
        }
        self.join = None;
        self.starting.clear();
        eprintln!("coordinator: re-planning for joined helpers {ready:?}");
        let previous = self.active.clone();
        if let Some(s) = self.server.take() {
            s.stop(); // bring_up moves the row SERVING -> PLANNED (no STOPPED on screen)
        }
        match self.bring_up() {
            Ok(()) => Ok(()),
            Err(e) => {
                // Keep the pool: leave the joined helpers out and bring the previous set back.
                for id in ready.iter().filter(|id| !previous.contains(id)) {
                    self.skipped.push(id.clone());
                }
                self.event(
                    "launch_failed",
                    &format!("join of {ready:?} failed ({e}); restoring the previous pool"),
                );
                self.bring_up()
            }
        }
    }

    fn skip(&mut self, id: &str, why: &str) {
        eprintln!("coordinator: {id} not added: {why}");
        self.event(
            "launch_failed",
            &format!("{id} not added to the pool: {why}"),
        );
        self.skipped.push(id.to_string());
        self.starting.retain(|s| s != id);
    }

    fn max_command_id(&self) -> u64 {
        self.conn
            .db
            .command()
            .iter()
            .map(|c| c.command_id)
            .max()
            .unwrap_or(0)
    }

    /// R4.13 stop: llama-server first, then every helper's rpc-server, then STOPPED.
    pub fn shutdown(&mut self) -> Result<(), String> {
        let helpers = std::mem::take(&mut self.active);
        self.stop_server()?;
        // Ponytail #14: keep going past a helper that's offline, so the others still stop.
        for id in helpers {
            let (cb, rx) = done();
            let sent = self
                .conn
                .reducers
                .issue_command_then(id.clone(), "stop_rpc_server".into(), "{}".into(), cb)
                .map_err(|e| e.to_string())
                .and_then(|_| wait(rx));
            if let Err(e) = sent {
                eprintln!("coordinator: stop_rpc_server {id}: {e}");
            }
        }
        Ok(())
    }

    fn stop_server(&mut self) -> Result<(), String> {
        if let Some(s) = self.server.take() {
            s.stop();
        }
        self.set_state("STOPPED", &[], "")
    }

    /// `host:port` of a helper's rpc-server once its agent reports it running and accepting.
    fn rpc_addr(&self, n: &NodeRow) -> Option<String> {
        let hb = self.conn.db.heartbeat().node_id().find(&n.node_id)?;
        let port = hb
            .rpc_port
            .filter(|_| hb.rpc_state == "running" && hb.port_accepting)?;
        let host = n
            .addresses
            .first()
            .cloned()
            .unwrap_or_else(|| n.hostname.clone());
        Some(format!("{host}:{port}"))
    }

    fn failed_command(&self, ids: &[String], since: u64) -> Option<String> {
        self.conn
            .db
            .command()
            .iter()
            .find(|c| {
                c.command_id > since
                    && c.status == "failed"
                    && c.kind == "start_rpc_server"
                    && ids.contains(&c.node_id)
            })
            .map(|c| format!("{}: {}", c.node_id, c.error_message.unwrap_or_default()))
    }

    fn set_state(&self, state: &str, active: &[String], endpoint: &str) -> Result<(), String> {
        let (cb, rx) = done();
        self.conn
            .reducers
            .set_cluster_status_then(
                state.into(),
                self.plan_id.clone(),
                self.cfg.model_id.clone(),
                endpoint.into(),
                active.to_vec(),
                "{}".into(),
                cb,
            )
            .map_err(|e| e.to_string())?;
        wait(rx)
    }

    /// A bring-up that didn't reach SERVING. Aayo (11:20pm): nothing on screen says FAILED; the cluster goes to
    /// STOPPED and the reason goes to the event log as `launch_failed`.
    fn fail(&mut self, why: &str) -> Result<(), String> {
        self.server = None; // kill-on-drop
        self.event("launch_failed", why);
        let _ = self.set_state("STOPPED", &[], "");
        Err(why.to_string())
    }

    /// Host other machines should use for llama-server (Ponytail #6): `public_host`, else `host` unless it's a
    /// wildcard, else main's LAN IP toward the first helper (a UDP connect sends no packet, it only picks the route).
    fn public_host(&self) -> String {
        if let Some(h) = &self.cfg.public_host {
            return h.clone();
        }
        if !matches!(self.cfg.host.as_str(), "0.0.0.0" | "::") {
            return self.cfg.host.clone();
        }
        let toward = self
            .active
            .iter()
            .chain(self.helpers().iter().map(|n| &n.node_id))
            .find_map(|id| self.conn.db.node().node_id().find(id))
            .and_then(|n| n.addresses.first().cloned());
        toward
            .and_then(|ip| {
                let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
                s.connect((ip.as_str(), 9)).ok()?;
                Some(s.local_addr().ok()?.ip().to_string())
            })
            .unwrap_or_else(|| "127.0.0.1".into())
    }

    fn event(&self, kind: &str, message: &str) {
        let _ = self.conn.reducers.record_event(
            kind.into(),
            String::new(),
            self.plan_id.clone(),
            message.into(),
            "{}".into(),
        );
    }
}

/// §6.5 plan for `ids` in --rpc order. Strategy llama_default: no -ts, llama.cpp splits by free memory.
/// shortcut: until WP-5's builder (#20) merges; then call it here and pass its integer -ts into the LaunchSpec.
fn plan_for(plan_id: &str, cfg: &Config, ids: &[String]) -> String {
    let mut order: Vec<String> = (0..ids.len()).map(|i| format!("RPC{i}")).collect();
    order.push("MTL0".into()); // O-23: main's own GPU holds layers, last in llama.cpp's device order
    serde_json::json!({
        "schema_version": contracts::SCHEMA_VERSION,
        "plan_id": plan_id,
        "model_id": cfg.model_id,
        "strategy": "llama_default",
        "ctx_size": cfg.ctx_size,
        "device_order": order,
        "tensor_split": null,
        "assignments": ids.iter().enumerate()
            .map(|(i, id)| serde_json::json!({ "node_id": id, "llama_device": format!("RPC{i}") }))
            .collect::<Vec<_>>(),
        "fits": true,
        "warnings": ["llama_default: llama.cpp places layers by free memory; WP-5 planner not wired yet"],
    })
    .to_string()
}

/// start_rpc_server args (§6.3). Only GPU devices: on macOS rpc-server also offers BLAS by default, and layers
/// placed there crash it ("ggml-blas: unsupported op RMS_NORM", found by Agent-P1 on b8665), so pass -d explicitly.
fn start_args(n: &NodeRow) -> String {
    let gpus: Vec<&str> = n
        .devices
        .iter()
        .map(|d| d.name.as_str())
        .filter(|name| !name.starts_with("BLAS") && !name.starts_with("CPU"))
        .collect();
    serde_json::json!({ "bind": null, "port": null, "devices": if gpus.is_empty() { None } else { Some(gpus) }, "cache": true })
        .to_string()
}

type ReducerResult = Result<Result<(), String>, spacetimedb_sdk::__codegen::InternalError>;

/// A reducer callback plus the receiver that gets its outcome, so each step can wait for the module's answer.
fn done() -> (
    impl FnOnce(&ReducerEventContext, ReducerResult) + Send + 'static,
    mpsc::Receiver<Result<(), String>>,
) {
    let (tx, rx) = mpsc::channel();
    let cb = move |_: &ReducerEventContext, r: ReducerResult| {
        let _ = tx.send(r.unwrap_or_else(|e| Err(format!("E_TRANSPORT: {e:?}"))));
    };
    (cb, rx)
}

fn wait(rx: mpsc::Receiver<Result<(), String>>) -> Result<(), String> {
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| "E_TRANSPORT: no reducer reply in 10 s".to_string())?
}
