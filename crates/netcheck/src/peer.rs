//! CONCEPT: the netcheck peer protocol. `serve` listens on a port (default 50060); `client` connects to it and
//!          measures echo RTT (small messages, TCP_NODELAY) and bulk throughput in both directions.
//! WHY: WP-10 R10.3 ("listener mode and client mode; RTT with many small messages and TCP_NODELAY; throughput
//!      with bulk transfer"). Echo RTT is closer to what llama.cpp RPC does per token (small request, small
//!      reply on an open connection) than connect-time RTT, and bulk throughput bounds model-load time.
//! DEPENDS ON: std::net + threads only. Port 50060 is the decided netcheck port (O-17, Aayo 7:55pm;
//!      it matches the §6.9 fixture's example `to_addr`).
//! AFFECTS: docs/network.md (firewall rule for 50060), result.rs, the WP-3 agent's `netcheck` command later
//!      (it would call `run_client` against the peer's `serve`).
//!
//! Wire protocol (every connection is one command; integers are big-endian u64):
//!   client -> 'P', then repeat { 8 bytes }        server echoes each 8 bytes back           (echo RTT)
//!   client -> 'U', len, len bytes                  server reads all, replies 1 byte 'K'      (client -> server)
//!   client -> 'D', len                             server sends len bytes                    (server -> client)
//!   connect then close, no bytes                   server closes quietly    (a `check` probe of port 50060)
//! Upload is timed until the server's 'K', so it counts bytes that arrived, not bytes sitting in a send buffer.
//! Each timing includes one extra RTT (the request or the ack); at 1 ms RTT and >= 16 MB that is < 1% error.
//!
//! Rejected: iperf3. Not installed on the machines, a separate download per OS, and its JSON isn't §6.9.
//! Rejected: fixed-duration transfers (§6.9 args have `duration_s`). Fixed bytes is simpler to make exact on
//!           both ends; `duration_s` in the JSON reports the measured seconds (open question in the report).

use crate::stats::{Summary, mbit_per_s, mbyte_per_s, summarize};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Default netcheck listen port (O-17, decided: 50060).
pub const DEFAULT_PORT: u16 = 50060;

const CMD_PING: u8 = b'P';
const CMD_UPLOAD: u8 = b'U';
const CMD_DOWNLOAD: u8 = b'D';
const ACK: u8 = b'K';
const CHUNK: usize = 64 * 1024;
/// Refuse absurd transfer sizes from a confused or hostile client (16 GiB).
const MAX_TRANSFER: u64 = 16 << 30;
/// A server-side connection that sends nothing for this long is dropped.
const SERVER_IDLE: Duration = Duration::from_secs(60);

// ------------------------------------------------------------------------------------------------- server

/// Binds `bind` (e.g. "0.0.0.0:50060") and serves forever, one thread per connection.
/// `on_conn` is called with each peer address (the dev binary prints it; tests pass a no-op).
pub fn serve(
    listener: TcpListener,
    on_conn: impl Fn(SocketAddr, &str) + Send + Sync + 'static,
) -> io::Result<()> {
    let on_conn = std::sync::Arc::new(on_conn);
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(_) => continue, // a single failed accept (peer reset during handshake) must not stop the server
        };
        let cb = on_conn.clone();
        thread::spawn(move || {
            let peer = stream.peer_addr().ok();
            let what = handle_conn(stream).unwrap_or("error");
            if let Some(p) = peer {
                cb(p, what);
            }
        });
    }
    Ok(())
}

/// Starts a server on a background thread; returns the bound address. Used by tests and by `client
/// --loopback`. Bind to port 0 to get a free port.
pub fn spawn_server(bind: &str) -> io::Result<(SocketAddr, JoinHandle<io::Result<()>>)> {
    let listener = TcpListener::bind(bind)?;
    let addr = listener.local_addr()?;
    let h = thread::spawn(move || serve(listener, |_, _| {}));
    Ok((addr, h))
}

/// Handles one connection. Returns a word describing what the client did (for the server log).
fn handle_conn(mut s: TcpStream) -> io::Result<&'static str> {
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(SERVER_IDLE))?;
    let mut cmd = [0u8; 1];
    if s.read(&mut cmd)? == 0 {
        return Ok("probe"); // connect-and-close from `netcheck check`
    }
    match cmd[0] {
        CMD_PING => {
            let mut buf = [0u8; 8];
            loop {
                match s.read_exact(&mut buf) {
                    Ok(()) => s.write_all(&buf)?,
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                    Err(e) => return Err(e),
                }
            }
            Ok("echo")
        }
        CMD_UPLOAD => {
            let len = read_u64(&mut s)?;
            check_len(len)?;
            let mut buf = vec![0u8; CHUNK];
            let mut left = len;
            while left > 0 {
                let want = left.min(CHUNK as u64) as usize;
                let got = s.read(&mut buf[..want])?;
                if got == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "upload cut short",
                    ));
                }
                left -= got as u64;
            }
            s.write_all(&[ACK])?;
            Ok("upload")
        }
        CMD_DOWNLOAD => {
            let len = read_u64(&mut s)?;
            check_len(len)?;
            write_zeros(&mut s, len)?;
            s.flush()?;
            // Wait for the client to close so the last bytes aren't lost to an RST on Windows.
            let mut sink = [0u8; 1];
            let _ = s.read(&mut sink);
            Ok("download")
        }
        _ => Ok("unknown command"),
    }
}

fn check_len(len: u64) -> io::Result<()> {
    if len > MAX_TRANSFER {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "transfer too large",
        ));
    }
    Ok(())
}

