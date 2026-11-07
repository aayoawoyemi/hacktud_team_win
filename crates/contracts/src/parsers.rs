//! CONCEPT: parsers for llama.cpp's human-readable output (WP-2 R2.4). They turn text the official binaries
//!          print into contract types, so the agent (WP-3) can fill NodeCapabilities and CommandResult.
//! WHY: O-01 = A: Rust runs the official llama.cpp binaries (build 8665, O-14) as child processes and has no
//!      API into them, so their stdout/stderr text is the only source for versions and devices.
//! DEPENDS ON: the output format of llama.cpp build 8665. Samples are in tests/llama_samples/ (captured on
//!             the Dell, PRD WP-2 "Parser samples"). A different build may change the format: that is one
//!             reason O-14 pins one build everywhere.
//! AFFECTS: WP-3 (probe, start_rpc_server result), WP-4 (version_mismatch check), WP-6.
//!
//! Design rules for every parser here:
//! - Input is the whole captured text (stdout and stderr together is fine). Line endings may be \n or \r\n.
//! - Lines starting with `load_backend:` or `ggml_vulkan:` are backend-loading noise that llama.cpp prints
//!   before the real output (R2.4); they are skipped before anything else looks at a line.
//!   Exception: `vulkan_uma_flags` reads the `ggml_vulkan:` lines on purpose, for `unified_memory`.
//! - Other unrecognised lines are ignored, not errors: future builds add lines (e.g. warnings).
//! - Missing *required* information is an error (`ParseError`), so the caller can report E_INTERNAL with
//!   the raw text instead of registering a node with made-up values.
//!
//! NOT HERE YET: the llama-server load-log parser (per-device buffers and layer counts, R2.4 last bullet).
//! Its format comes from WP-1 fixtures, which haven't landed.

use crate::v1::{DeviceInfo, RpcServerBanner};
use std::collections::BTreeMap;

/// Why a parser gave up. Carries enough to show a human what was missing.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("no `{0}` line found in llama.cpp output")]
    Missing(&'static str),
    #[error("malformed `{what}` line: {line:?}")]
    Malformed { what: &'static str, line: String },
}

/// True for the noise lines R2.4 says to skip.
pub fn is_noise_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("load_backend:") || t.starts_with("ggml_vulkan:")
}

/// The lines a parser should look at: noise removed, trailing `\r` and spaces removed.
fn content_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(|l| l.trim_end())
        .filter(|l| !is_noise_line(l))
}

// -------------------------------------------------------------------------------------------------
// Device lines (shared by --list-devices and the rpc-server banner)
// -------------------------------------------------------------------------------------------------

/// CONCEPT: parses one device line: `  Vulkan1: NVIDIA GeForce RTX 3050 Ti Laptop GPU (3965 MiB, 3370 MiB free)`.
/// Returns None for any line that isn't a device line, so callers can scan freely.
/// The description may itself contain parentheses ("Intel(R) UHD Graphics"), so the memory part is taken
/// from the LAST " (" in the line.
pub fn parse_device_line(line: &str) -> Option<DeviceInfo> {
    let line = line.trim();
    let (name, rest) = line.split_once(": ")?;
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let rest = rest.strip_suffix(" MiB free)")?;
    let open = rest.rfind(" (")?;
    let description = rest[..open].trim().to_string();
    let (total, free) = rest[open + 2..].split_once(" MiB, ")?;
    let total_mib = total.trim().parse().ok()?;
    let free_mib = free.trim().parse().ok()?;
    let backend = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .to_string();
    // Apple GPUs always share system memory (§6.1). Metal devices are named MTL<n> in llama.cpp.
    // VERIFY on a Mac fixture (WP-1). Other backends: unknown here; see vulkan_uma_flags.
    let unified_memory = if backend == "MTL" { Some(true) } else { None };
    Some(DeviceInfo {
        name: name.to_string(),
        description,
        backend,
        total_mib,
        free_mib,
        unified_memory,
    })
}

