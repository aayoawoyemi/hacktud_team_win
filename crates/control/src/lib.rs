//! CONCEPT: the pool's control plane as a SpacetimeDB module (WP-2, D-13). It holds every piece of shared state:
//!          which machines exist (`node`), how each one is doing (`heartbeat`), what the coordinator asked a
//!          machine to do (`command`), the chosen split (`split_plan`), the pool's state (`cluster_status`) and
//!          the history (`event`). Agents and the coordinator call reducers to change it and subscribe to read it.
//! WHY: D-13 (Aries, 6:10pm): SpacetimeDB replaces gRPC. Registry, heartbeats, live status and history become
//!      tables plus subscriptions, so there is no custom server, status API or event store to write. The table
//!      and reducer list is Aries' plan on #11 ("@pi (WP-2): next steps after D-13").
//! RULE: reducers only store and validate state. Everything that touches a machine (starting rpc-server or
//!      llama-server, probing hardware, netcheck) stays in the agent / coordinator processes: SpacetimeDB module
//!      code runs inside the database and cannot reach the LAN or the filesystem.
//! DEPENDS ON: spacetimedb =2.14.1 (same version as the `spacetime` CLI/server on main and the client SDK).
//!      PRD §5.6 (state machines), §6 (field names), §6.10 (event kinds), §6.11 (error codes).
//! AFFECTS: crates/contracts/src/control/ (generated client bindings; regenerate after ANY change here:
//!      `spacetime generate --lang rust --out-dir crates/contracts/src/control --module-path crates/control`),
//!      WP-3 agent, WP-4 coordinator, WP-7 status, WP-9 recovery. Changing a table is a contract change:
//!      post concept-change on #11 (and note SpacetimeDB refuses most column changes on republish without
//!      `--delete-data`; add new columns at the END of a struct, with a default).
//!
//! Conventions:
//! - Enum-like columns are strings with the §6 JSON spellings ("running", "ACTIVE", "SERVING", "node_lost",
//!   "E_PORT_IN_USE"), the same strings `contracts` writes in JSON. Strings keep `spacetime sql` readable
//!   (`WHERE state = 'ACTIVE'`). Reducers reject values not in the lists below.
//! - Full §6 payloads ride along as `*_json` strings (the contracts crate's serde JSON, guarded by its fixture
//!   tests). Scalar fields used for logic or queries are real columns. This crate does not depend on
//!   `contracts`, which pulls the native client SDK and cannot compile to wasm32.
//! - Errors are `Err(String)` starting with a §6.11 code ("E_CONFIG: ..."). An Err rolls the whole reducer
//!   call back; the caller sees it through the generated `on_<reducer>` callback.

use spacetimedb::{Identity, ReducerContext, SpacetimeType, Table, Timestamp, reducer, table};

// =================================================================================================
// Allowed values (mirror the enums in crates/contracts/proto/contracts.proto)
// =================================================================================================

/// §6.3 command kinds.
pub const COMMAND_KINDS: &[&str] = &[
    "start_rpc_server",
    "stop_rpc_server",
    "probe",
    "speed_probe",
    "netcheck",
];
/// §6.10 event kinds.
pub const EVENT_KINDS: &[&str] = &[
    "node_registered",
    "node_capabilities_changed",
    "node_lost",
    "node_recovered",
    "version_mismatch",
    "rpc_server_started",
    "rpc_server_exited",
    "plan_created",
    "plan_rejected",
    "launch_started",
    "llama_server_ready",
    "llama_server_exited",
    "placement_mismatch",
    "request_failed",
    "recovery_started",
    "recovery_succeeded",
    "recovery_failed",
    "netcheck_result",
    "bench_completed",
    // Added (Aayo 11:20pm Sat): a bring-up that didn't reach SERVING. The cluster goes to STOPPED, not FAILED, so
    // nothing on screen shows a failure; the reason lives here in the event log.
    "launch_failed",
    // Added (P1, Sun 2am): the dashboard asked for a different model (message = model id). The coordinator acts on it.
    "model_requested",
];
/// §6.2 rpc_server.state.
pub const RPC_STATES: &[&str] = &["not_started", "starting", "running", "exited", "failed"];
/// §5.1 roles.
pub const ROLES: &[&str] = &["main", "helper"];
/// §6.5 strategies (which one is used is O-07).
pub const STRATEGIES: &[&str] = &[
    "llama_default",
    "proportional_usable",
    "fastest_first",
    "min_machines",
];
/// §5.6 cluster states.
pub const CLUSTER_STATES: &[&str] = &[
    "EMPTY",
    "PLANNED",
    "STARTING_HELPERS",
    "LAUNCHING",
    "SERVING",
    "DEGRADED",
    "RECOVERING",
    "FAILED",
    "STOPPED",
];

