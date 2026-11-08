//! CONCEPT: dev binary for the netcheck library: `check`, `serve`, `client`.
//! WHY: WP-10 needs a runnable tool now (reachability answer, preflight before launch). O-02 puts every tool
//!      behind one shared CLI later; that hook lands in one PR after #18, and this file can then be deleted.
//! DEPENDS ON: the netcheck library. Hand-rolled argument parsing (no clap) so the shared CLI owns that choice.
//! AFFECTS: docs/network.md quotes these commands; keep flags in step.
//!
//! Exit codes: 0 = every target passed, 1 = something failed, 2 = bad arguments.

use netcheck::peer::{self, ClientOptions};
use netcheck::probe::{self, ProbeOptions};
use netcheck::result::{NetcheckResult, local_node_name, now_rfc3339};
use netcheck::target::{self, Target};
use netcheck::{DEFAULT_PORT, report};
use std::net::{TcpListener, UdpSocket};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
netcheck: reachability, RTT and throughput between machines (WP-10)

USAGE:
  netcheck check  [LABEL=]HOST:PORT ... [--targets-file FILE] [--samples N] [--timeout-ms MS] [--json] [--from NAME]
  netcheck serve  [--bind ADDR]                       (default 0.0.0.0:50060)
  netcheck client [LABEL=]HOST:PORT [--mb N] [--samples N] [--timeout-ms MS] [--json] [--from NAME]

check   TCP-connects to each target N times (default 20, timeout 3000 ms): status + connect RTT.
        Works against any listener: rpc-server 50052, llama-server 8080, SpacetimeDB 3000, netcheck 50060.
serve   Listens for `client` (echo RTT + throughput). Leave it running on the far machine.
client  Against a `serve` peer: echo RTT (default 100 x 8 bytes, TCP_NODELAY), then pushes N MB
        (default 64, 10^6 bytes) each way and reports MB/s. --mb 0 skips throughput.
--json  Prints the PRD 6.9 NetcheckResult JSON (an array for check) instead of the table.
";

struct Args {
    positional: Vec<String>,
    targets_file: Option<PathBuf>,
    samples: Option<u32>,
    timeout_ms: Option<u64>,
    mb: Option<u64>,
    bind: Option<String>,
    from: Option<String>,
    json: bool,
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let mut a = Args {
        positional: vec![],
        targets_file: None,
        samples: None,
        timeout_ms: None,
        mb: None,
        bind: None,
        from: None,
        json: false,
    };
    let mut it = raw.iter();
    while let Some(arg) = it.next() {
        let mut val = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        let num = |name: &str, v: String| {
            v.parse::<u64>()
                .map_err(|_| format!("{name}: `{v}` is not a number"))
        };
        match arg.as_str() {
            "--json" => a.json = true,
            "--targets-file" | "-f" => a.targets_file = Some(PathBuf::from(val(arg)?)),
            "--samples" | "-n" => a.samples = Some(num(arg, val(arg)?)? as u32),
            "--timeout-ms" => a.timeout_ms = Some(num(arg, val(arg)?)?),
            "--mb" => a.mb = Some(num(arg, val(arg)?)?),
            "--bind" => a.bind = Some(val(arg)?),
            "--from" => a.from = Some(val(arg)?),
            s if s.starts_with('-') => return Err(format!("unknown flag {s}")),
            s => a.positional.push(s.to_string()),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = raw.first().cloned() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    if matches!(cmd.as_str(), "-h" | "--help" | "help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let args = match parse_args(&raw[1..]) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let res = match cmd.as_str() {
        "check" => cmd_check(&args),
        "serve" => cmd_serve(&args),
        "client" => cmd_client(&args),
        other => Err(format!("unknown command `{other}`")),
    };
    match res {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn timeout(args: &Args, default_ms: u64) -> Duration {
    Duration::from_millis(args.timeout_ms.unwrap_or(default_ms))
}

fn cmd_check(args: &Args) -> Result<bool, String> {
    let mut targets: Vec<Target> = Vec::new();
    if let Some(f) = &args.targets_file {
        targets.extend(target::load_targets_file(f).map_err(|e| e.to_string())?);
    }
    for p in &args.positional {
        targets.push(target::parse_target_arg(p).map_err(|e| e.to_string())?);
    }
    if targets.is_empty() {
        return Err("check needs at least one target (args or --targets-file)".into());
    }
    let opts = ProbeOptions {
        samples: args.samples.unwrap_or(20),
        timeout: timeout(args, 3000),
        ..Default::default()
    };
    let from = args.from.clone().unwrap_or_else(local_node_name);
    let at = now_rfc3339();
    let reports = probe::connect_probe_all(&targets, &opts);
    if args.json {
        let out: Vec<NetcheckResult> = reports
            .iter()
            .map(|r| NetcheckResult::from_probe(&from, r, &at))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
        );
    } else {
        println!("netcheck check from {from} at {at}\n");
        print!("{}", report::check_table(&reports));
    }
    Ok(reports.iter().all(|r| r.ok()))
}

/// Best guess of this machine's LAN address: the source IP the OS would use for an outside address.
/// A UDP "connect" sends no packet. Can be wrong with several adapters (VPN, Hyper-V); the docs say to
/// confirm with ipconfig / System Settings.
fn guess_lan_ip() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?; // TEST-NET-1: never routed, just picks the default interface
    Some(s.local_addr().ok()?.ip().to_string())
}

fn cmd_serve(args: &Args) -> Result<bool, String> {
    let bind = args
        .bind
        .clone()
        .unwrap_or_else(|| format!("0.0.0.0:{DEFAULT_PORT}"));
    let listener = TcpListener::bind(&bind).map_err(|e| format!("can't listen on {bind}: {e}"))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    println!("netcheck serve listening on {local}");
    if let Some(ip) = guess_lan_ip() {
        println!("this machine's LAN IP is probably {ip}; from another machine run:");
        println!("  netcheck client {ip}:{}", local.port());
    }
    println!("Ctrl+C to stop.");
    peer::serve(listener, |p, what| println!("{p}: {what}")).map_err(|e| e.to_string())?;
    Ok(true)
}

fn cmd_client(args: &Args) -> Result<bool, String> {
    let [t] = args.positional.as_slice() else {
        return Err(
            "client needs exactly one target (the machine running `netcheck serve`)".into(),
        );
    };
    let t = target::parse_target_arg(t).map_err(|e| e.to_string())?;
    let sa = probe::resolve(&t.addr).map_err(|f| format!("{}: {}", f.kind.as_str(), f.message))?;
    let opts = ClientOptions {
        rtt_samples: args.samples.unwrap_or(100),
        bytes: args.mb.unwrap_or(64) * 1_000_000,
        timeout: timeout(args, 5000),
    };
    let from = args.from.clone().unwrap_or_else(local_node_name);
    let at = now_rfc3339();
    let r = peer::run_client(sa, &opts);
    if args.json {
        let out = NetcheckResult::from_client(&from, &t.label, &t.addr, &r, &at);
        println!(
            "{}",
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
        );
    } else {
        print!("{}", report::client_table(&t.label, &r, opts.rtt_samples));
    }
    Ok(r.error.is_none())
}
