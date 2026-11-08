//! CONCEPT: coordinator crate (WP-4; WP-7 status will live in src/status/, WP-9 recovery in src/recovery/). The main
//!          Mac's control process: turns registered nodes plus a SplitPlan into a running llama-server (PRD §5.5).
//! WHY: O-06 (main is a Mac) needs one process that owns the llama-server lifecycle. Under D-13 the registry, commands
//!      and events live in SpacetimeDB tables; this crate does the local work SpacetimeDB modules can't (start
//!      processes, reach the LAN).
//! DEPENDS ON: contracts (PR #18) and the SpacetimeDB module `crates/control` (not merged yet); planner (PR #20).
//! AFFECTS: agent (receives commands), bench (needs the endpoint), the O-10 display.
//!
//! Built in slices. Slice 1 (this): launch.rs, the llama-server lifecycle, with no dependency on unmerged PRs.
//! Next: subscribe to SpacetimeDB, send start_rpc_server commands, take the SplitPlan from the planner.

pub mod launch;
