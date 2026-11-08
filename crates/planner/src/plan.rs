// CONCEPT: Contiguous llama.cpp layer placement using the published WP-2 types.
// WHY: R5.7–11; #11 requests a usable SplitPlan for WP-4, not aggregate RAM alone.
// DEPENDS ON: contracts §6.5; WP-1 verifies b8665's RPC-first/main-last device order.
// AFFECTS: WP-4 launch; assumes layer split, full GPU offload and offloaded KV.
use crate::{
    ModelInspection, Result, add,
    capacity::{Machine, usable_bytes},
    invalid, mul,
};
use contracts::{LayerAssignment, SplitPlan, SplitStrategy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

// CONCEPT: Explicit launch order and measured inputs, local to the planner.
// WHY: Sorting device names can silently scramble --rpc and -ts correspondence.
// DEPENDS ON: WP-3 probes; main's CPU budget is separate only for discrete GPUs.
// AFFECTS: Caller must pass exactly the order WP-4 will give llama-server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDevice {
    pub node_id: String,
    pub device: String,
    pub llama_device: String,
    pub is_main: bool,
    pub memory: Machine,
    pub measured_bytes_per_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanRequest {
    pub plan_id: String,
    pub created_at: String,
    pub strategy: String,
    pub ctx_size: u32,
    pub parallel: u32,
    pub devices: Vec<PlanDevice>,
    pub kv_bytes_per_token_override: Option<u64>,
    // Available CPU bytes AFTER reserves. For unified main, CPU tensors instead
    // consume the same measured memory pool as its GPU; never add RAM to Metal.
    pub main_cpu_available_bytes: Option<u64>,
}

// CONCEPT: Exact byte checks first, then optimize among feasible partitions.
// WHY: Heterogeneous layers and the output tensor invalidate byte-share guesses.
// DEPENDS ON: gguf layer spans; total KV is rounded UP per layer conservatively.
// AFFECTS: Unknown KV or invalid budgets fail closed; no automatic launch occurs.
pub fn build_plan(model: &ModelInspection, request: &PlanRequest) -> Result<SplitPlan> {
    let n = usize::try_from(model.n_layers)?;
    let slots = n
        .checked_add(1)
        .ok_or_else(|| invalid("layer count overflow"))?;
    if n == 0 || n != model.layer_bytes.len() || slots > 4096 {
        return Err(invalid("need 1..4095 layers and matching layer_bytes"));
    }
    if request.ctx_size == 0
        || request.parallel == 0
        || request.devices.is_empty()
        || request.plan_id.trim().is_empty()
        || request.created_at.trim().is_empty()
    {
        return Err(invalid(
            "plan identity, timestamp, devices, context and parallel are required",
        ));
    }
    let strategy = match request.strategy.as_str() {
        "llama_default" => SplitStrategy::LlamaDefault,
        "proportional_usable" => SplitStrategy::ProportionalUsable,
        "fastest_first" => SplitStrategy::FastestFirst,
        _ => {
            return Err(invalid(
                "supported strategies: llama_default, proportional_usable, fastest_first",
            ));
        }
    };
    let mut pools = BTreeSet::new();
    let mut names = BTreeSet::new();
    let d = request.devices.len();
    for (i, device) in request.devices.iter().enumerate() {
        let m = &device.memory;
        if device.node_id.trim().is_empty()
            || device.device.trim().is_empty()
            || device.llama_device.trim().is_empty()
            || !names.insert(&device.llama_device)
            || m.memory_pool_id.trim().is_empty()
            || !pools.insert(&m.memory_pool_id)
        {
            return Err(invalid(
                "device names and physical memory pools must be nonempty and unique",
            ));
        }
        if device.is_main != (i == d - 1) {
            return Err(invalid(
                "exactly one main device must be last, after RPC devices",
            ));
        }
        if m.unified_memory && m.device_limit_bytes.is_none() {
            return Err(invalid(
                "unified memory needs the measured Metal/device budget",
            ));
        }
        if strategy == SplitStrategy::FastestFirst
            && !device
                .measured_bytes_per_second
                .is_some_and(|s| s.is_finite() && s > 0.0)
        {
            return Err(invalid(
                "fastest_first requires positive finite measured speeds for every device",
            ));
        }
    }
    let per_token = request
        .kv_bytes_per_token_override
        .or(model.kv_bytes_per_token_f16)
        .ok_or_else(|| {
            invalid("unknown KV; provide a measured total-model KV bytes/token override")
        })?;
    let kv_layer = mul(
        mul(
            per_token.div_ceil(model.n_layers),
            u64::from(request.ctx_size),
        )?,
        u64::from(request.parallel),
    )?;
    let mut cpu = 0;
    let mut output = 0;
    let mut cpu_names = Vec::new();
    for (name, &bytes) in &model.non_layer_bytes {
        if name.starts_with("output.") || name.starts_with("output_norm.") {
            output = add(output, bytes)?;
        } else {
            cpu = add(cpu, bytes)?;
            cpu_names.push(name.clone());
        }
    }
    let mut warnings = model.warnings.clone();
    // Tied embeddings may require an additional output-device copy. Charge it
    // conservatively instead of assuming CPU input storage also backs the GPU.
    if !model.non_layer_bytes.contains_key("output.weight")
        && let Some(&bytes) = model.non_layer_bytes.get("token_embd.weight")
    {
        output = add(output, bytes)?;
        warnings.push("Tied output: conservatively charged a second token_embd.weight copy on the output device.".into());
    }
    let main = &request.devices[d - 1];
    let cpu_fits =
        main.memory.unified_memory || request.main_cpu_available_bytes.is_some_and(|b| cpu <= b);
    let budgets: Vec<u64> = request
        .devices
        .iter()
        .enumerate()
        .map(|(i, dev)| {
            let base = usable_bytes(&dev.memory, 0);
            if i == d - 1 && dev.memory.unified_memory {
                base.saturating_sub(cpu)
            } else {
                base
            }
        })
        .collect();
    let cpu_fits =
        cpu_fits && (!main.memory.unified_memory || cpu <= usable_bytes(&main.memory, 0));
    let mut weights = model.layer_bytes.clone();
    weights.push(output);
    let mut prefix = vec![0u64];
    let mut weight_prefix = vec![0u64];
    for (i, &weight) in weights.iter().enumerate() {
        prefix.push(add(
            *prefix.last().unwrap(),
            add(weight, if i < n { kv_layer } else { 0 })?,
        )?);
        weight_prefix.push(add(*weight_prefix.last().unwrap(), weight)?);
    }
    let counts = if strategy == SplitStrategy::LlamaDefault {
        // Mirror float cumulative shares + upper_bound used by llama.cpp. Do
        // not replace this with rounding byte ratios into layer counts.
        let mut total = 0f32;
        let mut cumulative = Vec::new();
        for dev in &request.devices {
            total += dev.memory.free_bytes as f32;
            cumulative.push(total);
        }
        if total == 0.0 {
            return Err(invalid("llama_default needs nonzero reported free memory"));
        }
        let mut result = vec![0usize; d];
        for slot in 0..slots {
            let fraction = slot as f32 / slots as f32;
            let owner = cumulative
                .iter()
                .position(|v| *v / total > fraction)
                .ok_or_else(|| invalid("invalid default split"))?;
            result[owner] += 1;
        }
        Some(result)
    } else {
        partition(&prefix, &weight_prefix, &budgets, request, strategy)
    };
    let mut plan = SplitPlan {
        schema_version: contracts::SCHEMA_VERSION,
        plan_id: request.plan_id.clone(),
        created_at: request.created_at.clone(),
        model_id: model.model_id.clone(),
        strategy: strategy as i32,
        ctx_size: request.ctx_size,
        parallel: request.parallel,
        device_order: request
            .devices
            .iter()
            .map(|d| d.llama_device.clone())
            .collect(),
        tensor_split: Vec::new(),
        assignments: Vec::new(),
        main_cpu_holds: cpu_names,
        fits: cpu_fits && counts.is_some(),
        est_ms_per_token: None,
        warnings: Vec::new(),
    };
    if let Some(counts) = counts {
        if strategy != SplitStrategy::LlamaDefault {
            plan.tensor_split = counts.iter().map(|&v| v as f64).collect();
        }
        let mut start = 0;
        for (i, (&count, device)) in counts.iter().zip(&request.devices).enumerate() {
            if count == 0 {
                continue;
            }
            let end = start + count;
            let weight = weight_prefix[end] - weight_prefix[start];
            let kv = mul(kv_layer, (end.min(n) - start.min(n)) as u64)?;
            let needed = add(weight, kv)?;
            plan.fits &= needed <= budgets[i];
            let headroom = i128::from(budgets[i]) - i128::from(needed);
            plan.assignments.push(LayerAssignment {
                node_id: device.node_id.clone(),
                device: device.device.clone(),
                llama_device: device.llama_device.clone(),
                layers: vec![start as u32, (end - 1) as u32],
                weight_bytes: weight,
                kv_bytes: kv,
                compute_buffer_bytes_est: Some(device.memory.compute_buffer_bytes),
                usable_bytes: Some(budgets[i]),
                headroom_bytes: Some(i64::try_from(headroom)?),
            });
            start = end;
        }
    } else {
        warnings.push("No contiguous placement fits the supplied per-device budgets (including KV, output and main CPU tensors).".into());
    }
    if !cpu_fits {
        warnings.push("Main CPU tensors exceed their shared budget, or discrete main CPU available bytes were not supplied.".into());
    }
    warnings.push("Layer index n_layers denotes the output slot; integer tensor_split shares sum to n_layers+1. Launch with full GPU offload and offloaded KV; verify b8665 placement against WP-1 load logs.".into());
    warnings.push("Uses measured GPU budgets, reserves and compute buffers. parallel multiplies context conservatively; no WP-1 load calibration or runtime load guarantee yet.".into());
    if model.expert_count.is_some() {
        warnings.push("MoE speed estimate omitted: active tensor bytes are not measured.".into());
    }
    plan.warnings = warnings;
    Ok(plan)
}

// CONCEPT: Dynamic programming over contiguous ranges in fixed launch order.
// WHY: Greedy fill can strand a large later layer even when a feasible split exists.
// DEPENDS ON: prefix sums include KV and output; main must own the output slot.
// AFFECTS: S1 minimizes squared deviation from usable-capacity layer shares;
// S2 minimizes measured serial weight-read time, filling faster devices first
// subject to contiguous ranges and budgets. RTT/speed prediction is left unknown.
fn partition(
    prefix: &[u64],
    weights: &[u64],
    budgets: &[u64],
    request: &PlanRequest,
    strategy: SplitStrategy,
) -> Option<Vec<usize>> {
    let slots = prefix.len() - 1;
    let total_budget: f64 = budgets.iter().map(|&v| v as f64).sum();
    if total_budget == 0.0 {
        return None;
    }
    let mut costs = vec![f64::INFINITY; slots + 1];
    costs[0] = 0.0;
    let mut parents = vec![vec![None; slots + 1]; budgets.len()];
    for (i, &budget) in budgets.iter().enumerate() {
        let mut next = vec![f64::INFINITY; slots + 1];
        let last = i == budgets.len() - 1;
        let target = slots as f64 * budget as f64 / total_budget;
        for end in 0..=if last { slots } else { slots - 1 } {
            if last && end != slots {
                continue;
            }
            for start in (0..=end).rev() {
                if prefix[end] - prefix[start] > budget {
                    break;
                }
                if last && start == end {
                    continue;
                }
                if !costs[start].is_finite() {
                    continue;
                }
                let score = if strategy == SplitStrategy::FastestFirst {
                    (weights[end] - weights[start]) as f64
                        / request.devices[i].measured_bytes_per_second.unwrap()
                } else {
                    ((end - start) as f64 - target).powi(2)
                };
                if costs[start] + score < next[end] {
                    next[end] = costs[start] + score;
                    parents[i][end] = Some(start);
                }
            }
        }
        costs = next;
    }
    if !costs[slots].is_finite() {
        return None;
    }
    let mut counts = vec![0; budgets.len()];
    let mut end = slots;
    for i in (0..budgets.len()).rev() {
        let start = parents[i][end]?;
        counts[i] = end - start;
        end = start;
    }
    Some(counts)
}
