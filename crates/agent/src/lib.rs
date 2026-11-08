//! CONCEPT: agent crate (WP-3). The node agent that runs on every machine: probes hardware, starts and
//!          supervises rpc-server, and (later) registers with the control plane and sends heartbeats.
//! WHY: O-01 = A: Rust supervises the official llama.cpp binaries (build 8665, D-04) as child processes; the
//!      agent is that supervisor on each node. D-05: all team code is Rust.
//! DEPENDS ON: the llama.cpp binaries `rpc-server` and `llama-server` (PATH or `llama_bin_dir`), tokio, sysinfo.
//! AFFECTS: coordinator (WP-4) through the control plane (D-13: SpacetimeDB), WP-6 6A (relay telemetry
//!          reads the rpc-server log ring), WP-7 status.
//!
//! Modules:
//! - [`config`]: the agent's TOML config (O-24), keys from PRD §6.12.
//! - [`parse`]: thin local parsers for llama.cpp text output. TODO(WP-2): switch to `contracts::parsers`
//!   once PR #18 merges.
//! - [`probe`]: hardware + llama.cpp probe (R3.1), a plain struct.
//! - [`port`]: TCP connect probe with timeout (R3.5, R3.9).
//! - [`ring`]: bounded ring buffer of child output lines.
//! - [`supervisor`]: spawn / watch / stop one rpc-server child (R3.4 to R3.6). No auto-restart (D-14).
//! - [`control`]: the `TODO(D-13)` seam where register_node / heartbeat plug in once WP-2's Spacetime
//!   bindings land.
//!
//! Out of scope on purpose (D-14, joins-only demo): restart, recovery, re-plan. R3.8 restart-with-backoff
//! is NOT implemented; a crashed rpc-server stays `Exited` and is reported as such.

pub mod config;
pub mod control;
pub mod parse;
pub mod port;
pub mod probe;
pub mod ring;
pub mod supervisor;

pub use config::AgentConfig;
pub use probe::NodeProbe;
pub use supervisor::{RpcLaunch, RpcServer, RpcState};

/// The agent's version, reported as `agent_version` (§6.1).
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
