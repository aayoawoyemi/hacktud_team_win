## Check-in cppcode 02: Nemotron-H over RPC

Full notes are in research/cppcode/02-nemotron-h-over-rpc.md. All references are to llama-src at commit e9a2a44. I read the code but did not run the model.

**Found (most important first, max 8 bullets, each with a source):**

- **Yes, the Mamba state lives on the Mac that owns the layer.** For each layer, the recurrent cache is allocated in the buffer type of `model.dev_layer(i)` when offload is on (`src/llama-memory-recurrent.cpp:84-93`). For a remote layer, that buffer is in the remote Mac's Metal memory, created through RPC ALLOC_BUFFER. The KV cache for the 6 attention layers works the same way (`src/llama-kv-cache.cpp:216-217`). The state update is a copy inside the remote graph (`src/models/mamba-base.cpp:260-264`). Per token, the only extras sent are a few tiny one-way inputs: s_copy row ids, out_ids, and the KQ mask if that Mac holds an attention layer. Offload is on by default (`cparams.offload_kqv`, `src/llama-model.cpp:3377-3391`). Never pass `-nkvo`: it would put all state on the head's CPU.
- **Yes, every Mamba-2 op runs on RPC + Metal.**
  - The RPC client says every op is supported, except non-truncated RoPE (`RPC.cpp:3056-3064`, "TODO: call the remote backend"). It forwards the graph unchanged, and the remote Mac runs it on Metal.
  - Metal supports SSM_SCAN and SSM_CONV on any chip with simdgroup reduction, which includes M1 and newer (`ggml-metal-device.m:1885-1888`).
  - Metal's SSM_SCAN handles the new ids and K arguments (`ggml-metal-ops.cpp:1888-2019`).
  - Nemotron's Mamba head size (64) matches the fast Metal prefill kernel (`ggml-metal-impl.h:186`).
  - The RoPE exception does not apply, because Nemotron-H attention has no RoPE (`src/models/nemotron-h.cpp:276-289`).
  - Rule of thumb: if a layer runs on one Mac, it runs on a remote Mac.
- **n_embd = 2688, confirmed** from NVIDIA's config.json (HF `nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16`, fetched Nov 7). Decode sends 10.5 KiB per hop.
  - Layer pattern: `MEMEM*EMEMEM*...EMEMEMEME`, which is 23 Mamba, 23 MoE and 6 attention layers.
  - Mamba-2 sizes: d_inner 4096, d_state 128, 8 groups.
- **Memory per Mac is small apart from the weights:**
  - Recurrent state is about 48 MiB per parallel slot in total (2 MiB SSM + 72 KiB conv per Mamba layer × 23 layers).
  - KV cache is 6 KiB per token (32k context = 192 MiB per slot).
  - MoE layers hold about 97% of the weight bytes (about 1.3 B params each, vs 39 M per Mamba layer).
- **Two Nemotron bugs are already fixed in our commit:**
  - #30894 (Oct 2026): Nemotron 3 GGUFs were tagged with the `pixtral` pre-tokenizer and tokenized with the wrong rules. `pixtral` now maps to tekken (`src/llama-vocab.cpp:2623-2624`). **All Macs must use our commit.**
  - #27141 (Aug 2026): `nemotron_h_moe` crashed on Metal in ssm_scan with K >= 1. K is now always at least 1 (`mamba-base.cpp:170`). The issue was closed as stale, and one commenter still crashed when an MTP draft model was attached, so **do not turn on speculative/MTP decoding**.
- **No public reports of RPC with a hybrid or Mamba model, working or broken.** I ran 8 `gh search` queries with zero hits. The only RPC + SSM_SCAN issue is #25765, a server out-of-bounds bug (security only); it shows SSM_SCAN does run through RPC.
- **Graph reuse works for the hybrid cache** (`src/llama-graph.cpp:1216-1236`). Steady decode sends a 12-byte recompute. The full graph is resent (rpc_tensor = 296 B per tensor) when the mask size or the batch mix changes. That happens once per change, not per token.
- **Tool calls (confirming cppneed):** the Qwen3-Coder XML parser is picked from the template text (`common/chat.cpp:1412-1419`), and the template turns thinking on by default (`NVIDIA-Nemotron-3-Nano-30B-A3B-BF16.jinja:12`). This all runs on the head, so RPC does not touch it.

**Verdict:** a 3-way split of Nemotron-3-Nano should work, with about 80% confidence that it loads and gives correct output on the first try. The remaining risk is that nobody has published this combination, plus the per-Mac memory split. An unsupported op is not the risk.

**Surprises / things that change the plan:**

- **The client cannot detect a missing op.** RPC never asks the remote what it supports. A missing op would crash the server mid-request (`RPC.cpp:2206`). Mixing in the Windows Dell (CPU/CUDA backend) is riskier than Mac-to-Mac for this reason.
- **The split counts layers, not bytes** (`src/llama-model.cpp:1776`). For Nemotron that is close enough, because MoE layers sit at every other index. The 16 GB M3 still needs an explicit smaller `--tensor-split` share.

**PRD suggestions:**

- Add these launch rules: same llama.cpp commit on every Mac; never `-nkvo`; no speculative/MTP decoding.
- Add the 30-minute go/no-go test (step 5 of my notes):
  - Workers run `rpc-server -H 0.0.0.0 -p 50052 -c`.
  - The head runs `llama-server --rpc A:50052,B:50052 -ngl 99 --tensor-split 9,5,9 -c 8192 -np 1`.
  - Check that the load log shows one RS buffer per RPC device.
  - Compare greedy output against a single-Mac run.

**Disagreements with cppneed:** none. Their R3 references (`chat.cpp:1412`, the bundled template) check out.

**Questions for Aayo:**

1. Has anyone run the go/no-go test yet? A load log from it, or the error if it fails, decides section 3's priorities.

**Next section:**

Section 3 is "strip and improve": what to drop, and improvements ranked by gain/effort, with the file or flag for each and a note on which ones also beat EXO.

Changes I would make to it, based on sections 1 and 2:
- Add "speed-weighted split from measured per-layer cost (Mamba vs MoE vs attention)".
- Add "ask the remote which ops it supports, so the Windows Dell can join safely" (`RPC.cpp:3056`).
- Add "make SET_TENSOR_HASH async and parallel to speed up restarts" (`RPC.cpp:863`).
