//! stdb-module: the pool's control plane as a SpacetimeDB 2.10.1 module.
//! WHY: Spacetime holds all control-plane state (nodes, commands, plan, cluster, events) and enforces the PRD §5.6
//!      state machine in reducers. Agents (WP-3) and the coordinator (WP-4) are thin Rust clients that supervise
//!      llama.cpp processes and report through these reducers.
//! Nested §6 blobs (NodeCapabilities, CommandResult data, SplitPlan, LaunchSpec) are stored as the WP-2 serde JSON in
//! `*_json: String` columns; scalar fields used for logic or queries are real columns.

use spacetimedb::{reducer, table, Identity, ReducerContext, ScheduleAt, Table, TimeDuration, Timestamp};

// ---------- config (O-26; module constants until a config table is needed) ----------
const NODE_TIMEOUT_MS: i64 = 6_000;
const SWEEP_EVERY_MS: i64 = 500;

// ---------- tables ----------

/// Private: the publisher's identity. Coordinator runs with this token; admin-only reducers check it.
#[table(accessor = admin)]
pub struct Admin {
    #[primary_key]
    identity: Identity,
}

/// One row per machine (§6.1 + latest §6.2). State per §5.6:
/// REGISTERED -> READY -> ACTIVE -> LOST; LOST -> REGISTERED on re-register.
#[table(accessor = node, public)]
pub struct Node {
    #[primary_key]
    pub node_id: String,
    #[unique]
    pub identity: Identity,
    pub role: String,            // "main" | "helper"
    pub state: String,           // REGISTERED | READY | ACTIVE | LOST
    pub hostname: String,
    pub os_family: String,
    pub llama_build: u32,
    pub rpc_protocol: String,
    pub usable_bytes: u64,
    pub rpc_addr: String,        // "192.168.8.12:50052"
    pub rpc_state: String,       // §6.2 rpc_server.state
    pub last_heartbeat_at: Timestamp,
    pub hb_seq: u64,
    pub cpu_util_pct: f32,
    pub last_error_code: String, // "" = none; else §6.11 code
    pub caps_json: String,       // full §6.1 NodeCapabilities
    pub heartbeat_json: String,  // full latest §6.2 Heartbeat
}

/// §6.3 Command + CommandResult in one row. The agent subscribes to `command WHERE node_id = '<me>'`.
#[table(accessor = command, public)]
pub struct Command {
    #[primary_key]
    #[auto_inc]
    pub command_id: u64,
    #[index(btree)]
    pub node_id: String,
    pub kind: String,        // start_rpc_server | stop_rpc_server | probe | speed_probe | netcheck
    pub args_json: String,
    pub status: String,      // pending | running | ok | error
    pub result_json: String, // §6.3 CommandResult.data
    pub error_code: String,
    pub created_at: Timestamp,
    pub finished_at: Option<Timestamp>,
}

/// §6.4 ModelInfo (coordinator parses the GGUF on main and uploads it).
#[table(accessor = model, public)]
pub struct Model {
    #[primary_key]
    pub model_id: String,
    pub file_bytes: u64,
    pub n_layers: u32,
    pub info_json: String,
}

/// §6.5 SplitPlan.
#[table(accessor = plan, public)]
pub struct Plan {
    #[primary_key]
    pub plan_id: String,
    pub model_id: String,
    pub strategy: String,
    pub status: String, // created | rejected | active | superseded
    pub plan_json: String,
    pub created_at: Timestamp,
}

/// Single row (id = 0): §5.6 cluster state + §6.6 LaunchSpec + top-level §6.7 fields.
#[table(accessor = cluster, public)]
pub struct Cluster {
    #[primary_key]
    pub id: u8,
    pub state: String, // EMPTY PLANNED STARTING_HELPERS LAUNCHING SERVING DEGRADED RECOVERING FAILED STOPPED
    pub plan_id: String,
    pub endpoint: String,
    pub llama_server_pid: u32,
    pub launch_spec_json: String,
    pub last_request_json: String,
    pub updated_at: Timestamp,
}

