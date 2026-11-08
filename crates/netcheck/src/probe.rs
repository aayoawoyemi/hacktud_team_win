//! CONCEPT: reachability + connect RTT. For one target, open N TCP connections with a timeout, time each
//!          handshake, close it, and classify any failure as refused / timed_out / unreachable / resolve / other.
//! WHY: WP-10 R10.1/R10.3 and risk R-06 (firewalls). The connect handshake (SYN -> SYN-ACK) costs one network
//!      round trip, so it measures RTT against ANY listener (rpc-server, llama-server, SpacetimeDB) without a
//!      netcheck peer. The error kind tells the human what to fix:
//!        refused     -> the machine answered but nothing listens on that port (start the service / wrong port)
//!        timed_out   -> no answer at all: usually a firewall dropping packets, or a wrong IP
//!        unreachable -> no route: different network, client isolation, or the machine is off the Wi-Fi
//! DEPENDS ON: std::net only; stats.rs.
//! AFFECTS: the `check` command, result.rs, docs/network.md (the meaning of each status).
//!
//! Notes:
//! - Windows retries a refused connect for about 2 s before reporting it (also on loopback), so a timeout
//!   shorter than ~2.5 s turns "refused" into "timed_out" on Windows. Default timeout is 3 s for that reason.
//! - If the FIRST attempt fails, the probe stops there: repeating a 3 s timeout 20 times only wastes time.
//! - Connecting to llama.cpp's rpc-server and closing at once is harmless: checked on the Dell (Windows,
//!   build 8665, 127.0.0.1), 40 bare connects, rpc-server logged "Accepted client connection" / "Client
//!   connection closed" for each and kept running. VERIFY the same on a Mac (Metal build).
//!   Probe rpc-server before llama-server connects to it, not while a model is loading.
//!
//! Rejected: ICMP ping. Needs raw sockets / admin on Windows, and a firewall may allow ICMP but block the TCP
//!           port that actually matters (or the reverse).

use crate::stats::{Summary, summarize};
use crate::target::Target;
use std::io;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::thread;
use std::time::{Duration, Instant};

/// Why a connect attempt failed. `as_str` is the spelling used in the table and the JSON error message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    Refused,
    TimedOut,
    Unreachable,
    Resolve,
    Other,
}

impl FailKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FailKind::Refused => "refused",
            FailKind::TimedOut => "timed_out",
            FailKind::Unreachable => "unreachable",
            FailKind::Resolve => "resolve_failed",
            FailKind::Other => "error",
        }
    }

    /// One-line hint for the human, printed under the table.
    pub fn hint(self) -> &'static str {
        match self {
            FailKind::Refused => {
                "machine reached, nothing listening on that port: start the service or fix the port"
            }
            FailKind::TimedOut => {
                "no answer: firewall dropping the port, wrong IP, or machine asleep"
            }
            FailKind::Unreachable => {
                "no route: not on the same network, client isolation on, or Wi-Fi off"
            }
            FailKind::Resolve => "host name did not resolve: use the IP address instead",
            FailKind::Other => "see the error message",
        }
    }
}

/// Maps an io::Error from `connect_timeout` to a [`FailKind`].
pub fn classify(e: &io::Error) -> FailKind {
    use io::ErrorKind as K;
    match e.kind() {
        K::ConnectionRefused => FailKind::Refused,
        K::TimedOut | K::WouldBlock => FailKind::TimedOut,
        K::HostUnreachable | K::NetworkUnreachable | K::NetworkDown | K::AddrNotAvailable => {
            FailKind::Unreachable
        }
        _ => FailKind::Other,
    }
}

/// One failed attempt.
#[derive(Debug, Clone)]
pub struct Failure {
    pub kind: FailKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ProbeOptions {
    /// Connect attempts per target.
    pub samples: u32,
    /// Timeout per attempt.
    pub timeout: Duration,
    /// Pause between attempts, so the samples don't all land in the same Wi-Fi burst.
    pub gap: Duration,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        ProbeOptions {
            samples: 20,
            timeout: Duration::from_millis(3000),
            gap: Duration::from_millis(20),
        }
    }
}

