//! CONCEPT: the JSON form of the contract messages. build.rs attaches `#[serde(with = "...")]` attributes
//!          that point into this module, so the generated prost types serialize the way PRD §6 shows them.
//! WHY: §6.0 conventions (snake_case, plain integers, `null` = unknown) and the §6 examples are the JSON the
//!      team reads in fixtures, logs and the WP-7 status output. prost's own types carry enums as `i32` and
//!      lists as never-null, so two small adapters are needed:
//!      - `enums::*`: enum fields as the short names from §6 ("running", "SERVING", "E_CONFIG").
//!      - `null_as_empty`: lists/maps §6 shows as `null` when unset.
//! DEPENDS ON: the generated enums in `crate::v1` (their `as_str_name` / `from_str_name`).
//! AFFECTS: JSON only. The protobuf wire format (gRPC) is not touched by anything here.

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

/// CONCEPT: a list or map that is JSON `null` when empty and reads `null` (or a missing key) as empty.
/// WHY: §6 uses `null` for "not set" on fields like `tensor_split` ("null for llama_default"). protobuf
///      repeated fields can't be absent, and for these fields an empty list means the same as "not set".
/// AFFECTS: the fields listed in build.rs NULLABLE_COLLECTIONS.
pub mod null_as_empty {
    use super::*;

    pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Serialize + Default + PartialEq,
        S: Serializer,
    {
        if *value == T::default() {
            serializer.serialize_none()
        } else {
            serializer.serialize_some(value)
        }
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: Deserialize<'de> + Default,
        D: Deserializer<'de>,
    {
        Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
    }
}

/// How an enum's protobuf value names map to its JSON names.
#[derive(Clone, Copy)]
pub enum Style {
    /// Strip the prefix and lower-case: RPC_SERVER_STATE_RUNNING -> "running".
    Lower,
    /// Strip the prefix, keep upper case: CLUSTER_STATE_SERVING -> "SERVING" (§6.7 spells states this way).
    Upper,
    /// Use the value name as is: E_CONFIG -> "E_CONFIG" (§6.11).
    Verbatim,
}

/// Converts one enum value to its JSON name. Values this build doesn't know (a newer writer) stay numbers
/// so nothing is lost on a round trip.
pub fn enum_to_json(proto_name: Option<&str>, prefix: &str, style: Style) -> Option<String> {
    let name = proto_name?;
    Some(match style {
        Style::Verbatim => name.to_string(),
        Style::Upper => name.strip_prefix(prefix).unwrap_or(name).to_string(),
        Style::Lower => name
            .strip_prefix(prefix)
            .unwrap_or(name)
            .to_ascii_lowercase(),
    })
}

/// Converts a JSON name back to the protobuf value name that `from_str_name` understands.
pub fn enum_from_json(json_name: &str, prefix: &str, style: Style) -> String {
    match style {
        Style::Verbatim => json_name.to_string(),
        Style::Upper | Style::Lower => format!("{prefix}{}", json_name.to_ascii_uppercase()),
    }
}

/// Visitor accepting either a JSON name (string) or a raw number.
/// Unknown names read as 0 (`*_UNSPECIFIED`): "readers ignore what they don't know" (§6.0) applied to
/// enum values, so a newer writer adding a value doesn't break an older reader.
pub struct EnumVisitor(pub fn(&str) -> i32);

impl<'de> Visitor<'de> for EnumVisitor {
    type Value = i32;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an enum name string or an integer")
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<i32, E> {
        Ok((self.0)(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<i32, E> {
        i32::try_from(v).map_err(E::custom)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<i32, E> {
        i32::try_from(v).map_err(E::custom)
    }
}

/// CONCEPT: generates one serde `with` module per proto enum: `serialize`/`deserialize` for a plain field
///          (`i32`), plus `option` (`Option<i32>`) and `repeated` (`Vec<i32>`) submodules.
/// WHY: prost enums expose `as_str_name` / `from_str_name` as inherent methods, not a trait, so a generic
///      function can't reach them; a macro per enum can.
macro_rules! json_enum {
    ($module:ident, $ty:ident, $prefix:literal, $style:ident) => {
        pub mod $module {
            use super::super::*;
            use crate::v1::$ty;

            fn from_name(s: &str) -> i32 {
                $ty::from_str_name(&enum_from_json(s, $prefix, Style::$style))
                    .map(|e| e as i32)
                    .unwrap_or(0)
            }

            /// Newtype so Option/Vec wrappers can reuse the same logic.
            pub struct Value(pub i32);
            impl Serialize for Value {
                fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    let name = $ty::try_from(self.0).ok().map(|e| e.as_str_name());
                    match enum_to_json(name, $prefix, Style::$style) {
                        Some(n) => s.serialize_str(&n),
                        None => s.serialize_i32(self.0),
                    }
                }
            }
            impl<'de> Deserialize<'de> for Value {
                fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    d.deserialize_any(EnumVisitor(from_name)).map(Value)
                }
            }

            pub fn serialize<S: Serializer>(v: &i32, s: S) -> Result<S::Ok, S::Error> {
                Value(*v).serialize(s)
            }
            pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
                Ok(Value::deserialize(d)?.0)
            }

            pub mod option {
                use super::Value;
                use serde::{Deserialize, Deserializer, Serialize, Serializer};
                pub fn serialize<S: Serializer>(v: &Option<i32>, s: S) -> Result<S::Ok, S::Error> {
                    v.map(Value).serialize(s)
                }
                pub fn deserialize<'de, D: Deserializer<'de>>(
                    d: D,
                ) -> Result<Option<i32>, D::Error> {
                    Ok(Option::<Value>::deserialize(d)?.map(|v| v.0))
                }
            }

            pub mod repeated {
                use super::Value;
                use serde::{Deserialize, Deserializer, Serialize, Serializer};
                pub fn serialize<S: Serializer>(v: &[i32], s: S) -> Result<S::Ok, S::Error> {
                    v.iter().map(|x| Value(*x)).collect::<Vec<_>>().serialize(s)
                }
                pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<i32>, D::Error> {
                    Ok(Vec::<Value>::deserialize(d)?
                        .into_iter()
                        .map(|v| v.0)
                        .collect())
                }
            }
        }
    };
}

/// CONCEPT: one line per proto enum. Module name = enum name in snake_case (build.rs derives the same
///          name, so a missing line here is a compile error naming the module).
/// AFFECTS: the JSON spelling of every enum value. The spelling follows §6: lower-case for kinds/states the
///          PRD writes in lower case, upper-case for §5.6 states, verbatim for §6.11 error codes.
pub mod enums {
    json_enum!(error_code, ErrorCode, "", Verbatim);
    json_enum!(rpc_server_state, RpcServerState, "RPC_SERVER_STATE_", Lower);
    json_enum!(relay_source, RelaySource, "RELAY_SOURCE_", Lower);
    json_enum!(split_strategy, SplitStrategy, "SPLIT_STRATEGY_", Lower);
    json_enum!(cluster_state, ClusterState, "CLUSTER_STATE_", Upper);
    json_enum!(node_state, NodeState, "NODE_STATE_", Upper);
    json_enum!(node_role, NodeRole, "NODE_ROLE_", Lower);
    json_enum!(event_kind, EventKind, "EVENT_KIND_", Lower);
}
