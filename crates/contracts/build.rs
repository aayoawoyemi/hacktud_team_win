//! CONCEPT: compiles proto/contracts.proto into Rust (prost messages + tonic services) at build time, and
//!          attaches serde attributes so every message also has the §6 JSON form.
//! WHY: O-03: gRPC with tonic, compiled with protox so no machine needs `protoc` installed (R12.5).
//!      protox parses the .proto in pure Rust and hands a FileDescriptorSet to tonic-prost-build.
//! DEPENDS ON: proto/contracts.proto; src/json.rs (the serde helper modules the attributes point at).
//! AFFECTS: the generated module `contracts::v1` (OUT_DIR/contracts.v1.rs). Any change to a field's
//!          serde behaviour happens here.
//!
//! Rejected alternatives:
//! - pbjson-build (generates proto3 canonical JSON): it writes 64-bit integers as strings ("4920000000"),
//!   camelCases names by default and spells enums "RPC_SERVER_STATE_RUNNING". The §6 examples use plain
//!   numbers, snake_case and short enum names, so readers of the fixtures and logs would see a different
//!   shape from the PRD.
//! - Hand-written serde structs next to the proto: two sources of truth that would drift.

use prost_types::field_descriptor_proto::{Label, Type};
use prost_types::{DescriptorProto, FileDescriptorSet};

const PROTO: &str = "proto/contracts.proto";
const PACKAGE: &str = "contracts.v1";

/// CONCEPT: list fields that §6 shows as `null` when unset. protobuf can't tell "empty list" from
///          "absent", so these are plain repeated/map fields that serialize to JSON `null` when empty and
///          read `null` back as empty (src/json.rs `null_as_empty`).
/// AFFECTS: the JSON form only; protobuf encoding is unchanged.
const NULLABLE_COLLECTIONS: &[&str] = &[
    "StartRpcServerArgs.devices",
    "SplitPlan.tensor_split",
    "LaunchSpec.tensor_split",
    "LaunchSpec.devices",
    "DevicePlacement.actual_layers",
    "LlamaServerStatus.metrics",
    "LlamaServerStatus.slots",
];

/// CONCEPT: oneofs and the JSON keys they become. Each becomes two sibling keys on the parent object:
///          `"kind": "<snake_case variant>"` and `"<content key>": { ... }` (§6.3 shape).
const ONEOFS: &[(&str, &str)] = &[("Command.args", "args"), ("CommandResult.data", "data")];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={PROTO}");
    println!("cargo:rerun-if-changed=build.rs");

    // 1. Parse with protox (pure Rust). Errors here print the .proto file, line and column.
    let fds: FileDescriptorSet = protox::compile([PROTO], ["proto"])?;

    // 2. Configure code generation.
    let mut builder = tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        // BTreeMap instead of HashMap: deterministic key order in JSON output (stable fixtures and diffs).
        .btree_map(".")
        // Every message: serde derives, and `default` so missing JSON fields read as their default
        // (proto3 semantics; also how "readers ignore unknown fields" pairs with "fields are only added").
        .message_attribute(
            ".",
            "#[derive(::serde::Serialize, ::serde::Deserialize)] #[serde(default)]",
        );

    for path in NULLABLE_COLLECTIONS {
        builder = builder.field_attribute(
            format!(".{PACKAGE}.{path}"),
            "#[serde(with = \"crate::json::null_as_empty\")]",
        );
    }

    let file = fds
        .file
        .iter()
        .find(|f| f.name() == "contracts.proto")
        .expect("protox returned contracts.proto");

    for (path, content) in ONEOFS {
        builder = builder
            .enum_attribute(
                format!(".{PACKAGE}.{path}"),
                format!(
                    "#[derive(::serde::Serialize, ::serde::Deserialize)]                      #[serde(tag = \"kind\", content = \"{content}\", rename_all = \"snake_case\")]"
                ),
            )
            // No leading dot on purpose. prost matches a field attribute against the field's full path,
            // its suffixes AND its prefixes, so ".contracts.v1.Command.args" would also hit every oneof
            // variant (".contracts.v1.Command.args.probe" has it as a prefix) and serde rejects `flatten`
            // on a variant. The suffix form "contracts.v1.Command.args" matches only the field itself.
            .field_attribute(format!("{PACKAGE}.{path}"), "#[serde(flatten)]");
    }

    // 3. Enum-typed fields are `i32` in prost. Point each one at the JSON helper for its enum so JSON
    //    carries the §6 names ("running", "SERVING", "E_CONFIG") instead of numbers. Found by walking the
    //    descriptors, so a new enum field needs no edit here; a new enum type needs one line in src/json.rs
    //    (the build fails with "could not find `<enum>` in `enums`" until it is added).
    let mut enum_fields = Vec::new();
    for msg in &file.message_type {
        collect_enum_fields(msg, PACKAGE, &mut enum_fields);
    }
    for (path, attr) in enum_fields {
        builder = builder.field_attribute(path, attr);
    }

    // 4. Generate. Output: OUT_DIR/contracts.v1.rs, included by src/lib.rs.
    builder.compile_fds(fds)?;
    Ok(())
}

/// Walks one message (and its nested messages) and returns (field path, serde attribute) for every
/// enum-typed field.
fn collect_enum_fields(msg: &DescriptorProto, scope: &str, out: &mut Vec<(String, String)>) {
    let msg_path = format!("{scope}.{}", msg.name());
    for field in &msg.field {
        if field.r#type() != Type::Enum {
            continue;
        }
        // A real oneof member (not the synthetic oneof proto3 `optional` creates) lives in the oneof's
        // Rust enum, where field attributes don't apply the same way. None exist today; fail loudly if
        // one is added rather than silently emitting numbers in JSON.
        if field.oneof_index.is_some() && !field.proto3_optional() {
            panic!(
                "enum field {msg_path}.{} is inside a oneof; teach build.rs to handle it",
                field.name()
            );
        }
        let enum_name = field.type_name().rsplit('.').next().unwrap_or_default();
        let module = format!("crate::json::enums::{}", to_snake_case(enum_name));
        let module = if field.label() == Label::Repeated {
            format!("{module}::repeated")
        } else if field.proto3_optional() {
            format!("{module}::option")
        } else {
            module
        };
        out.push((
            format!(".{msg_path}.{}", field.name()),
            format!("#[serde(with = \"{module}\")]"),
        ));
    }
    for nested in &msg.nested_type {
        collect_enum_fields(nested, &msg_path, out);
    }
}

/// "RpcServerState" -> "rpc_server_state"
fn to_snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
