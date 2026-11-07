//! CONCEPT: coordinator crate (WP-4 (+ WP-7 status in src/status/, WP-9 recovery in src/recovery/)). Registry of nodes, launcher of llama-server on main, owner of the cluster state machine (PRD §5.6).
//! WHY: empty skeleton from WP-12 R12.1 so every work package starts from a workspace that already builds;
//!      the main machine (O-06: a Mac) needs one process that turns registered nodes plus a SplitPlan into a running llama-server.
//! DEPENDS ON: contracts, planner (SplitPlan).
//! AFFECTS: agent (sends it Commands), bench (reads ClusterStatus), the O-10 display.
//!
//! Empty on purpose. The owning work package fills it in on its own branch.
