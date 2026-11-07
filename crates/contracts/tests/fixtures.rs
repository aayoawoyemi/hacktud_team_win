//! CONCEPT: round-trip tests for every JSON fixture in fixtures/json/ (WP-2 R2.2).
//! WHY: the fixtures are the PRD §6 examples. If a contract change renames a field, changes an enum
//!      spelling or loses a value, these tests fail before another work package builds on the change.
//!      They also let WP-3 and WP-4 build mocks of each other from the same files (PRD §5.8).
//! DEPENDS ON: fixtures/json/*.json; the generated types and src/json.rs.
//! AFFECTS: nothing at runtime.
//!
//! Each fixture goes through:
//!   1. JSON -> Rust type (serde).
//!   2. Rust type -> JSON. Every key and value in the fixture must appear unchanged in the output.
//!      ("Subset", not "equal": the output may add keys the fixture omits, written as their defaults.)
//!   3. JSON output -> Rust type again: must equal step 1.
//!   4. Rust type -> protobuf bytes -> Rust type: must equal step 1 (the gRPC path loses nothing either).

use contracts::*;
use prost::Message;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::fmt::Debug;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/json")
        .join(name)
}

/// Asserts every key/value in `expected` appears in `actual` (recursing into objects; arrays compared
/// element by element). Numbers compare by value so 41.0 == 41.
fn assert_subset(expected: &Value, actual: &Value, path: &str) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (k, ev) in e {
                let av = a
                    .get(k)
                    .unwrap_or_else(|| panic!("{path}.{k}: missing from re-serialized JSON"));
                assert_subset(ev, av, &format!("{path}.{k}"));
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            assert_eq!(e.len(), a.len(), "{path}: array length");
            for (i, (ev, av)) in e.iter().zip(a).enumerate() {
                assert_subset(ev, av, &format!("{path}[{i}]"));
            }
        }
        (Value::Number(e), Value::Number(a)) => {
            assert_eq!(e.as_f64(), a.as_f64(), "{path}: number");
        }
        _ => assert_eq!(expected, actual, "{path}"),
    }
}

