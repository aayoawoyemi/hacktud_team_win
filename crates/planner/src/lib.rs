//! CONCEPT: planner crate (WP-5). PLACEHOLDER. GGUF reader, capacity normalization and split planner.
//! WHY: empty skeleton from WP-12 R12.1 so every work package starts from a workspace that already builds;
//!      WP-5 is being written by another agent in his own crate; he moves it into this folder. This stub only exists so the workspace builds before then.
//! DEPENDS ON: contracts (ModelInfo, SplitPlan) once WP-5 lands.
//! AFFECTS: coordinator (asks for a SplitPlan).
//!
//! Empty on purpose. The owning work package fills it in on its own branch.
//!
//! HAND-OFF (WP-5): replace this whole folder with your crate. Keep the package name `planner` (or rename it
//! in the root Cargo.toml `members` list in the same PR) so the workspace keeps building.
