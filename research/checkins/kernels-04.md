## Check-in kernels 04: Apple Silicon, Nemotron per-token cost, network round-trips

**Found (most important first, max 8 bullets, each with a source):**
- **Nemotron-3-Nano-30B-A3B, from its config.json:**
  - 52 layers: 23 Mamba-2, 23 MoE and only 6 attention, each block a single mixer. 128 experts, 6 routed + 1 shared, using relu2 (2 matrices per expert). Hidden size 2688; vocab 131,072, not tied.
  - **3.46B active per token** (0.35B of that is the lm_head) out of 31.8B total. My computed file sizes match EXO's cards within 0.2 GB.
  - **Weight reads per token: 1.95 GB at 4-bit, 2.38 GB at 5-bit, 2.82 GB at 6-bit.**
  - **KV cache is only 6 KB per token** (32k context = 0.2 GB), plus a fixed 49 MB of Mamba state per conversation. Long legacy-code files and long agent traces are nearly free in memory.
- **There is no MTP head and no speculative decoding path in MLX for Nemotron 3 Nano.**
  - The NVIDIA BF16 checkpoint index has 0 `mtp.*` tensors.
  - mlx-lm `nemotron_h.py` `sanitize()` drops `mtp.` weights anyway.
  - Draft-model speculation needs a trimmable cache, and the Mamba layers use `ArraysCache`, whose `is_trimmable()` returns False (`cache.py` line 164).
  - Nemotron **3.5** Lightning does ship an MTP head, which mlx-optiq keeps in a "sidecar" file. I found no Mac measurement that uses it.
- **Measured Mac decode for this layout** (Nemotron 3.5 Lightning 30B-A3B, same `nemotron_h` 30B-A3B shape; llmcheck.net/benchmarks, Kapitsa harness, Oct 18-19 2026):
  - mlx-lm 4-bit: **93.7 tok/s on M5 Pro, 139.3 on M5 Max**.
  - oMLX on M5 Max: 164.7. llama.cpp Q4_0 on M5 Max: 116.8. Base M6, llama.cpp: 45.
  - My fit to the mlx-lm points: about 2.14 GB effective per token + 3.7 ms fixed per token on Pro/Max GPUs.
- **Warning about the base M5 (10-core GPU).**
  - Three community MoE runs on base M5 imply a fixed cost of **4.5 to 15 ms per token**, not 3.7: LFM2.5-24B-A2B at 74.2 tok/s; KAT-Coder 35B-A3B at 2-bit, 49.5; North Mini Code 30B-A3B at 4-bit, 34.2 (llmcheck).
  - Single-Mac estimate for Nano-30B 4-bit: **base M5 about 34 to 50 tok/s, M3 about 22 to 32**.
- **Pool estimate, Nano-30B over M5/M3/M5** (12/7/12 GB memory-share split; full table in notes section 3). SOL is the speed of light: peak bandwidth with no overhead and no network.

| Quant | SOL | Ethernet | Wi-Fi |
|---|---|---|---|
| 4-bit | 68 tok/s | 28 to 42 tok/s | 14 to 30 tok/s |
| 5-bit | 55 | 25 to 37 | 13 to 27 |
| 6-bit | 47 | 23 to 32 | 12 to 24 |

- **The 6-bit memory margin is thin.**
  - Need: 25.7 GB of weights, plus embedding and lm_head copies on each rank (my reading of EXO), plus buffers, so **about 28 to 30 GB of the 31 GB budget**.
  - The M3's share (about 6.1 to 6.6 GB) is close to its 7 GB budget, and one browser tab could tip it into a failed load or a stall.
  - **5-bit (21.7 GB = 20.2 GiB) is safer and still exceeds any one 24 GB Mac's GPU ceiling (16 to 18.7 GiB).**
  - 4-bit (16.6 GiB) could fit on one M5 with apps closed, which weakens the "no single laptop" claim.
- **The routing model Nano-9B-v2 is dense**, from its config: 56 layers, Mamba-2 + MLP, no experts.
  - It reads about all 5 GB per token, so base M5 gets **about 19 to 24 tok/s, likely slower than the pooled 30B-A3B**.
  - Routing exists for concurrency and resilience, not per-request speed.
