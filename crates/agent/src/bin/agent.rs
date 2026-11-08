//! CONCEPT: the node agent process, one per machine: `agent <config.toml>`. Registers in SpacetimeDB and runs
//!          rpc-server when the coordinator asks (spacetime.rs). Ctrl-C stops rpc-server and exits.
//! WHY: O-02 picks one `pool` CLI, which doesn't exist yet; like `netcheck` and `coordinator`, this crate ships
//!      its own binary until the shared `pool agent` hook lands (a one-file move).
//! DEPENDS ON: agent::spacetime::run; O-24 (TOML per role; example in crates/agent/agent.example.toml).
//! AFFECTS: the demo runbook: this is the one command each helper machine runs.

#[tokio::main]
async fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: agent <config.toml>");
        std::process::exit(2);
    };
    let cfg = match agent::AgentConfig::load(path.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("agent: E_CONFIG: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = agent::spacetime::run(cfg).await {
        eprintln!("agent: {e}");
        std::process::exit(1);
    }
}
