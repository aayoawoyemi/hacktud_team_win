// CONCEPT: Placement safety and shared payload regression tests for WP-5.
// WHY: #11's missing SplitPlan blocks WP-4; aggregate memory can hide OOM.
// DEPENDS ON: Published contracts fixture; no models, network or inference.
// AFFECTS: Per-device limits, output slot, KV, strategy selection and serialization.
use planner::{
    ModelInspection,
    capacity::Machine,
    plan::{PlanDevice, PlanRequest, build_plan},
    shared::model_info,
};

fn model() -> ModelInspection {
    let mut json: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/json/model_info.json")).unwrap();
    json["metadata_bytes"] = 0.into();
    let mut m: ModelInspection = serde_json::from_value(json).unwrap();
    m.n_layers = 4;
    m.layer_bytes = vec![100; 4];
    m.non_layer_bytes = [
        ("token_embd.weight".into(), 40),
        ("output.weight".into(), 60),
    ]
    .into();
    m.kv_bytes_per_token_f16 = Some(40);
    m
}

fn request(strategy: &str, helper: u64, main: u64) -> PlanRequest {
    PlanRequest {
        plan_id: "test-plan".into(),
        created_at: "2026-11-08T02:00:00Z".into(),
        strategy: strategy.into(),
        ctx_size: 1,
        parallel: 1,
        kv_bytes_per_token_override: None,
        main_cpu_available_bytes: None,
        devices: [helper, main]
            .into_iter()
            .enumerate()
            .map(|(i, free)| PlanDevice {
                node_id: format!("node-{i}"),
                device: "MTL0".into(),
                llama_device: if i == 0 { "RPC0".into() } else { "MTL0".into() },
                is_main: i == 1,
                measured_bytes_per_second: Some(if i == 0 { 1000.0 } else { 100.0 }),
                memory: Machine {
                    name: format!("node-{i}"),
                    memory_pool_id: format!("pool-{i}"),
                    unified_memory: true,
                    free_bytes: free,
                    device_limit_bytes: Some(free),
                    reserve_bytes: 0,
                    compute_buffer_bytes: 0,
                },
            })
            .collect(),
    }
}

#[test]
fn exact_boundary_counts_output_kv_and_cpu_once() {
    let m = model();
    let p = build_plan(&m, &request("proportional_usable", 220, 320)).unwrap();
    assert!(p.fits);
    assert_eq!(p.tensor_split, vec![2.0, 3.0]);
    assert_eq!(p.assignments[0].layers, [0, 1]);
    assert_eq!(p.assignments[1].layers, [2, 4]);
    assert_eq!(
        p.assignments.iter().map(|a| a.weight_bytes).sum::<u64>(),
        460
    );
    assert_eq!(p.assignments.iter().map(|a| a.kv_bytes).sum::<u64>(), 40);
    assert!(p.assignments.iter().all(|a| a.headroom_bytes == Some(0)));
    let json = serde_json::to_string(&p).unwrap();
    let shared: contracts::SplitPlan = serde_json::from_str(&json).unwrap();
    assert_eq!(shared.device_order, ["RPC0", "MTL0"]);
}

#[test]
fn one_byte_short_and_stranded_large_layer_do_not_fit() {
    assert!(
        !build_plan(&model(), &request("proportional_usable", 219, 320))
            .unwrap()
            .fits
    );
    let mut m = model();
    m.layer_bytes = vec![400, 1, 1, 1];
    // Aggregate capacity exceeds all bytes, but neither device holds layer 0.
    assert!(
        !build_plan(&m, &request("proportional_usable", 350, 350))
            .unwrap()
            .fits
    );
}

#[test]
fn fastest_first_fills_faster_helper_without_reordering_devices() {
    let p = build_plan(&model(), &request("fastest_first", 1000, 1000)).unwrap();
    assert!(p.fits);
    assert_eq!(p.tensor_split, [4.0, 1.0]);
    let mut r = request("fastest_first", 1000, 1000);
    r.devices[1].measured_bytes_per_second = Some(10000.0);
    let p = build_plan(&model(), &r).unwrap();
    assert!(p.fits);
    assert_eq!(p.tensor_split, [0.0, 5.0]);
    assert_eq!(p.device_order, ["RPC0", "MTL0"]);
    assert_eq!(p.assignments.len(), 1);
    assert_eq!(p.assignments[0].layers, [0, 4]);
}

#[test]
fn default_predicts_actual_free_memory_split_and_checks_reserves() {
    let mut r = request("llama_default", 500, 500);
    let p = build_plan(&model(), &r).unwrap();
    assert!(p.tensor_split.is_empty());
    // upper_bound uses slot/(n_layers+1): 0, .2, .4 land on helper.
    assert_eq!(p.assignments[0].layers, [0, 2]);
    r.devices[0].memory.reserve_bytes = 200;
    assert!(!build_plan(&model(), &r).unwrap().fits);
}

