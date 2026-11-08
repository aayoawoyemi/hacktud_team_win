//! CONCEPT: netcheck crate (WP-10). Checks that machines reach each other on the ports the run needs, and
//!          measures round-trip time and throughput between them.
//! WHY: WP-10 R10.1/R10.3. Token latency in a layer split is dominated by per-hop RTT (PRD §4.2); the team
//!      needs measured numbers per network option (O-05), and a one-command preflight before launch so a
//!      firewall or wrong IP shows up as a clear FAIL line instead of a llama-server abort (cppneed-02: an
//!      unreachable helper at launch aborts llama-server).
//! DEPENDS ON: std::net (no async runtime), serde/serde_json (JSON), toml (targets file).
//! AFFECTS: planner (WP-5 uses RTT), coordinator status (WP-7), the WP-3 agent's `netcheck` command
//!          (it can call `peer::run_client`), docs/network.md (preflight commands).
//!
//! Modules:
//! - `target`: labelled host:port list from CLI args or TOML.
//! - `probe`:  `check` mode, timed TCP connects against any listener + failure kind.
//! - `peer`:   `serve` / `client` modes, echo RTT and bulk throughput against a netcheck peer.
//! - `stats`:  min / p50 / p95 / max.
//! - `result`: §6.9 NetcheckResult JSON (mirror of the WP-2 type until PR #18 merges).
//! - `report`: the printed table.
//!
//! The dev binary is src/bin/netcheck.rs. The shared CLI (O-02) will call these functions after PR #18.

pub mod peer;
pub mod probe;
pub mod report;
pub mod result;
pub mod stats;
pub mod target;

pub use peer::{ClientOptions, ClientReport, DEFAULT_PORT, run_client, spawn_server};
pub use probe::{FailKind, ProbeOptions, ProbeReport, connect_probe, connect_probe_all};
pub use result::NetcheckResult;
pub use target::Target;