fn check(value: &str, allowed: &[&str], what: &str) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "E_CONFIG: unknown {what} {value:?}; allowed: {allowed:?}"
        ))
    }
}

// =================================================================================================
// Argument and column types
// =================================================================================================

/// CONCEPT: one compute device (§6.1 `devices[]`), the part the planner (WP-5) needs.
#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    pub backend: String,
    pub total_mib: u64,
    pub free_mib: u64,
    pub unified_memory: Option<bool>,
}

/// CONCEPT: the queryable summary of a §6.1 NodeCapabilities, as a reducer argument.
/// WHY: one struct argument instead of fifteen positional ones; the generated client gets the same struct.
///      `contracts::control_glue::node_info` builds it from a `contracts::NodeCapabilities`.
#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub struct NodeInfo {
    pub node_id: String,
    /// "main" | "helper" (§5.1).
    pub role: String,
    pub hostname: String,
    pub os_family: String,
    pub arch: String,
    pub cpu_model: String,
    pub logical_cores: Option<u32>,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub devices: Vec<Device>,
    pub llama_build: Option<u32>,
    pub llama_commit: Option<String>,
    pub rpc_protocol: Option<String>,
    pub rpc_server_devices: Vec<String>,
    pub addresses: Vec<String>,
}

/// CONCEPT: the queryable part of a §6.2 Heartbeat, as a reducer argument.
#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub struct HeartbeatInfo {
    pub seq: u64,
    /// §6.2 rpc_server.state.
    pub rpc_state: String,
    pub rpc_pid: Option<u32>,
    pub rpc_port: Option<u32>,
    pub port_accepting: bool,
    pub cpu_util_pct: Option<f64>,
    pub mem_used_bytes: Option<u64>,
    pub relay_bytes_in: Option<u64>,
    pub relay_bytes_out: Option<u64>,
    pub relay_graph_computes: Option<u64>,
    pub on_ac: Option<bool>,
    pub battery_pct: Option<u32>,
    /// §6.11 code, or None.
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
}

// =================================================================================================
// Tables
// =================================================================================================

/// CONCEPT: identities allowed to call the coordinator-only reducers (issue_command, set_split_plan,
///          set_cluster_status, add_admin). Private: clients can't read it.
/// WHY: any machine on the Wi-Fi can connect; only the coordinator should command nodes. `init` makes the
///      publisher (whoever ran `spacetime publish` on main) the first admin; the publisher then authorizes the
///      coordinator's own identity with `add_admin`, so no token has to be copied between processes.
#[table(accessor = admin)]
pub struct Admin {
    #[primary_key]
    pub identity: Identity,
}

/// CONCEPT: one row per machine (§6.1). `state` is the §5.6 node state:
///          REGISTERED -> READY -> ACTIVE -> LOST; a LOST node that registers again goes back to REGISTERED.
/// `online` is the live connection flag (connect/disconnect lifecycle reducers); `state` is the pool's view.
#[table(accessor = node, public)]
pub struct NodeRow {
    #[primary_key]
    pub node_id: String,
    /// The SpacetimeDB identity that registered this node_id (the agent's saved token). Only it may act as
    /// this node. Unique: one agent identity = one node.
    #[unique]
    pub identity: Identity,
    pub role: String,
    pub state: String,
    pub online: bool,
    pub hostname: String,
    pub os_family: String,
    pub arch: String,
    pub cpu_model: String,
    pub logical_cores: Option<u32>,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub devices: Vec<Device>,
    pub llama_build: Option<u32>,
    pub llama_commit: Option<String>,
    pub rpc_protocol: Option<String>,
    pub rpc_server_devices: Vec<String>,
    pub addresses: Vec<String>,
    pub registered_at: Timestamp,
    pub last_seen: Timestamp,
    /// Full §6.1 NodeCapabilities as contracts JSON.
    pub caps_json: String,
}

/// CONCEPT: the latest §6.2 heartbeat per node (one row per node, overwritten). History of interest goes to
///          `event` (rpc_server_started / rpc_server_exited), not here, so the table never grows.
#[table(accessor = heartbeat, public)]
pub struct HeartbeatRow {
    #[primary_key]
    pub node_id: String,
    pub received_at: Timestamp,
    pub seq: u64,
    pub rpc_state: String,
    pub rpc_pid: Option<u32>,
    pub rpc_port: Option<u32>,
    pub port_accepting: bool,
    pub cpu_util_pct: Option<f64>,
    pub mem_used_bytes: Option<u64>,
    pub relay_bytes_in: Option<u64>,
    pub relay_bytes_out: Option<u64>,
    pub relay_graph_computes: Option<u64>,
    pub on_ac: Option<bool>,
    pub battery_pct: Option<u32>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    /// Full §6.2 Heartbeat as contracts JSON.
    pub heartbeat_json: String,
}