/// §6.10 Event log. Written in the same transaction as the state change it describes.
#[table(accessor = event, public)]
pub struct Event {
    #[primary_key]
    #[auto_inc]
    pub seq: u64,
    pub at: Timestamp,
    pub kind: String,
    pub node_id: String,
    pub plan_id: String,
    pub message: String,
    pub data_json: String,
}

/// §6.8 BenchResult rows (WP-8 also keeps its JSON-lines file).
#[table(accessor = bench_result, public)]
pub struct BenchResult {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub run_id: String,
    pub scenario: String,
    pub gen_tokens_per_s: Option<f64>,
    pub ttft_ms: Option<f64>,
    pub raw_json: String,
}

/// Schedule table: fires `sweep` every SWEEP_EVERY_MS (WP-9 detection).
#[table(accessor = sweep_schedule, scheduled(sweep))]
pub struct SweepSchedule {
    #[primary_key]
    #[auto_inc]
    scheduled_id: u64,
    scheduled_at: ScheduleAt,
}

// ---------- helpers ----------

fn is_admin(ctx: &ReducerContext) -> bool {
    ctx.db.admin().identity().find(ctx.sender()).is_some()
}

fn require_admin(ctx: &ReducerContext) -> Result<(), String> {
    if is_admin(ctx) { Ok(()) } else { Err("E_CONFIG: admin only".into()) }
}

fn log_event(ctx: &ReducerContext, kind: &str, node_id: &str, plan_id: &str, message: String) {
    ctx.db.event().insert(Event {
        seq: 0, // auto_inc
        at: ctx.timestamp,
        kind: kind.into(),
        node_id: node_id.into(),
        plan_id: plan_id.into(),
        message,
        data_json: "{}".into(),
    });
}

/// §5.6 cluster transitions. STOPPED is allowed from any state.
fn cluster_transition_ok(from: &str, to: &str) -> bool {
    if to == "STOPPED" { return true; }
    matches!(
        (from, to),
        ("EMPTY" | "STOPPED" | "FAILED", "PLANNED")
            | ("PLANNED", "STARTING_HELPERS")
            | ("STARTING_HELPERS", "LAUNCHING")
            | ("LAUNCHING", "SERVING")
            | ("STARTING_HELPERS" | "LAUNCHING", "FAILED")
            | ("SERVING", "DEGRADED")
            | ("DEGRADED", "RECOVERING")
            | ("RECOVERING", "SERVING" | "FAILED")
    )
}

fn node_transition_ok(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("REGISTERED", "READY")
            | ("READY", "ACTIVE")
            | ("ACTIVE", "READY")
            | ("REGISTERED" | "READY" | "ACTIVE", "LOST")
            | ("LOST", "REGISTERED")
    )
}

fn cluster_row(ctx: &ReducerContext) -> Cluster {
    ctx.db.cluster().id().find(0).expect("cluster row is created in init")
}

// ---------- lifecycle ----------

#[reducer(init)]
pub fn init(ctx: &ReducerContext) {
    ctx.db.admin().insert(Admin { identity: ctx.sender() });
    ctx.db.cluster().insert(Cluster {
        id: 0,
        state: "EMPTY".into(),
        plan_id: String::new(),
        endpoint: String::new(),
        llama_server_pid: 0,
        launch_spec_json: String::new(),
        last_request_json: String::new(),
        updated_at: ctx.timestamp,
    });
    ctx.db.sweep_schedule().insert(SweepSchedule {
        scheduled_id: 0,
        scheduled_at: ScheduleAt::Interval(TimeDuration::from_micros(SWEEP_EVERY_MS * 1_000)),
    });
}

