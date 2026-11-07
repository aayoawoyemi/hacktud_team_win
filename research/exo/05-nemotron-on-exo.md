# 05 Nemotron on EXO (replaces "what people say")

Code: exo main `a60b956` (2026-11-06). mlx-lm pinned by exo at `6b572ab1` (pyproject `[tool.uv.sources]`).
Model files checked on Hugging Face (`mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-6Bit` config, tokenizer and
chat template, fetched 2026-11-07).

## 0. Correction that changes the install plan: the app v1.0.71 is 6.5 months old

- The macOS app's Sparkle feed (`https://assets.exolabs.net/appcast.xml`, from `app/EXO/EXO/Info.plist`) and the
  GitHub releases API both list **1.0.71, published 2026-04-23**, as the newest app. No app release has been made
  since. The DMG is built only on `v*` tags (`.github/workflows/build-app.yml`).
- Everything Nemotron-relevant landed on main after that:
  - #2515 (merged 2026-11-01): Nemotron-H fixes for tensor and pipeline parallelism, plus the Nemotron 3.5 Lightning
    cards;
  - #2509 (2026-10-30): mlx-lm moved to upstream;
  - #2507: per-family model folders;
  - #2597 (2026-11-06): serving without the master.
  The `EXO_LIBP2P_NAMESPACE` deprecation in `main.py` `default_namespace()` suggests the libp2p→Zenoh switch is also
  newer than the app (not verified).
- **So "pin the app v1.0.71" is wrong for Nemotron.** Pin a source build of main (commit `a60b956` or a later
  commit we test) on all 3 Macs. Sections 1 to 4 describe main, not the app.
- Source build needs on every Mac: Xcode CLT, `brew install uv node`, Rust (rustup), `macmon` from the pinned fork
  (`cargo install --git https://github.com/vladkens/macmon ...`, README lines 210 to 217), then
  `cd dashboard && npm install && npm run build`, `uv sync` (compiles `exo_rs` with Rust), `uv run exo`.
  This needs internet and maybe 15 to 30 min per Mac (unmeasured), so do it **tonight**.
- A source build and the app don't join each other (README line 117), so all three must be source builds of the
  same commit.

## 1. Nemotron-H in EXO: files and what they do

`src/exo/worker/engines/mlx/models/nemotron_h/` (478 lines):
- `spec.py`: family `nemotron_h`, model types `nemotron_h`, `nemotron_h_omni`; tensor-parallel architecture
  `NemotronHForCausalLM`.
- `family.py`: wires the model class, `NemotronHShardingStrategy` (tensor) and `fix_pipeline_shard` (pipeline). The
  docstring lists Nemotron 3 Nano, Super, Ultra, 3.5 Lightning and Nano 9B v2.
- `model.py` (124): mlx-lm's `nemotron_h` model with the MoE router logits (`Float32RouterGate`) and the Mamba-2
  mixer conv/scan/gated norm (`Float32Mamba2Mixer`) in float32, as NVIDIA's reference `modeling_nemotron_h.py` does.
  Per #2515 this costs 3.8% decode speed on Lightning 4-bit.
- `sharding.py` (264): the tensor-parallel split. Per #2515 it slices every per-row quantization parameter of the
  Mamba `in_proj` together; before the fix, quantized TP produced garbage. It also splits the expert MLPs on
  64-unit quantization-group boundaries (1856 = 29 groups → 960 + 896). `fix_pipeline_shard`: a pipeline shard
  holding no attention layer, or no Mamba layer, gets `patch_hybrid_cache(...)` and `fa_idx`/`ssm_idx` set, so the
  attention mask and SSM cache work on that shard.
- Tests: `tests/test_pipeline.py` ("a shard without attention runs on restored SSM states"), `test_sharding.py`,
  `test_tensor_parallel.py`, on a tiny config.

### How pipeline sharding handles the Mamba layers

- Pipeline cuts the 52 blocks into contiguous ranges (generic `sharding/pipeline.py`). Mamba layers need no
  special handling across ranks, because their state is per layer and stays on the rank that owns the layer. Only
  the hidden state (2688 values, about 5 KB in bf16) moves between ranks per token.