/// CONCEPT: §6.3 Command + CommandResult in one row. Status: pending -> running -> ok | failed.
/// The agent subscribes to `SELECT * FROM command WHERE node_id = '<me>'`, claims pending rows, does the work
/// locally, then completes them. The coordinator watches the same rows for the result.
#[table(accessor = command, public)]
pub struct CommandRow {
    #[primary_key]
    #[auto_inc]
    pub command_id: u64,
    #[index(btree)]
    pub node_id: String,
    /// §6.3 kind.
    pub kind: String,
    /// The `args` object of a contracts Command, as JSON.
    pub args_json: String,
    pub status: String,
    /// The `data` object of a contracts CommandResult, as JSON ("" until completed).
    pub result_json: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: Timestamp,
    pub claimed_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

/// CONCEPT: §6.10 event log. Append-only: this is the pool's history. Written in the same transaction as the
///          state change it describes, so the log and the state can't disagree.
#[table(accessor = event, public)]
pub struct EventRow {
    #[primary_key]
    #[auto_inc]
    pub seq: u64,
    pub at: Timestamp,
    pub kind: String,
    /// "" when not about one node.
    pub node_id: String,
    /// "" when not about a plan.
    pub plan_id: String,
    pub message: String,
    /// §6.10 `data` object as JSON ("{}" when empty).
    pub data_json: String,
}

/// CONCEPT: §6.5 SplitPlan, one row per plan computed by WP-5 (on the coordinator) and stored here.
#[table(accessor = split_plan, public)]
pub struct SplitPlanRow {
    #[primary_key]
    pub plan_id: String,
    pub model_id: String,
    pub strategy: String,
    pub fits: bool,
    pub created_at: Timestamp,
    /// Full §6.5 SplitPlan as contracts JSON.
    pub plan_json: String,
}

/// CONCEPT: the single pool-status row (id = 0): §5.6 cluster state plus the §6.7 ClusterStatus snapshot.
#[table(accessor = cluster_status, public)]
pub struct ClusterStatusRow {
    #[primary_key]
    pub id: u8,
    pub cluster_state: String,
    pub plan_id: String,
    pub model_id: String,
    pub endpoint: String,
    pub updated_at: Timestamp,
    /// Full §6.7 ClusterStatus as contracts JSON ("" until the coordinator writes one).
    pub status_json: String,
}

/// CONCEPT: the model the operator wants the pool to serve (one row, id = 0). The dashboard writes it with
///          `request_model`; the coordinator re-plans to that model if it is in its own coordinator.toml [models].
/// WHY: Preston (Sun 2am): switch the active model between our three from the dashboard.
#[table(accessor = model_request, public)]
pub struct ModelRequestRow {
    #[primary_key]
    pub id: u8,
    pub model_id: String,
    pub requested_at: Timestamp,
}

// =================================================================================================
// Helpers
// =================================================================================================

fn is_admin(ctx: &ReducerContext) -> bool {
    ctx.db.admin().identity().find(ctx.sender()).is_some()
}

fn require_admin(ctx: &ReducerContext) -> Result<(), String> {
    if is_admin(ctx) {
        Ok(())
    } else {
        Err("E_CONFIG: coordinator (admin) only".into())
    }
}

fn log_event(ctx: &ReducerContext, kind: &str, node_id: &str, plan_id: &str, message: String) {
    ctx.db.event().insert(EventRow {
        seq: 0, // auto_inc assigns it
        at: ctx.timestamp,
        kind: kind.into(),
        node_id: node_id.into(),
        plan_id: plan_id.into(),
        message,
        data_json: "{}".into(),
    });
}

fn cluster(ctx: &ReducerContext) -> ClusterStatusRow {
    ctx.db
        .cluster_status()
        .id()
        .find(0)
        .expect("init creates cluster_status row 0")
}

/// The node row owned by the caller, or E_CONFIG.
fn own_node(ctx: &ReducerContext) -> Result<NodeRow, String> {
    ctx.db
        .node()
        .identity()
        .find(ctx.sender())
        .ok_or_else(|| "E_CONFIG: this identity has not called register_node".to_string())
}

/// A command addressed to the caller's node, or E_CONFIG.
fn own_command(ctx: &ReducerContext, command_id: u64) -> Result<CommandRow, String> {
    let c = ctx
        .db
        .command()
        .command_id()
        .find(command_id)
        .ok_or(format!("E_CONFIG: no command {command_id}"))?;
    let n = own_node(ctx)?;
    if c.node_id != n.node_id {
        return Err(format!(
            "E_CONFIG: command {command_id} belongs to node {}",
            c.node_id
        ));
    }
    Ok(c)
}

/// CONCEPT: §5.6 cluster transitions. STOPPED is reachable from any state; re-planning is allowed from EMPTY,
///          STOPPED and FAILED; DEGRADED / RECOVERING follow O-11 (policy open, transitions fixed).
pub fn cluster_transition_ok(from: &str, to: &str) -> bool {
    if from == to || to == "STOPPED" {
        return true;
    }
    matches!(
        (from, to),
        ("EMPTY" | "STOPPED" | "FAILED", "PLANNED")
            // Re-plan when a helper joins (D-14), straight from SERVING, so the screen shows PLANNED ->
            // STARTING_HELPERS -> LAUNCHING -> SERVING instead of a STOPPED in between (Aayo 11:20pm).
            | ("SERVING" | "DEGRADED", "PLANNED")
            | ("PLANNED", "STARTING_HELPERS")
            | ("STARTING_HELPERS", "LAUNCHING")
            | ("LAUNCHING", "SERVING")
            | ("PLANNED" | "STARTING_HELPERS" | "LAUNCHING", "FAILED")
            | ("SERVING", "DEGRADED")
            | ("DEGRADED", "RECOVERING")
            | ("RECOVERING", "SERVING" | "FAILED")
    )
}

/// Event to log for a cluster transition, if §6.10 has one.
fn cluster_transition_event(from: &str, to: &str) -> Option<&'static str> {
    match (from, to) {
        (_, "LAUNCHING") => Some("launch_started"),
        ("RECOVERING", "SERVING") => Some("recovery_succeeded"),
        (_, "SERVING") => Some("llama_server_ready"),
        (_, "RECOVERING") => Some("recovery_started"),
        ("RECOVERING", "FAILED") => Some("recovery_failed"),
        _ => None,
    }
}