#[test]
fn validates_order_pool_ids_unknown_kv_and_measured_speed() {
    let m = model();
    let mut r = request("proportional_usable", 1000, 1000);
    r.devices.swap(0, 1);
    assert!(build_plan(&m, &r).is_err());
    let mut r = request("proportional_usable", 1000, 1000);
    r.devices[1].memory.memory_pool_id = r.devices[0].memory.memory_pool_id.clone();
    assert!(build_plan(&m, &r).is_err());
    let mut r = request("fastest_first", 1000, 1000);
    for speed in [None, Some(0.0), Some(f64::NAN), Some(f64::INFINITY)] {
        r.devices[0].measured_bytes_per_second = speed;
        assert!(build_plan(&m, &r).is_err());
    }
    let mut m = m;
    m.kv_bytes_per_token_f16 = None;
    let mut r = request("proportional_usable", 1000, 1000);
    assert!(build_plan(&m, &r).is_err());
    r.kv_bytes_per_token_override = Some(40);
    assert!(build_plan(&m, &r).unwrap().fits);
}

#[test]
fn metal_cap_and_main_cpu_reservation_are_enforced() {
    let mut r = request("proportional_usable", 10000, 10000);
    r.devices[0].memory.device_limit_bytes = Some(219);
    r.devices[1].memory.device_limit_bytes = Some(320);
    assert!(!build_plan(&model(), &r).unwrap().fits);
    r.devices[0].memory.device_limit_bytes = None;
    assert!(build_plan(&model(), &r).is_err());
    let mut r = request("proportional_usable", 1000, 1000);
    r.devices[1].memory.unified_memory = false;
    assert!(!build_plan(&model(), &r).unwrap().fits);
    r.main_cpu_available_bytes = Some(40);
    assert!(build_plan(&model(), &r).unwrap().fits);
    r.main_cpu_available_bytes = Some(39);
    assert!(!build_plan(&model(), &r).unwrap().fits);
}

#[test]
fn tied_output_storage_is_charged_conservatively() {
    let mut m = model();
    m.non_layer_bytes.remove("output.weight");
    let p = build_plan(&m, &request("fastest_first", 440, 80)).unwrap();
    assert!(p.fits);
    assert_eq!(p.assignments[1].weight_bytes, 40);
    assert_eq!(p.assignments[1].headroom_bytes, Some(0));
    assert!(p.warnings.iter().any(|w| w.contains("Tied output")));
}

#[test]
fn kv_rounding_parallel_and_overflow_fail_closed() {
    let mut m = model();
    m.kv_bytes_per_token_f16 = Some(41);
    let mut r = request("proportional_usable", 220, 320);
    assert!(!build_plan(&m, &r).unwrap().fits);
    r.devices[0].memory.free_bytes = 10000;
    r.devices[0].memory.device_limit_bytes = Some(10000);
    r.devices[1].memory.free_bytes = 10000;
    r.devices[1].memory.device_limit_bytes = Some(10000);
    r.parallel = 2;
    r.ctx_size = 2;
    let p = build_plan(&m, &r).unwrap();
    assert_eq!(p.assignments.iter().map(|a| a.kv_bytes).sum::<u64>(), 176);
    r.kv_bytes_per_token_override = Some(u64::MAX);
    r.ctx_size = u32::MAX;
    assert!(build_plan(&m, &r).is_err());
}

#[test]
fn shared_model_adapter_checks_required_fields_and_widths() {
    let m = model();
    let shared = model_info(&m).unwrap();
    assert_eq!(shared.layer_bytes, m.layer_bytes);
    assert_eq!(shared.non_layer_bytes["token_embd.weight"], 40);
    let mut bad = m;
    bad.n_head = None;
    assert!(model_info(&bad).is_err());
    bad.n_head = Some(u64::MAX);
    assert!(model_info(&bad).is_err());
}

#[test]
fn all_small_partitions_match_an_independent_feasibility_oracle() {
    let mut m = model();
    m.n_layers = 3;
    m.kv_bytes_per_token_f16 = Some(3);
    m.non_layer_bytes = [("token_embd.weight".into(), 1), ("output.weight".into(), 2)].into();
    // Exhaust every 3-layer model with weights 1..3 and all two-device
    // capacities 0..14. The oracle enumerates every possible split boundary.
    for a in 1..=3 {
        for b in 1..=3 {
            for c in 1..=3 {
                m.layer_bytes = vec![a, b, c];
                for helper in 0..=14 {
                    for main in 0..=14 {
                        let expected = (0..=3).any(|k| {
                            let left: u64 = m.layer_bytes[..k].iter().sum::<u64>() + k as u64;
                            let right: u64 =
                                m.layer_bytes[k..].iter().sum::<u64>() + (3 - k) as u64 + 2 + 1;
                            left <= helper && right <= main
                        });
                        for strategy in ["proportional_usable", "fastest_first"] {
                            let p = build_plan(&m, &request(strategy, helper, main)).unwrap();
                            assert_eq!(
                                p.fits, expected,
                                "{:?}, {helper}/{main}, {strategy}",
                                m.layer_bytes
                            );
                            if p.fits {
                                assert_eq!(p.tensor_split.iter().sum::<f64>(), 4.0);
                                assert!(
                                    p.assignments.iter().all(|a| a.headroom_bytes.unwrap() >= 0)
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