- **What EXO does per decode token, from the code** (`exo/.../sharding/pipeline.py`, `PipelineLastLayer` lines about 100 to 173):
  - It calls `mx.eval` before each `send`, then every rank runs `all_gather` of the final hidden state, evaluated on the CPU to avoid the GPU watchdog. Every rank then computes the lm_head itself.
  - That makes **about 4 sequential network latencies per token**: 2 sends + 2 ring steps.
  - Each payload is only 5.4 KB, and compute and communication do not overlap within a token.
  - Compressing activations is pointless (dropped). Latency per hop is the whole network cost.

**Surprises / things that change the plan:**
- **6-bit vs 5-bit is a demo-risk call, not a quality call.** I recommend 5-bit as the default pool model. It is about 15% faster than 6-bit (2.38 vs 2.82 GB per token), still bigger than any one Mac, and leaves about 5 GB of margin instead of 1 to 3. Keep 6-bit as the "with apps closed" variant only if first-hour memory readings show at least 3 GB free on the M3 after loading.
- **A dense 9B for routing is a slow pick on base chips.** One option is to route with the same Nano-30B-A3B 4-bit on a single M5 with apps closed. It reads 1.95 vs 5 GB per token, so it is likely 1.5 to 2x faster, and it is the same model family. But 17.8 GB on a 24 GB Mac is tight; measure before deciding.
- **The biggest speed levers are network, not kernels.**
  - Ethernet saves about 4 x (3 to 10 ms) per token.
  - Replacing EXO's decode `all_gather` with "last rank sends the token id to rank 0" saves about 1 latency per token (10 to 20% on Wi-Fi). It is a patch to EXO, medium risk.
  - Using 2 Macs (M5+M5) instead of 3 when the model fits removes a hop and the slow M3 stage.

**PRD suggestions:**
- In Must have, Pool: change the default to "`Nemotron-3-Nano-30B-A3B` **5-bit** (21.7 GB). Use 6-bit (25.7 GB) only if the first-hour `/state` shows at least 3 GB free on the M3 after loading. 4-bit is the fallback; at 16.6 GiB it might fit on one M5 with apps closed, so don't use it for the 'too big for one Mac' claim."
- In Why pooling works, add: "Nemotron 3 Nano keeps KV for only 6 of 52 layers: 6 KB per token plus 49 MB of fixed Mamba state. A 32k-token legacy file costs 0.2 GB, so agents can read whole codebases on borrowed laptops."
- In Open question 3, replace the Qwen numbers with Nemotron 5-bit on 3 Macs: SOL 55 tok/s; estimate 25 to 37 on Ethernet and 13 to 27 on Wi-Fi. Single M5 at 4-bit (if it fits) about 34 to 50. Nano-9B routed on one M5 about 19 to 24.
- In First hour, add:
  - `mlx_lm.benchmark` for Nano-9B-v2-4bits (`-p 512 -g 256 -n 3`, plus `-b 4`) on each Mac.
  - The Nano-30B 4-bit benchmark on one M5 with apps closed, to give the single-Mac baseline.
  - `ping -c 50` between Macs.
  - Then predicted pool ms per token = stage bytes / measured bandwidth + measured fixed cost + 4 x one-way latency.
- In Roadmap, add: "Nemotron 3.5 MTP head (multi-token prediction) to verify several tokens per network round-trip; needs an MLX fork today."

**Questions for Aayo:**
1. 5-bit or 6-bit for the pool? I recommend 5-bit (safer memory, about 15% faster, still too big for one Mac) unless the extra quality of 6-bit matters to the judges.
2. Is a small EXO patch acceptable? Replacing the decode `all_gather` with a token-id send saves about 1 network hop per token. It touches EXO's pipeline code, so the exo worker would own it. If not, I won't propose it in section 5.

**Next section:** Section 5 is the ranked list of techniques we can actually use or show this weekend. Each will have expected gain, effort, how to measure it live, and the honest claim to make. Candidates:
- Ethernet vs Wi-Fi.
- Quant choice.
- The speed-of-light meter and prediction vs measurement.
- 2- vs 3-node placement and placement by speed.
- Batching for routing.
- The all_gather patch.
- Hybrid-model memory (6 KB per token) as the agent-context story.

I'll flag NVIDIA-only items for the Dell customer roadmap and tie everything to the NVIDIA track framing (Nemotron agents) and the Spacetime control plane (which metrics it should store and stream).