fn round_trip<T>(name: &str) -> T
where
    T: Message + Default + PartialEq + Debug + Serialize + DeserializeOwned,
{
    let text =
        std::fs::read_to_string(fixture_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
    let fixture: Value = serde_json::from_str(&text).unwrap();

    let parsed: T =
        serde_json::from_value(fixture.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let out = serde_json::to_value(&parsed).unwrap();
    assert_subset(&fixture, &out, name);

    let reparsed: T = serde_json::from_value(out).unwrap();
    assert_eq!(
        parsed, reparsed,
        "{name}: JSON round trip changed the value"
    );

    let bytes = parsed.encode_to_vec();
    let decoded = T::decode(bytes.as_slice()).unwrap();
    assert_eq!(
        parsed, decoded,
        "{name}: protobuf round trip changed the value"
    );

    assert_eq!(fixture["schema_version"], 1, "{name}: schema_version");
    parsed
}

macro_rules! fixtures {
    ($($test:ident: $ty:ty = $file:literal;)*) => {
        $( #[test] fn $test() { round_trip::<$ty>($file); } )*
    };
}

fixtures! {
    node_capabilities: NodeCapabilities = "node_capabilities.json";
    heartbeat: Heartbeat = "heartbeat.json";
    command_start_rpc_server: Command = "command_start_rpc_server.json";
    command_stop_rpc_server: Command = "command_stop_rpc_server.json";
    command_netcheck: Command = "command_netcheck.json";
    command_result_start_rpc_server: CommandResult = "command_result_start_rpc_server.json";
    command_result_error: CommandResult = "command_result_error.json";
    model_info: ModelInfo = "model_info.json";
    split_plan: SplitPlan = "split_plan.json";
    launch_spec: LaunchSpec = "launch_spec.json";
    cluster_status: ClusterStatus = "cluster_status.json";
    bench_result: BenchResult = "bench_result.json";
    netcheck_result: NetcheckResult = "netcheck_result.json";
    event: Event = "event.json";
    register_request: RegisterRequest = "register_request.json";
    register_response: RegisterResponse = "register_response.json";
    heartbeat_stream_summary: HeartbeatStreamSummary = "heartbeat_stream_summary.json";
}

/// Every .json file in fixtures/json/ must be listed above, so nothing goes untested.
#[test]
fn every_fixture_file_is_tested() {
    let src = include_str!("fixtures.rs");
    for entry in std::fs::read_dir(fixture_path("")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.ends_with(".json") {
            assert!(
                src.contains(&format!("\"{name}\"")),
                "{name} has no round-trip test"
            );
        }
    }
}

// ---- spot checks: the typed values are what the JSON says -------------------------------------

#[test]
fn enums_read_as_typed_values() {
    let hb: Heartbeat = round_trip("heartbeat.json");
    let rpc = hb.rpc_server.unwrap();
    assert_eq!(rpc.state(), RpcServerState::Running);
    assert_eq!(hb.relay.unwrap().source(), RelaySource::DebugLog);

    let cs: ClusterStatus = round_trip("cluster_status.json");
    assert_eq!(cs.cluster_state(), ClusterState::Serving);
    assert_eq!(cs.nodes[0].state(), NodeState::Active);
    assert_eq!(cs.nodes[0].role(), NodeRole::Helper);
    assert_eq!(cs.strategy(), SplitStrategy::LlamaDefault);

    let ev: Event = round_trip("event.json");
    assert_eq!(ev.kind(), EventKind::NodeLost);

    let err: CommandResult = round_trip("command_result_error.json");
    assert_eq!(err.error.unwrap().code(), ErrorCode::EPortInUse);
    assert!(err.data.is_none());
}

#[test]
fn command_kind_selects_the_args_type() {
    let cmd: Command = round_trip("command_start_rpc_server.json");
    match cmd.args {
        Some(command::Args::StartRpcServer(a)) => {
            assert_eq!(a.port, Some(50052));
            assert_eq!(a.cache, Some(true));
            assert!(
                a.devices.is_empty(),
                "devices: null -> empty (rpc-server default)"
            );
            assert_eq!(a.threads, None);
        }
        other => panic!("wrong args: {other:?}"),
    }
    let res: CommandResult = round_trip("command_result_start_rpc_server.json");
    match res.data {
        Some(command_result::Data::StartRpcServer(d)) => {
            assert_eq!(d.pid, Some(12345));
            assert_eq!(d.banner.unwrap().devices, vec!["Vulkan0", "Vulkan1"]);
        }
        other => panic!("wrong data: {other:?}"),
    }
}

#[test]
fn nullable_lists_write_null_when_empty() {
    let plan: SplitPlan = round_trip("split_plan.json");
    assert!(plan.tensor_split.is_empty());
    assert_eq!(plan.assignments[1].layers, vec![6, 18]);
    let with_split = SplitPlan {
        tensor_split: vec![0.2, 0.4, 0.4],
        ..plan
    };
    let v = serde_json::to_value(&with_split).unwrap();
    assert_eq!(v["tensor_split"], serde_json::json!([0.2, 0.4, 0.4]));
    let v = serde_json::to_value(SplitPlan::default()).unwrap();
    assert_eq!(v["tensor_split"], Value::Null);
}

#[test]
fn readers_ignore_unknown_fields_and_values() {
    // §6.0: unknown fields are ignored; a newer writer's enum value reads as UNSPECIFIED, not an error.
    let json = r#"{ "schema_version": 1, "seq": 1, "kind": "some_future_kind",
                    "brand_new_field": { "x": 1 }, "message": "hi" }"#;
    let ev: Event = serde_json::from_str(json).unwrap();
    assert_eq!(ev.kind(), EventKind::Unspecified);
    assert_eq!(ev.message, "hi");
}

#[test]
fn missing_fields_read_as_defaults() {
    let hb: Heartbeat = serde_json::from_str(r#"{ "schema_version": 1, "node_id": "n" }"#).unwrap();
    assert_eq!(hb.seq, 0);
    assert!(hb.rpc_server.is_none());
}

#[test]
fn error_codes_use_section_6_11_spelling() {
    let e = ErrorInfo {
        code: ErrorCode::EModelDoesNotFit as i32,
        message: String::new(),
    };
    assert_eq!(
        serde_json::to_value(&e).unwrap()["code"],
        "E_MODEL_DOES_NOT_FIT"
    );
}

// ---- gRPC services exist with the expected shapes (compile-time check) ------------------------

#[allow(dead_code)]
fn services_compile() {
    use contracts::v1::agent_service_client::AgentServiceClient;
    use contracts::v1::agent_service_server::{AgentService, AgentServiceServer};
    use contracts::v1::coordinator_service_client::CoordinatorServiceClient;
    use contracts::v1::coordinator_service_server::{CoordinatorService, CoordinatorServiceServer};
    let _ = AgentServiceClient::<tonic::transport::Channel>::new;
    let _ = CoordinatorServiceClient::<tonic::transport::Channel>::new;
    fn assert_server<S: AgentService>(s: S) -> AgentServiceServer<S> {
        AgentServiceServer::new(s)
    }
    fn assert_coord<S: CoordinatorService>(s: S) -> CoordinatorServiceServer<S> {
        CoordinatorServiceServer::new(s)
    }
}