- Two things were hybrid-specific and are fixed on main:
  - the cache/mask of a shard without attention layers (`fix_pipeline_shard`, #2515);
  - prefill snapshots of the SSM state (`generator/generate.py` line 381, `pre_gen = snapshots[-2] if has_ssm`).
- **Open risk:** PR #2264 (open since 2026-08-17, "pipeline sharding corruption for hybrid SSM/attention models",
  covers Qwen3.5/3.6, Kimi-Linear and Nemotron) says `snapshots[-2]` is wrong and should be `snapshots[-1]`, and lists
  4 other causes of corrupted output under 2-node pipeline. Main still has `snapshots[-2]`. I can't tell whether
  #2515 made those points moot. **Test on day 1:** compare the 3-Mac pipeline output with one-Mac output for the same
  prompt at temperature 0. Use the 4-bit 17.8 GB build on one M5 with `iogpu.wired_limit_mb` raised to about 19 GB.

### Layer layout (from the HF config)

`hybrid_override_pattern = MEMEM*EMEMEM*EMEMEM*EMEMEM*EMEMEM*EMEMEMEM*EMEMEMEME`
(M = Mamba-2, E = MoE, * = attention):

- **Layer counts:** 23 Mamba, 23 MoE, 6 attention.
- **Model shape:** hidden 2688, 128 routed experts (6 active) + 1 shared, vocab 131072, 2 kv heads.
- **Params:** 31.58 B total. My per-layer arithmetic reproduces this exactly.

Where the weight sits:
- **MoE layer:** about 1.30 B params, so the 23 MoE layers hold about 94% of the weights. At 6-bit that is about
  1.05 GB per MoE layer.
- **Mamba layer:** about 39 M params (about 32 MB at 6-bit).
- **Attention layer:** about 23 M params.
- **Embeddings and lm_head:** about 0.35 B each.

EXO's placement assumes every layer weighs the same (`resident_size × layers / n_layers`,
`placement_utils.py` line 96). Because E and M alternate, any range of k layers holds about k/2 MoE layers (±1).
The real per-node memory is therefore within about ±1 GB of EXO's estimate. That matters on the M3.

### Cache memory is tiny (good for agent prompts)

- **Mamba state per sequence:** 64 heads × 64 dim × 128 state × 4 B ≈ 2 MB per Mamba layer, so about 48 MB for
  23 layers, whatever the context length.
- **KV cache:** only 6 attention layers × 2 kv heads × 128 × 2 (k, v) × 2 B ≈ 6 KB per token. 8k tokens ≈ 50 MB;
  32k ≈ 200 MB.

A long agent context (tool outputs, code files) costs very little memory. This is a real advantage over Qwen3 MoE
for our 7 GB M3.

## 2. 3-way pipeline on M5 24 / M3 16 / M5 24: per-node memory

The method is EXO's allocator (proportional to usable memory, largest remainder, min 1 layer). Real GB counts which
blocks fall in each range, plus embed on rank 0 and lm_head on the last rank.

Budgets:
- **A (apps open, assumed):** 12 / 7 / 12 GB.
- **B (apps quit, default Metal limit):** 16 / 10.7 / 16 GB.

The two orders are M3 in the middle and M3 first; EXO's ring order depends on the topology cycle.

| Build (EXO card id `mlx-community/...`) | Size | A: layers M5a / M3 / M5b | A: real GB (EXO est.) | B: real GB |
|---|---|---|---|---|
| `NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-MXFP4` | 16.8 | 20 / 12 / 20 | 5.9 / 4.3 / 6.6 (6.5 / 3.9 / 6.5) | 5.9 / 4.3 / 6.6 |
| `NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-4Bit` | 17.8 | 20 / 12 / 20 | 6.3 / 4.5 / 7.0 | 6.3 / 4.5 / 7.0 |
| `NVIDIA-Nemotron-3.5-Lightning-30B-A3B-4bit` | 17.8 | 20 / 12 / 20 | same as above | same |
| `NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-5Bit` | 21.7 | 20 / 12 / 20 | 7.7 / 5.5 / 8.5 | 7.7 / 5.5 / 8.5 |
| `NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-6Bit` | 25.7 | 20 / 12 / 20 | 9.1 / **6.5 of 7** / 10.1 (M3 first: 9.8 / 5.8 / 10.1) | 9.1 / 6.5 / 10.1 |
| `NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-8Bit` | 33.6 | doesn't fit (31) | n/a | 11.9 / 8.5 / 13.2 (≈2 to 3 GB headroom each) |
| `NVIDIA-Nemotron-3.5-Lightning-30B-A3B-8bit` | 33.6 | doesn't fit | n/a | same as 8-bit above |

