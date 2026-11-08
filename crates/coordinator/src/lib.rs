//! CONCEPT: coordinator crate (WP-4; WP-7 status will live in src/status/, WP-9 recovery in src/recovery/). The main
//!          Mac's control process: turns registered nodes plus a SplitPlan into a running llama-server (PRD §5.5).
//! WHY: O-06 (main is a Mac) needs one process that owns the llama-server lifecycle. Under D-13 the registry, commands
//!      and events live in SpacetimeDB tables; this crate does the local work SpacetimeDB modules can't (start
//!      processes, reach the LAN).
//! DEPENDS ON: contracts (types + SpacetimeDB bindings for `crates/control`); planner (#20) once merged.
//! AFFECTS: agent (receives commands), bench (needs the endpoint), the O-10 display.
//!
//! Slice 1: launch.rs, the llama-server lifecycle. Slice 2: cycle.rs (plan -> start helpers -> launch -> SERVING,
//! re-plan on join, stop) and the `coordinator` binary. Next: WP-5's SplitPlan (integer -ts) instead of llama_default.

pub mod cycle;
pub mod launch;
