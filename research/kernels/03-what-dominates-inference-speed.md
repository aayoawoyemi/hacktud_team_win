# 03: What dominates LLM inference speed at batch 1 on our Macs

Researched Sat Nov 7 2026. The hardware comes from feedback:
- M5 24 GB (153 GB/s, Apple spec page).
- M3 16 GB (base M3 is 100 GB/s, Apple spec; my recollection, not re-fetched today).
- A third Mac with 24 GB, chip unknown. I assume a base M5 below; if it is an M3 Pro (150 GB/s) or an M4 Pro (273 GB/s), the numbers move.

Pool model: `mlx-community/Qwen3.6-35B-A3B-4bit` via the EXO pipeline. Fallback: `Qwen3-30B-A3B-4bit`.

Everything below is either read from code and configs (cited) or computed from them (shown). Guesses are labelled.

## 1. The model, from its actual config

From `config.json` of `mlx-community/Qwen3.6-35B-A3B-4bit` (fetched Nov 7) and `mlx_lm/models/qwen3_5.py`:

| Field | Qwen3.6-35B-A3B | Qwen3-30B-A3B (fallback) |
|---|---|---|
| Architecture | `qwen3_5_moe`, hybrid | `qwen3_moe`, standard |
| Layers | 40: 30 Gated DeltaNet (linear attention) + 10 full attention (every 4th) | 48, all full attention |
| Hidden size | 2048 | 2048 |
| Experts | 256 routed, 8 per token, + 1 shared; expert FFN 512 | 128 routed, 8 per token; expert FFN 768 |
| Full-attention KV heads x head dim | 2 x 256 | 4 x 128 |
| Vocab | 248,320 (lm_head not tied) | 151,936 |
| Quantization | 4-bit affine, group 64; router gates 8-bit | 4-bit affine, group 64 |
| MTP head | `mtp_num_hidden_layers: 1` in config, but **no MTP code in mlx-lm `qwen3_5.py`** | none |

**Weight bytes read per generated token** (my computation from the config):
- Qwen3.6:
  - Per layer: 9 active experts x 3 x 2048 x 512 + attention or DeltaNet projections.
  - Active parameters: 2.44B in the layers + 0.51B lm_head = **2.95B**. The total comes to 34.7B, which checks against "35B".
  - MLX 4-bit with group 64 costs 4.5 bits per weight (a 16-bit scale and a 16-bit bias per 64 weights). So decode reads **1.66 GB per token** at 4-bit and 3.13 GB at 8-bit.
  - The lm_head alone is 0.29 GB per token (17%), because of the 248k vocabulary.
- Qwen3-30B-A3B: 3.04B active, so **1.71 GB per token** at 4-bit.

**KV cache, the memory that grows with context** (my computation):
- Qwen3.6:
  - Only the 10 full-attention layers keep KV: 10 x 2 x 256 x 2 (K and V) x 2 B = **20 KB per token**. A 32k-token context is 0.66 GB.
  - The 30 DeltaNet layers keep a fixed state of 30 x 32 heads x 128 x 128 x 4 B = 63 MB per conversation, whatever the length. The fp32 dtype is a guess.
- Qwen3-30B-A3B: 48 x 4 x 128 x 2 x 2 = **96 KB per token**, 4.8x more. A 32k context is 3.1 GB.
- For the demo this is a real argument for Qwen3.6. Long office documents and several users at once cost little memory, which matters on Macs with apps open.

## 2. Usable GPU memory: the `iogpu.wired_limit_mb` check

How the limit works:
- macOS lets the GPU "wire" (pin) only up to a ceiling. `iogpu.wired_limit_mb = 0`, the default, means a kernel heuristic based on installed RAM.
- Metal reports the ceiling as `recommendedMaxWorkingSetSize`, and MLX exposes it as `mx.device_info()["max_recommended_working_set_size"]`.
- The setting resets on reboot. It is global, and setting it too high can freeze the Mac, because wired pages cannot be paged out (ModelPiper, 2026; marianposaceanu.com, 2026).

Measured or reported ceilings:
- ModelPiper measured 26.8 GB = **78%** of a 32 GB M2 Max on macOS 26.5.2.
- The commonly quoted rule is 75%.
- llama.cpp logs on 16 GB Macs widely show `recommendedMaxWorkingSetSize = 11453.25 MB`, which is 2/3 of 16 GiB. That is from my memory of many llama.cpp issue logs and was not re-fetched today.
- So the heuristic is somewhere between 2/3 and 78%, and it may depend on the macOS version. **Measure it on each Mac.**