/// Logs `version_mismatch` when the llama.cpp builds or RPC protocols of online nodes differ (O-14 pins 8665).
fn check_versions(ctx: &ReducerContext) {
    let nodes: Vec<NodeRow> = ctx.db.node().iter().filter(|n| n.state != "LOST").collect();
    let builds: std::collections::BTreeSet<Option<u32>> =
        nodes.iter().map(|n| n.llama_build).collect();
    let protos: std::collections::BTreeSet<Option<String>> =
        nodes.iter().map(|n| n.rpc_protocol.clone()).collect();
    if builds.len() > 1 || protos.len() > 1 {
        log_event(
            ctx,
            "version_mismatch",
            "",
            "",
            format!("llama.cpp builds {builds:?}, rpc protocols {protos:?} differ between nodes"),
        );
    }
}

/// Marks a node LOST (if it wasn't) and degrades a SERVING cluster when that node held layers.
fn mark_lost(ctx: &ReducerContext, mut n: NodeRow, why: &str) {
    let was_active = n.state == "ACTIVE";
    let id = n.node_id.clone();
    n.online = false;
    let changed = n.state != "LOST";
    n.state = "LOST".into();
    n.last_seen = ctx.timestamp;
    ctx.db.node().node_id().update(n);
    if changed {
        log_event(ctx, "node_lost", &id, "", why.into());
    }
    let mut c = cluster(ctx);
    if was_active && c.cluster_state == "SERVING" {
        c.cluster_state = "DEGRADED".into();
        c.updated_at = ctx.timestamp;
        ctx.db.cluster_status().id().update(c);
    }
}

fn node_row(
    ctx: &ReducerContext,
    info: NodeInfo,
    caps_json: String,
    state: String,
    registered_at: Timestamp,
) -> NodeRow {
    NodeRow {
        node_id: info.node_id,
        identity: ctx.sender(),
        role: info.role,
        state,
        online: true,
        hostname: info.hostname,
        os_family: info.os_family,
        arch: info.arch,
        cpu_model: info.cpu_model,
        logical_cores: info.logical_cores,
        total_memory_bytes: info.total_memory_bytes,
        available_memory_bytes: info.available_memory_bytes,
        devices: info.devices,
        llama_build: info.llama_build,
        llama_commit: info.llama_commit,
        rpc_protocol: info.rpc_protocol,
        rpc_server_devices: info.rpc_server_devices,
        addresses: info.addresses,
        registered_at,
        last_seen: ctx.timestamp,
        caps_json,
    }
}

// =================================================================================================
// Lifecycle reducers
// =================================================================================================

/// First publish (or `--delete-data`): the publisher becomes admin; the pool starts EMPTY.
#[reducer(init)]
pub fn init(ctx: &ReducerContext) {
    ctx.db.admin().insert(Admin {
        identity: ctx.sender(),
    });
    ctx.db.cluster_status().insert(ClusterStatusRow {
        id: 0,
        cluster_state: "EMPTY".into(),
        plan_id: String::new(),
        model_id: String::new(),
        endpoint: String::new(),
        updated_at: ctx.timestamp,
        status_json: String::new(),
    });
}

