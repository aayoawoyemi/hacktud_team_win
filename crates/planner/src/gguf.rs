// CONCEPT: Seek-only GGUF v2/v3 inspection, including multipart models.
// WHY: R5.1 avoids loading model weights; offset spans include alignment padding
// conservatively (R5.3) and work for new tensor encodings without guessing bits.
// DEPENDS ON: https://github.com/ggml-org/ggml/blob/master/docs/gguf.md
// AFFECTS: Layer sizing and KV estimates; unknown architectures fail KV estimation.
use crate::{ModelInspection, Result, add, invalid, mul};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq)]
enum Value {
    UInt(u64),
    Text(String),
    Other,
}
struct Reader {
    file: File,
    len: u64,
    big: bool,
}
impl Reader {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut bytes = [0; N];
        self.file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.bytes()?;
        Ok(if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }
    fn u64(&mut self) -> Result<u64> {
        let b = self.bytes()?;
        Ok(if self.big {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        })
    }
    fn skip(&mut self, size: u64) -> Result<()> {
        let end = add(self.file.stream_position()?, size)?;
        if end > self.len {
            return Err(invalid("truncated GGUF metadata"));
        }
        self.file.seek(SeekFrom::Start(end))?;
        Ok(())
    }
    fn text(&mut self, keep: bool) -> Result<String> {
        let len = self.u64()?;
        if !keep {
            self.skip(len)?;
            return Ok(String::new());
        }
        if len > 1_048_576 || add(self.file.stream_position()?, len)? > self.len {
            return Err(invalid("invalid GGUF string length"));
        }
        let mut b = vec![0; len as usize];
        self.file.read_exact(&mut b)?;
        Ok(String::from_utf8(b)?)
    }
    fn value(&mut self, ty: u32, keep: bool, depth: usize) -> Result<Value> {
        if depth > 16 {
            return Err(invalid("metadata array nesting too deep"));
        }
        Ok(match ty {
            0 => Value::UInt(self.bytes::<1>()?[0] as u64),
            2 => {
                let b = self.bytes()?;
                Value::UInt(
                    (if self.big {
                        u16::from_be_bytes(b)
                    } else {
                        u16::from_le_bytes(b)
                    }) as u64,
                )
            }
            4 => Value::UInt(self.u32()? as u64),
            10 => Value::UInt(self.u64()?),
            8 => Value::Text(self.text(keep)?),
            9 => {
                let item = self.u32()?;
                let n = self.u64()?;
                if item > 12 || n > self.len {
                    return Err(invalid("invalid GGUF array"));
                }
                // Tokenizer arrays may be large. Skip values without allocating them.
                let width = match item {
                    0 | 1 | 7 => Some(1),
                    2 | 3 => Some(2),
                    4..=6 => Some(4),
                    10..=12 => Some(8),
                    _ => None,
                };
                if let Some(w) = width {
                    self.skip(mul(n, w)?)?;
                } else {
                    for _ in 0..n {
                        self.value(item, false, depth + 1)?;
                    }
                }
                Value::Other
            }
            1 => {
                let n = self.bytes::<1>()?[0] as i8;
                if n >= 0 {
                    Value::UInt(n as u64)
                } else {
                    Value::Other
                }
            }
            3 => {
                let b = self.bytes()?;
                let n = if self.big {
                    i16::from_be_bytes(b)
                } else {
                    i16::from_le_bytes(b)
                };
                if n >= 0 {
                    Value::UInt(n as u64)
                } else {
                    Value::Other
                }
            }
            5 => {
                let n = self.u32()? as i32;
                if n >= 0 {
                    Value::UInt(n as u64)
                } else {
                    Value::Other
                }
            }
            11 => {
                let n = self.u64()? as i64;
                if n >= 0 {
                    Value::UInt(n as u64)
                } else {
                    Value::Other
                }
            }
            7 => {
                if self.bytes::<1>()?[0] > 1 {
                    return Err(invalid("invalid boolean"));
                }
                Value::Other
            }
            6 => {
                self.skip(4)?;
                Value::Other
            }
            12 => {
                self.skip(8)?;
                Value::Other
            }
            _ => return Err(invalid(format!("unknown metadata type {ty}"))),
        })
    }
}
struct Part {
    version: u32,
    len: u64,
    data_start: u64,
    big: bool,
    meta: BTreeMap<String, Value>,
    tensors: Vec<(String, u64, Option<u64>)>,
}
fn number(meta: &BTreeMap<String, Value>, key: &str) -> Option<u64> {
    match meta.get(key) {
        Some(Value::UInt(v)) => Some(*v),
        _ => None,
    }
}
fn text(meta: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    match meta.get(key) {
        Some(Value::Text(v)) => Some(v.clone()),
        _ => None,
    }
}
fn read_part(path: &Path) -> Result<Part> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut r = Reader {
        file,
        len,
        big: false,
    };
    if r.bytes::<4>()? != *b"GGUF" {
        return Err(invalid("not a GGUF file"));
    }
    let version_bytes = r.bytes::<4>()?;
    r.big = !matches!(u32::from_le_bytes(version_bytes), 2 | 3);
    let version = if r.big {
        u32::from_be_bytes(version_bytes)
    } else {
        u32::from_le_bytes(version_bytes)
    };
    if !matches!(version, 2 | 3) {
        return Err(invalid("only GGUF v2/v3 supported"));
    }
    let count = r.u64()?;
    let keys = r.u64()?;
    if count > len / 24 || keys > len / 12 {
        return Err(invalid("invalid GGUF counts"));
    }
    let mut meta = BTreeMap::new();
    for _ in 0..keys {
        let key = r.text(true)?;
        let ty = r.u32()?;
        let keep = !key.starts_with("tokenizer.");
        let value = r.value(ty, keep, 0)?;
        if meta.insert(key, value).is_some() {
            return Err(invalid("duplicate metadata key"));
        }
    }
    let alignment = number(&meta, "general.alignment").unwrap_or(32);
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(invalid("invalid alignment"));
    }
    let mut tensors = Vec::new();
    for _ in 0..count {
        let name = r.text(true)?;
        let dims = r.u32()?;
        if name.len() > 64 || dims == 0 || dims > 4 {
            return Err(invalid("invalid tensor descriptor"));
        }
        let mut elements = 1;
        for _ in 0..dims {
            let dim = r.u64()?;
            if dim == 0 {
                return Err(invalid("zero tensor dimension"));
            }
            elements = mul(elements, dim)?;
        }
        let ggml_type = r.u32()?;
        // F32/F16 are unambiguous, so reject truncated spans for those types.
        // Quantized/unknown encodings still use conservative offset spans: this
        // reader does not claim to validate their encoded payload length.
        let minimum = match ggml_type {
            0 => Some(mul(elements, 4)?),
            1 => Some(mul(elements, 2)?),
            _ => None,
        };
        let offset = r.u64()?;
        if offset % alignment != 0 {
            return Err(invalid("unaligned tensor offset"));
        }
        tensors.push((name, offset, minimum));
    }
    let pos = r.file.stream_position()?;
    let data_start = add(pos, (alignment - pos % alignment) % alignment)?;
    if data_start > len {
        return Err(invalid("truncated tensor section"));
    }
    tensors.sort_by_key(|t| t.1);
    if let Some(first) = tensors.first() {
        if first.1 != 0 {
            return Err(invalid("first tensor must start at offset zero"));
        }
    } else if data_start != len {
        return Err(invalid("unexpected payload without tensors"));
    }
    Ok(Part {
        version,
        len,
        data_start,
        big: r.big,
        meta,
        tensors,
    })
}

