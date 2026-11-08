//! CONCEPT: netcheck end to end on 127.0.0.1: an open port passes, a closed port is "refused", and the
//!          serve/client pair measures echo RTT and throughput both ways.
//! WHY: PRD test plan, Unit row: "netcheck on loopback" (WP-10). Runs on every machine with no network.
//! DEPENDS ON: the netcheck library only. Ports are OS-assigned (bind :0), so tests can run in parallel.
//! AFFECTS: nothing; failing here means the tool would mislabel a real link.

use netcheck::peer::{self, ClientOptions};
use netcheck::probe::{FailKind, ProbeOptions, connect_probe};
use netcheck::result::{NetcheckResult, now_rfc3339};
use netcheck::target::Target;
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

fn target(addr: String) -> Target {
    Target {
        label: "loopback".into(),
        addr,
    }
}

#[test]
fn open_port_is_ok() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming() {
            drop(s);
        }
    });
    let opts = ProbeOptions {
        samples: 10,
        timeout: Duration::from_secs(5),
        gap: Duration::ZERO,
    };
    let r = connect_probe(&target(addr.to_string()), &opts);
    assert!(r.ok(), "failures: {:?}", r.failures);
    assert_eq!(r.status(), "ok");
    assert_eq!(r.attempts, 10);
    let s = r.summary.unwrap();
    assert_eq!(s.samples, 10);
    assert!(s.min >= 0.0 && s.min <= s.p50 && s.p50 <= s.p95 && s.p95 <= s.max);

    let j = NetcheckResult::from_probe("test", &r, &now_rfc3339());
    assert!(j.error.is_none());
    assert_eq!(j.rtt_ms.samples, 10);
    assert!(j.throughput_mbps.send.is_none());
}

#[test]
fn closed_port_is_refused() {
    // Get a free port, then close it. Nothing listens there afterwards.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    // Windows retries a refused connect for ~2 s, so the timeout must be longer than that (see probe.rs).
    let opts = ProbeOptions {
        samples: 5,
        timeout: Duration::from_secs(5),
        gap: Duration::ZERO,
    };
    let r = connect_probe(&target(format!("127.0.0.1:{port}")), &opts);
    assert!(!r.ok());
    assert_eq!(
        r.first_failure().unwrap().kind,
        FailKind::Refused,
        "{:?}",
        r.failures
    );
    assert_eq!(r.status(), "refused");
    assert_eq!(r.attempts, 1, "a failing first attempt stops the probe");

    let j = NetcheckResult::from_probe("test", &r, &now_rfc3339());
    let e = j.error.unwrap();
    assert_eq!(e.code, "E_NODE_UNREACHABLE");
    assert!(e.message.starts_with("refused:"), "{}", e.message);
    assert_eq!(j.rtt_ms.samples, 0);
    assert!(j.rtt_ms.p50.is_none());
}

#[test]
fn check_probe_of_netcheck_server_is_ok() {
    // `check` against port 50060 connects and closes with no bytes; the server must survive that.
    let (addr, _h) = peer::spawn_server("127.0.0.1:0").unwrap();
    let opts = ProbeOptions {
        samples: 5,
        timeout: Duration::from_secs(5),
        gap: Duration::ZERO,
    };
    assert!(connect_probe(&target(addr.to_string()), &opts).ok());
    let r = peer::run_client(
        addr,
        &ClientOptions {
            rtt_samples: 5,
            bytes: 0,
            timeout: Duration::from_secs(5),
        },
    );
    assert!(r.error.is_none(), "{:?}", r.error);
}

#[test]
fn throughput_loopback() {
    let (addr, _h) = peer::spawn_server("127.0.0.1:0").unwrap();
    let opts = ClientOptions {
        rtt_samples: 50,
        bytes: 8_000_000,
        timeout: Duration::from_secs(10),
    };
    let r = peer::run_client(addr, &opts);
    assert!(r.error.is_none(), "{:?}", r.error);

    let rtt = r.rtt.unwrap();
    assert_eq!(rtt.samples, 50);
    assert!(rtt.p50 < 50.0, "loopback echo p50 {} ms", rtt.p50);

    let (send, recv) = (r.send.unwrap(), r.recv.unwrap());
    assert_eq!(send.bytes, 8_000_000);
    assert_eq!(recv.bytes, 8_000_000);
    assert!(send.secs > 0.0 && recv.secs > 0.0);
    // Loopback moves far more than 10 MB/s on any laptop; this only catches a broken measurement.
    assert!(
        send.mbyte_per_s() > 10.0,
        "send {} MB/s",
        send.mbyte_per_s()
    );
    assert!(
        recv.mbyte_per_s() > 10.0,
        "recv {} MB/s",
        recv.mbyte_per_s()
    );

    let j = NetcheckResult::from_client("test", "loopback", &addr.to_string(), &r, &now_rfc3339());
    assert!(j.tcp_nodelay);
    assert!(j.throughput_mbps.send.unwrap() > 80.0); // Mbit/s = 8 x MB/s
    assert!(j.throughput_mbps.duration_s >= 1);
    let v = serde_json::to_value(&j).unwrap();
    for k in [
        "schema_version",
        "from_node",
        "to_node",
        "to_addr",
        "measured_at",
        "rtt_ms",
        "throughput_mbps",
        "tcp_nodelay",
        "during_generation",
        "error",
    ] {
        assert!(v.get(k).is_some(), "missing {k}");
    }
}

#[test]
fn client_against_non_netcheck_listener_fails_cleanly() {
    // A listener that accepts and closes at once (like a service that hangs up): client must report an
    // error, not hang or panic.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming() {
            drop(s);
        }
    });
    let r = peer::run_client(
        addr,
        &ClientOptions {
            rtt_samples: 5,
            bytes: 1_000_000,
            timeout: Duration::from_secs(3),
        },
    );
    assert!(r.error.is_some());
    assert!(r.rtt.is_none());
}