/// A client connected. If it is a known agent, mark it online (it still calls register_node to leave LOST).
#[reducer(client_connected)]
pub fn client_connected(ctx: &ReducerContext) {
    if let Some(mut n) = ctx.db.node().identity().find(ctx.sender()) {
        n.online = true;
        n.last_seen = ctx.timestamp;
        ctx.db.node().node_id().update(n);
    }
}

/// A client disconnected (clean close or TCP reset). A known agent goes offline and LOST at once, logs
/// `node_lost`, and a SERVING cluster that used it becomes DEGRADED (§5.6). Detecting a hung agent that stays
/// connected (heartbeat timeout, O-26) is WP-9's, e.g. a scheduled reducer over `heartbeat.received_at`.
#[reducer(client_disconnected)]
pub fn client_disconnected(ctx: &ReducerContext) {
    if let Some(n) = ctx.db.node().identity().find(ctx.sender()) {
        mark_lost(ctx, n, "agent disconnected");
    }
}

// =================================================================================================
// Agent reducers (WP-3)
// =================================================================================================

/// CONCEPT: an agent announces its machine. Binds `info.node_id` to the caller's identity on first use.
/// Events: node_registered (new), node_recovered (was LOST), node_capabilities_changed (re-register).
#[reducer]
pub fn register_node(
    ctx: &ReducerContext,
    info: NodeInfo,
    caps_json: String,
) -> Result<(), String> {
    check(&info.role, ROLES, "role")?;
    if info.node_id.is_empty() {
        return Err("E_CONFIG: node_id is empty".into());
    }
    // One identity = one node: a second node_id from the same identity would break `identity` uniqueness.
    if let Some(other) = ctx.db.node().identity().find(ctx.sender())
        && other.node_id != info.node_id
    {
        return Err(format!(
            "E_CONFIG: this identity already registered node {}",
            other.node_id
        ));
    }
    let existing = ctx.db.node().node_id().find(info.node_id.clone());
    let id = info.node_id.clone();
    let (kind, state, registered_at) = match &existing {
        None => ("node_registered", "REGISTERED".to_string(), ctx.timestamp),
        Some(n) if n.identity != ctx.sender() => {
            return Err(format!(
                "E_CONFIG: node_id {id} is owned by another identity"
            ));
        }
        Some(n) if n.state == "LOST" => {
            ("node_recovered", "REGISTERED".to_string(), n.registered_at)
        }
        // register_node only runs at agent start, so this is a new agent process: its rpc-server state is unknown
        // until the first fresh heartbeat (REGISTERED, then READY from send_heartbeat).
        Some(n) => (
            "node_capabilities_changed",
            "REGISTERED".to_string(),
            n.registered_at,
        ),
    };
    // Ponytail #1: drop the previous process's heartbeat. Its `seq` counted from the old start, so the new agent's
    // heartbeats were ignored and its stale "running, port_accepting" row sent the coordinator to a dead rpc-server.
    ctx.db.heartbeat().node_id().delete(&id);
    let row = node_row(ctx, info, caps_json, state, registered_at);
    if existing.is_some() {
        ctx.db.node().node_id().update(row);
    } else {
        ctx.db.node().insert(row);
    }
    log_event(ctx, kind, &id, "", String::new());
    check_versions(ctx);
    Ok(())
}

/// CONCEPT: a registered agent's capabilities changed (§6.1 "whenever it changes"): new free memory, a device
/// appeared, llama.cpp was updated. Keeps the node's state. Event: node_capabilities_changed.
#[reducer]
pub fn update_capabilities(
    ctx: &ReducerContext,
    info: NodeInfo,
    caps_json: String,
) -> Result<(), String> {
    check(&info.role, ROLES, "role")?;
    let n = own_node(ctx)?;
    if n.node_id != info.node_id {
        return Err(format!(
            "E_CONFIG: this identity is node {}, not {}",
            n.node_id, info.node_id
        ));
    }
    let id = n.node_id.clone();
    let row = node_row(ctx, info, caps_json, n.state, n.registered_at);
    ctx.db.node().node_id().update(row);
    log_event(ctx, "node_capabilities_changed", &id, "", String::new());
    check_versions(ctx);
    Ok(())
}