#[reducer(client_disconnected)]
pub fn client_disconnected(ctx: &ReducerContext) {
    // Clean close or TCP reset: mark the node LOST at once instead of waiting for the sweep.
    if let Some(mut n) = ctx.db.node().identity().find(ctx.sender()) {
        if n.state != "LOST" {
            let id = n.node_id.clone();
            n.state = "LOST".into();
            ctx.db.node().node_id().update(n);
            log_event(ctx, "node_lost", &id, "", "agent disconnected".into());
            degrade_if_active(ctx, &id);
        }
    }
}

fn degrade_if_active(ctx: &ReducerContext, _node_id: &str) {
    // Simplification: any lost node while SERVING degrades the cluster (O-11 decides recovery).
    let mut c = cluster_row(ctx);
    if c.state == "SERVING" {
        c.state = "DEGRADED".into();
        c.updated_at = ctx.timestamp;
        let plan = c.plan_id.clone();
        ctx.db.cluster().id().update(c);
        log_event(ctx, "recovery_started", "", &plan, "node lost while serving".into());
    }
}

// ---------- agent reducers (WP-3) ----------

#[reducer]
pub fn register_node(
    ctx: &ReducerContext,
    node_id: String,
    role: String,
    hostname: String,
    os_family: String,
    llama_build: u32,
    rpc_protocol: String,
    usable_bytes: u64,
    rpc_addr: String,
    caps_json: String,
) -> Result<(), String> {
    // A node_id is bound to the first identity that registers it (token file on that machine).
    let existing = ctx.db.node().node_id().find(node_id.clone());
    let kind = match &existing {
        None => "node_registered",
        Some(n) if n.identity != ctx.sender() => return Err(format!("E_CONFIG: node_id {node_id} owned by another identity")),
        Some(n) if n.state == "LOST" => "node_recovered",
        Some(_) => "node_capabilities_changed",
    };
    let row = Node {
        node_id: node_id.clone(),
        identity: ctx.sender(),
        role,
        state: match &existing { Some(n) if n.state != "LOST" => n.state.clone(), _ => "REGISTERED".into() },
        hostname,
        os_family,
        llama_build,
        rpc_protocol,
        usable_bytes,
        rpc_addr,
        rpc_state: existing.as_ref().map(|n| n.rpc_state.clone()).unwrap_or_else(|| "not_started".into()),
        last_heartbeat_at: ctx.timestamp,
        hb_seq: 0,
        cpu_util_pct: 0.0,
        last_error_code: String::new(),
        caps_json,
        heartbeat_json: String::new(),
    };
    if existing.is_some() { ctx.db.node().node_id().update(row); } else { ctx.db.node().insert(row); }
    log_event(ctx, kind, &node_id, "", String::new());
    check_versions(ctx);
    Ok(())
}

fn check_versions(ctx: &ReducerContext) {
    let builds: std::collections::BTreeSet<u32> =
        ctx.db.node().iter().filter(|n| n.state != "LOST").map(|n| n.llama_build).collect();
    if builds.len() > 1 {
        log_event(ctx, "version_mismatch", "", "", format!("llama.cpp builds differ: {builds:?}"));
    }
}

#[reducer]
pub fn heartbeat(
    ctx: &ReducerContext,
    seq: u64,
    rpc_state: String,
    cpu_util_pct: f32,
    last_error_code: String,
    heartbeat_json: String,
) -> Result<(), String> {
    let mut n = ctx.db.node().identity().find(ctx.sender()).ok_or("E_CONFIG: register_node first")?;
    if seq <= n.hb_seq && n.hb_seq != 0 { return Ok(()); } // stale or duplicate
    if n.rpc_state != rpc_state {
        let kind = match rpc_state.as_str() { "running" => "rpc_server_started", "exited" | "failed" => "rpc_server_exited", _ => "" };
        if !kind.is_empty() { log_event(ctx, kind, &n.node_id, "", String::new()); }
    }
    // READY = rpc-server healthy (helpers) or not needed (main).
    if n.state == "REGISTERED" && (rpc_state == "running" || n.role == "main") && node_transition_ok("REGISTERED", "READY") {
        n.state = "READY".into();
    }
    n.hb_seq = seq;
    n.rpc_state = rpc_state;
    n.cpu_util_pct = cpu_util_pct;
    n.last_error_code = last_error_code;
    n.heartbeat_json = heartbeat_json;
    n.last_heartbeat_at = ctx.timestamp;
    ctx.db.node().node_id().update(n);
    Ok(())
}

