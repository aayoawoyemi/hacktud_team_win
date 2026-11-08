// CONCEPT: Checked capacity arithmetic and human-readable forward/reverse fit.
// WHY: R5.6, R5.13–15 and D-07 need transparent numbers before layer placement.
// DEPENDS ON: ModelInspection; caller supplies measured free memory and buffer estimates.
// AFFECTS: These are capacity estimates, not guarantees that llama.cpp will load.
use crate::{ModelInspection, Result, add, invalid, mul};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    // Shared system/GPU memory must have ONE entry. Distinct physical memory
    // pools have distinct ids; repeated ids are rejected instead of double counted.
    pub memory_pool_id: String,
    // Required in JSON: a caller must classify the physical pool explicitly.
    // Unified memory (including Metal) requires a measured GPU budget below.
    pub unified_memory: bool,
    pub free_bytes: u64,
    pub device_limit_bytes: Option<u64>,
    pub reserve_bytes: u64,
    pub compute_buffer_bytes: u64,
}

// CONCEPT: Saturate exhausted budgets to zero, never unsigned-wrap to huge RAM.
// WHY: Free memory changes while other apps run. Device limits can be below RAM.
// DEPENDS ON: WP-1 measured compute buffers; O-25 context settings.
// AFFECTS: Caller must refresh free_bytes before using a fit estimate to launch.
pub fn usable_bytes(machine: &Machine, kv_share_bytes: u64) -> u64 {
    // This helper is public, so enforce the cap even outside fit(). That path
    // reports a descriptive error; a raw budget query fails closed to zero.
    if machine.unified_memory && machine.device_limit_bytes.is_none() {
        return 0;
    }
    machine
        .free_bytes
        .min(machine.device_limit_bytes.unwrap_or(u64::MAX))
        .saturating_sub(machine.reserve_bytes)
        .saturating_sub(machine.compute_buffer_bytes)
        .saturating_sub(kv_share_bytes)
}
#[derive(Debug, Serialize)]
pub struct FitRow {
    pub name: String,
    pub usable_weight_bytes: u64,
    pub largest_model_bytes: u64,
    pub fits: bool,
}
#[derive(Debug, Serialize)]
pub struct FitReport {
    pub weight_bytes: u64,
    pub kv_bytes: u64,
    pub machines: Vec<FitRow>,
    pub pool: FitRow,
    pub model_to_largest_machine_ratio: Option<f64>,
    pub warnings: Vec<String>,
}

pub fn fit(
    model: &ModelInspection,
    machines: &[Machine],
    ctx: u64,
    parallel: u64,
    kv_bytes_per_token_override: Option<u64>,
) -> Result<FitReport> {
    if machines.is_empty() || ctx == 0 || parallel == 0 {
        return Err(invalid("machines, context and parallel must be nonzero"));
    }
    let mut pools = BTreeSet::new();
    for m in machines {
        if m.memory_pool_id.trim().is_empty() || !pools.insert(&m.memory_pool_id) {
            return Err(invalid(
                "memory_pool_id must be unique: consolidate unified system/GPU RAM into one entry",
            ));
        }
        if m.unified_memory && m.device_limit_bytes.is_none() {
            return Err(invalid(
                "unified memory requires device_limit_bytes from the GPU budget; do not use total RAM",
            ));
        }
    }
    let kv_per_token = kv_bytes_per_token_override
        .or(model.kv_bytes_per_token_f16)
        .ok_or_else(|| {
            invalid("unknown KV size; provide an explicit measured KV bytes/token override")
        })?;
    let kv = mul(mul(kv_per_token, ctx)?, parallel)?;
    let mut weights = 0;
    for bytes in model
        .layer_bytes
        .iter()
        .chain(model.non_layer_bytes.values())
    {
        weights = add(weights, *bytes)?;
    }
    if weights == 0 {
        return Err(invalid("model contains no tensor bytes"));
    }
    let mut rows = Vec::new();
    let mut pool_base = 0;
    let mut largest = 0;
    for m in machines {
        pool_base = add(pool_base, usable_bytes(m, 0))?;
        let usable = usable_bytes(m, kv);
        largest = largest.max(usable);
        // Checking full requirement against base capacity avoids reporting that
        // a zero-byte model fits after the cache already exceeded free memory.
        let fits = add(weights, kv)? <= usable_bytes(m, 0);
        rows.push(FitRow {
            name: m.name.clone(),
            usable_weight_bytes: usable,
            largest_model_bytes: usable,
            fits,
        });
    }
    let pool_usable = pool_base.saturating_sub(kv);
    let mut warnings = model.warnings.clone();
    warnings.push("Pool fit is aggregate capacity only; layer placement, main-node tensors, KV distribution and actual buffers are not verified (R5.7–12/R5.16 deferred).".into());
    warnings.push("Reverse fit holds this model's architecture, KV size, context and buffer estimates fixed; it is not a prediction for arbitrary larger models.".into());
    warnings.push("parallel multiplies full context conservatively; verify llama.cpp -np context sharing on the pinned build.".into());
    if model.n_ctx_train.map(|n| ctx > n).unwrap_or(false) {
        warnings.push("Requested context exceeds training context.".into());
    }
    if kv_bytes_per_token_override.is_some() {
        warnings.push("Explicit KV override supplied by caller.".into());
    }
    Ok(FitReport {
        weight_bytes: weights,
        kv_bytes: kv,
        machines: rows,
        pool: FitRow {
            name: "POOL (capacity only)".into(),
            usable_weight_bytes: pool_usable,
            largest_model_bytes: pool_usable,
            fits: add(weights, kv)? <= pool_base,
        },
        model_to_largest_machine_ratio: if largest > 0 {
            Some(weights as f64 / largest as f64)
        } else {
            None
        },
        warnings,
    })
}

impl FitReport {
    pub fn table(&self) -> String {
        let mut out = format!(
            "Weights: {} bytes; KV: {} bytes\n{:<24} {:>18} {:>18} {:>6}\n",
            self.weight_bytes, self.kv_bytes, "Machine", "Usable weight B", "Max model B", "Fits"
        );
        for r in self.machines.iter().chain(std::iter::once(&self.pool)) {
            out.push_str(&format!(
                "{:<24} {:>18} {:>18} {:>6}\n",
                r.name, r.usable_weight_bytes, r.largest_model_bytes, r.fits
            ));
        }
        out.push_str(&format!(
            "Model / largest single-machine usable weights: {}\n",
            self.model_to_largest_machine_ratio
                .map(|r| format!("{r:.3}x"))
                .unwrap_or_else(|| "unknown (zero capacity)".into())
        ));
        for w in &self.warnings {
            out.push_str(&format!("Warning: {w}\n"));
        }
        out
    }
}