What mlx-lm does (`mlx_lm/generate.py`, `wired_limit()` at about line 249):
- It raises MLX's own wired limit to the recommended maximum while generating.
- It prints `[WARNING] Generating with a model that requires ... MB` if the model is more than 0.9 x the ceiling.
- It never changes the sysctl. The README says to raise it with `sudo sysctl iogpu.wired_limit_mb=N` only if the model is slow.

| Mac | RAM | Ceiling at 2/3 | Ceiling at 78% | exo worker's budget with apps open | Ceiling binding? |
|---|---|---|---|---|---|
| M5 | 24 GB | 16.0 GiB | 18.7 GiB | about 12 GB | No |
| M3 | 16 GB | 10.7 GiB | 12.5 GiB | about 7 GB | No |
| Third | 24 GB | 16.0 GiB | 18.7 GiB | about 12 GB | No |

What this means:
- **The 12/7/12 GB budgets are below the wired ceiling on every Mac.** The binding constraint is free RAM with personal apps open, not the wired limit. So do not touch the sysctl on personal Macs; it buys nothing here and risks freezes.
- The pool needs about 20.4 GB of weights + 0.3 GB per Mac for the replicated lm_head (my reading of `qwen3_5.py`: after the pipeline, `all_gather` gives every rank the final hidden state, and each rank computes norm and lm_head) + the KV/state cache (under 1 GB at 8k to 16k context) + MLX buffers (about 0.5 to 1 GB per Mac, a guess). That totals about **22 to 23.5 GB against a 31 GB budget**: it fits with about 8 GB to spare.
- **"Too big for any one Mac" holds by default.** 20.4 GB = 19.0 GiB, which is above even the 78% ceiling of a 24 GB Mac (18.7 GiB). It is also far above the 12 GB that is actually free with apps open. A judge could object: "with every app closed and the sysctl raised, an M5 24 GB could just hold it". The honest phrasing is "no laptop as people actually use it".

Commands for the first hour on each Mac, read-only and safe:

```bash
sysctl hw.memsize iogpu.wired_limit_mb
python3 -c "import mlx.core as mx; i=mx.device_info(); print(i['memory_size']/2**30, i['max_recommended_working_set_size']/2**30)"
memory_pressure | tail -1            # "System-wide memory free percentage"
sysctl vm.swapusage
```

## 3. Decode at batch 1: bandwidth plus a fixed overhead per token

The mlx-lm maintainers publish M4 Max (546 GB/s) numbers (`mlx_lm/BENCHMARKS.md`, 128 generated tokens, 2048-token prompt):

| Model | Precision | Generation tok/s | Bytes per token (my calc) | Implied ms per token |
|---|---|---|---|---|
| Qwen3-4B | bf16 | 52.47 | 8.04 GB | 19.06 |
| Qwen3-4B | q8 | 86.91 | 4.27 GB | 11.51 |
| Qwen3-4B | q4 | 134.52 | 2.26 GB | 7.43 |
| Qwen3-30B-A3B | q8 | 83.16 | 3.23 GB | 12.03 |
| Qwen3-30B-A3B | q6 | 94.14 | 2.47 GB | 10.62 |
| Qwen3-30B-A3B | q4 | 113.33 | 1.71 GB | 8.82 |

A straight-line fit of time per token = bytes / effective bandwidth + fixed overhead (my fit):
- **Dense 4B:** effective bandwidth 497 GB/s (91% of peak), fixed overhead **2.9 ms per token**. It predicts q8 at 87.2 tok/s; the measured value is 86.9.
- **MoE 30B-A3B:** effective bandwidth 474 GB/s (87%), fixed overhead **5.2 ms per token**. It predicts q6 at 96 and q5 at 104; the measured values are 94 and 101.
- **What the overhead is:** kernel dispatch, expert routing and gather, sampling and Python, paid once per token whatever the bit width. MoE pays about 2x the dense overhead: 48 layers, routing, and many small expert matmuls.
- **At q4 on a fast Mac, 59% of the MoE token time is overhead, not bandwidth.** "Decode is bandwidth-bound" is true for weight reads. For small-active MoE at 4-bit, the fixed cost per token is about as large as the reads. This is the same lesson as RadeonFlow's 170 µs launch versus 5 µs compute (section 02).