#[reducer]
pub fn ack_command(ctx: &ReducerContext, command_id: u64) -> Result<(), String> {
    let mut c = own_command(ctx, command_id)?;
    if c.status != "pending" { return Err(format!("command {command_id} is {}", c.status)); }
    c.status = "running".into();
    ctx.db.command().command_id().update(c);
    Ok(())
}

#[reducer]
pub fn complete_command(ctx: &ReducerContext, command_id: u64, ok: bool, result_json: String, error_code: String) -> Result<(), String> {
    let mut c = own_command(ctx, command_id)?;
    if c.status == "ok" || c.status == "error" { return Ok(()); } // idempotent
    c.status = if ok { "ok".into() } else { "error".into() };
    c.result_json = result_json;
    c.error_code = error_code;
    c.finished_at = Some(ctx.timestamp);
    ctx.db.command().command_id().update(c);
    Ok(())
}

fn own_command(ctx: &ReducerContext, command_id: u64) -> Result<Command, String> {
    let c = ctx.db.command().command_id().find(command_id).ok_or("no such command")?;
    let n = ctx.db.node().identity().find(ctx.sender()).ok_or("E_CONFIG: not a node")?;
    if c.node_id != n.node_id { return Err("E_CONFIG: command belongs to another node".into()); }
    Ok(c)
}

// ---------- coordinator reducers (WP-4, admin only) ----------

#[reducer]
pub fn send_command(ctx: &ReducerContext, node_id: String, kind: String, args_json: String) -> Result<(), String> {
    require_admin(ctx)?;
    ctx.db.node().node_id().find(node_id.clone()).ok_or("E_NODE_UNREACHABLE: unknown node")?;
    ctx.db.command().insert(Command {
        command_id: 0,
        node_id,
        kind,
        args_json,
        status: "pending".into(),
        result_json: String::new(),
        error_code: String::new(),
        created_at: ctx.timestamp,
        finished_at: None,
    });
    Ok(())
}

#[reducer]
pub fn upsert_model(ctx: &ReducerContext, model_id: String, file_bytes: u64, n_layers: u32, info_json: String) -> Result<(), String> {
    require_admin(ctx)?;
    let row = Model { model_id: model_id.clone(), file_bytes, n_layers, info_json };
    if ctx.db.model().model_id().find(model_id).is_some() { ctx.db.model().model_id().update(row); } else { ctx.db.model().insert(row); }
    Ok(())
}

/// Stores a plan computed by WP-5. If the planner crate compiles to wasm32, call it here instead and take only
/// (model_id, strategy) as arguments: the plan is then computed inside the database.
#[reducer]
pub fn create_plan(ctx: &ReducerContext, plan_id: String, model_id: String, strategy: String, fits: bool, plan_json: String) -> Result<(), String> {
    require_admin(ctx)?;
    ctx.db.model().model_id().find(model_id.clone()).ok_or("E_MODEL_NOT_FOUND")?;
    let status = if fits { "created" } else { "rejected" };
    ctx.db.plan().insert(Plan { plan_id: plan_id.clone(), model_id, strategy, status: status.into(), plan_json, created_at: ctx.timestamp });
    log_event(ctx, if fits { "plan_created" } else { "plan_rejected" }, "", &plan_id, String::new());
    if fits { set_state(ctx, "PLANNED", Some(plan_id))?; }
    Ok(())
}

#[reducer]
pub fn set_cluster_state(ctx: &ReducerContext, state: String) -> Result<(), String> {
    require_admin(ctx)?;
    set_state(ctx, &state, None)
}

