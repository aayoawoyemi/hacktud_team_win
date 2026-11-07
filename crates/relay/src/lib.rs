//! CONCEPT: relay crate (WP-6). Relay telemetry between main and helpers: parse GGML_RPC_DEBUG logs (option 6A), or a TCP proxy in the data path only if O-08 picks 6B.
//! WHY: empty skeleton from WP-12 R12.1 so every work package starts from a workspace that already builds;
//!      O-08 (6A vs 6B) is still open; telemetry parsing is needed either way.
//! DEPENDS ON: contracts (Heartbeat relay block).
//! AFFECTS: agent (fills Heartbeat.relay), coordinator status (WP-7).
//!
//! Empty on purpose. The owning work package fills it in on its own branch.
