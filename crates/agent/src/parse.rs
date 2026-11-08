//! CONCEPT: thin local parsers for llama.cpp's human-readable output: `--version`, `--list-devices`, the
//!          rpc-server start banner, and the `load_backend:` lines that name the loaded backends.
//! WHY: the agent needs these now for the probe (R3.1) and the supervisor (banner), but WP-2's
//!      `contracts::parsers` is being reworked in PR #18 and `crates/contracts` is not ours to edit.
//!      This module mirrors the #18 function names and shapes so the switch is a search-and-replace.
//! TODO(WP-2): once PR #18 merges, delete this module and use `contracts::parsers` (+ its DeviceInfo,
//!      RpcServerBanner types). Keep `parse_loaded_backends` only if contracts doesn't grow an equivalent.
//! DEPENDS ON: llama.cpp build 8665 output format (D-04, O-14). Samples: tests/samples/*.dell-8665.txt,
//!      copied byte for byte from #18's crates/contracts/tests/llama_samples/.
//! AFFECTS: probe.rs, supervisor.rs.
//!
//! Noise lines (`load_backend:`, `ggml_vulkan:`) are skipped by the line-oriented parsers but read by
//! `vulkan_uma_flags` and `parse_loaded_backends`, so callers can pass stdout + stderr concatenated.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("no `{0}` line found in llama.cpp output")]
    Missing(&'static str),
    #[error("malformed `{what}` line: {line:?}")]
    Malformed { what: &'static str, line: String },
}

/// One llama.cpp device line (§6.1 `devices[]`). MiB as printed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// e.g. "Vulkan1", "MTL0".
    pub name: String,
    /// e.g. "NVIDIA GeForce RTX 3050 Ti Laptop GPU".
    pub description: String,
    /// name without trailing digits: "Vulkan", "MTL", "CUDA".
    pub backend: String,
    pub total_mib: u64,
    pub free_mib: u64,
    /// true = uses system RAM. None = unknown (not every output carries the `uma:` flag).
    pub unified_memory: Option<bool>,
}

/// `--version` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlamaVersion {
    pub build: u32,
    pub commit: String,
    pub built_with: Option<String>,
}

/// The rpc-server start banner.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcBanner {
    /// "3.6.1": must match across nodes (§6.1 `rpc_protocol`).
    pub rpc_protocol: String,
    pub endpoint: String,
    pub local_cache: String,
    pub devices: Vec<DeviceInfo>,
}

/// True for llama.cpp's backend-init noise lines.
pub fn is_noise_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("load_backend:") || t.starts_with("ggml_vulkan:")
}

fn content_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !is_noise_line(l))
}

/// `  Vulkan1: NVIDIA GeForce RTX 3050 Ti Laptop GPU (3965 MiB, 3370 MiB free)` -> DeviceInfo.
/// None for anything else. The description can hold parentheses ("Intel(R)"), so the memory part starts at
/// the LAST " (".
pub fn parse_device_line(line: &str) -> Option<DeviceInfo> {
    let line = line.trim();
    let (name, rest) = line.split_once(": ")?;
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let rest = rest.strip_suffix(" MiB free)")?;
    let open = rest.rfind(" (")?;
    let (total, free) = rest[open + 2..].split_once(" MiB, ")?;
    let backend = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .to_string();
    // Apple GPUs always share system memory (§6.1). VERIFY the "MTL" name on a Mac fixture (WP-1).
    let unified_memory = (backend == "MTL").then_some(true);
    Some(DeviceInfo {
        name: name.to_string(),
        description: rest[..open].trim().to_string(),
        backend,
        total_mib: total.trim().parse().ok()?,
        free_mib: free.trim().parse().ok()?,
        unified_memory,
    })
}

/// `ggml_vulkan: 0 = Intel(R) UHD Graphics (Intel Corporation) | uma: 1 | ...` -> {0: true}.
pub fn vulkan_uma_flags(text: &str) -> BTreeMap<u32, bool> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("ggml_vulkan:") else {
            continue;
        };
        let Some((idx, tail)) = rest.trim().split_once(" = ") else {
            continue;
        };
        let Ok(idx) = idx.trim().parse::<u32>() else {
            continue;
        };
        for part in tail.split('|') {
            match part.trim().strip_prefix("uma:").map(str::trim) {
                Some("1") => {
                    out.insert(idx, true);
                }
                Some("0") => {
                    out.insert(idx, false);
                }
                _ => {}
            }
        }
    }
    out
}

