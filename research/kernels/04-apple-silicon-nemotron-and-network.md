# 04: Apple Silicon, Nemotron per-token cost, and the network side

Researched Sat Nov 7 2026, after the 4:10pm switch to NVIDIA Nemotron.

Hardware: M5 24 GB, M3 16 GB, M5 24 GB (third Mac confirmed).
- Base M5: 153 GB/s and a 10-core GPU (apple.com/macbook-pro/specs).
- Base M3: 100 GB/s (Apple spec; my recollection, not re-fetched).

The Metal-kernel stretch is treated as NO.

## 1. Nemotron-3-Nano-30B-A3B, from its config.json

Source: `mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-6Bit/config.json` (fetched Nov 7). The model type is `nemotron_h`, which runs in mlx-lm's `models/nemotron_h.py` and EXO's `nemotron_h/sharding.py`.

| Field | Value |
|---|---|
| Layers | 52 = **23 Mamba-2 (M) + 23 MoE (E) + 6 attention (*)**. Pattern `MEMEM*EMEMEM*EMEMEM*EMEMEM*EMEMEM*EMEMEMEM*EMEMEMEME`. Each block is a single mixer: Mamba-2 or attention or MoE, not attention+FFN pairs |
| Hidden size | 2688 |
| MoE | 128 routed experts, **6 per token**, + 1 shared expert (FFN 3712). Expert FFN 1856, **relu2 (non-gated, 2 matrices per expert, not 3)**, `routed_scaling_factor 2.5` |
| Attention | 32 query heads, **2 KV heads**, head dim 128 |
| Mamba-2 | 64 heads x 64 dim (d_inner 5376), state 128, 8 groups, conv kernel 4, `mamba_ssm_cache_dtype: float32` |
| Vocab / lm_head | 131,072, not tied, so lm_head = 352M parameters |
| MTP | **None.** The NVIDIA BF16 checkpoint's `model.safetensors.index.json` has 0 `mtp.*` tensors. mlx-lm `nemotron_h.py sanitize()` drops any `mtp.` weights anyway (line about 182) |

Per-layer parameters, my computation from the config:

| Block | Parameters | Count |
|---|---|---|
| Mamba-2 | 49.1M | 23 layers |
| Attention | 23.4M | 6 layers |
| MoE, active (6 routed + shared + router) | 80.2M | 23 layers |
| MoE, total (all 128 experts) | 1,297M | 23 layers |
| lm_head | 352M | 1 |

**Active parameters per token: 3.46B (3.11B in the layers + 0.35B lm_head). Total: 31.8B.**

My size estimates match EXO's card sizes within 0.2 GB, so the counting is right:

| Quant | Bits per weight (MLX affine, group 64) | Estimated file | EXO card | **Weight bytes read per token** |
|---|---|---|---|---|
| MXFP4 | 4.25 | 16.9 GB | 16.8 GB | 1.84 GB |
| 4-bit | 4.5 | 17.9 | 17.8 | **1.95 GB** |
| 5-bit | 5.5 | 21.9 | 21.7 | **2.38 GB** |
| 6-bit | 6.5 | 25.8 | 25.7 | **2.82 GB** |
| 8-bit | 8.5 | 33.8 | 33.6 | 3.68 GB |

State and KV, my computation:
- **KV cache: only 6 attention layers x 2 KV heads x 128 x 2 (K,V) x 2 B = 6 KB per token.** A 32k context is 0.2 GB, and 128k is 0.8 GB. This is 3.4x smaller than Qwen3.6 and 16x smaller than Qwen3-30B-A3B.
- Mamba state is fixed: 23 x (64 x 64 x 128 x 4 B SSM + small conv state) = **49 MB per conversation**, whatever the length.
- Long legacy-code files and many agent steps cost almost no extra memory. That fits the "agent modernizes a legacy codebase" use case well.