/// CONCEPT: the agent's periodic heartbeat (§6.2). Overwrites the node's heartbeat row, refreshes last_seen,
/// logs rpc_server_started / rpc_server_exited when the rpc-server state changes, and moves the node
/// REGISTERED -> READY once rpc-server runs (helpers) or right away (main, which needs no rpc-server).
/// Out-of-order or duplicate heartbeats (seq not above the stored one) are ignored, not errors.
/// NAME: `send_heartbeat`, not `heartbeat` (Aries' list): the `heartbeat` table's generated accessor trait is
/// also called `heartbeat`, and Rust rejects a reducer fn with the same name in the same module.
#[reducer]
pub fn send_heartbeat(
    ctx: &ReducerContext,
    hb: HeartbeatInfo,
    heartbeat_json: String,
) -> Result<(), String> {
    check(&hb.rpc_state, RPC_STATES, "rpc_state")?;
    let mut n = own_node(ctx)?;
    let id = n.node_id.clone();
    let prev = ctx.db.heartbeat().node_id().find(id.clone());
    if let Some(p) = &prev
        && hb.seq <= p.seq
        && hb.seq != 0
    {
        return Ok(());
    }
    let prev_state = prev
        .as_ref()
        .map(|p| p.rpc_state.clone())
        .unwrap_or_else(|| "not_started".into());
    if prev_state != hb.rpc_state {
        match hb.rpc_state.as_str() {
            "running" => log_event(ctx, "rpc_server_started", &id, "", String::new()),
            "exited" | "failed" => log_event(
                ctx,
                "rpc_server_exited",
                &id,
                "",
                hb.last_error_message.clone().unwrap_or_default(),
            ),
            _ => {}
        }
    }
    let row = HeartbeatRow {
        node_id: id.clone(),
        received_at: ctx.timestamp,
        seq: hb.seq,
        rpc_state: hb.rpc_state.clone(),
        rpc_pid: hb.rpc_pid,
        rpc_port: hb.rpc_port,
        port_accepting: hb.port_accepting,
        cpu_util_pct: hb.cpu_util_pct,
        mem_used_bytes: hb.mem_used_bytes,
        relay_bytes_in: hb.relay_bytes_in,
        relay_bytes_out: hb.relay_bytes_out,
        relay_graph_computes: hb.relay_graph_computes,
        on_ac: hb.on_ac,
        battery_pct: hb.battery_pct,
        last_error_code: hb.last_error_code,
        last_error_message: hb.last_error_message,
        heartbeat_json,
    };
    if prev.is_some() {
        ctx.db.heartbeat().node_id().update(row);
    } else {
        ctx.db.heartbeat().insert(row);
    }
    let healthy = hb.rpc_state == "running" && hb.port_accepting;
    if n.state == "REGISTERED" && (healthy || n.role == "main") {
        n.state = "READY".into();
    } else if n.state == "READY" && n.role == "helper" && !healthy {
        // rpc-server died before the node got layers: not READY any more.
        n.state = "REGISTERED".into();
    }
    n.online = true;
    n.last_seen = ctx.timestamp;
    ctx.db.node().node_id().update(n);
    Ok(())
}

/// CONCEPT: the agent takes a pending command addressed to it (pending -> running), before doing the work.
/// Errors if the command is not pending, so two agent threads can't both run it.
#[reducer]
pub fn claim_command(ctx: &ReducerContext, command_id: u64) -> Result<(), String> {
    let mut c = own_command(ctx, command_id)?;
    if c.status != "pending" {
        return Err(format!(
            "E_CONFIG: command {command_id} is {}, not pending",
            c.status
        ));
    }
    c.status = "running".into();
    c.claimed_at = Some(ctx.timestamp);
    ctx.db.command().command_id().update(c);
    Ok(())
}

/// CONCEPT: the agent reports the outcome (running -> ok | failed). `result_json` is the contracts
/// CommandResult `data` object for the command's kind; `error_code` a §6.11 code when ok = false.
/// Idempotent: completing an already finished command is a no-op (an agent may retry after a reconnect).
#[reducer]
pub fn complete_command(
    ctx: &ReducerContext,
    command_id: u64,
    ok: bool,
    result_json: String,
    error_code: Option<String>,
    error_message: Option<String>,
) -> Result<(), String> {
    let mut c = own_command(ctx, command_id)?;
    if c.status == "ok" || c.status == "failed" {
        return Ok(());
    }
    c.status = if ok { "ok" } else { "failed" }.into();
    c.result_json = result_json;
    c.error_code = error_code;
    c.error_message = error_message;
    if c.claimed_at.is_none() {
        c.claimed_at = Some(ctx.timestamp);
    }
    c.finished_at = Some(ctx.timestamp);
    ctx.db.command().command_id().update(c);
    Ok(())
}

// =================================================================================================
// Either side
// =================================================================================================