/// Fills `unified_memory` on `VulkanN` devices from uma flags found in `text`.
pub fn apply_vulkan_uma(devices: &mut [DeviceInfo], text: &str) {
    let uma = vulkan_uma_flags(text);
    for d in devices.iter_mut() {
        let idx = d
            .name
            .strip_prefix("Vulkan")
            .and_then(|n| n.parse::<u32>().ok());
        if let Some(flag) = idx.and_then(|i| uma.get(&i)) {
            d.unified_memory = Some(*flag);
        }
    }
}

/// `llama-server --list-devices` -> devices. A header with no device lines is a CPU-only machine: Ok(empty).
pub fn parse_list_devices(text: &str) -> Result<Vec<DeviceInfo>, ParseError> {
    let mut lines = content_lines(text).skip_while(|l| l.trim() != "Available devices:");
    if lines.next().is_none() {
        return Err(ParseError::Missing("Available devices:"));
    }
    let mut devices: Vec<DeviceInfo> = lines.map_while(parse_device_line).collect();
    apply_vulkan_uma(&mut devices, text);
    Ok(devices)
}

/// `version: 8665 (b8635075f)` + `built with ...` -> LlamaVersion.
pub fn parse_version(text: &str) -> Result<LlamaVersion, ParseError> {
    let mut version = None;
    let mut built_with = None;
    for line in content_lines(text) {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("version:") {
            let malformed = || ParseError::Malformed {
                what: "version:",
                line: t.to_string(),
            };
            let (build, commit) = rest.trim().split_once(' ').ok_or_else(malformed)?;
            let build = build.parse::<u32>().map_err(|_| malformed())?;
            let commit = commit
                .trim()
                .strip_prefix('(')
                .and_then(|c| c.strip_suffix(')'))
                .ok_or_else(malformed)?
                .to_string();
            version = Some((build, commit));
        } else if let Some(rest) = t.strip_prefix("built with ") {
            built_with = Some(rest.trim().to_string());
        }
    }
    let (build, commit) = version.ok_or(ParseError::Missing("version:"))?;
    Ok(LlamaVersion {
        build,
        commit,
        built_with,
    })
}

/// The rpc-server start banner. Needs the "Starting RPC server v" and "endpoint" lines; cache and devices
/// may be absent. The endpoint key is split at the FIRST ':' so "127.0.0.1:50999" survives.
pub fn parse_rpc_server_banner(text: &str) -> Result<RpcBanner, ParseError> {
    let mut banner = RpcBanner::default();
    let (mut seen_start, mut seen_endpoint, mut in_devices) = (false, false, false);
    for line in content_lines(text) {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("Starting RPC server v") {
            banner.rpc_protocol = v.trim().to_string();
            seen_start = true;
            in_devices = false;
        } else if t == "Devices:" {
            in_devices = true;
        } else if in_devices {
            match parse_device_line(t) {
                Some(d) => banner.devices.push(d),
                None => in_devices = false,
            }
        } else if let Some((key, value)) = t.split_once(':') {
            match key.trim() {
                "endpoint" => {
                    banner.endpoint = value.trim().to_string();
                    seen_endpoint = true;
                }
                "local cache" => banner.local_cache = value.trim().to_string(),
                _ => {}
            }
        }
    }
    if !seen_start {
        return Err(ParseError::Missing("Starting RPC server v"));
    }
    if !seen_endpoint {
        return Err(ParseError::Missing("endpoint"));
    }
    apply_vulkan_uma(&mut banner.devices, text);
    Ok(banner)
}

