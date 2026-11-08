//! CONCEPT: tests for the SpacetimeDB client side of the contracts crate: the §6 -> reducer-argument glue, and
//!          (ignored by default) a live round trip against a local `spacetime start` server.
//! WHY: the module (crates/control) and the §6 types are separate type systems joined by `control_glue`; a field
//!      dropped there would silently leave a column empty. The live test proves the committed bindings, the
//!      SDK and the published module agree (D-13 go/no-go).
//! DEPENDS ON: fixtures/json/node_capabilities.json, heartbeat.json; for the live test a server on
//!      127.0.0.1:3000 with the module published as database `pool-smoke`
//!      (`spacetime publish -s local -p crates/control pool-smoke -y`).
//! AFFECTS: nothing at runtime.

use contracts::control::{DbConnection, NodeTableAccess, register_node_reducer::register_node};
use contracts::control_glue::{heartbeat_info, node_info};
use contracts::{Heartbeat, NodeCapabilities};
use spacetimedb_sdk::{DbContext, Table};
use std::path::PathBuf;

fn fixture<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/json")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn node_info_copies_the_queryable_fields() {
    let caps: NodeCapabilities = fixture("node_capabilities.json");
    let info = node_info(&caps, "helper");
    assert_eq!(info.node_id, "example-dell");
    assert_eq!(info.role, "helper");
    assert_eq!(
        (info.os_family.as_str(), info.arch.as_str()),
        ("windows", "x86_64")
    );
    assert_eq!(info.logical_cores, Some(16));
    assert_eq!(info.total_memory_bytes, 16_857_000_000);
    assert_eq!(info.devices.len(), 2);
    assert_eq!(info.devices[1].name, "Vulkan1");
    assert_eq!(info.devices[1].free_mib, 3370);
    assert_eq!(info.devices[0].unified_memory, Some(true));
    assert_eq!(info.llama_build, Some(8665));
    assert_eq!(info.rpc_protocol.as_deref(), Some("3.6.1"));
    assert_eq!(info.rpc_server_devices, vec!["Vulkan0", "Vulkan1"]);
    assert_eq!(info.addresses, vec!["192.0.2.10"]);
}

#[test]
fn heartbeat_info_uses_section_6_spellings() {
    let hb: Heartbeat = fixture("heartbeat.json");
    let info = heartbeat_info(&hb);
    assert_eq!(info.seq, 42);
    assert_eq!(info.rpc_state, "running");
    assert_eq!(info.rpc_port, Some(50052));
    assert!(info.port_accepting);
    assert_eq!(info.cpu_util_pct, Some(63.5));
    assert_eq!(info.relay_bytes_out, Some(2_097_152));
    assert_eq!(info.battery_pct, Some(88));
    assert_eq!(info.last_error_code, None);

    let empty = heartbeat_info(&Heartbeat::default());
    assert_eq!(
        empty.rpc_state, "not_started",
        "no rpc_server block -> not_started"
    );
}

/// Live: connect with the SDK, call register_node, and see the row arrive through a subscription.
/// Run: `cargo test -p contracts --test control -- --ignored` with the server and module from the header.
#[test]
#[ignore = "needs a local spacetime server with crates/control published as pool-smoke"]
fn live_register_node_round_trip() {
    let (tx, rx) = std::sync::mpsc::channel();
    let conn = DbConnection::builder()
        .with_uri("http://127.0.0.1:3000")
        .with_database_name("pool-smoke")
        .on_connect(|conn, _identity, _token| {
            conn.subscription_builder()
                .subscribe(["SELECT * FROM node"]);
        })
        .build()
        .expect("connect to local spacetime");
    conn.db.node().on_insert(move |_ctx, row| {
        let _ = tx.send(row.clone());
    });
    let _handle = conn.run_threaded();

    let mut caps: NodeCapabilities = fixture("node_capabilities.json");
    caps.node_id = format!("sdk-test-{}", std::process::id());
    conn.reducers
        .register_node(
            node_info(&caps, "helper"),
            serde_json::to_string(&caps).unwrap(),
        )
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let row = rx.recv_timeout(left).expect("node row via subscription");
        if row.node_id == caps.node_id {
            assert_eq!(row.state, "REGISTERED");
            assert!(row.online);
            assert_eq!(row.llama_build, Some(8665));
            assert_eq!(row.devices.len(), 2);
            break;
        }
    }
    assert!(conn.db.node().count() >= 1);
    conn.disconnect().ok();
}
