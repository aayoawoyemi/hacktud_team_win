//! CONCEPT: tests for the llama.cpp output parsers (WP-2 R2.4) on the Dell samples from the PRD.
//! WHY: the agent (WP-3) builds NodeCapabilities from these parsers; a silent mis-parse would make the
//!      planner (WP-5) split the model on wrong memory numbers.
//! DEPENDS ON: tests/llama_samples/*.dell-8665.txt (byte copies of the PRD samples).
//! AFFECTS: nothing at runtime; guards src/parsers.rs.

use contracts::parsers::{
    ParseError, is_noise_line, parse_device_line, parse_list_devices, parse_rpc_server_banner,
    parse_version, vulkan_uma_flags,
};

const BANNER: &str = include_str!("llama_samples/rpc_server_banner.dell-8665.txt");
const LIST: &str = include_str!("llama_samples/list_devices.dell-8665.txt");
const VERSION: &str = include_str!("llama_samples/version.dell-8665.txt");

/// Backend-loading noise llama.cpp prints before real output (format per llama.cpp source; not a capture).
const NOISE: &str = "\
load_backend: loaded RPC backend from C:\\llama\\ggml-rpc.dll
ggml_vulkan: Found 2 Vulkan devices:
ggml_vulkan: 0 = Intel(R) UHD Graphics (Intel Corporation) | uma: 1 | fp16: 1 | bf16: 0 | warp size: 32 | shared memory: 32768 | int dot: 1 | matrix cores: none
ggml_vulkan: 1 = NVIDIA GeForce RTX 3050 Ti Laptop GPU (NVIDIA) | uma: 0 | fp16: 1 | bf16: 1 | warp size: 32 | shared memory: 49152 | int dot: 1 | matrix cores: NV_coopmat2
load_backend: loaded Vulkan backend from C:\\llama\\ggml-vulkan.dll
load_backend: loaded CPU backend from C:\\llama\\ggml-cpu-alderlake.dll
";

fn noisy(sample: &str) -> String {
    format!("{NOISE}{sample}")
}

fn crlf(sample: &str) -> String {
    sample.replace('\n', "\r\n")
}

// ---- --list-devices --------------------------------------------------------------------------

#[test]
fn list_devices_dell_sample() {
    let devices = parse_list_devices(LIST).unwrap();
    assert_eq!(devices.len(), 2);
    let d0 = &devices[0];
    assert_eq!(d0.name, "Vulkan0");
    assert_eq!(d0.description, "Intel(R) UHD Graphics");
    assert_eq!(d0.backend, "Vulkan");
    assert_eq!((d0.total_mib, d0.free_mib), (8051, 7393));
    assert_eq!(d0.unified_memory, None, "no ggml_vulkan lines -> unknown");
    let d1 = &devices[1];
    assert_eq!(d1.name, "Vulkan1");
    assert_eq!(d1.description, "NVIDIA GeForce RTX 3050 Ti Laptop GPU");
    assert_eq!((d1.total_mib, d1.free_mib), (3965, 3370));
}

#[test]
fn list_devices_skips_noise_and_reads_uma() {
    for text in [noisy(LIST), crlf(&noisy(LIST))] {
        let devices = parse_list_devices(&text).unwrap();
        assert_eq!(devices.len(), 2, "noise lines must not become devices");
        assert_eq!(
            devices[0].unified_memory,
            Some(true),
            "Intel iGPU prints uma: 1"
        );
        assert_eq!(devices[1].unified_memory, Some(false));
        assert_eq!(
            devices[1].description,
            "NVIDIA GeForce RTX 3050 Ti Laptop GPU"
        );
    }
}

#[test]
fn list_devices_cpu_only_is_empty_not_error() {
    assert_eq!(parse_list_devices("Available devices:\n").unwrap(), vec![]);
}

#[test]
fn list_devices_without_header_is_error() {
    assert_eq!(
        parse_list_devices(NOISE),
        Err(ParseError::Missing("Available devices:"))
    );
}

// ---- --version --------------------------------------------------------------------------------

#[test]
fn version_dell_sample() {
    for text in [VERSION.to_string(), noisy(VERSION), crlf(&noisy(VERSION))] {
        let v = parse_version(&text).unwrap();
        assert_eq!(v.build, 8665);
        assert_eq!(v.commit, "b8635075f");
        assert_eq!(
            v.built_with.as_deref(),
            Some("Clang 19.1.5 for Windows x86_64")
        );
    }
}

#[test]
fn version_missing_or_malformed() {
    assert_eq!(parse_version(NOISE), Err(ParseError::Missing("version:")));
    assert!(matches!(
        parse_version("version: abc (def)"),
        Err(ParseError::Malformed { .. })
    ));
}

// ---- rpc-server banner ------------------------------------------------------------------------

#[test]
fn rpc_server_banner_dell_sample() {
    for text in [BANNER.to_string(), noisy(BANNER), crlf(&noisy(BANNER))] {
        let parsed = parse_rpc_server_banner(&text).unwrap();
        let b = &parsed.banner;
        assert_eq!(b.rpc_protocol, "3.6.1");
        assert_eq!(
            b.endpoint, "127.0.0.1:50999",
            "port after the second ':' must survive"
        );
        assert_eq!(b.local_cache, "n/a");
        assert_eq!(b.devices, vec!["Vulkan0", "Vulkan1"]);
        assert_eq!(parsed.devices.len(), 2);
        assert_eq!(parsed.devices[1].free_mib, 3370);
    }
}

#[test]
fn rpc_server_banner_ignores_trailing_lines() {
    // Anything after the device list (e.g. a warning a later build prints) must not break parsing.
    let text =
        format!("{BANNER}\n!!! WARNING: never expose the RPC server to an open network !!!\n");
    let parsed = parse_rpc_server_banner(&text).unwrap();
    assert_eq!(parsed.banner.devices.len(), 2);
}

#[test]
fn rpc_server_banner_requires_start_and_endpoint() {
    assert_eq!(
        parse_rpc_server_banner(LIST),
        Err(ParseError::Missing("Starting RPC server v"))
    );
    assert_eq!(
        parse_rpc_server_banner("Starting RPC server v3.6.1\n"),
        Err(ParseError::Missing("endpoint"))
    );
}

// ---- building blocks --------------------------------------------------------------------------

#[test]
fn noise_detection() {
    assert!(is_noise_line("load_backend: loaded CPU backend from x"));
    assert!(is_noise_line("ggml_vulkan: Found 2 Vulkan devices:"));
    assert!(!is_noise_line(
        "  Vulkan0: Intel(R) UHD Graphics (8051 MiB, 7393 MiB free)"
    ));
}

#[test]
fn device_line_edge_cases() {
    assert!(parse_device_line("  endpoint       : 127.0.0.1:50999").is_none());
    assert!(parse_device_line("Available devices:").is_none());
    let mtl = parse_device_line("  MTL0: Apple M5 (12124 MiB, 12123 MiB free)").unwrap();
    assert_eq!(mtl.backend, "MTL");
    assert_eq!(
        mtl.unified_memory,
        Some(true),
        "Apple GPUs share system memory"
    );
}

#[test]
fn uma_flags() {
    let flags = vulkan_uma_flags(NOISE);
    assert_eq!(flags.get(&0), Some(&true));
    assert_eq!(flags.get(&1), Some(&false));
    assert_eq!(
        flags.len(),
        2,
        "'Found 2 Vulkan devices' is not a device line"
    );
}