Cross-check for Qwen3.6-35B-A3B across chips (llmcheck.net, Oct 2026; mixed runtimes, plain decoding, MTP off):
- Measured: Mac mini M6 (170 GB/s) **64 tok/s**; M5 Pro (307 GB/s) median 79; M5 Max (614 GB/s) **118 to 126**.
- Fitting M6 against M5 Max gives about 1.79 GB effective bytes per token (my estimate was 1.66 GB + state reads, which agrees) and a fixed cost of about **5.1 ms**, the same as the M4 Max MoE fit.
- Prediction for our chips, if the whole model fitted on one Mac (a guess; base-chip GPUs have fewer cores, so the overhead may be larger):
  - M5 (153 GB/s): 1.79/0.153 = 11.7 ms + 5 to 8 ms, about **50 to 60 tok/s**.
  - M3 (100 GB/s): 17.9 ms + 6 to 9 ms, about **37 to 42 tok/s**.

## 4. Pool estimate and speed-of-light meter: Qwen3.6 split over 3 Macs

Assumptions:
- EXO places layers by memory share 12/7/12, so about 16/9/15 layers (the exo worker should confirm the exact split).
- 34.3 MB per layer per token at 4-bit. The lm_head (286 MB) runs on every rank in parallel, so the slowest one (M3) sets that part.
- Effective bandwidth is 85% of peak.

| Term | M5 (16 layers) | M3 (9 layers) | 3rd Mac as M5 (15 layers) | Critical path |
|---|---|---|---|---|
| Weight reads at peak bandwidth (speed of light) | 3.6 ms | 3.1 ms | 3.4 ms | + lm_head 2.9 ms = **12.9 ms, 77 tok/s SOL** |
| Weight reads at 85% | 4.2 | 3.6 | 4.0 | + 3.4 = 15.2 ms |
| Fixed overhead, split by layers (guess, 5 to 8 ms in total) | | | | +5 to 8 ms |
| Network: 2 pipeline sends + 1 all_gather per token; activation is 2048 x 2 B = 4 KB, so only latency matters | | | | Ethernet via router: +1.5 to 3 ms. Wi-Fi: +15 to 45 ms |
| **Estimate** | | | | **Ethernet about 22 to 26 ms, 38 to 45 tok/s. Wi-Fi about 35 to 70 ms, 14 to 28 tok/s** |

The speed-of-light meter for the demo:
- SOL (tok/s) = 1 / (sum over stages of bytes per stage / peak bandwidth + the slowest lm_head).
- Show the measured tok/s and the percentage of SOL. My estimate is 50 to 60% on Ethernet and 20 to 40% on Wi-Fi.
- The gap shows where time goes: network hops and fixed overhead per token, not weight reads. This is honest and explainable on stage.

## 5. The usual speed levers, rated for our setup (batch 1, Apple Silicon, MLX via EXO)

