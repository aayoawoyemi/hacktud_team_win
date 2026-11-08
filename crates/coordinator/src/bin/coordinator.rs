//! CONCEPT: the coordinator process on the main Mac: `coordinator <config.toml>`. Waits for helpers, brings the pool
//!          up, re-plans when a helper joins, and stops everything when you press Enter.
//! WHY: O-02 picks one `pool` CLI with subcommands, but that shared binary doesn't exist yet; like `netcheck`, this
//!      crate ships its own binary until the `pool coordinator` hook lands (a one-file move).
//! DEPENDS ON: cycle.rs; O-24 (TOML per role); SPACETIME_TOKEN = the publisher's token (admin), never committed.
//!             Without it the coordinator gets a fresh identity and prints the add_admin command to run.
//! AFFECTS: the demo runbook (WP-13): this is the command that turns registered machines into a serving pool.

use coordinator::cycle::{Config, Coordinator};
use std::io::BufRead;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn main() {
    if let Err(e) = run() {
        eprintln!("coordinator: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: coordinator <config.toml>")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("E_CONFIG: {path}: {e}"))?;
    let mut cfg: Config = toml::from_str(&text).map_err(|e| format!("E_CONFIG: {path}: {e}"))?;
    cfg.token = std::env::var("SPACETIME_TOKEN").ok();
    let mut c = Coordinator::connect(cfg)?;
    if let Some(id) = c.identity() {
        eprintln!("coordinator identity {}", id.to_hex());
        eprintln!(
            "  (not admin yet? run: spacetime call -s local <db> add_admin '\"0x{}\"')",
            id.to_hex()
        );
    }

    // Enter on stdin = graceful stop (Ctrl-C would skip stop_rpc_server and leave the cluster row stale).
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    std::thread::spawn(move || {
        let _ = std::io::stdin().lock().lines().next();
        s.store(true, Ordering::SeqCst);
    });

    eprintln!("waiting for helpers... (press Enter to stop)");
    while c.helpers().is_empty() && !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(500));
    }
    if !stop.load(Ordering::SeqCst) {
        c.bring_up()?;
        eprintln!("SERVING on {:?}: {:?}", c.cfg.port, c.active());
    }
    while !stop.load(Ordering::SeqCst) {
        let before = c.active().len();
        c.tick()?;
        if c.active().len() != before {
            eprintln!("re-planned, SERVING with {:?}", c.active());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    c.shutdown()?;
    eprintln!("stopped");
    Ok(())
}