/// CONCEPT: reads `uma:` flags from `ggml_vulkan:` init lines, keyed by Vulkan device index, e.g.
///   `ggml_vulkan: 0 = Intel(R) UHD Graphics (Intel Corporation) | uma: 1 | fp16: 1 | ...`
/// WHY: §6.1 `unified_memory`: integrated GPUs use system RAM, so the planner must not count their memory
///      on top of the machine's RAM.
/// ASSUMPTION (VERIFY with a WP-1 fixture): the line format above, from llama.cpp's Vulkan backend source.
///      Lines that don't match are ignored, so a format change yields "unknown", never a wrong value.
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
            if let Some(v) = part.trim().strip_prefix("uma:") {
                match v.trim() {
                    "1" => {
                        out.insert(idx, true);
                    }
                    "0" => {
                        out.insert(idx, false);
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// Fills `unified_memory` on VulkanN devices from the `ggml_vulkan:` uma flags found in the same text.
fn apply_vulkan_uma(devices: &mut [DeviceInfo], text: &str) {
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

// -------------------------------------------------------------------------------------------------
// llama-server --list-devices
// -------------------------------------------------------------------------------------------------

/// CONCEPT: `llama-server --list-devices` output -> one DeviceInfo per device (NodeCapabilities.devices).
/// Format (build 8665):
/// ```text
/// Available devices:
///   Vulkan0: Intel(R) UHD Graphics (8051 MiB, 7393 MiB free)
///   Vulkan1: NVIDIA GeForce RTX 3050 Ti Laptop GPU (3965 MiB, 3370 MiB free)
/// ```
/// An "Available devices:" header with no device lines is valid (CPU-only machine) and returns an empty list.
pub fn parse_list_devices(text: &str) -> Result<Vec<DeviceInfo>, ParseError> {
    let mut lines = content_lines(text).skip_while(|l| l.trim() != "Available devices:");
    if lines.next().is_none() {
        return Err(ParseError::Missing("Available devices:"));
    }
    let mut devices: Vec<DeviceInfo> = lines.map_while(parse_device_line).collect();
    apply_vulkan_uma(&mut devices, text);
    Ok(devices)
}

// -------------------------------------------------------------------------------------------------
// --version
// -------------------------------------------------------------------------------------------------

/// What `llama-server --version` (or `rpc-server --version`) reports.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LlamaVersion {
    /// Build number, e.g. 8665. Compared across nodes for `version_mismatch` (O-14 pins 8665).
    pub build: u32,
    /// Short commit hash, e.g. "b8635075f".
    pub commit: String,
    /// The "built with ..." line without its prefix, e.g. "Clang 19.1.5 for Windows x86_64". Informational.
    pub built_with: Option<String>,
}

/// CONCEPT: `--version` output -> build number and commit (LlamaCppInfo.build / .commit).
/// Format (build 8665):
/// ```text
/// version: 8665 (b8635075f)
/// built with Clang 19.1.5 for Windows x86_64
/// ```
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
            let rest = rest.trim();
            let (build, commit) = rest.split_once(' ').ok_or_else(malformed)?;
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

// -------------------------------------------------------------------------------------------------
// rpc-server start banner
// -------------------------------------------------------------------------------------------------

/// The parsed rpc-server banner: the contract's RpcServerBanner (device names only, as §6.3 shows) plus the
/// full device lines, which the agent can use for NodeCapabilities.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRpcBanner {
    pub banner: RpcServerBanner,
    pub devices: Vec<DeviceInfo>,
}

/// CONCEPT: the rpc-server start banner -> protocol version, endpoint, cache and devices.
/// Format (build 8665):
/// ```text
/// Starting RPC server v3.6.1
///   endpoint       : 127.0.0.1:50999
///   local cache    : n/a
/// Devices:
///   Vulkan0: Intel(R) UHD Graphics (8051 MiB, 7393 MiB free)
///   Vulkan1: NVIDIA GeForce RTX 3050 Ti Laptop GPU (3965 MiB, 3370 MiB free)
/// ```
/// WHY: the agent reports this as StartRpcServerResult.banner; `rpc_protocol` must match across nodes
///      (E_VERSION_MISMATCH), and `devices` is what the helper really exposes (NodeCapabilities.rpc_server_devices).
/// Requires the "Starting RPC server v" and "endpoint" lines; "local cache" and devices may be absent.
pub fn parse_rpc_server_banner(text: &str) -> Result<ParsedRpcBanner, ParseError> {
    let mut banner = RpcServerBanner::default();
    let mut devices = Vec::new();
    let mut seen_start = false;
    let mut seen_endpoint = false;
    let mut in_devices = false;
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
                Some(d) => devices.push(d),
                None => in_devices = false,
            }
        } else if let Some((key, value)) = t.split_once(':') {
            // "endpoint       : 127.0.0.1:50999": split at the FIRST ':' so the port survives.
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
    apply_vulkan_uma(&mut devices, text);
    banner.devices = devices.iter().map(|d| d.name.clone()).collect();
    Ok(ParsedRpcBanner { banner, devices })
}
