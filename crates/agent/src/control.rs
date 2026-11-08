//! CONCEPT: the seam between the agent and the control plane: register the node, send heartbeats.
//! WHY: D-13: SpacetimeDB is the control plane (replaces gRPC). Its generated client bindings come from
//!      WP-2's module (PR #18 rework), which hasn't landed. Everything else in this crate is independent of
//!      the transport, so the transport sits behind this one trait and the rest can ship now.
//! TODO(D-13): implement `ControlPlane` for a Spacetime client once WP-2's bindings land:
//!      - `register_node` -> the module's register_node reducer, payload mapped from `NodeProbe` (§6.1;
//!        add `node_id` per O-16 from `AgentConfig::node_id` or the hostname).
//!      - `heartbeat` -> the heartbeat reducer every `heartbeat_interval_ms` (O-26), payload §6.2 built from
//!        `RpcServer::status()` + `probe::probe_host()` memory.
//!      - commands (§6.3 start/stop_rpc_server) arrive as table rows the agent subscribes to; map them onto
//!        `RpcServer::start` / `stop`. Not modelled here yet: the shape depends on the module.
//! DEPENDS ON: probe.rs (NodeProbe), supervisor.rs (RpcStatus).
//! AFFECTS: the agent's main loop (not written yet, O-02 decides the binary); WP-4 reads what this sends.
//!
//! Rejected: a gRPC client here (superseded by D-13); pulling in the Spacetime SDK before its version and
//! module exist (it would pin a dependency other crates must match).

use crate::probe::NodeProbe;
use crate::supervisor::RpcStatus;
use serde::Serialize;

/// Heartbeat payload owned by the agent until the contract types settle. Mirrors §6.2's core fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeartbeatBody {
    pub node_id: String,
    pub seq: u64,
    /// None = rpc-server not started (§6.2 `not_started`).
    pub rpc_server: Option<RpcStatus>,
    pub mem_available_bytes: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("control plane: {0}")]
pub struct ControlError(pub String);

/// The transport. One implementation per control plane; `LogOnly` until D-13 bindings exist.
pub trait ControlPlane: Send + Sync {
    fn register_node(
        &self,
        node_id: &str,
        probe: &NodeProbe,
    ) -> impl Future<Output = Result<(), ControlError>> + Send;

    fn heartbeat(
        &self,
        body: &HeartbeatBody,
    ) -> impl Future<Output = Result<(), ControlError>> + Send;
}

/// Stand-in transport: prints JSON lines to stderr. Lets the agent loop and examples run end to end now.
#[derive(Debug, Default, Clone, Copy)]
pub struct LogOnly;

impl ControlPlane for LogOnly {
    async fn register_node(&self, node_id: &str, probe: &NodeProbe) -> Result<(), ControlError> {
        let json = serde_json::to_string(probe).map_err(|e| ControlError(e.to_string()))?;
        eprintln!("[control:log-only] register_node {node_id} {json}");
        Ok(())
    }

    async fn heartbeat(&self, body: &HeartbeatBody) -> Result<(), ControlError> {
        let json = serde_json::to_string(body).map_err(|e| ControlError(e.to_string()))?;
        eprintln!("[control:log-only] heartbeat {json}");
        Ok(())
    }
}
