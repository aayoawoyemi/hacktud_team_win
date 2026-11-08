<!-- CONCEPT: WP-5 workspace handoff and verification instructions.
WHY: D-05/D-13; inspect locally without inference or a control-plane dependency.
DEPENDS ON: notes:PRD.md sections 4.4, 6.4, WP-5; Rust and Cargo.
AFFECTS: WP-2 integration and WP-11 model selection. -->
# Planner: GGUF reader and fit calculator

**Current follow-up:** [FOLLOWUP.md](FOLLOWUP.md) records the #11 suggestions,
shared ModelInfo adapter, three SplitPlan strategies, CLI examples and current
validation limits after rebasing onto #18. The handoff below is historical;
its deferred SplitPlan and unpublished-schema statements are superseded.

Scope: R5.1–R5.6 and R5.13–R5.15. Split strategies, SplitPlan output,
speed prediction, and buffer calibration are deferred as requested.

```sh
cargo +stable test -p planner --frozen
cargo +stable run -p planner --frozen -- inspect /path/to/model.gguf
cargo +stable run -p planner --frozen -- fit /path/to/model.gguf crates/planner/machines.example.json 8192
```

Optional fit arguments: parallel contexts (default 1), followed by an explicit
measured total-model KV bytes/token override. All memory quantities are bytes.
Example memory numbers are illustrative, not calibrated defaults.

The reader opens model files but reads only headers, metadata, and tensor
descriptors. Tokenizer array contents are skipped. Tensor sizes use sorted
offset spans, including alignment padding. Metadata/header bytes are excluded
from weight capacity, while `file_bytes` includes the entire file(s).
The final tensor span is conservatively charged through EOF; this method
cannot independently identify trailing junk or validate tensor payloads.

Multipart models are discovered beside the supplied shard using the standard
five-digit filenames. All shards must exist; indices, counts, architecture
fields, duplicate tensor names, and total tensor count are checked.

Simple f16 KV is estimated for `llama`, `qwen2`, and `qwen3` full-attention
metadata. Unknown architectures, per-layer head arrays, sliding windows,
or SSM metadata produce null KV plus warnings, and require an explicit override
to calculate fit. This prevents reporting unknown cache memory as zero.

Represent a unified-memory machine once, with one `memory_pool_id`; repeated
pool ids are rejected. Apply the GPU limit as `device_limit_bytes`, never add
system and GPU memory together. Reserves and compute buffers are caller inputs.

For unified memory set `unified_memory: true`; a missing GPU budget is an error.
This flag is required even for discrete memory (`false`). The caller must consolidate
CPU and GPU entries sharing RAM, and supply the measured Metal budget, not total RAM.

Pool fit is an aggregate capacity estimate, not a verified allocation plan.
Reverse fit assumes this model's architecture and KV allocation remain fixed.
The ratio uses tensor storage bytes divided by the largest single machine's
remaining weight capacity after its full cache and overhead.

## Integration status

Integrated under `crates/planner` on team main commit
`8006aaa19e449d7a1cdd73e1bca563c5c833a7a6`. Package metadata and serde dependencies
inherit the workspace settings. Root manifests and other WPs are untouched; Cargo.lock records the generated dependency resolution.
The original standalone repo and commit a0587d3 remain unchanged.

`ModelInspection` is a local inspection result, **not** the shared `ModelInfo`
contract or a SpacetimeDB table. Main's contracts crate is empty; D-13 supersedes
PR #18's gRPC schema. WP-2 must provide the schema before an adapter can be built.
The inspect/fit binary is a development utility. O-02 now selects one team CLI
(`pool` with subcommands); its hooks belong in the shared CLI integration PR. Strategies, SplitPlan, telemetry and calibration
remain outside this first handoff.

`metadata_bytes` includes each shard's header, metadata, descriptors and initial
alignment. It plus the tensor spans equals `file_bytes`. F32 and F16 spans are
checked against their dimensions. Quantized/unknown payload lengths, final
padding/trailing junk and architecture array contents across shards are not
independently validated. Offset gaps are conservative storage estimates.

GGUF specification: https://github.com/ggml-org/ggml/blob/master/docs/gguf.md

## Verification status

See [VALIDATION.md](VALIDATION.md) for actual attempts and the hand calculation.
Full Cargo validation passes on Rust 1.99.1 (current PRD O-14b): formatting,
workspace build, workspace tests (17 passed) and strict planner Clippy. Built CLI
smoke checks pass for inspect JSON, fit table/ratio and invalid-command handling.
Cargo.lock is regenerated and committed; dependencies are cached with a separate
local vendor backup, allowing subsequent checks with `--frozen`.

On this base use `cargo +stable` to override the old root toolchain pin. This Mac
also needs the existing SDK workaround described in VALIDATION.md. No real GGUF
has been inspected; real-model validation, WP-2's shared-type adapter and WP-1
buffer calibration remain pending.

Small real-file candidates from the original handoff (not downloaded):

- https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf
- https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q8_0.gguf
