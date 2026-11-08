//! CONCEPT: a target is one labelled `host:port` to check ("mac-a rpc-server" -> 192.168.8.11:50052).
//!          Targets come from CLI args (`label=host:port` or bare `host:port`) or a TOML file.
//! WHY: WP-10 R10.2/R10.3. The preflight before launch checks every port the run needs on every machine
//!      (rpc-server 50052, llama-server 8080, SpacetimeDB 3000, netcheck 50060); a file lets the team write
//!      that list once per network and rerun it.
//! DEPENDS ON: toml + serde (file format below).
//! AFFECTS: probe.rs and the dev binary. The PRD §6.12 netcheck config key `targets` (O-24 still open) should
//!          reuse this `[[target]]` shape if the config mechanism ends up TOML.
//!
//! File format (example: `crates/netcheck/targets.example.toml`):
//! ```toml
//! [[target]]
//! label = "mac-a rpc-server"
//! addr  = "192.168.8.11:50052"
//! ```

use serde::Deserialize;
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Target {
    /// Human name shown in the table and written as `to_node` in the JSON.
    pub label: String,
    /// `host:port`. Host may be an IPv4 address, `[IPv6]`, or a name (`mac-a.local`).
    pub addr: String,
}

#[derive(Debug, Deserialize)]
struct TargetsFile {
    #[serde(default)]
    target: Vec<Target>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetError(pub String);

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TargetError {}

/// Checks that `addr` ends in `:port` with a port in 1..=65535. Doesn't resolve the host (probe.rs does).
pub fn validate_addr(addr: &str) -> Result<(), TargetError> {
    let (host, port) = addr
        .rsplit_once(':')
        .ok_or_else(|| TargetError(format!("`{addr}` has no `:port`")))?;
    if host.is_empty() {
        return Err(TargetError(format!("`{addr}` has no host")));
    }
    match port.parse::<u16>() {
        Ok(p) if p > 0 => Ok(()),
        _ => Err(TargetError(format!(
            "`{addr}`: `{port}` is not a port (1-65535)"
        ))),
    }
}

/// Parses one CLI target: `label=host:port`, or `host:port` (the label is then the address).
pub fn parse_target_arg(s: &str) -> Result<Target, TargetError> {
    let (label, addr) = match s.split_once('=') {
        Some((l, a)) => (l.trim().to_string(), a.trim().to_string()),
        None => (s.trim().to_string(), s.trim().to_string()),
    };
    validate_addr(&addr)?;
    if label.is_empty() {
        return Err(TargetError(format!("`{s}` has an empty label")));
    }
    Ok(Target { label, addr })
}

/// Parses the TOML text of a targets file.
pub fn parse_targets_toml(text: &str) -> Result<Vec<Target>, TargetError> {
    let file: TargetsFile =
        toml::from_str(text).map_err(|e| TargetError(format!("targets file: {e}")))?;
    for t in &file.target {
        validate_addr(&t.addr).map_err(|e| TargetError(format!("target `{}`: {e}", t.label)))?;
    }
    Ok(file.target)
}

/// Reads and parses a targets file from disk.
pub fn load_targets_file(path: &Path) -> Result<Vec<Target>, TargetError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| TargetError(format!("can't read {}: {e}", path.display())))?;
    parse_targets_toml(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labelled_and_bare_args() {
        let t = parse_target_arg("mac-a rpc=192.168.8.11:50052").unwrap();
        assert_eq!(t.label, "mac-a rpc");
        assert_eq!(t.addr, "192.168.8.11:50052");
        let t = parse_target_arg("127.0.0.1:8080").unwrap();
        assert_eq!(t.label, "127.0.0.1:8080");
    }

    #[test]
    fn ipv6_and_names_ok() {
        assert!(parse_target_arg("[::1]:50052").is_ok());
        assert!(parse_target_arg("mac-a.local:50052").is_ok());
    }

    #[test]
    fn bad_addrs_rejected() {
        assert!(parse_target_arg("192.168.8.11").is_err());
        assert!(parse_target_arg("x=192.168.8.11:0").is_err());
        assert!(parse_target_arg("x=192.168.8.11:99999").is_err());
        assert!(parse_target_arg("x=:50052").is_err());
        assert!(parse_target_arg("=1.2.3.4:5").is_err());
    }

    #[test]
    fn toml_file() {
        let text = r#"
            [[target]]
            label = "mac-a rpc-server"
            addr = "192.168.8.11:50052"

            [[target]]
            label = "main spacetimedb"
            addr = "192.168.8.10:3000"
        "#;
        let ts = parse_targets_toml(text).unwrap();
        assert_eq!(ts.len(), 2);
        assert_eq!(ts[1].addr, "192.168.8.10:3000");
        assert!(parse_targets_toml("[[target]]\nlabel='x'\naddr='nope'\n").is_err());
        assert!(parse_targets_toml("").unwrap().is_empty());
    }

    #[test]
    fn example_file_parses() {
        let text = include_str!("../targets.example.toml");
        assert!(!parse_targets_toml(text).unwrap().is_empty());
    }
}