Reading the table:
- **6-bit (25.7 GB)** is larger than any one MacBook's RAM (24 GB), so it is the strongest "needs the pool" claim.
  It fits all three with apps open, but the M3 is at 6.5 of 7 GB in the M3-in-the-middle order. Quit apps on the M3,
  or raise its wired limit to about 11 GB (`sudo sysctl iogpu.wired_limit_mb=11264`).
- **2-node behaviour:** with apps quit, 2 M5s (32 GB) also hold the 6-bit build, so `min_nodes=3` is needed to use
  the M3. With apps open (24 GB) it needs all three.
- **5-bit (21.7 GB)** is the safe fallback: it is still above one Mac's default GPU budget, and the M3 holds 5.5 GB.
- **Lightning 4-bit vs 3 Nano 4-bit:** same 17.8 GB and same pattern. Lightning is newer (Aug 2026) and has the
  thinking toggle in its card. There is no Lightning 5- or 6-bit card in EXO.

### Speed (estimate only, unmeasured)

- **Active params:** about 3.2 B per token, so a 6-bit build reads about 2.6 GB per token. Split ~38/24/38% over
  153/100/153 GB/s, the bandwidth floor is about 6.5 + 6 + 6.5 ≈ 19 ms.
- **Kernel overhead:** #2526 (open) measured Lightning 4-bit on an M3 Ultra at 6.9 ms/token against a 2.8 ms floor
  (~1,000 small GPU ops per token). That PR, not yet merged, fuses them.
- **Estimate:** applying a similar 2 to 2.5× overhead plus 3 Ethernet hops gives roughly 15 to 25 tokens/s for 6-bit
  and 20 to 30 for 4-bit. Measure.
- **Community data points:**
  - #2196: 3-node TCP pipeline, Mac Studios, Qwen3-Coder-Next-5bit at ~33 tok/s.
  - #2264: 2-node TB5, Qwen3.6-35B-A3B at ~71 tok/s.
  - Nemotron-3-Nano bf16 (55 GiB) on 3-node pipeline failed with 111 s to first token in #2196 (bf16 only;
    quantized was fine). Don't use bf16.

## 3. Tool calling

- **API:** `ChatCompletionRequest` accepts `tools`, `tool_choice` and `parallel_tool_calls`
  (`src/exo/api/types/api.py` lines 225 to 251). Messages accept role `tool` with `tool_call_id` (line 89 and 96).
  The response returns OpenAI `tool_calls` deltas with `finish_reason: "tool_calls"`
  (`api/adapters/chat_completions.py` lines 258 to 271, 349 to 355). Tools are passed to the chat template
  (`utils_mlx.py` `render_chat_template`, ~line 640).
- **`tool_choice` and `parallel_tool_calls` are accepted but ignored.** They appear only in the request type, not
  in the worker. So `tool_choice: "required"` won't force a call. The agent loop must handle a turn with no tool call.
- **Parser choice:** `builder.py` line 92 uses mlx-lm's `TokenizerWrapper.tool_parser`. mlx-lm `_infer_tool_parser()`
  (`mlx_lm/tokenizer_utils.py` line 652 at `6b572ab1`) picks **`qwen3_coder`** when the template contains
  `<tool_call>\n<function=`.
  - **Nemotron 3 Nano and 3.5 Lightning:** their templates render exactly
    `'<tool_call>\n<function=' ~ tool_call.name ~ '>\n'`, with `<parameter=...>` XML (checked in the HF
    `chat_template.jinja`), so they get the qwen3_coder parser.
  - **Single tokens:** `<tool_call>` and `</tool_call>` are single special tokens (ids 14 and 15 in `tokenizer.json`),
    which EXO's streaming detector needs: `model_output_parsers.py` line 229 checks
    `response.text.startswith("<tool_call>")`.
  - **Arguments:** they are coerced to the tool's JSON schema (`tool_parsers.py`
    `_coerce_tool_calls_to_schema`).
  - **Parse failure:** the text comes back as plain content, not as an error (`model_output_parsers.py` line 258).
