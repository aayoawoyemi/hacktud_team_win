# cppcode 02: Nemotron-H over RPC

Worker `cppcode`. Sat Nov 7 2026. Source `llama-src` commit `e9a2a44`. `RPC.cpp` = `ggml/src/ggml-rpc/ggml-rpc.cpp`.
cppneed has only section 01 so far (read before 01); nothing new to reconcile. PRD unchanged since 4:06pm.

## 0. Model facts, from the config (not memory)

Source: `huggingface.co/nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16/raw/main/config.json` (fetched Nov 7 2026).

| Field | Value |
|---|---|
| hidden_size (n_embd) | **2688** (confirmed; 01's guess was right) |
| num_hidden_layers | 52 |
| hybrid_override_pattern | `MEMEM*EMEMEM*EMEMEM*EMEMEM*EMEMEM*EMEMEMEM*EMEMEMEME` = 23 M (Mamba-2), 23 E (MoE), 6 `*` (attention) |
| Mamba-2 | 64 heads x head_dim 64 = d_inner 4096, ssm_state_size 128, n_groups 8, conv_kernel 4, state dtype float32 |
| Attention | 32 Q heads, 2 KV heads, head_dim 128, no RoPE in the llama.cpp graph (no rope call in `src/models/nemotron-h.cpp:276-289`) |
| MoE | 128 routed experts, 6 used, 1 shared; expert ffn 1856, shared 3712, relu^2 activation |
| vocab | 131072 |

GGUF arch string is `nemotron_h_moe` (`src/llama-arch.cpp:105`; confirmed by GGUF metadata quoted in issue #27141).

Derived numbers (my arithmetic):
- Per-hop activation (decode): 2688 x 4 B = **10.5 KiB**. Prefill 512-token ubatch: 5.25 MiB.
- Weight share per layer type (params): MoE layer ~1.30 B (2 x 128 x 1856 x 2688 + shared 2 x 3712 x 2688);
  Mamba layer ~39 M; attention layer ~23 M. MoE layers hold ~97% of the bytes.
- Recurrent state per sequence slot per Mamba layer: SSM state 128 x 4096 x 4 B = 2 MiB; conv state
  3 x (4096 + 2x8x128) x 4 B = 72 KiB. 23 layers = **~48 MiB per slot** (`--parallel 4` -> ~190 MiB total, split by
  layer across Macs).
- KV cache: 6 layers x 2 heads x 128 x (K+V) x 2 B (f16) = 6 KiB per token; 32k context = 192 MiB per slot.

## 1. Code path

| What | Where |
|---|---|
| Arch class: `nemotron_h_moe` reuses `nemotron_h`'s graph | `src/models/models.h:1728-1732` (`using graph = llama_model_nemotron_h::graph`) |
| Main graph: per layer, `is_recr(il)` -> Mamba-2, `n_ff(il)==0` -> attention, else FFN/MoE | `src/models/nemotron-h.cpp:202-273` |
| Mamba-2 layer builder | `src/models/mamba-base.cpp` ~`:160-292` (`build_mamba2_layer`) |
| Memory type: hybrid (attention KV + recurrent) | `src/llama-arch.cpp:1352-1362` (`llm_arch_is_hybrid` lists NEMOTRON_H, NEMOTRON_H_MOE); filters `src/llama-model.cpp:3303-3310` |
| Recurrent cache | `src/llama-memory-recurrent.cpp:78-123` |
| KV cache | `src/llama-kv-cache.cpp:216-217` |
| MTP draft head (only with speculative MTP) | `src/models/nemotron-h-moe.cpp:11-158` |

Ops used by a Mamba-2 layer (`mamba-base.cpp`): MUL_MAT (ssm_in, ssm_out), RESHAPE/VIEW/TRANSPOSE/CONT, CONCAT
(`:205`), CPY into the state cache (`:206` via `build_conv_state_store`, `:260-264`), **SSM_CONV** (`:217`), ADD, SILU
(Metal fuses SSM_CONV+SILU, `ggml-metal-fusion.cpp:177-189`), **SSM_SCAN** with state-row ids and K (`:250`),
GET_ROWS (state gather in `build_rs`), MUL, SWIGLU split (`:274`), RMS_NORM grouped (`:278-280`).
MoE layer: MUL_MAT for router, MUL_MAT_ID for experts, RELU+SQR (relu^2), sigmoid gating, ADD.

## 2. Key question A: does the recurrent state live on the device that owns the layer?

**Yes.** `llama_memory_recurrent` constructor, per layer `i` (`src/llama-memory-recurrent.cpp:84-93`):

```
buft = ggml_backend_cpu_buffer_type();
if (offload) {
    auto * dev = model.dev_layer(i);
    buft = ggml_backend_dev_buffer_type(dev);
```

`model.dev_layer(i)` is the same device the layer's weights were assigned to (`src/llama-model.cpp:1788-1790`). For a
layer on a remote Mac that device is the RPC device, and its buffer type is `RPC<dev>[host:port]`
(`RPC.cpp:3048-3054`), so `cache_r_l<i>` and `cache_s_l<i>` are allocated **in the remote Mac's Metal memory** via
ALLOC_BUFFER. The KV cache for the 6 attention layers follows the same rule (`src/llama-kv-cache.cpp:216-217`).

- `offload` = `cparams.offload_kqv` (the `llama_memory_hybrid` constructor gets `cparams.offload_kqv`, `src/llama-model.cpp:3377-3391`), default on. **Do not pass
  `--no-kv-offload` / `-nkvo`**: then all state and KV go to the head's CPU, and every remote Mamba/attention layer
  would read and write state across the network each token.
- Per token, the state update is a CPY inside the remote graph (`mamba-base.cpp:260-264`); it never crosses the
  network. The only per-token extras for the remote are small user inputs set with one-way SET_TENSOR: `s_copy`
  (int32 row ids, `n_rs` entries), `inp_out_ids`, and the KQ mask if that remote holds an attention layer.
- The scheduler pins ops whose destination is a pre-allocated buffer to that buffer's backend, so the state CPY runs
  where the state lives (scheduler pass 1, `ggml-backend.cpp` "assign backends to ops with pre-allocated inputs",
  `:1503`).

## 3. Key question B: are SSM_SCAN and SSM_CONV supported on RPC + Metal?

Two layers of "supported":

1. **RPC client says yes to every op.** `ggml_backend_rpc_device_supports_op` returns `true` for all ops except
   non-truncated ROPE (`RPC.cpp:3056-3064`, with "TODO: call the remote backend and cache the results"). So the
   scheduler never moves an op off a remote device. If the remote backend could not run an op, the server would abort
   (`GGML_ASSERT(status == GGML_STATUS_SUCCESS ...)`, `RPC.cpp:2206`), and the client would then die on the next
   reply (`RPC_STATUS_ASSERT`, `RPC.cpp:31`). The ROPE exception does not matter: Nemotron-H's attention has no RoPE.
2. **The remote backend is plain Metal**, the same backend that runs the head's own layers. Metal `supports_op`:
   `SSM_SCAN` and `SSM_CONV` return `has_simdgroup_reduction` (`ggml/src/ggml-metal/ggml-metal-device.m:1885-1888`),
   true on M1 and newer, so true on M3 and M5.
   - Metal SSM_SCAN implementation handles the new `ids` source and K snapshots (`ggml-metal-ops.cpp:1888-2019`;
     asserts `K >= 1` at `:1931`).
   - The fast chunked MMA kernel needs `d_inner == 64` per head (`OP_SSM_SCAN_SSD_HD 64`,
     `ggml-metal-impl.h:186`) and `d_state % 8 == 0`. Nemotron-3-Nano has mamba_head_dim 64 and d_state 128, so
     prefill gets the fast path; decode (1 token) uses the sequential kernel (`:1999-2001`).
   - K = 1 unless speculative MTP/draft is on (`mamba-base.cpp:170`; `common/common.h:403-410`;
     `src/llama-context.cpp:204-208`).
   - Tests exist for all of these in `tests/test-backend-ops.cpp` (`test_ssm_conv` `:4765`, `test_ssm_scan` `:4831`,
     `test_ssm_scan_rollback` `:4919`, `test_ssm_scan_cache_fusion` `:5028`).

Conclusion: every op Nemotron-H needs is something Metal runs, and RPC adds nothing op-specific; it ships the graph
and Metal runs it. **If the model runs on one Mac with Metal, the same layers run on a remote Mac over RPC.** The
risk is not an unsupported op.

## 4. Graph reuse with a hybrid model (affects per-token bytes)

`llm_graph_input_mem_hybrid::can_reuse` (`src/llama-graph.cpp:1216-1236`) requires the same n_tokens, a reusable
KQ mask, the same number of recurrent rows, and the same recurrent `head` and `rs_z`. In steady single-stream decode
these hold, so the client sends 12-byte GRAPH_RECOMPUTE. When the KV mask size changes (context grows past a padding
step) or the batch mix changes (`--parallel` slots joining/leaving), the client re-serializes the split graph and
sends a full GRAPH_COMPUTE (rpc_tensor is 296 bytes, from the packed struct format in issue #25765:
`<QIQ4I4II16ii10QQQQ64s4s`). Cost: a few hundred KB once per change, not per token. Unmeasured.

## 5. GitHub issues (searched Nov 7 2026 with `gh search issues --repo ggml-org/llama.cpp`)

Searches "rpc nemotron", "rpc mamba", "rpc hybrid model", "rpc recurrent", "rpc qwen3next", "rpc granite hybrid",
"rpc falcon-h1", "nemotron 3 nano rpc": **zero results**. Nobody has reported RPC + hybrid either broken or working.

Relevant hits:

| Issue | State | What | Matters to us? |
|---|---|---|---|
| #25765 (Jul 16 2026) | closed | RPC SSM_SCAN: the `ids` tensor values are not bounds-checked, so a malicious RPC client can read/write out of bounds on the server | Security only; it shows SSM_SCAN does go through RPC. Our network is closed |
| #27141 (Aug 15 2026) | closed as stale | `nemotron_h_moe` aborts at startup in `ggml_ssm_scan`, `GGML_ASSERT(K >= 1)`, Metal, b10360 | Looks fixed in our commit: K is computed as `n_rs_seq > 0 ? n_rs_seq + 1 : 1` (`mamba-base.cpp:170`), so it is always >= 1. A comment says a variant still crashed only with an external MTP draft model attached. **Do not enable speculative/MTP drafting** |
| #30894 (Oct 20 2026) | closed | Nemotron 3 GGUFs tagged pre-tokenizer `pixtral`, mapped to the Llama-3 regex instead of Tekken | Fixed in our commit: `pixtral` now maps with `tekken` (`src/llama-vocab.cpp:2623-2624`). Older llama.cpp builds tokenize Nemotron slightly wrong. Use our commit on every Mac |
| #30798 (Oct 19 2026) | open | Feature request: `nemotron_h_moe` support for `--split-mode tensor` | Confirms 01: tensor mode is not available for this arch |
| #28345 (Sep 3 2026) | closed | SIGFPE loading `nemotron_h_moe` GGUFs whose MTP blocks have zeroed expert arrays | Only for GGUFs with MTP blocks; check the load on day 1 |

## 6. Tool calling and thinking (verify cppneed, no repeat)

- Parser choice: template text containing `<tool_call>`, `<function=`, `<parameter=` selects the Qwen3-Coder XML
  parser (`common/chat.cpp:1412-1419`, comment names Nemotron Nano 3). Confirmed.
- Bundled template `models/templates/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16.jinja`: `enable_thinking` defaults to
  True (`:12`), tool-call format `<tool_call>\n<function=NAME>\n<parameter=...>` (`:92`, `:148-156`), strips old
  `<think>` blocks from history (`:108-142`). The GGUF's embedded template is what is actually used unless
  `--chat-template-file` is passed; it should match, but check the load log line "Using specialized template:
  Qwen3-Coder" (`LOG_DBG`, so needs `-v`).
- RPC does not touch any of this: templating and parsing run in llama-server on the head.

## 7. Verdict: will a 3-way split of Nemotron-3-Nano work?

**Yes, likely: ~80% confidence it loads and gives correct output on the first try with our commit.**

For:
- State and KV are allocated on the device that owns each layer (section 2); nothing hybrid-specific crosses the
  network.
- Every op runs on Metal; RPC forwards graphs blindly (section 3). Same backend as the head.
- Tokenizer and K>=1 bugs are fixed in this commit (section 5).

Against / unknowns (the remaining 20%):
- No public report of RPC + any hybrid/Mamba model, working or broken.
- The memory split counts layers, not bytes (`src/llama-model.cpp:1776`). MoE layers hold ~97% of bytes and sit at
  every other index, so a layer-count split is close to a byte split, but the 16 GB M3 needs an explicit small
  `--tensor-split` share and a check of the load log.
- Graph re-serialization on mask/batch changes and `--parallel` > 1 behaviour over RPC are unmeasured.

Smallest test that settles it (30 min):
1. Every Mac on commit e9a2a44, built with `-DGGML_RPC=ON -DGGML_METAL=ON`.
2. Mac A, B: `rpc-server -H 0.0.0.0 -p 50052 -c`.
3. Head: `llama-server -m nemotron.gguf --rpc A:50052,B:50052 -ngl 99 --tensor-split 9,5,9 -c 8192 -np 1`
   (M5 24 GB, M3 16 GB, M5 24 GB in `--rpc` order then local; numbers are a starting guess).
4. Check the load log: `RS buffer size` lines show one RS buffer per RPC device (`llama-memory-recurrent.cpp:122`),
   and no `-nkvo`.
5. Same prompt with `--temp 0` on the 3-way split and on one Mac with `-ngl` lowered to fit: the token sequences
   should match for the first ~50 tokens (some drift is possible from different kernel paths; identical is the
   ideal).