Speculative decoding paths in MLX for Nemotron 3 Nano: **none.**
1. No MTP head exists (above).
2. mlx-lm draft-model speculation needs a trimmable cache, but `make_cache()` gives `ArraysCache` for the Mamba layers (`nemotron_h.py` about line 176), and `ArraysCache.is_trimmable()` returns False (`cache.py` line 164).
3. For contrast, Nemotron **3.5** models do ship MTP heads. mlx-optiq says Lightning-30B-A3B's "MTP speculative head is preserved in the sidecar" (mlx-optiq.com/docs/nemotron). I found no measured Mac speedup from Nemotron MTP; the llmcheck rows for Lightning all say "no MTP path".
4. Stock mlx-lm still strips `mtp.` weights. MTP for Nemotron on Macs would need a fork (for example optiq's sidecar), and only for 3.5 models. Unverified that any fork actually uses it for decoding.
5. Unverified: whether EXO's Nemotron 3.5 Lightning card keeps or uses the MTP head. I assume not.

## 2. Measured Mac decode speeds for this architecture (mlx-lm, batch 1, no speculation)

Source: the llmcheck.net/benchmarks table, fetched Nov 7. Nemotron 3.5 Lightning-30B-A3B has the same `nemotron_h` 30B-A3B layout and the same 17.8 GB 4-bit size in EXO, so I use it as the Nano proxy.

| Chip (bandwidth, GPU cores) | Runtime, quant | tok/s | Source note |
|---|---|---|---|
| M5 Pro (307 GB/s, 20-core) | mlx-lm 0.32.2, mlx-community 4-bit | **93.7** (greedy 96.8) | Kapitsa harness, 512-token coding answers, Oct 19 |
| M5 Pro | LM Studio llama.cpp Q4_K_M | 72.3 | same harness |
| M5 Max (614 GB/s, 40-core) | mlx-lm 0.32.2, 4-bit | **139.3** | same harness |
| M5 Max | oMLX 0.7.1rc1, 4-bit | 164.7 | same harness, +18% over mlx-lm |
| M6 base (32 GB) | llama.cpp Q4_K_M | 45 | community |
| M5 Max | llama.cpp Q4_0 | 116.8 | same harness |
| **Base M5 (10-core GPU, 32 GB)** | oMLX 0.6.4, 4-bit + 4-bit KV | **34.2** (30.5 at 4k) | a *different* 30B-A3B (North Mini Code 1.0, architecture not checked) |
| **Base M5 (32 GB)** | MLX 4-bit | **74.2** | LFM2.5-24B-A2B (2.45B active), Sep 2026 |
| **Base M5 (32 GB)** | MLX oQ2e (about 2-bit) | **49.5** | KAT-Coder-V2.5 35B-A3B (a Qwen3.6 fine-tune), Oct 2026 |

Fit of time per token = bytes / effective bandwidth + fixed cost, using the M5 Pro and M5 Max mlx-lm points: **effective bytes 2.14 GB per token** (my weight count is 1.95 GB; the rest is state, embeddings and activations) and **fixed cost 3.7 ms per token**. The fixed cost is lower than Qwen3-30B-A3B's 5.2 ms on the M4 Max: fewer experts per token (6 vs 8) and fewer layers with experts (23 vs 48).

**The base M5 warning.** The base M5 has a 10-core GPU, a quarter of the M5 Max's. The three base-M5 MoE points give a wide implied fixed cost:
- LFM2.5 (about 1.38 GB per token): 13.5 ms measured vs 9 ms of bandwidth, so about 4.5 ms of overhead.
- KAT-Coder at 2-bit (about 1 GB per token): 20 ms vs 6.3 ms, so about 14 ms.
- North Mini Code at 4-bit: 29 ms vs about 14 ms, so about 15 ms.

These are my byte estimates applied to community runs on different models. So the fixed cost per token on a 10-core GPU is somewhere between **4.5 and 15 ms**, not 3.7 ms. Measure it. It lowers every estimate below for our base chips.

Single-Mac estimates, if the model fitted:

| Model | Base M5 | Base M3 |
|---|---|---|
| Nemotron-3-Nano-30B-A3B 4-bit | **about 34 to 50 tok/s** | about 22 to 32 |
| Nemotron-3-Nano-30B-A3B 6-bit | about 26 to 38 | n/a |

Routing model, my computation from `nvidia/NVIDIA-Nemotron-Nano-9B-v2/config.json`:
- Nano-9B-v2 is **dense**: 56 layers (Mamba-2 + MLP + 4 attention), hidden 4480, no experts. So it reads about all 5.0 GB of its 4-bit weights per token.
- Base M5: 5.0 / (0.153 x 0.87) = 37.6 ms + about 4.5 to 15 ms, so **about 19 to 24 tok/s**. Base M3: about 14 to 16 tok/s.
- **The routed 9B decodes slower than the pooled 30B-A3B is likely to** (section 3). The dense 9B reads 2.6x more bytes per token than the MoE's active 1.95 GB.
- Implication: routing exists for concurrency (several agents or users at once) and resilience, not per-request speed. Measure it.

## 3. Pool estimate: Nemotron-3-Nano-30B-A3B over 3 Macs (M5 / M3 / M5)

Assumptions:
- EXO splits layers by memory share, 12/7/12 GB. Each layer's active bytes go with it.
- The lm_head runs on every rank after the all_gather (below), in parallel; the M3 is the slowest, so its time sets that term.
- Effective bandwidth is 87% of peak.
- Fixed cost per token is 4.5 to 15 ms in total for base chips (base-M5 range above; 3.7 ms on Pro/Max chips).
- 4 sequential network latencies per token: see section 4.

| Quant | Layer bytes split M5/M3/M5 | **SOL (peak bandwidth, no overhead, no network)** | Ethernet via router (about 0.5 to 1 ms per latency) | Venue or travel Wi-Fi (about 3 to 10 ms per latency) |
|---|---|---|---|---|
| 4-bit (17.8 GB) | 0.68 / 0.40 / 0.68 GB | 14.8 ms, **68 tok/s** | 24 to 36 ms, **28 to 42 tok/s** | 34 to 72 ms, **14 to 30 tok/s** |
| 5-bit (21.7 GB) | 0.83 / 0.48 / 0.83 | 18.1 ms, **55 tok/s** | 27 to 40 ms, **25 to 37 tok/s** | 37 to 76 ms, **13 to 27 tok/s** |
| 6-bit (25.7 GB) | 0.98 / 0.57 / 0.98 | 21.4 ms, **47 tok/s** | 31 to 44 ms, **23 to 32 tok/s** | 41 to 80 ms, **12 to 24 tok/s** |

Memory check for 6-bit, the leaning choice:
- 25.7 GB of weights + 2 extra copies of embedding and lm_head (about 0.57 GB each; my reading is that every EXO rank keeps them) + KV/state (under 0.3 GB) + MLX buffers (about 0.5 to 1 GB per Mac).
- Total: **about 28 to 30 GB against the 31 GB budget, only 1 to 3 GB of margin.**
- The M3 share is about 6.1 to 6.6 GB against its 7 GB budget. A browser tab could tip it into swap, and swapping wired GPU memory is not possible, so the load fails or the Mac stalls.
- **5-bit (about 24 to 26 GB total) is the safer pick.** It still holds "bigger than any one Mac": 21.7 GB is 20.2 GiB, above the 24 GB M5's 16 to 18.7 GiB ceiling.
- The 4-bit model (17.8 GB = 16.6 GiB) **could fit on one M5 24 GB** under the 78% ceiling with apps closed. That weakens the "no single laptop" claim, so present 4-bit only as the fallback.

Quality per bit (mlx-optiq, uniform 4-bit vs OptiQ mixed 4-bit on Nano-30B-A3B):

| Benchmark | Uniform 4-bit | OptiQ mixed 4-bit |
|---|---|---|
| MMLU | 74.8 | 76.2 |
| GSM8K | 78.5 | 81.6 |
| HumanEval | 86.0 | 89.0 |
| BFCL-v3 (tool calls) | 74.0 | 74.0 |

I found no published 5-bit or 6-bit numbers; I expect them to land at or above the OptiQ figures, but that is a guess. Tool calling (BFCL) did not move with mixed precision, which matters for the NVIDIA "agents with tools" track.

## 4. Network side: what EXO actually does per token (code read)

Source: `exo/src/exo/worker/engines/mlx/sharding/pipeline.py` (342 lines, main, fetched Nov 7).

- `PipelineFirstLayer.__call__` (about line 61): rank r>0 does `mx.distributed.recv_like(x, r-1)` before its first layer.
- `PipelineLastLayer.__call__` (about lines 100 to 173):
  - `mx.eval(output)`, then `mx.distributed.send(output, r+1)`, then `mx.eval` again. The comment says: "Eval layer output to materialize it before send, this splits the graph".
  - In decode (`not self.is_prefill`), **every rank** then runs `mx.distributed.all_gather(output)` and `mx.eval(gathered)`. The comment explains this waits on the CPU so that the GPU watchdog does not kill a command buffer waiting on a slow rank.
  - So every rank gets the final hidden state and runs norm + lm_head itself.
- Prefill can queue sends (`queue_sends`, `flush_prefill_sends`, lines 13 to 20); decode cannot.

What that means at batch 1:
- **Per-token network latency chain:** send M5→M3, send M3→M5, then an all_gather across 3 ranks. On MLX's ring backend over TCP, an all_gather takes s-1 = 2 sequential steps (MLX distributed docs: ring nodes only talk to neighbours). Total: **about 4 sequential network latencies per token**.
- The payload is tiny: 2688 x 2 B = **5.4 KB per hop** (the hidden state in bf16). Bandwidth is irrelevant; latency and jitter are everything. **Activation compression gains nothing** (confirmed; dropped).
- **No compute/communication overlap within a token.** Each stage must finish before the next starts, and the explicit `mx.eval` calls add a GPU sync per stage (cost not measured; guess 0.1 to 0.5 ms each). Overlap is only possible across *different* requests, where stage 2 works on request A while stage 1 works on request B. EXO's decode loop does not appear to do this for one instance. Unverified; the exo worker should confirm.
- **Ideas to cut round-trips per token**, for section 5 to rank:
  1. Replace the decode `all_gather` with "last rank samples, sends one token id to rank 0". That is 1 hop instead of 2 ring steps, saving about 1 latency per token: about 10 to 20% on Wi-Fi, about 3% on Ethernet. It needs a patch to EXO's pipeline and identical sampling, so the risk is medium.
  2. Put the two M5s adjacent and the M3 at one end so it does less. Order matters little for latency, though.
  3. **Use 2 Macs instead of 3** when the model fits (4-bit or 5-bit on M5 + M5 = 24 GB budget). That means 1 fewer hop and no slow M3 stage, so it is probably faster, but it is a less impressive "3 machines" demo. EXO's `min_nodes=3` forces 3.
  4. Ethernet over Wi-Fi: about 4 x (3 to 10 ms) saved per token. **This is the biggest lever by far.**
  5. Multi-token prediction, which would verify k tokens per network trip. Not available for Nemotron 3 Nano (section 1). A roadmap item via Nemotron 3.5 + oMLX/mlx-vlm.

## 5. Apple Silicon bandwidth reference (decode ceiling = bandwidth / bytes per token)

| Chip | Bandwidth | Source |
|---|---|---|
| M5 (base) | 153 GB/s, 10-core GPU | apple.com MacBook Pro specs (fetched in section 03) |
| M5 Pro / M5 Max | 307 / 614 GB/s | llmcheck.net table (Oct 2026) |
| M6 (base, Mac mini) | 170 GB/s | llmcheck.net |
| M4 Pro / M4 Max | 273 / 546 GB/s | llmcheck.net; mlx-lm BENCHMARKS |
| M3 (base) / M3 Max | 100 / 300 to 400 GB/s | Apple (recollection) / llmcheck (binned 300) |
| M1 / M2 base | 68 / 100 GB/s | Apple (recollection, not re-verified) |

Fraction of peak that MLX reaches on weight reads: **87 to 91%** on the M4 Max (my fit, section 03). The total per-token efficiency, counting fixed overhead, is much lower on small-GPU chips (base M5: about 45 to 50% by the one data point). oMLX was 18% faster than stock mlx-lm on the M5 Max for Lightning, so runtime choice is a measurable lever, but EXO uses its own MLX engine and cannot swap it.

## 6. First-hour measurements (replace the guesses)

```bash
# per Mac, apps open as in the demo
mlx_lm.benchmark --model mlx-community/NVIDIA-Nemotron-Nano-9B-v2-4bits -p 512 -g 256 -n 3        # routing model, real number
mlx_lm.benchmark --model mlx-community/NVIDIA-Nemotron-Nano-9B-v2-4bits -p 512 -g 256 -n 3 -b 4   # batching gain for routing
# one M5 only: does 4-bit Nano-30B fit and how fast? (quit apps; tells us the single-Mac baseline for the SOL meter)
mlx_lm.benchmark --model mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-4Bit -p 512 -g 256 -n 3
ping -c 50 <other-mac-ip>    # median and p95 one-way ≈ RTT/2; x4 = network ms per token
```

Then: predicted pool ms per token = (sum of stage bytes / measured effective bandwidth) + measured fixed cost + 4 x one-way latency. Compare it live with EXO's measured tok/s.