- **Nemotron-Nano-9B-v2 (the routing model) has no parser in EXO.** Its template uses
  `<TOOLCALL>[{"name":..., "arguments":...}]</TOOLCALL>`; mlx-lm at this rev has no matching parser (its
  `tool_parsers/` has 15 parsers, none for TOOLCALL). Tool calls from the 9B would arrive as raw text. Either send all
  agent tool turns to the pooled 30B, or regex-parse `<TOOLCALL>` in our agent code.
- Nemotron is not in EXO's tool-call eval configs (`bench/eval_configs/models.toml` has only Cascade 2 and 3 Super),
  and there are no Nemotron tool-parsing tests (`src/exo/api/tests/` has GLM and Claude ones). **Untested in EXO.**
  Test one tool call on day 1. Fallback: `bench/eval_tool_calls.py` against our endpoint.

## 4. Thinking toggle

- The request field `enable_thinking: bool` is on `ChatCompletionRequest` (line 246). `reasoning_effort` is also
  accepted.
- `render_chat_template()` (`utils_mlx.py` lines 623 to 627) passes `enable_thinking` (and `thinking`) to the
  template **for any model**, not gated by the card. Both Nemotron 3 Nano and 3.5 Lightning templates read
  `enable_thinking` and use `<think>`/`</think>` (single tokens 12 and 13). So the toggle works for both. The card
  flag `thinking_toggle` (Lightning only) mainly affects the dashboard (unverified).
- Reasoning text streams as `reasoning_content` in the delta (`chat_completions.py` line 201). Lightning's card
  sets `reasoning_dialect = "post_last_user"`: earlier turns' reasoning is dropped from the prompt, which suits a
  multi-step agent.
- Agent tip: `enable_thinking: true` for the planning step, `false` for tool-execution turns (faster).
  NVIDIA sampling defaults from the card: temperature 1.0, top_p 0.95. Use lower temperature for tool turns.

## 5. Nemotron GitHub issues/PRs (GitHub search "nemotron", 2026-11-07)

| # | State | Date | What |
|---|---|---|---|
| 2515 | merged | 2026-11-01 | TP garbage on quantized, TP load failure (1856 split), pipeline crash on attention-free shard; Lightning/Ultra cards; float32 router/Mamba |
| 2526 | open | 2026-10-31 | fused float32 Mamba-2 and MoE Metal kernels (6.9 ms vs 2.8 ms floor on M3 Ultra); would speed us up, not merged |
| 2521 / 2525 | open | 2026-10-30/31 | pipeline: build decode graph early; pipelined prefill |
| 2264 | open | 2026-08-17 | hybrid SSM pipeline corruption (5 causes); main still has `snapshots[-2]` |
| 2196 | open | 2026-07-02 | bf16 fails on 3-node pipeline (Nemotron-3-Nano bf16 111 s to first token); quantized fine |
| 2208 | open | 2026-07-08 | long-context hangs on 2-node Tensor/RDMA (not our mode) |
| 1821 | open | 2026-03-31 | asymmetric tensor parallelism for heterogeneous clusters (not merged) |
| 2229 | open | 2026-07-21 | "bandwidth-aware placement" + token relay, 10 to 40% faster pipeline decode (not merged) |
| 1766 | closed | 2026-03-21 | runner crash on M4 (Metal 4) bfloat16 shader; check that M5 is fine on main |

## 6. Community survey (short)

Skipped for time beyond the issue data above. Relevant field numbers: 3 × Mac Studio TCP pipeline about 33 tok/s on a
51 GiB 5-bit MoE (#2196); 2 × Mac TB5 Qwen3.6-35B-A3B about 71 tok/s (#2264). No MacBook-over-Ethernet Nemotron
numbers found.