| Lever | Status in our stack (source) | Gain for us | Verdict |
|---|---|---|---|
| Weight quantization (4-bit vs 8-bit) | Built in; we choose the model file | 8-bit reads 1.9x the bytes. On the M4 Max, 30B-A3B q4 was 113 tok/s vs 83 at q8, but q4 MMLU-Pro was 70.7 vs 72.5 (BENCHMARKS.md) | **Use 4-bit.** It is also what makes the model fit |
| MoE instead of dense | Model choice | Qwen3.6 reads 1.66 GB per token while holding 20 GB. A dense model of the same 20 GB would read all 20 GB, so it is about 12x fewer bytes | **The core of the pitch**: capacity from the pool, speed from sparsity |
| Hybrid linear attention (Qwen3.6) | Model choice | KV is 20 KB per token vs 96 KB; long-context decode stays flat | **Use it**: cheap long documents and multiple users |
| KV-cache quantization (`--kv-bits 4/8`) | mlx-lm has it, starting after `quantized_kv_start=5000` tokens; **it turns off server batching** (SERVER.md) | Small, since Qwen3.6 KV is already small | **Skip** |
| Speculative decoding (draft model) | mlx-lm needs a trimmable cache; `ArraysCache` (DeltaNet state) returns `is_trimmable() == False` (`models/cache.py` line 164), so it is **unavailable for Qwen3.6**. Qwen3.6's built-in MTP head is not implemented in mlx-lm `qwen3_5.py` | For a pipeline over Wi-Fi it would help most: verifying k tokens per pass divides the network hops per token by k | **Not this weekend.** Possible for Qwen3-30B-A3B with a Qwen3-0.6B draft in plain mlx-lm, but EXO support is unknown (exo worker should check). Roadmap item: "MTP to cut network round-trips" |
| Batching several users (routing mode) | mlx-lm server batches when every cache layer supports `merge`; `ArraysCache` has `merge` (cache.py), so Qwen3.6 is batchable. Turned off with a draft model or `--kv-bits` (`server.py` line 475) | Dense: about free extra users per weight read. MoE: each extra user activates up to 8 more experts per layer, so the gain is smaller (no measured number found) | **Use for routing.** Measure aggregate tok/s at 1, 2 and 4 concurrent users with `mlx_lm.benchmark --batch-size` |
| Pipeline fill (different requests on different stages at once) | Depends on EXO; Parallax does it | Up to about 3x aggregate throughput with 3 stages and 3 or more concurrent users | **Ask the exo worker** whether EXO overlaps requests across stages |
| Fewer launches, compiled graphs (the CUDA-graph analog) | MLX evaluates lazily and encodes a graph per token. The fixed overhead of about 5 ms per token (section 3) is the remaining cost | Not tunable by us in a day | Explain it, don't fix it |
| FlashAttention-style attention | MLX has `mx.fast.scaled_dot_product_attention`; only 10 of 40 layers use attention | Small at office context lengths | Already there |
| CUDA graphs, FlashAttention-3, FP8/NVFP4 tensor cores, the GeForce unlock | NVIDIA only | n/a | **Drop** (no NVIDIA at the venue) |

Prefill (reading the prompt) is compute-bound, unlike decode. A 5,000-token office document is prefilled stage by stage through the pipeline. Apple says the M5 GPU has neural accelerators for faster prefill (Apple M5 spec page lists "Neural Accelerators"). I did not find a base-M5 prefill number for this model. **Measure it** with `mlx_lm.benchmark -p 4096 -g 128`. Prefill time is what a user feels on "summarize this PDF".

## 6. Measurement plan for the first hour (feeds the speed-of-light meter)

On each Mac, alone and with the same apps open as in the demo:

```bash
mlx_lm.benchmark --model mlx-community/Qwen3-4B-Instruct-2507-4bit -p 512 -g 256 -n 3       # fits everywhere; baseline
mlx_lm.benchmark --model mlx-community/Qwen3-4B-Instruct-2507-4bit -p 512 -g 256 -n 3 -b 4  # batching gain
```

1. Run the 4B model at q4 and at bf16 if memory allows. Two points give that Mac's own effective bandwidth and fixed overhead per token (the section 3 fit).
2. Plug them into section 4's formula to predict the pool's tok/s before running it.
3. Then measure the pool end to end: distinct prompts, a fixed 512-token prompt, 256 generated tokens, wall-clock time.

Prediction versus measurement on stage is a strong, honest demo moment.

## Sources

- Configs: huggingface.co/mlx-community/Qwen3.6-35B-A3B-4bit/resolve/main/config.json and Qwen3-30B-A3B-4bit (fetched Nov 7 2026).
- mlx-lm main branch (fetched Nov 7 2026): `README.md` "Large Models" section; `mlx_lm/BENCHMARKS.md`; `mlx_lm/generate.py` (wired_limit about line 249, speculative_generate_step about line 496); `mlx_lm/models/cache.py` (`_BaseCache.is_trimmable` line 164, ArraysCache line 596 with `merge`); `mlx_lm/models/qwen3_5.py` (pipeline send/recv about lines 290 to 312); `mlx_lm/server.py` (is_batchable about line 475); `mlx_lm/SERVER.md` (kv-bits turns off batching).
- modelpiper.com/blog/iogpu-wired-limit-mb-mac (2026; 78% measured on a 32 GB M2 Max, macOS 26.5.2).
- ark.marianposaceanu.com/apple-silicon-gpu-memory-limits-wired-ram (2026).
- llmcheck.net/blog/qwen-36-35b-a3b-mac-new-number-one (Oct 2026; per-chip tok/s for Qwen3.6-35B-A3B).
- apple.com/macbook-pro/specs (M5: 153 GB/s).
