//! CONCEPT: the human-readable output. A fixed-width table, one row per target, then a hint line per failure.
//! WHY: WP-10 R10.2 acceptance: "a teammate can add a new machine using docs/network.md alone". The table has
//!      to say pass/fail and what to fix, with no JSON reading.
//! DEPENDS ON: probe.rs, peer.rs reports.
//! AFFECTS: docs/network.md shows this output as "what a pass looks like"; keep the two in step.
//!
//! RTT cells print 2 decimals (LAN RTTs are often below 1 ms). Throughput prints MB/s (10^6 bytes) and Mbit/s.

use crate::peer::ClientReport;
use crate::probe::ProbeReport;
use std::fmt::Write;

/// p95 above this gets a "slow" note. 20 ms: a healthy Wi-Fi LAN hop is ~1-10 ms; above 20 ms the per-token
/// cost of a layer split grows noticeably (PRD §4.2). A note only, not a failure.
pub const SLOW_P95_MS: f64 = 20.0;

fn ms(v: Option<f64>) -> String {
    v.map_or_else(|| "-".into(), |x| format!("{x:.2}"))
}

fn pad(s: &str, w: usize) -> String {
    format!("{s:<w$}")
}

/// Table for `check`. Returns the text; the caller prints it.
pub fn check_table(reports: &[ProbeReport]) -> String {
    let lw = reports
        .iter()
        .map(|r| r.target.label.len())
        .max()
        .unwrap_or(0)
        .max(6);
    let aw = reports
        .iter()
        .map(|r| r.target.addr.len())
        .max()
        .unwrap_or(0)
        .max(4);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}  {}  {:<14}  {:>5}  {:>8}  {:>8}  {:>8}  {:>8}  NOTE",
        pad("TARGET", lw),
        pad("ADDR", aw),
        "STATUS",
        "OK/N",
        "MIN ms",
        "P50 ms",
        "P95 ms",
        "MAX ms"
    );
    for r in reports {
        let s = r.summary;
        let note = match (r.ok(), s) {
            (true, Some(s)) if s.p95 > SLOW_P95_MS => format!("slow: p95 > {SLOW_P95_MS} ms"),
            (true, _) if !r.failures.is_empty() => format!("{} attempts failed", r.failures.len()),
            (false, _) => r
                .first_failure()
                .map(|f| f.message.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let _ = writeln!(
            out,
            "{}  {}  {:<14}  {:>5}  {:>8}  {:>8}  {:>8}  {:>8}  {}",
            pad(&r.target.label, lw),
            pad(&r.target.addr, aw),
            r.status(),
            format!("{}/{}", r.rtts_ms.len(), r.attempts),
            ms(s.map(|s| s.min)),
            ms(s.map(|s| s.p50)),
            ms(s.map(|s| s.p95)),
            ms(s.map(|s| s.max)),
            note
        );
    }
    let failed: Vec<&ProbeReport> = reports.iter().filter(|r| !r.ok()).collect();
    let _ = writeln!(out);
    if failed.is_empty() {
        let _ = writeln!(
            out,
            "PASS: {} of {} targets reachable.",
            reports.len(),
            reports.len()
        );
    } else {
        let _ = writeln!(
            out,
            "FAIL: {} of {} targets unreachable.",
            failed.len(),
            reports.len()
        );
        for r in failed {
            if let Some(f) = r.first_failure() {
                let _ = writeln!(
                    out,
                    "  {} ({}): {}",
                    r.target.label,
                    f.kind.as_str(),
                    f.kind.hint()
                );
            }
        }
    }
    out
}

/// Text block for `client`.
pub fn client_table(label: &str, r: &ClientReport, rtt_requested: u32) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "netcheck client -> {label} ({})", r.addr);
    match r.rtt {
        Some(s) => {
            let _ = writeln!(
                out,
                "  echo RTT (8-byte messages, TCP_NODELAY, {}/{} samples): min {:.2}  p50 {:.2}  p95 {:.2}  max {:.2} ms",
                s.samples, rtt_requested, s.min, s.p50, s.p95, s.max
            );
        }
        None => {
            let _ = writeln!(out, "  echo RTT: -");
        }
    }
    for (name, t) in [
        ("send (this -> peer)", r.send),
        ("recv (peer -> this)", r.recv),
    ] {
        if let Some(t) = t {
            let _ = writeln!(
                out,
                "  {name}: {:.1} MB in {:.2} s = {:.1} MB/s ({:.0} Mbit/s)",
                t.bytes as f64 / 1e6,
                t.secs,
                t.mbyte_per_s(),
                t.mbit_per_s()
            );
        }
    }
    match &r.error {
        None => {
            let _ = writeln!(out, "\nPASS");
        }
        Some((k, m)) => {
            let _ = writeln!(out, "\nFAIL ({}): {m}\n  hint: {}", k.as_str(), k.hint());
        }
    }
    out
}
