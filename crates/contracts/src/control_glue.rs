//! CONCEPT: converts the §6 contract types into the control module's reducer arguments, so an agent calls
//!          `conn.reducers.register_node(node_info(&caps, "helper"), serde_json::to_string(&caps)?)` instead of
//!          copying fifteen fields by hand.
//! WHY: the module (crates/control) can't depend on this crate (the client SDK doesn't compile to wasm32), so its
//!      argument structs (`control::NodeInfo`, `control::HeartbeatInfo`) are separate types. One mapping here
//!      keeps every agent and test consistent.
//! DEPENDS ON: the generated `crate::control` bindings and the generated §6 types in `crate::v1`.
//! AFFECTS: WP-3 agent (register_node, update_capabilities, send_heartbeat).

// prost derives `Copy` for messages whose fields are all scalars (MemoryInfo, LoadInfo, PowerInfo today). `.clone()`
// stays on purpose: adding a String field to one of them (contracts only grow) removes `Copy`, and a bare move
// here would then stop compiling.
#![allow(clippy::clone_on_copy)]

use crate::control::{Device, HeartbeatInfo, NodeInfo};
use crate::{Heartbeat, NodeCapabilities, RpcServerState};

/// Builds the `register_node` / `update_capabilities` argument from a NodeCapabilities.
/// `role` is "main" or "helper" (§5.1); the module rejects anything else.
pub fn node_info(caps: &NodeCapabilities, role: &str) -> NodeInfo {
    let os = caps.os.clone().unwrap_or_default();
    let cpu = caps.cpu.clone().unwrap_or_default();
    let mem = caps.memory.clone().unwrap_or_default();
    let llama = caps.llama_cpp.clone().unwrap_or_default();
    NodeInfo {
        node_id: caps.node_id.clone(),
        role: role.to_string(),
        hostname: caps.hostname.clone(),
        os_family: os.family,
        arch: os.arch,
        cpu_model: cpu.model,
        logical_cores: cpu.logical_cores,
        total_memory_bytes: mem.total_bytes,
        available_memory_bytes: mem.available_bytes,
        devices: caps
            .devices
            .iter()
            .map(|d| Device {
                name: d.name.clone(),
                backend: d.backend.clone(),
                total_mib: d.total_mib,
                free_mib: d.free_mib,
                unified_memory: d.unified_memory,
            })
            .collect(),
        llama_build: llama.build,
        llama_commit: llama.commit,
        rpc_protocol: llama.rpc_protocol,
        rpc_server_devices: caps.rpc_server_devices.clone(),
        addresses: caps
            .network
            .clone()
            .map(|n| n.addresses)
            .unwrap_or_default(),
    }
}

/// The §6.2 JSON spelling of an rpc-server state ("running"), which is what the module's `rpc_state` column holds.
pub fn rpc_state_name(state: RpcServerState) -> String {
    state
        .as_str_name()
        .strip_prefix("RPC_SERVER_STATE_")
        .unwrap_or("unspecified")
        .to_ascii_lowercase()
}

/// Builds the `send_heartbeat` argument from a Heartbeat. A heartbeat without an rpc_server block reports
/// "not_started".
pub fn heartbeat_info(hb: &Heartbeat) -> HeartbeatInfo {
    let rpc = hb.rpc_server.clone().unwrap_or_default();
    let state = match rpc.state() {
        RpcServerState::Unspecified => RpcServerState::NotStarted,
        s => s,
    };
    let load = hb.load.clone().unwrap_or_default();
    let relay = hb.relay.clone().unwrap_or_default();
    let power = hb.power.clone().unwrap_or_default();
    let error_code = hb.last_error.as_ref().map(|e| {
        // Same spelling as the JSON form: the value name itself ("E_PORT_IN_USE").
        e.code().as_str_name().to_string()
    });
    HeartbeatInfo {
        seq: hb.seq,
        rpc_state: rpc_state_name(state),
        rpc_pid: rpc.pid,
        rpc_port: rpc.port,
        port_accepting: rpc.port_accepting,
        cpu_util_pct: load.cpu_util_pct,
        mem_used_bytes: load.mem_used_bytes,
        relay_bytes_in: relay.bytes_in,
        relay_bytes_out: relay.bytes_out,
        relay_graph_computes: relay.graph_computes,
        on_ac: power.on_ac,
        battery_pct: power.battery_pct,
        last_error_code: error_code,
        last_error_message: hb.last_error.as_ref().map(|e| e.message.clone()),
    }
}
