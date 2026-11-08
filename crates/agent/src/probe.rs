//! CONCEPT: the node probe (R3.1): host facts from the OS plus llama.cpp facts from running the pinned
//!          binaries, returned as one plain struct. Shaped like §6.1 NodeCapabilities so the D-13 control
//!          plane wiring can map it field by field, but owned here until WP-2's types settle (PR #18).
//! WHY: the coordinator (WP-4) and planner (WP-5) need RAM, devices and the llama.cpp build of every node to
//!      plan a split and to flag `version_mismatch`. O-01 = A gives no API into llama.cpp, so device and
//!      version facts come from its CLI output.
//! DEPENDS ON: sysinfo ("system" feature), config.rs (binary paths), parse.rs, the binaries on PATH or in
//!      `llama_bin_dir`:
//!        - `llama-server --version`       -> build, commit (stdout or stderr; both are read)
//!        - `llama-server --list-devices`  -> devices with total/free MiB
//!        - `rpc-server --help`            -> its `load_backend:` + `ggml_vulkan:` stderr lines: the backends
//!          rpc-server loads and the Vulkan `uma:` flags. rpc-server 8665 has NO --list-devices or --version
//!          (verified on the Dell: "error: unknown argument: --version"); --help initialises the backends
//!          and exits 0, which is the cheapest way to see what it will expose.
//! AFFECTS: examples/probe.rs (R3.2 human printout), control.rs (register_node payload, later).
//!
//! Failure policy: the probe never fails as a whole. A missing or broken binary leaves `llama_cpp` None and
//! records why in `errors`, so a node with no llama.cpp still reports its RAM (useful while setting up).
//! Not probed yet (R3.1 lists them; later work): free disk at the cache folder, network addresses, power,
//! `rpc_server_devices` (comes from the running server's banner, see supervisor.rs), speed (O-22).

use crate::config::AgentConfig;
use crate::parse::{self, DeviceInfo};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

/// Each llama.cpp CLI call gets this long. Vulkan init on the Dell takes ~1 s; 20 s covers a cold disk.
pub const BINARY_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NodeProbe {
    pub hostname: String,
    pub agent_version: String,
    pub os: OsInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    /// None when the llama.cpp binaries could not be run or parsed; see `errors`.
    pub llama_cpp: Option<LlamaCppInfo>,
    /// One per `llama-server --list-devices` line. Empty = CPU only, or unknown if `errors` says so.
    pub devices: Vec<DeviceInfo>,
    /// Human-readable reasons for any part that could not be probed.
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OsInfo {
    /// `std::env::consts::OS`: "windows", "macos", "linux".
    pub family: String,
    /// e.g. "11 (26100)" on Windows, "15.5" on macOS. None if the OS doesn't say.
    pub version: Option<String>,
    /// `std::env::consts::ARCH`: "x86_64", "aarch64".
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CpuInfo {
    pub model: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LlamaCppInfo {
    /// Where the binaries were found: the configured dir, or None when resolved on PATH.
    pub bin_dir: Option<PathBuf>,
    pub build: u32,
    pub commit: String,
    pub built_with: Option<String>,
    /// Backends rpc-server loads, e.g. ["RPC", "Vulkan", "CPU"]. Empty = unknown (static build).
    pub backends: Vec<String>,
}

/// Host facts only (no child processes). Cheap; safe to call for every heartbeat.
pub fn probe_host() -> (String, OsInfo, CpuInfo, MemoryInfo) {
    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    let hostname = System::host_name().unwrap_or_else(|| "unknown".to_string());
    let os = OsInfo {
        family: std::env::consts::OS.to_string(),
        version: System::os_version(),
        arch: std::env::consts::ARCH.to_string(),
    };
    let cpu = CpuInfo {
        model: sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_default(),
        physical_cores: System::physical_core_count(),
        logical_cores: sys.cpus().len(),
    };
    let memory = MemoryInfo {
        total_bytes: sys.total_memory(),
        available_bytes: sys.available_memory(),
    };
    (hostname, os, cpu, memory)
}

/// stdout + stderr of `bin args`, concatenated (stderr first: that's where the init noise goes, and the
/// parsers skip it). Err(reason) if it can't start or times out. Exit code is ignored on purpose: the
/// parsers decide whether the output is usable.
pub async fn run_capture(bin: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let child = tokio::process::Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("{} {}: {e}", bin.display(), args.join(" ")))?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => Ok(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        )),
        Ok(Err(e)) => Err(format!("{} {}: {e}", bin.display(), args.join(" "))),
        Err(_) => Err(format!(
            "{} {}: timed out after {timeout:?}",
            bin.display(),
            args.join(" ")
        )),
    }
}

/// Full probe: host + llama.cpp. The three binary calls run concurrently.
pub async fn probe(cfg: &AgentConfig) -> NodeProbe {
    let (hostname, os, cpu, memory) = probe_host();
    let llama_server = cfg.llama_server_binary();
    let rpc_server = cfg.rpc_server_binary();
    let (version, list, rpc_help) = tokio::join!(
        run_capture(&llama_server, &["--version"], BINARY_TIMEOUT),
        run_capture(&llama_server, &["--list-devices"], BINARY_TIMEOUT),
        run_capture(&rpc_server, &["--help"], BINARY_TIMEOUT),
    );

    let mut errors = Vec::new();
    let rpc_text = rpc_help.unwrap_or_else(|e| {
        errors.push(e);
        String::new()
    });

    let llama_cpp = match version.map(|t| parse::parse_version(&t).map_err(|e| e.to_string())) {
        Ok(Ok(v)) => Some(LlamaCppInfo {
            bin_dir: cfg.llama_bin_dir.clone(),
            build: v.build,
            commit: v.commit,
            built_with: v.built_with,
            backends: parse::parse_loaded_backends(&rpc_text),
        }),
        Ok(Err(e)) | Err(e) => {
            errors.push(format!("llama-server --version: {e}"));
            None
        }
    };

    let devices = match list {
        Ok(text) => match parse::parse_list_devices(&text) {
            Ok(mut d) => {
                // llama-server --list-devices prints no `ggml_vulkan:` lines on 8665; rpc-server --help does.
                parse::apply_vulkan_uma(&mut d, &rpc_text);
                d
            }
            Err(e) => {
                errors.push(format!("llama-server --list-devices: {e}"));
                Vec::new()
            }
        },
        Err(e) => {
            errors.push(e);
            Vec::new()
        }
    };

    NodeProbe {
        hostname,
        agent_version: crate::AGENT_VERSION.to_string(),
        os,
        cpu,
        memory,
        llama_cpp,
        devices,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_probe_has_ram_and_cpu() {
        let (host, os, cpu, mem) = probe_host();
        assert!(!host.is_empty());
        assert_eq!(os.family, std::env::consts::OS);
        assert!(cpu.logical_cores >= 1);
        assert!(mem.total_bytes > 0);
        assert!(mem.available_bytes <= mem.total_bytes);
    }

    #[tokio::test]
    async fn missing_binaries_degrade_not_fail() {
        let cfg = AgentConfig {
            llama_bin_dir: Some(PathBuf::from("definitely/not/a/dir")),
            ..AgentConfig::default()
        };
        let p = probe(&cfg).await;
        assert!(p.llama_cpp.is_none());
        assert!(p.devices.is_empty());
        assert_eq!(p.errors.len(), 3, "{:?}", p.errors);
        assert!(p.memory.total_bytes > 0);
    }
}