/// CONCEPT: `load_backend: loaded Vulkan backend from C:\...\ggml-vulkan.dll` -> ["Vulkan", ...], in order.
/// WHY: §6.1 `llama_cpp.backends` ("CPU", "Vulkan", "RPC"). `--version` doesn't list them; every llama.cpp
///      binary prints these lines to stderr at startup (seen on the Dell, build 8665, dynamic-backend build).
/// ASSUMPTION: a statically linked build (some Mac packages) prints no `load_backend:` lines -> empty list,
///      meaning "unknown", not "none". VERIFY on a Mac.
pub fn parse_loaded_backends(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("load_backend: loaded ") else {
            continue;
        };
        let Some((name, _)) = rest.split_once(" backend") else {
            continue;
        };
        let name = name.trim().to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = include_str!("../tests/samples/list_devices.dell-8665.txt");
    const BANNER: &str = include_str!("../tests/samples/rpc_server_banner.dell-8665.txt");
    const VERSION: &str = include_str!("../tests/samples/version.dell-8665.txt");
    /// Real stderr noise captured from `rpc-server --help` on the Dell (build 8665), 2026-11-07.
    const NOISE: &str = "load_backend: loaded RPC backend from C:\\x\\ggml-rpc.dll\n\
ggml_vulkan: Found 2 Vulkan devices:\n\
ggml_vulkan: 0 = Intel(R) UHD Graphics (Intel Corporation) | uma: 1 | fp16: 1 | bf16: 0 | warp size: 32 | shared memory: 32768 | int dot: 1 | matrix cores: none\n\
ggml_vulkan: 1 = NVIDIA GeForce RTX 3050 Ti Laptop GPU (NVIDIA) | uma: 0 | fp16: 1 | bf16: 1 | warp size: 32 | shared memory: 49152 | int dot: 1 | matrix cores: NV_coopmat2\n\
load_backend: loaded Vulkan backend from C:\\x\\ggml-vulkan.dll\n\
load_backend: loaded CPU backend from C:\\x\\ggml-cpu-icelake.dll\n";

    #[test]
    fn list_devices_sample() {
        let d = parse_list_devices(LIST).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].name, "Vulkan0");
        assert_eq!(d[0].description, "Intel(R) UHD Graphics");
        assert_eq!(d[0].backend, "Vulkan");
        assert_eq!((d[0].total_mib, d[0].free_mib), (8051, 7393));
        assert_eq!(d[0].unified_memory, None, "no uma lines in this sample");
        assert_eq!(d[1].description, "NVIDIA GeForce RTX 3050 Ti Laptop GPU");
        assert_eq!((d[1].total_mib, d[1].free_mib), (3965, 3370));
    }

    #[test]
    fn list_devices_with_noise_fills_uma() {
        let text = format!("{NOISE}{LIST}");
        let d = parse_list_devices(&text).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].unified_memory, Some(true));
        assert_eq!(d[1].unified_memory, Some(false));
    }

    #[test]
    fn list_devices_cpu_only_and_missing() {
        assert_eq!(parse_list_devices("Available devices:\n").unwrap(), vec![]);
        assert_eq!(
            parse_list_devices("garbage"),
            Err(ParseError::Missing("Available devices:"))
        );
    }

    #[test]
    fn version_sample() {
        let v = parse_version(&format!("{NOISE}{VERSION}")).unwrap();
        assert_eq!(v.build, 8665);
        assert_eq!(v.commit, "b8635075f");
        assert_eq!(
            v.built_with.as_deref(),
            Some("Clang 19.1.5 for Windows x86_64")
        );
        assert!(matches!(
            parse_version("version: x (y)"),
            Err(ParseError::Malformed { .. })
        ));
        assert_eq!(parse_version(""), Err(ParseError::Missing("version:")));
    }

    #[test]
    fn banner_sample() {
        let b = parse_rpc_server_banner(&format!("{NOISE}{BANNER}")).unwrap();
        assert_eq!(b.rpc_protocol, "3.6.1");
        assert_eq!(b.endpoint, "127.0.0.1:50999");
        assert_eq!(b.local_cache, "n/a");
        let names: Vec<_> = b.devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Vulkan0", "Vulkan1"]);
        assert_eq!(b.devices[0].unified_memory, Some(true));
    }

    #[test]
    fn banner_partial_is_an_error_until_endpoint_arrives() {
        // The supervisor re-parses as lines stream in; a half banner must not count as parsed.
        assert_eq!(
            parse_rpc_server_banner("Starting RPC server v3.6.1\n"),
            Err(ParseError::Missing("endpoint"))
        );
    }

    #[test]
    fn device_line_rejects_non_devices() {
        assert!(parse_device_line("Available devices:").is_none());
        assert!(parse_device_line("  endpoint       : 127.0.0.1:50999").is_none());
        let mtl = parse_device_line("  MTL0: Apple M5 (24576 MiB, 24000 MiB free)").unwrap();
        assert_eq!(mtl.backend, "MTL");
        assert_eq!(mtl.unified_memory, Some(true));
    }

    #[test]
    fn loaded_backends() {
        assert_eq!(parse_loaded_backends(NOISE), ["RPC", "Vulkan", "CPU"]);
        assert!(parse_loaded_backends(LIST).is_empty());
    }
}
