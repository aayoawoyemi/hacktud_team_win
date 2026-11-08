// CONCEPT: Checked adapter from detailed GGUF inspection to shared §6.4 ModelInfo.
// WHY: WP-2 landed in #18; WP-4 can now consume the same published Rust type.
// DEPENDS ON: contracts::ModelInfo; required scalar metadata must be known.
// AFFECTS: Local inspect retains unknown fields; shared export refuses missing
// required values rather than turning unknown dimensions into zero silently.
use crate::{ModelInspection, Result, invalid};

fn required(value: Option<u64>, field: &str) -> Result<u32> {
    Ok(value
        .ok_or_else(|| invalid(format!("shared ModelInfo requires {field}")))?
        .try_into()?)
}

pub fn model_info(model: &ModelInspection) -> Result<contracts::ModelInfo> {
    Ok(contracts::ModelInfo {
        schema_version: contracts::SCHEMA_VERSION,
        model_id: model.model_id.clone(),
        path: model.path.clone(),
        file_bytes: model.file_bytes,
        split_files: model.split_files.try_into()?,
        gguf_version: model.gguf_version,
        architecture: model.architecture.clone(),
        name: model.name.clone().unwrap_or_else(|| model.model_id.clone()),
        file_type: required(model.file_type, "general.file_type")?,
        n_layers: model.n_layers.try_into()?,
        n_embd: required(model.n_embd, "embedding_length")?,
        n_head: required(model.n_head, "attention.head_count")?,
        n_head_kv: required(model.n_head_kv, "attention.head_count_kv")?,
        key_length: required(model.key_length, "key_length")?,
        value_length: required(model.value_length, "value_length")?,
        n_ctx_train: model
            .n_ctx_train
            .ok_or_else(|| invalid("shared ModelInfo requires context_length"))?,
        expert_count: model.expert_count.map(u32::try_from).transpose()?,
        expert_used_count: model.expert_used_count.map(u32::try_from).transpose()?,
        layer_bytes: model.layer_bytes.clone(),
        non_layer_bytes: model.non_layer_bytes.clone().into_iter().collect(),
        kv_bytes_per_token_f16: model.kv_bytes_per_token_f16,
        warnings: model.warnings.clone(),
    })
}
