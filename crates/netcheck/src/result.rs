//! CONCEPT: the JSON netcheck prints: one §6.9 NetcheckResult per target, field for field.
//! WHY: WP-10 R10.3 says netcheck prints NetcheckResult (§6.9). WP-5 (speed estimate) and WP-7 (status) read
//!      it. The Rust type lives in `contracts` on the WP-2 branch (PR #18, not on main yet), so this file
//!      mirrors its JSON shape (fixtures/json/netcheck_result.json on that branch). After #18 merges, replace
//!      these structs with `contracts::NetcheckResult` and keep the `from_*` builders.
//! DEPENDS ON: serde; probe.rs and peer.rs reports; the WP-2 fixture shape (schema_version 1).
//! AFFECTS: WP-5, WP-7 JSON readers; the `netcheck_result` event (§6.10) once the agent sends it.
//!
//! Mapping choices (flag in #11 if WP-2 disagrees):
//! - `check` (connect probe): `rtt_ms` = TCP handshake times, `tcp_nodelay` = false (no payload is sent),
//!   `throughput_mbps` = { send: null, recv: null, duration_s: 0 }.
//! - `client` (netcheck peer): `rtt_ms` = 8-byte echo times with TCP_NODELAY, `tcp_nodelay` = true,
//!   `throughput_mbps` in Mbit/s (10^6 bits, the proto comment's unit), `duration_s` = measured seconds of
//!   upload + download, rounded up (the fixture's duration_s is a requested length; netcheck sends a fixed
//!   byte count instead, see peer.rs).
//! - `error`: `{ code, message }` with §6.11 codes. Every connect failure is E_NODE_UNREACHABLE except "other"
//!   (E_TRANSPORT). The message starts with the failure kind ("refused: ..."), so readers can branch on it
//!   without a new field. `error` is null when at least one RTT sample succeeded.

use crate::peer::ClientReport;
use crate::probe::{FailKind, ProbeReport};
use crate::stats::Summary;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetcheckResult {
    pub schema_version: u32,
    pub from_node: String,
    pub to_node: String,
    pub to_addr: String,
    pub measured_at: String,
    pub rtt_ms: RttStats,
    pub throughput_mbps: ThroughputStats,
    pub tcp_nodelay: bool,
    pub during_generation: bool,
    pub error: Option<ErrorInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RttStats {
    pub min: Option<f64>,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub max: Option<f64>,
    pub samples: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThroughputStats {
    pub send: Option<f64>,
    pub recv: Option<f64>,
    pub duration_s: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
}

/// Rounds to `places` decimals so the JSON reads `0.069`, not `0.0686000000004`. RTT keeps 3 decimals
/// (1 microsecond, below any clock jitter that matters here); throughput keeps 1 (0.1 Mbit/s).
fn round(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (x * f).round() / f
}

impl RttStats {
    fn from_summary(s: Option<Summary>) -> Self {
        let r = |x: f64| Some(round(x, 3));
        match s {
            Some(s) => RttStats {
                min: r(s.min),
                p50: r(s.p50),
                p95: r(s.p95),
                max: r(s.max),
                samples: s.samples,
            },
            None => RttStats {
                min: None,
                p50: None,
                p95: None,
                max: None,
                samples: 0,
            },
        }
    }
}

fn error_info(kind: FailKind, message: &str) -> ErrorInfo {
    let code = match kind {
        FailKind::Other => "E_TRANSPORT",
        _ => "E_NODE_UNREACHABLE",
    };
    ErrorInfo {
        code: code.into(),
        message: format!("{}: {message}", kind.as_str()),
    }
}

impl NetcheckResult {
    /// From a `check` connect probe.
    pub fn from_probe(from_node: &str, r: &ProbeReport, measured_at: &str) -> Self {
        NetcheckResult {
            schema_version: SCHEMA_VERSION,
            from_node: from_node.into(),
            to_node: r.target.label.clone(),
            to_addr: r.target.addr.clone(),
            measured_at: measured_at.into(),
            rtt_ms: RttStats::from_summary(r.summary),
            throughput_mbps: ThroughputStats {
                send: None,
                recv: None,
                duration_s: 0,
            },
            tcp_nodelay: false,
            during_generation: false,
            error: if r.ok() {
                None
            } else {
                r.first_failure().map(|f| error_info(f.kind, &f.message))
            },
        }
    }

    /// From a `client` run against a netcheck peer.
    pub fn from_client(
        from_node: &str,
        to_node: &str,
        to_addr: &str,
        r: &ClientReport,
        measured_at: &str,
    ) -> Self {
        let secs = r.send.map_or(0.0, |t| t.secs) + r.recv.map_or(0.0, |t| t.secs);
        NetcheckResult {
            schema_version: SCHEMA_VERSION,
            from_node: from_node.into(),
            to_node: to_node.into(),
            to_addr: to_addr.into(),
            measured_at: measured_at.into(),
            rtt_ms: RttStats::from_summary(r.rtt),
            throughput_mbps: ThroughputStats {
                send: r.send.map(|t| round(t.mbit_per_s(), 1)),
                recv: r.recv.map(|t| round(t.mbit_per_s(), 1)),
                duration_s: secs.ceil() as u32,
            },
            tcp_nodelay: true,
            during_generation: false,
            error: r.error.as_ref().map(|(k, m)| error_info(*k, m)),
        }
    }
}

/// UTC timestamp, RFC 3339 with seconds ("2026-11-07T22:30:00Z"), the §6.9 `measured_at` format.
/// Hand-rolled (Howard Hinnant's days-to-civil) so the crate needs no chrono/time dependency.
pub fn rfc3339_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}

pub fn now_rfc3339() -> String {
    rfc3339_utc(SystemTime::now())
}

/// This machine's name for `from_node`: COMPUTERNAME (Windows) or HOSTNAME, else the `hostname` command.
/// The dev binary's `--from` overrides it; the agent will pass its node_id instead.
pub fn local_node_name() -> String {
    for var in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(v) = std::env::var(var)
            && !v.trim().is_empty()
        {
            return v.trim().to_string();
        }
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps() {
        assert_eq!(rfc3339_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        // 2026-11-07T22:30:00Z, the §6.9 fixture's timestamp.
        assert_eq!(
            rfc3339_utc(UNIX_EPOCH + Duration::from_secs(1_794_090_600)),
            "2026-11-07T22:30:00Z"
        );
        // Leap day.
        assert_eq!(
            rfc3339_utc(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00Z"
        );
    }

    /// The WP-2 fixture (copied from origin/aayoawoyemi/16-wp-2-contracts:fixtures/json/netcheck_result.json)
    /// must round-trip through these structs unchanged.
    #[test]
    fn wp2_fixture_round_trips() {
        let fixture = r#"{
          "schema_version": 1,
          "from_node": "example-dell",
          "to_node": "example-mac-a",
          "to_addr": "192.0.2.11:50060",
          "measured_at": "2026-11-07T22:30:00Z",
          "rtt_ms": { "min": null, "p50": null, "p95": null, "max": null, "samples": 100 },
          "throughput_mbps": { "send": null, "recv": null, "duration_s": 10 },
          "tcp_nodelay": true,
          "during_generation": false,
          "error": null
        }"#;
        let r: NetcheckResult = serde_json::from_str(fixture).unwrap();
        let a: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let b = serde_json::to_value(&r).unwrap();
        assert_eq!(a, b);
    }
}