fn set_state(ctx: &ReducerContext, to: &str, plan_id: Option<String>) -> Result<(), String> {
    let mut c = cluster_row(ctx);
    if !cluster_transition_ok(&c.state, to) {
        return Err(format!("E_INTERNAL: illegal cluster transition {} -> {to}", c.state));
    }
    let event = match to { "LAUNCHING" => "launch_started", "FAILED" => "recovery_failed", _ => "" };
    if let Some(p) = plan_id { c.plan_id = p; }
    c.state = to.into();
    c.updated_at = ctx.timestamp;
    let plan = c.plan_id.clone();
    ctx.db.cluster().id().update(c);
    if !event.is_empty() { log_event(ctx, event, "", &plan, String::new()); }
    Ok(())
}

/// llama-server answered /health. Marks the cluster SERVING and the plan's nodes ACTIVE.
#[reducer]
pub fn llama_ready(ctx: &ReducerContext, pid: u32, endpoint: String, launch_spec_json: String, active_node_ids: Vec<String>) -> Result<(), String> {
    require_admin(ctx)?;
    let from = cluster_row(ctx).state;
    set_state(ctx, "SERVING", None)?;
    let mut c = cluster_row(ctx);
    c.llama_server_pid = pid;
    c.endpoint = endpoint;
    c.launch_spec_json = launch_spec_json;
    let plan = c.plan_id.clone();
    ctx.db.cluster().id().update(c);
    for id in active_node_ids {
        if let Some(mut n) = ctx.db.node().node_id().find(id) {
            if node_transition_ok(&n.state, "ACTIVE") { n.state = "ACTIVE".into(); ctx.db.node().node_id().update(n); }
        }
    }
    let kind = if from == "RECOVERING" { "recovery_succeeded" } else { "llama_server_ready" };
    log_event(ctx, kind, "", &plan, String::new());
    Ok(())
}

/// Free-form events the clients observe (placement_mismatch, request_failed, llama_server_exited, netcheck_result, ...).
#[reducer]
pub fn report_event(ctx: &ReducerContext, kind: String, node_id: String, message: String, data_json: String) -> Result<(), String> {
    if !is_admin(ctx) && ctx.db.node().identity().find(ctx.sender()).is_none() {
        return Err("E_CONFIG: unknown sender".into());
    }
    let plan = cluster_row(ctx).plan_id;
    ctx.db.event().insert(Event { seq: 0, at: ctx.timestamp, kind, node_id, plan_id: plan, message, data_json });
    Ok(())
}

#[reducer]
pub fn record_bench(ctx: &ReducerContext, run_id: String, scenario: String, gen_tokens_per_s: Option<f64>, ttft_ms: Option<f64>, raw_json: String) -> Result<(), String> {
    require_admin(ctx)?;
    ctx.db.bench_result().insert(BenchResult { id: 0, run_id, scenario, gen_tokens_per_s, ttft_ms, raw_json });
    Ok(())
}

// ---------- WP-9 detection ----------

#[reducer]
pub fn sweep(ctx: &ReducerContext, _arg: SweepSchedule) -> Result<(), String> {
    // Only the scheduler may call this.
    if ctx.sender() != ctx.database_identity() { return Err("sweep is scheduler-only".into()); }
    let now = ctx.timestamp;
    let stale: Vec<Node> = ctx
        .db
        .node()
        .iter()
        .filter(|n| n.state != "LOST")
        .filter(|n| now.time_duration_since(n.last_heartbeat_at).map(|d| d.to_micros() / 1_000 > NODE_TIMEOUT_MS).unwrap_or(false))
        .collect();
    for mut n in stale {
        let id = n.node_id.clone();
        n.state = "LOST".into();
        ctx.db.node().node_id().update(n);
        log_event(ctx, "node_lost", &id, "", format!("no heartbeat for {NODE_TIMEOUT_MS} ms"));
        degrade_if_active(ctx, &id);
    }
    Ok(())
}