/// CONCEPT: append a §6.10 event observed outside the module (placement_mismatch, request_failed,
/// llama_server_exited, netcheck_result, bench_completed, ...). Callers: the coordinator (admin) or a
/// registered agent.
#[reducer]
pub fn record_event(
    ctx: &ReducerContext,
    kind: String,
    node_id: String,
    plan_id: String,
    message: String,
    data_json: String,
) -> Result<(), String> {
    check(&kind, EVENT_KINDS, "event kind")?;
    if !is_admin(ctx) && ctx.db.node().identity().find(ctx.sender()).is_none() {
        return Err("E_CONFIG: unknown sender (register_node first)".into());
    }
    let data_json = if data_json.is_empty() {
        "{}".to_string()
    } else {
        data_json
    };
    ctx.db.event().insert(EventRow {
        seq: 0,
        at: ctx.timestamp,
        kind,
        node_id,
        plan_id,
        message,
        data_json,
    });
    Ok(())
}

/// CONCEPT: ask the pool to serve another model. Anyone may call it (the dashboard calls it anonymously over the
///          SpacetimeDB HTTP API): on our private router (R-09) the only effect is a re-plan, and the coordinator
///          ignores ids that aren't in its own config, so a caller can't point it at an arbitrary file.
#[reducer]
pub fn request_model(ctx: &ReducerContext, model_id: String) -> Result<(), String> {
    let model_id = model_id.trim().to_string();
    if model_id.is_empty()
        || model_id.len() > 128
        || !model_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        return Err("E_CONFIG: model id must be 1-128 chars of [A-Za-z0-9._-]".into());
    }
    let row = ModelRequestRow {
        id: 0,
        model_id: model_id.clone(),
        requested_at: ctx.timestamp,
    };
    if ctx.db.model_request().id().find(0).is_some() {
        ctx.db.model_request().id().update(row);
    } else {
        ctx.db.model_request().insert(row);
    }
    log_event(ctx, "model_requested", "", "", model_id);
    Ok(())
}

// =================================================================================================
// Coordinator reducers (WP-4; admin only)
// =================================================================================================

/// CONCEPT: authorize another identity (the coordinator process) as admin. Called once by the publisher, e.g.
/// `spacetime call <db> add_admin 0x<coordinator identity>`.
#[reducer]
pub fn add_admin(ctx: &ReducerContext, identity: Identity) -> Result<(), String> {
    require_admin(ctx)?;
    if ctx.db.admin().identity().find(identity).is_none() {
        ctx.db.admin().insert(Admin { identity });
    }
    Ok(())
}

/// CONCEPT: queue a §6.3 command for one node (status pending). The agent sees it through its subscription.
#[reducer]
pub fn issue_command(
    ctx: &ReducerContext,
    node_id: String,
    kind: String,
    args_json: String,
) -> Result<(), String> {
    require_admin(ctx)?;
    check(&kind, COMMAND_KINDS, "command kind")?;
    let n = ctx
        .db
        .node()
        .node_id()
        .find(node_id.clone())
        .ok_or(format!("E_NODE_UNREACHABLE: unknown node {node_id}"))?;
    if !n.online {
        return Err(format!("E_NODE_UNREACHABLE: node {node_id} is offline"));
    }
    ctx.db.command().insert(CommandRow {
        command_id: 0, // auto_inc
        node_id,
        kind,
        args_json: if args_json.is_empty() {
            "{}".into()
        } else {
            args_json
        },
        status: "pending".into(),
        result_json: String::new(),
        error_code: None,
        error_message: None,
        created_at: ctx.timestamp,
        claimed_at: None,
        finished_at: None,
    });
    Ok(())
}

/// CONCEPT: store a §6.5 SplitPlan computed by WP-5 on the coordinator. Event: plan_created, or plan_rejected
/// when it doesn't fit (E_MODEL_DOES_NOT_FIT stays the coordinator's error to show). Does not change the
/// cluster state; the coordinator moves it to PLANNED with set_cluster_status.
#[reducer]
pub fn set_split_plan(
    ctx: &ReducerContext,
    plan_id: String,
    model_id: String,
    strategy: String,
    fits: bool,
    plan_json: String,
) -> Result<(), String> {
    require_admin(ctx)?;
    check(&strategy, STRATEGIES, "strategy")?;
    let row = SplitPlanRow {
        plan_id: plan_id.clone(),
        model_id,
        strategy,
        fits,
        created_at: ctx.timestamp,
        plan_json,
    };
    if ctx
        .db
        .split_plan()
        .plan_id()
        .find(plan_id.clone())
        .is_some()
    {
        ctx.db.split_plan().plan_id().update(row);
    } else {
        ctx.db.split_plan().insert(row);
    }
    log_event(
        ctx,
        if fits {
            "plan_created"
        } else {
            "plan_rejected"
        },
        "",
        &plan_id,
        String::new(),
    );
    Ok(())
}