// CONCEPT: Discover shards by the standard filename and verify split metadata.
// WHY: Missing shards must not produce a deceptively small capacity requirement.
// DEPENDS ON: llama.cpp split.no (zero based), split.count, split.tensors.count.
// AFFECTS: ModelInspection aggregates all shard file and tensor sizes.
fn paths(path: &Path, count: u64) -> Result<Vec<PathBuf>> {
    if count == 0 || count > 65535 {
        return Err(invalid("invalid split.count"));
    }
    let stem = path
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or_else(|| invalid("invalid shard filename"))?;
    // A shard-like filename without matching metadata must not silently become
    // a complete model. Even a one-shard split has a verifiable suffix.
    if count == 1 && !stem.contains("-of-") {
        return Ok(vec![path.to_owned()]);
    }
    let suffix = format!("-of-{count:05}");
    let head = stem
        .strip_suffix(&suffix)
        .ok_or_else(|| invalid("shard filename disagrees with split.count"))?;
    let (base, index) = head
        .rsplit_once('-')
        .ok_or_else(|| invalid("invalid shard filename"))?;
    if index.len() != 5
        || index
            .parse::<u64>()
            .ok()
            .filter(|i| *i > 0 && *i <= count)
            .is_none()
    {
        return Err(invalid("invalid shard index"));
    }
    Ok((1..=count)
        .map(|i| path.with_file_name(format!("{base}-{i:05}-of-{count:05}.gguf")))
        .collect())
}
pub fn read_model(path: impl AsRef<Path>) -> Result<ModelInspection> {
    let path = path.as_ref();
    let first = read_part(path)?;
    let files = paths(path, number(&first.meta, "split.count").unwrap_or(1))?;
    let arch =
        text(&first.meta, "general.architecture").ok_or_else(|| invalid("missing architecture"))?;
    let field = |suffix: &str| number(&first.meta, &format!("{arch}.{suffix}"));
    let n_layers = field("block_count").ok_or_else(|| invalid("missing block_count"))?;
    if n_layers == 0 || n_layers > 100_000 {
        return Err(invalid("invalid block_count"));
    }
    let mut model = ModelInspection {
        schema_version:1, model_id:path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
        path:path.to_string_lossy().into_owned(), file_bytes:0, metadata_bytes:0, split_files:files.len(), gguf_version:first.version,
        architecture:arch.clone(), name:text(&first.meta,"general.name"), file_type:number(&first.meta,"general.file_type"),
        n_layers, n_embd:field("embedding_length"), n_head:field("attention.head_count"),
        n_head_kv:field("attention.head_count_kv").or_else(||field("attention.head_count")),
        key_length:field("attention.key_length"), value_length:field("attention.value_length"),
        n_ctx_train:field("context_length"), expert_count:field("expert_count"), expert_used_count:field("expert_used_count"),
        layer_bytes:vec![0;n_layers as usize], non_layer_bytes:BTreeMap::new(), kv_bytes_per_token_f16:None,
        warnings:vec!["Tensor sizes use offset spans including alignment padding; model_id is provisional (O-16).".into()],
    };
    let head_dim = model
        .n_embd
        .zip(model.n_head)
        .filter(|(e, h)| *h > 0 && e % h == 0)
        .map(|(e, h)| e / h);
    model.key_length = model.key_length.or(head_dim);
    model.value_length = model.value_length.or(head_dim);
    // Restrict the simple formula to known full-attention architectures. Merely
    // having head counts does not prove a hybrid model allocates ordinary KV.
    let simple_arch = matches!(arch.as_str(), "llama" | "qwen2" | "qwen3");
    let special = first.meta.iter().any(|(key, value)| {
        key.starts_with(&format!("{arch}."))
            && (key.contains("sliding_window")
                || key.contains("ssm.")
                || ((key.contains("attention.head_count")
                    || key.ends_with("attention.key_length")
                    || key.ends_with("attention.value_length")
                    || key.ends_with("embedding_length"))
                    && !matches!(value, Value::UInt(_))))
    });
    if simple_arch
        && !special
        && let (Some(h), Some(k), Some(v)) = (model.n_head_kv, model.key_length, model.value_length)
        && h > 0
        && k > 0
        && v > 0
    {
        model.kv_bytes_per_token_f16 = Some(mul(mul(mul(n_layers, h)?, add(k, v)?)?, 2)?);
    }
    if model.kv_bytes_per_token_f16.is_none() {
        model.warnings.push("KV formula unsupported or metadata incomplete; supply a measured KV override to calculate fit.".into());
    }
    let mut names = BTreeSet::new();
    for (index, file) in files.iter().enumerate() {
        let part = read_part(file)?;
        if part.version != first.version
            || part.big != first.big
            || text(&part.meta, "general.architecture").as_deref() != Some(arch.as_str())
        {
            return Err(invalid("inconsistent shard model metadata"));
        }
        if files.len() > 1
            && (number(&part.meta, "split.no") != Some(index as u64)
                || number(&part.meta, "split.count") != Some(files.len() as u64))
        {
            return Err(invalid("inconsistent shard numbering"));
        }
        if files.len() == 1 && number(&part.meta, "split.no").is_some_and(|n| n != 0) {
            return Err(invalid("single-file split.no must be zero"));
        }
        // Compare scalar architecture fields in both directions, including KV
        // lengths and hybrid flags. Array values are skipped, so their contents
        // are not compared; per-layer KV arrays always disable the simple formula.
        let prefix = format!("{arch}.");
        for key in part.meta.keys().chain(first.meta.keys()) {
            if (key.starts_with(&prefix)
                || matches!(
                    key.as_str(),
                    "general.name" | "general.file_type" | "split.tensors.count"
                ))
                && part.meta.get(key) != first.meta.get(key)
            {
                return Err(invalid("inconsistent model metadata across shards"));
            }
        }
        model.file_bytes = add(model.file_bytes, part.len)?;
        model.metadata_bytes = add(model.metadata_bytes, part.data_start)?;
        for (i, (name, offset, minimum)) in part.tensors.iter().enumerate() {
            if !names.insert(name.clone()) {
                return Err(invalid("duplicate tensor name"));
            }
            let end = part
                .tensors
                .get(i + 1)
                .map(|t| t.1)
                .unwrap_or(part.len - part.data_start);
            if end <= *offset || end > part.len - part.data_start {
                return Err(invalid("invalid tensor offsets"));
            }
            let bytes = end - offset;
            if minimum.is_some_and(|n| n > bytes) {
                return Err(invalid("truncated tensor payload"));
            }
            if let Some(rest) = name.strip_prefix("blk.") {
                let layer = rest.split('.').next().unwrap_or("").parse::<usize>()?;
                let entry = model
                    .layer_bytes
                    .get_mut(layer)
                    .ok_or_else(|| invalid("layer index exceeds block_count"))?;
                *entry = add(*entry, bytes)?;
            } else {
                model.non_layer_bytes.insert(name.clone(), bytes);
            }
        }
    }
    if (files.len() > 1 || first.meta.contains_key("split.tensors.count"))
        && number(&first.meta, "split.tensors.count") != Some(names.len() as u64)
    {
        return Err(invalid("split tensor count mismatch"));
    }
    Ok(model)
}
