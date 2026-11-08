// CONCEPT: WP-5 metadata inspection and capacity estimates.
// WHY: D-04/D-05: inspect llama.cpp's GGUF models in Rust before loading weights.
// DEPENDS ON: PRD sections 4.4, 6.4, R5.1–R5.6 and R5.13–R5.15.
// AFFECTS: WP-11 model selection and WP-4 shared SplitPlan output.
pub mod capacity;
pub mod gguf;
pub mod plan;
pub mod shared;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// CONCEPT: Local inspection result, deliberately distinct from shared ModelInfo.
// WHY: Keep detailed/optional metadata that shared §6.4 does not represent.
// DEPENDS ON: shared::model_info adapts to WP-2's published type; O-16 for identity.
// AFFECTS: Development CLI only. Do not persist this as a control-plane contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInspection {
    pub schema_version: u32,
    pub model_id: String,
    pub path: String,
    pub file_bytes: u64,
    // Header + metadata + tensor descriptors + padding before the payload.
    // This makes the offset-span accounting auditable: file = metadata + tensors.
    pub metadata_bytes: u64,
    pub split_files: usize,
    pub gguf_version: u32,
    pub architecture: String,
    pub name: Option<String>,
    pub file_type: Option<u64>,
    pub n_layers: u64,
    pub n_embd: Option<u64>,
    pub n_head: Option<u64>,
    pub n_head_kv: Option<u64>,
    pub key_length: Option<u64>,
    pub value_length: Option<u64>,
    pub n_ctx_train: Option<u64>,
    pub expert_count: Option<u64>,
    pub expert_used_count: Option<u64>,
    pub layer_bytes: Vec<u64>,
    pub non_layer_bytes: BTreeMap<String, u64>,
    pub kv_bytes_per_token_f16: Option<u64>,
    pub warnings: Vec<String>,
}

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub(crate) fn invalid(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}
pub(crate) fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .ok_or_else(|| invalid("byte count overflow"))
}
pub(crate) fn mul(a: u64, b: u64) -> Result<u64> {
    a.checked_mul(b)
        .ok_or_else(|| invalid("byte count overflow"))
}
