//! CONCEPT: contracts crate (WP-2). The shared types every other crate exchanges (PRD §6), generated from
//!          proto/contracts.proto, plus parsers that turn llama.cpp's text output into those types.
//! WHY: O-03 (decided): gRPC with tonic; the .proto file is the contract; protox compiles it (no protoc).
//!      One crate means the agent (WP-3) and the coordinator (WP-4) can't disagree on a shape: both compile
//!      the same generated code.
//! DEPENDS ON: build.rs (code generation), proto/contracts.proto (source of truth).
//! AFFECTS: agent, coordinator, planner, relay, bench, netcheck. See CONTRACTS.md for the shapes in prose.
//!
//! Layout:
//! - `v1`: generated messages and enums (also re-exported at the crate root: `contracts::Heartbeat`), and
//!   the gRPC services: `v1::agent_service_client::AgentServiceClient`,
//!   `v1::agent_service_server::{AgentService, AgentServiceServer}`, and the same for `coordinator_service_*`.
//! - `json`: serde adapters so every message reads and writes the §6 JSON form (`serde_json::to_string`).
//! - `parsers`: llama.cpp text output -> contract types (WP-2 R2.4).

/// Generated from proto/contracts.proto (package `contracts.v1`). Do not edit; edit the .proto.
pub mod v1 {
    // Generated code: allow lints we don't control.
    #![allow(clippy::all, clippy::pedantic, missing_docs)]
    tonic::include_proto!("contracts.v1");
}

pub mod json;
pub mod parsers;

pub use v1::*;

/// The `schema_version` every top-level message carries today (§6.0). Bump only on a breaking change,
/// which the "add, never rename" rule should make rare.
pub const SCHEMA_VERSION: u32 = 1;