/// CONCEPT: the coordinator moves the pool through §5.6 and stores the latest §6.7 snapshot.
/// Illegal transitions return E_INTERNAL and change nothing. Entering SERVING marks `active_node_ids` ACTIVE
/// (READY -> ACTIVE: they hold layers); entering STOPPED or FAILED returns ACTIVE nodes to READY.
/// Events per transition: launch_started, llama_server_ready, recovery_started, recovery_succeeded, recovery_failed.
#[reducer]
pub fn set_cluster_status(
    ctx: &ReducerContext,
    cluster_state: String,
    plan_id: String,
    model_id: String,
    endpoint: String,
    active_node_ids: Vec<String>,
    status_json: String,
) -> Result<(), String> {
    require_admin(ctx)?;
    check(&cluster_state, CLUSTER_STATES, "cluster_state")?;
    let mut c = cluster(ctx);
    let from = c.cluster_state.clone();
    if !cluster_transition_ok(&from, &cluster_state) {
        return Err(format!(
            "E_INTERNAL: illegal cluster transition {from} -> {cluster_state}"
        ));
    }
    if !plan_id.is_empty()
        && ctx
            .db
            .split_plan()
            .plan_id()
            .find(plan_id.clone())
            .is_none()
    {
        return Err(format!(
            "E_CONFIG: unknown plan {plan_id} (set_split_plan first)"
        ));
    }
    if cluster_state == "SERVING" {
        for id in &active_node_ids {
            if let Some(mut n) = ctx.db.node().node_id().find(id.clone())
                && n.state == "READY"
            {
                n.state = "ACTIVE".into();
                ctx.db.node().node_id().update(n);
            }
        }
    } else if cluster_state == "STOPPED" || cluster_state == "FAILED" || cluster_state == "PLANNED"
    {
        let active: Vec<NodeRow> = ctx
            .db
            .node()
            .iter()
            .filter(|n| n.state == "ACTIVE")
            .collect();
        for mut n in active {
            n.state = "READY".into();
            ctx.db.node().node_id().update(n);
        }
    }
    c.cluster_state = cluster_state.clone();
    c.plan_id = plan_id;
    c.model_id = model_id;
    c.endpoint = endpoint;
    c.updated_at = ctx.timestamp;
    c.status_json = status_json;
    let plan = c.plan_id.clone();
    ctx.db.cluster_status().id().update(c);
    if from != cluster_state
        && let Some(kind) = cluster_transition_event(&from, &cluster_state)
    {
        log_event(ctx, kind, "", &plan, format!("{from} -> {cluster_state}"));
    }
    Ok(())
}

// =================================================================================================
// Native unit tests for the pure helpers (reducers need a running database; see the smoke test in CONTRACTS.md)
// =================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_happy_path_and_recovery() {
        for (from, to) in [
            ("EMPTY", "PLANNED"),
            ("PLANNED", "STARTING_HELPERS"),
            ("STARTING_HELPERS", "LAUNCHING"),
            ("LAUNCHING", "SERVING"),
            ("SERVING", "DEGRADED"),
            ("DEGRADED", "RECOVERING"),
            ("RECOVERING", "SERVING"),
            ("SERVING", "STOPPED"),
            ("STOPPED", "PLANNED"),
            ("SERVING", "PLANNED"),
        ] {
            assert!(cluster_transition_ok(from, to), "{from} -> {to}");
        }
    }

    #[test]
    fn cluster_rejects_skips() {
        assert!(!cluster_transition_ok("EMPTY", "SERVING"));
        assert!(!cluster_transition_ok("PLANNED", "SERVING"));
        assert!(!cluster_transition_ok("SERVING", "LAUNCHING"));
    }

    #[test]
    fn transition_events() {
        assert_eq!(
            cluster_transition_event("STARTING_HELPERS", "LAUNCHING"),
            Some("launch_started")
        );
        assert_eq!(
            cluster_transition_event("LAUNCHING", "SERVING"),
            Some("llama_server_ready")
        );
        assert_eq!(
            cluster_transition_event("RECOVERING", "SERVING"),
            Some("recovery_succeeded")
        );
        assert_eq!(
            cluster_transition_event("RECOVERING", "FAILED"),
            Some("recovery_failed")
        );
        assert_eq!(cluster_transition_event("EMPTY", "PLANNED"), None);
    }

    #[test]
    fn every_listed_kind_is_a_known_event() {
        for k in [
            "node_lost",
            "version_mismatch",
            "plan_rejected",
            "launch_started",
            "recovery_failed",
        ] {
            assert!(EVENT_KINDS.contains(&k), "{k}");
        }
        assert_eq!(
            EVENT_KINDS.len(),
            21,
            "§6.10 lists 19 kinds, plus launch_failed (Aayo 11:20pm) and model_requested (Sun 2am)"
        );
    }
}
