//! CONCEPT: TCP port probe: "does something accept a connection at host:port within T?".
//! WHY: R3.5 rpc-server health = process alive AND port accepting (§6.2 `rpc_server.port_accepting`);
//!      R3.9 detect a port already in use before spawning (`E_PORT_IN_USE`). The supervisor also uses it as
//!      the Listening signal, because rpc-server's banner goes to buffered stdout and its arrival time
//!      is not guaranteed (see supervisor.rs); a successful connect is the fact that matters to llama-server.
//! DEPENDS ON: tokio net + time.
//! AFFECTS: supervisor.rs; later the heartbeat's `port_accepting`.
//!
//! The probe connects and immediately drops the socket. rpc-server (build 8665) serves one client at a time
//! and treats a connection that closes before its HELLO as a finished client, so probing is harmless while
//! it is idle. ASSUMPTION: while llama-server is connected, a probe waits in the accept backlog and times
//! out (reported as not accepting). Callers should not probe a Listening server in a tight loop.

use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{TcpStream, lookup_host};

/// True if a TCP connect to `host:port` succeeds within `timeout`. Any error (refused, unreachable, DNS,
/// timeout) is `false`: the caller only needs yes/no.
pub async fn tcp_accepting(host: &str, port: u16, timeout: Duration) -> bool {
    let fut = async {
        let addrs: Vec<SocketAddr> = lookup_host((host, port)).await.ok()?.collect();
        for addr in addrs {
            if TcpStream::connect(addr).await.is_ok() {
                return Some(());
            }
        }
        None
    };
    matches!(tokio::time::timeout(timeout, fut).await, Ok(Some(())))
}

/// The address to probe for a server bound to `bind`. A wildcard bind (0.0.0.0 / ::) isn't connectable on
/// every OS (Windows refuses 0.0.0.0), so probe loopback instead.
pub fn probe_host_for_bind(bind: &str) -> &str {
    match bind {
        "0.0.0.0" | "" => "127.0.0.1",
        "::" | "[::]" => "::1",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accepting_then_closed() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(tcp_accepting("127.0.0.1", port, Duration::from_secs(2)).await);
        drop(l);
        // Not the freed port: under parallel tests another test can bind it again in between (flaked on macOS).
        // Port 1 (tcpmux) has no listener on any dev machine, so connect is refused deterministically.
        assert!(!tcp_accepting("127.0.0.1", 1, Duration::from_millis(500)).await);
    }

    #[test]
    fn wildcard_maps_to_loopback() {
        assert_eq!(probe_host_for_bind("0.0.0.0"), "127.0.0.1");
        assert_eq!(probe_host_for_bind("192.168.1.5"), "192.168.1.5");
    }
}