/// Result of probing one target.
#[derive(Debug, Clone)]
pub struct ProbeReport {
    pub target: Target,
    /// The resolved address actually dialled (None if resolution failed).
    pub resolved: Option<SocketAddr>,
    pub attempts: u32,
    pub rtts_ms: Vec<f64>,
    pub failures: Vec<Failure>,
    pub summary: Option<Summary>,
}

impl ProbeReport {
    /// Reachable = at least one handshake completed.
    pub fn ok(&self) -> bool {
        !self.rtts_ms.is_empty()
    }

    /// The most informative failure: the first one (later ones usually repeat it).
    pub fn first_failure(&self) -> Option<&Failure> {
        self.failures.first()
    }

    /// Status word for the table: "ok", or the failure kind.
    pub fn status(&self) -> &'static str {
        match (self.ok(), self.first_failure()) {
            (true, _) => "ok",
            (false, Some(f)) => f.kind.as_str(),
            (false, None) => "error",
        }
    }
}

/// Resolves `host:port`. Prefers IPv4: the LAN plan is IPv4 and a `.local` name can also return a
/// link-local IPv6 address that needs a scope id to dial.
pub fn resolve(addr: &str) -> Result<SocketAddr, Failure> {
    let addrs: Vec<SocketAddr> = addr
        .to_socket_addrs()
        .map_err(|e| Failure {
            kind: FailKind::Resolve,
            message: e.to_string(),
        })?
        .collect();
    addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or_else(|| addrs.first())
        .copied()
        .ok_or_else(|| Failure {
            kind: FailKind::Resolve,
            message: format!("{addr} resolved to nothing"),
        })
}

/// Probes one target: N timed TCP connects.
pub fn connect_probe(target: &Target, opts: &ProbeOptions) -> ProbeReport {
    let mut report = ProbeReport {
        target: target.clone(),
        resolved: None,
        attempts: 0,
        rtts_ms: Vec::new(),
        failures: Vec::new(),
        summary: None,
    };
    let sa = match resolve(&target.addr) {
        Ok(sa) => sa,
        Err(f) => {
            report.failures.push(f);
            return report;
        }
    };
    report.resolved = Some(sa);
    for i in 0..opts.samples.max(1) {
        if i > 0 && !opts.gap.is_zero() {
            thread::sleep(opts.gap);
        }
        report.attempts += 1;
        let t0 = Instant::now();
        match TcpStream::connect_timeout(&sa, opts.timeout) {
            Ok(stream) => {
                report.rtts_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                drop(stream);
            }
            Err(e) => {
                report.failures.push(Failure {
                    kind: classify(&e),
                    message: e.to_string(),
                });
                if i == 0 {
                    break; // see module notes: don't repeat a failing first attempt
                }
            }
        }
    }
    report.summary = summarize(&report.rtts_ms);
    report
}

/// Probes every target, one after another (sequential on purpose: parallel probes on one Wi-Fi link would
/// inflate each other's RTT).
pub fn connect_probe_all(targets: &[Target], opts: &ProbeOptions) -> Vec<ProbeReport> {
    targets.iter().map(|t| connect_probe(t, opts)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use io::ErrorKind as K;

    #[test]
    fn classify_kinds() {
        assert_eq!(
            classify(&io::Error::from(K::ConnectionRefused)),
            FailKind::Refused
        );
        assert_eq!(classify(&io::Error::from(K::TimedOut)), FailKind::TimedOut);
        assert_eq!(
            classify(&io::Error::from(K::HostUnreachable)),
            FailKind::Unreachable
        );
        assert_eq!(
            classify(&io::Error::from(K::NetworkUnreachable)),
            FailKind::Unreachable
        );
        assert_eq!(
            classify(&io::Error::from(K::PermissionDenied)),
            FailKind::Other
        );
    }

    #[test]
    fn resolve_failure_is_resolve_kind() {
        let t = Target {
            label: "x".into(),
            addr: "no-such-host.invalid:50052".into(),
        };
        let r = connect_probe(
            &t,
            &ProbeOptions {
                samples: 3,
                ..Default::default()
            },
        );
        assert!(!r.ok());
        assert_eq!(r.status(), "resolve_failed");
        assert_eq!(r.attempts, 0);
    }
}
