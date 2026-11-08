// CONCEPT: Small synthetic GGUF fixtures and capacity boundary tests.
// WHY: WP-5 can be exercised without main models or llama.cpp (D-05).
// DEPENDS ON: Public planner API; GGUF v3 scalar/array and tensor descriptors.
// AFFECTS: Reader bounds checks, shard completeness, and fit arithmetic.
use planner::{
    capacity::{Machine, fit, usable_bytes},
    gguf::read_model,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "planner-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn write(&self, name: &str, b: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, b).unwrap();
        p
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn s(v: &str) -> Vec<u8> {
    let mut b = (v.len() as u64).to_le_bytes().to_vec();
    b.extend(v.as_bytes());
    b
}
fn kv(key: &str, ty: u32, val: &[u8]) -> Vec<u8> {
    let mut b = s(key);
    b.extend(ty.to_le_bytes());
    b.extend(val);
    b
}
fn fixture(arch: &str, shard: Option<u64>, extra: Vec<Vec<u8>>) -> Vec<u8> {
    let mut meta = vec![kv("general.architecture", 8, &s(arch))];
    for (key, n) in [
        ("block_count", 2u64),
        ("embedding_length", 256),
        ("attention.head_count", 4),
        ("attention.head_count_kv", 2),
    ] {
        meta.push(kv(&format!("{arch}.{key}"), 10, &n.to_le_bytes()));
    }
    if let Some(i) = shard {
        meta.push(kv("split.no", 10, &i.to_le_bytes()));
        meta.push(kv("split.count", 10, &2u64.to_le_bytes()));
        meta.push(kv("split.tensors.count", 5, &4i32.to_le_bytes()));
    }
    meta.extend(extra);
    let names = if let Some(i) = shard {
        vec![format!("blk.{i}.weight"), format!("extra.{i}")]
    } else {
        vec!["blk.0.weight".into(), "token_embd.weight".into()]
    };
    let mut b = b"GGUF".to_vec();
    b.extend(3u32.to_le_bytes());
    b.extend(2u64.to_le_bytes());
    b.extend((meta.len() as u64).to_le_bytes());
    for m in meta {
        b.extend(m);
    }
    for (i, name) in names.iter().enumerate() {
        b.extend(s(name));
        b.extend(1u32.to_le_bytes());
        b.extend(8u64.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend((i as u64 * 32).to_le_bytes());
    }
    b.resize(b.len().div_ceil(32) * 32, 0);
    b.extend([0u8; 64]);
    b
}
fn machine(id: &str, free: u64) -> Machine {
    Machine {
        name: id.into(),
        memory_pool_id: id.into(),
        unified_memory: false,
        free_bytes: free,
        device_limit_bytes: None,
        reserve_bytes: 0,
        compute_buffer_bytes: 0,
    }
}

#[test]
fn metadata_layers_and_kv() {
    let t = Temp::new();
    let p = t.write("tiny.gguf", &fixture("llama", None, vec![]));
    let m = read_model(p).unwrap();
    assert_eq!(m.layer_bytes, vec![32, 0]);
    assert_eq!(m.non_layer_bytes["token_embd.weight"], 32);
    assert_eq!(m.kv_bytes_per_token_f16, Some(1024));
    let json = serde_json::to_value(m).unwrap();
    assert_eq!(json["schema_version"], 1);
}
#[test]
fn multiple_architectures() {
    let t = Temp::new();
    for arch in ["qwen2", "qwen3"] {
        let p = t.write(&format!("{arch}.gguf"), &fixture(arch, None, vec![]));
        let m = read_model(p).unwrap();
        assert_eq!(m.architecture, arch);
        assert_eq!(m.kv_bytes_per_token_f16, Some(1024));
    }
}
#[test]
fn arrays_skip_without_losing_tensor_table() {
    let mut array = 8u32.to_le_bytes().to_vec();
    array.extend(2u64.to_le_bytes());
    array.extend(s("a"));
    array.extend(s("b"));
    let t = Temp::new();
    let p = t.write(
        "array.gguf",
        &fixture("llama", None, vec![kv("tokenizer.ggml.tokens", 9, &array)]),
    );
    assert_eq!(read_model(p).unwrap().layer_bytes[0], 32);
}
#[test]
fn hybrid_requires_override() {
    let t = Temp::new();
    let p = t.write(
        "hybrid.gguf",
        &fixture(
            "llama",
            None,
            vec![kv(
                "llama.attention.sliding_window",
                10,
                &4096u64.to_le_bytes(),
            )],
        ),
    );
    let m = read_model(p).unwrap();
    assert_eq!(m.kv_bytes_per_token_f16, None);
    assert!(fit(&m, &[machine("a", 10_000)], 1, 1, None).is_err());
    assert!(fit(&m, &[machine("a", 10_000)], 1, 1, Some(1024)).is_ok());
}
#[test]
fn shards_sum_and_missing_shard_rejected() {
    let t = Temp::new();
    let a = t.write(
        "tiny-00001-of-00002.gguf",
        &fixture("qwen2", Some(0), vec![]),
    );
    assert!(read_model(&a).is_err());
    let b = t.write(
        "tiny-00002-of-00002.gguf",
        &fixture("qwen2", Some(1), vec![]),
    );
    let m = read_model(&b).unwrap();
    assert_eq!(m.split_files, 2);
    assert_eq!(m.layer_bytes, vec![32, 32]);
    assert_eq!(
        m.file_bytes,
        std::fs::metadata(a).unwrap().len() + std::fs::metadata(b).unwrap().len()
    );
}
#[test]
fn truncated_metadata_is_rejected() {
    let t = Temp::new();
    let mut b = fixture("llama", None, vec![]);
    b.truncate(40);
    assert!(read_model(t.write("bad.gguf", &b)).is_err());
}
#[test]
fn bad_magic_and_version_are_rejected() {
    let t = Temp::new();
    let mut b = fixture("llama", None, vec![]);
    b[0] = 0;
    assert!(read_model(t.write("magic.gguf", &b)).is_err());
    b[0] = b'G';
    b[4..8].copy_from_slice(&99u32.to_le_bytes());
    assert!(read_model(t.write("version.gguf", &b)).is_err());
}
#[test]
fn capacity_boundaries_and_reverse() {
    let t = Temp::new();
    let m = read_model(t.write("tiny.gguf", &fixture("llama", None, vec![]))).unwrap();
    let exact = fit(&m, &[machine("a", 1088)], 1, 1, None).unwrap();
    assert!(exact.machines[0].fits);
    assert_eq!(exact.machines[0].largest_model_bytes, 64);
    assert_eq!(exact.model_to_largest_machine_ratio, Some(1.0));
    assert!(!fit(&m, &[machine("a", 1087)], 1, 1, None).unwrap().machines[0].fits);
    let pooled = fit(&m, &[machine("a", 600), machine("b", 600)], 1, 1, None).unwrap();
    assert!(pooled.pool.fits);
    assert!(!pooled.machines[0].fits);
    assert_eq!(pooled.pool.largest_model_bytes, 176);
}
#[test]
fn unified_memory_limit_and_saturation() {
    let mut m = machine("unified", 2000);
    m.device_limit_bytes = Some(1000);
    m.reserve_bytes = 100;
    m.compute_buffer_bytes = 200;
    assert_eq!(usable_bytes(&m, 300), 400);
    assert_eq!(usable_bytes(&m, 2000), 0);
}
#[test]
fn duplicate_memory_and_overflow_are_rejected() {
    let t = Temp::new();
    let m = read_model(t.write("tiny.gguf", &fixture("llama", None, vec![]))).unwrap();
    assert!(
        fit(
            &m,
            &[machine("same", 2000), machine("same", 2000)],
            1,
            1,
            None
        )
        .is_err()
    );
    assert!(fit(&m, &[machine("a", 2000)], u64::MAX, 2, None).is_err());
}

// CONCEPT: Regression coverage for review findings during workspace integration.
// WHY: R5.3/R5.4/R5.6 must fail closed on incomplete models and GPU budgets.
// DEPENDS ON: Synthetic files above; no inference runtime or model download.
// AFFECTS: File accounting, shard consistency, and usable-memory safeguards.
#[test]
fn metadata_and_tensor_spans_account_for_entire_file() {
    let t = Temp::new();
    let m = read_model(t.write("tiny.gguf", &fixture("llama", None, vec![]))).unwrap();
    let weights: u64 = m.layer_bytes.iter().chain(m.non_layer_bytes.values()).sum();
    assert_eq!(m.metadata_bytes + weights, m.file_bytes);
}

#[test]
fn unified_memory_requires_a_gpu_budget() {
    let t = Temp::new();
    let m = read_model(t.write("tiny.gguf", &fixture("llama", None, vec![]))).unwrap();
    let mut mac = machine("mac", 4096);
    mac.unified_memory = true;
    assert_eq!(usable_bytes(&mac, 0), 0);
    assert!(fit(&m, &[mac.clone()], 1, 1, None).is_err());
    mac.device_limit_bytes = Some(2048);
    assert!(fit(&m, &[mac], 1, 1, None).unwrap().machines[0].fits);
}

#[test]
fn truncated_float_tensor_is_rejected() {
    let t = Temp::new();
    let mut bytes = fixture("llama", None, vec![]);
    bytes.pop();
    assert!(read_model(t.write("short.gguf", &bytes)).is_err());
}

#[test]
fn shard_filename_without_matching_count_is_rejected() {
    let t = Temp::new();
    let bytes = fixture("llama", None, vec![]);
    assert!(read_model(t.write("tiny-00001-of-00002.gguf", &bytes)).is_err());
}

#[test]
fn shard_kv_lengths_must_agree() {
    let t = Temp::new();
    let a = t.write(
        "tiny-00001-of-00002.gguf",
        &fixture(
            "llama",
            Some(0),
            vec![kv("llama.attention.key_length", 10, &64u64.to_le_bytes())],
        ),
    );
    t.write(
        "tiny-00002-of-00002.gguf",
        &fixture(
            "llama",
            Some(1),
            vec![kv("llama.attention.key_length", 10, &128u64.to_le_bytes())],
        ),
    );
    assert!(read_model(a).is_err());
}

#[test]
fn per_layer_key_lengths_disable_simple_kv() {
    let t = Temp::new();
    let mut array = 10u32.to_le_bytes().to_vec();
    array.extend(2u64.to_le_bytes());
    array.extend(64u64.to_le_bytes());
    array.extend(128u64.to_le_bytes());
    let m = read_model(t.write(
        "array.gguf",
        &fixture(
            "llama",
            None,
            vec![kv("llama.attention.key_length", 9, &array)],
        ),
    ))
    .unwrap();
    assert_eq!(m.kv_bytes_per_token_f16, None);
}

#[test]
fn big_endian_v3_metadata_and_offsets_are_supported() {
    fn be_string(value: &str) -> Vec<u8> {
        let mut bytes = (value.len() as u64).to_be_bytes().to_vec();
        bytes.extend(value.as_bytes());
        bytes
    }
    let mut bytes = b"GGUF".to_vec();
    bytes.extend(3u32.to_be_bytes());
    bytes.extend(1u64.to_be_bytes());
    bytes.extend(2u64.to_be_bytes());
    bytes.extend(be_string("general.architecture"));
    bytes.extend(8u32.to_be_bytes());
    bytes.extend(be_string("llama"));
    bytes.extend(be_string("llama.block_count"));
    bytes.extend(4u32.to_be_bytes());
    bytes.extend(1u32.to_be_bytes());
    bytes.extend(be_string("blk.0.weight"));
    bytes.extend(1u32.to_be_bytes());
    bytes.extend(8u64.to_be_bytes());
    bytes.extend(0u32.to_be_bytes());
    bytes.extend(0u64.to_be_bytes());
    bytes.resize(bytes.len().div_ceil(32) * 32, 0);
    bytes.extend([0u8; 32]);
    let t = Temp::new();
    let m = read_model(t.write("big.gguf", &bytes)).unwrap();
    assert_eq!(m.layer_bytes, vec![32]);
    assert_eq!(m.metadata_bytes + 32, m.file_bytes);
}
