//! CONCEPT: agent crate (WP-3). The node agent that runs on every machine: probes hardware, registers with the coordinator, starts and supervises rpc-server.
//! WHY: empty skeleton from WP-12 R12.1 so every work package starts from a workspace that already builds;
//!      O-01 = A: Rust supervises the official llama.cpp binaries as child processes; the agent is that supervisor on each node.
//! DEPENDS ON: contracts (NodeCapabilities, Heartbeat, Command, CommandResult).
//! AFFECTS: coordinator (consumes registration, heartbeats and command results).
//!
//! Empty on purpose. The owning work package fills it in on its own branch.