fn read_u64(s: &mut TcpStream) -> io::Result<u64> {
    let mut b = [0u8; 8];
    s.read_exact(&mut b)?;
    Ok(u64::from_be_bytes(b))
}

fn write_zeros(s: &mut TcpStream, len: u64) -> io::Result<()> {
    // Zeros are fine: nothing on the path compresses (plain TCP on a LAN, no VPN).
    let buf = vec![0u8; CHUNK];
    let mut left = len;
    while left > 0 {
        let n = left.min(CHUNK as u64) as usize;
        s.write_all(&buf[..n])?;
        left -= n as u64;
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------- client

#[derive(Debug, Clone, Copy)]
pub struct ClientOptions {
    /// Echo round trips to time (one extra warm-up round trip is not counted).
    pub rtt_samples: u32,
    /// Bytes to push each direction. 0 skips the throughput test.
    pub bytes: u64,
    /// Connect and read timeout.
    pub timeout: Duration,
}

impl Default for ClientOptions {
    fn default() -> Self {
        ClientOptions {
            rtt_samples: 100,
            bytes: 64_000_000,
            timeout: Duration::from_secs(5),
        }
    }
}

/// One direction of a bulk transfer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transfer {
    pub bytes: u64,
    pub secs: f64,
}

impl Transfer {
    pub fn mbit_per_s(&self) -> f64 {
        mbit_per_s(self.bytes, self.secs)
    }
    pub fn mbyte_per_s(&self) -> f64 {
        mbyte_per_s(self.bytes, self.secs)
    }
}

/// Everything `client` measured against one peer.
#[derive(Debug, Clone)]
pub struct ClientReport {
    pub addr: SocketAddr,
    pub rtt: Option<Summary>,
    /// client -> server
    pub send: Option<Transfer>,
    /// server -> client
    pub recv: Option<Transfer>,
    /// The first error, if any step failed. Steps after a failed step are skipped.
    pub error: Option<(crate::probe::FailKind, String)>,
}

fn connect(addr: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    let s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    Ok(s)
}

/// Echo RTT: `samples` timed 8-byte round trips on one connection with TCP_NODELAY.
pub fn echo_rtt(addr: SocketAddr, samples: u32, timeout: Duration) -> io::Result<Vec<f64>> {
    let mut s = connect(addr, timeout)?;
    s.write_all(&[CMD_PING])?;
    let mut back = [0u8; 8];
    let mut out = Vec::with_capacity(samples as usize);
    for i in 0..=samples as u64 {
        let msg = i.to_be_bytes();
        let t0 = Instant::now();
        s.write_all(&msg)?;
        s.read_exact(&mut back)?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if back != msg {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "echo mismatch: peer is not netcheck",
            ));
        }
        if i > 0 {
            out.push(ms); // i == 0 is the warm-up
        }
    }
    let _ = s.shutdown(Shutdown::Write);
    Ok(out)
}

/// Client -> server bulk transfer of `bytes`.
pub fn upload(addr: SocketAddr, bytes: u64, timeout: Duration) -> io::Result<Transfer> {
    let mut s = connect(addr, timeout)?;
    let mut hdr = vec![CMD_UPLOAD];
    hdr.extend_from_slice(&bytes.to_be_bytes());
    s.write_all(&hdr)?;
    let t0 = Instant::now();
    write_zeros(&mut s, bytes)?;
    let mut ack = [0u8; 1];
    s.read_exact(&mut ack)?;
    let secs = t0.elapsed().as_secs_f64();
    if ack[0] != ACK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad upload ack: peer is not netcheck",
        ));
    }
    Ok(Transfer { bytes, secs })
}

/// Server -> client bulk transfer of `bytes`.
pub fn download(addr: SocketAddr, bytes: u64, timeout: Duration) -> io::Result<Transfer> {
    let mut s = connect(addr, timeout)?;
    let mut hdr = vec![CMD_DOWNLOAD];
    hdr.extend_from_slice(&bytes.to_be_bytes());
    let t0 = Instant::now();
    s.write_all(&hdr)?;
    let mut buf = vec![0u8; CHUNK];
    let mut got = 0u64;
    while got < bytes {
        let n = s.read(&mut buf)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "download cut short",
            ));
        }
        got += n as u64;
    }
    let secs = t0.elapsed().as_secs_f64();
    let _ = s.shutdown(Shutdown::Both);
    Ok(Transfer { bytes: got, secs })
}

/// Runs echo RTT, then upload, then download against a `serve` peer. Stops at the first failure.
pub fn run_client(addr: SocketAddr, opts: &ClientOptions) -> ClientReport {
    let mut r = ClientReport {
        addr,
        rtt: None,
        send: None,
        recv: None,
        error: None,
    };
    let fail = |e: io::Error| (crate::probe::classify(&e), e.to_string());
    match echo_rtt(addr, opts.rtt_samples.max(1), opts.timeout) {
        Ok(v) => r.rtt = summarize(&v),
        Err(e) => {
            r.error = Some(fail(e));
            return r;
        }
    }
    if opts.bytes == 0 {
        return r;
    }
    match upload(addr, opts.bytes, opts.timeout) {
        Ok(t) => r.send = Some(t),
        Err(e) => {
            r.error = Some(fail(e));
            return r;
        }
    }
    match download(addr, opts.bytes, opts.timeout) {
        Ok(t) => r.recv = Some(t),
        Err(e) => r.error = Some(fail(e)),
    }
    r
}
