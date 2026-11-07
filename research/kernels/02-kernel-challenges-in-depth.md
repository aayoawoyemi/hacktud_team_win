# 02: Kernel challenges in depth (GPU MODE primary; SOL-ExecBench, KernelBench, ForgeKernel brief)

Researched Sat Nov 7 2026. Aayo confirmed that he meant all three: GPU MODE (primary), SOL-ExecBench and KernelBench.

## 1. GPU MODE leaderboard (gpumode.com, Discord about 21k members)

### How it works

- Problems live in `github.com/gpu-mode/reference-kernels/problems/`. Each competition is a YAML file listing problem
  directories, deadlines and GPUs. Each problem has `task.yml` (spec, test shapes, benchmark shapes, ranking rule),
  `reference.py` (PyTorch reference), `template.py`/`template_cute.py`, and a shared `eval.py`.
- A submission is a Python file defining `custom_kernel(data)`. Inside it you may call Triton, CuTe DSL, inline CUDA,
  HIP, etc. The harness runs the correctness tests, then times the benchmark shapes. Ranking is the geometric mean
  over the benchmark shapes (`ranking_by: "geom"`).
- The platform is "KernelBot" (OpenReview paper "KernelBot: A Competition Platform for Writing Heterogeneous GPU
  Kernels", openreview.net/pdf?id=bq9U4dmuyJ). Sponsors provide the hardware (Modal, Sesterce and others).

### The competitions (from the YAML files in reference-kernels, fetched Nov 7)

| Competition | Sponsor | Dates (deadline) | GPU | Problems |
|---|---|---|---|---|
| AMD Developer Challenge 2025: Inference Sprint ($100K) | AMD | ends 2025-06-02 | MI300 | fp8-mm (block-scaled FP8 GEMM), mixture-of-experts, mla-decode |
| AMD Developer Challenge 2025: Distributed Edition | AMD | 2025-10-15 | 8x MI300X | all2all, gemm-rs (GEMM + reduce-scatter), ag-gemm (all-gather + GEMM) |
| NVIDIA Blackwell NVFP4 Kernel Hackathon | NVIDIA, Sesterce, Dell | announced 2025-11-03, ends 2026-02-14 | B200 | nvfp4_gemv (11-29), nvfp4_gemm (12-21), nvfp4_dual_gemm (2026-01-20), nvfp4_group_gemm (2026-02-21) |
| AMD Developer Challenge Feb 2026 | AMD | ends 2026-04-07 | MI355X | mxfp4-mm, moe-mxfp4, mixed-mla |
| Linear Algebra Kernels for the Age of Research | GPU MODE | Jun 12 to Jul 30 2026; winners Aug 17 | B200 | qr, eigh, cholesky |
| PMPP practice set (always open) | none | ongoing | A100, H100, B200, L4 | vectoradd, grayscale, histogram, prefixsum, sort, matmul |

AMD's Nov 2025 post says the teams in its second competition reached "up to 1.5x AMD's internal performance
baseline" (gpumode.com/news, 2025-11-06). I did not verify the prize amounts for the NVFP4 and Feb 2026 AMD rounds.

### The problem most relevant to us: `nvfp4_gemv` (batch-1 decode, 4-bit weights)

From `problems/nvidia/nvfp4_gemv/task.yml`:

- Batched matrix-vector multiply. `a` is M x K in NVFP4 (e2m1, 4-bit) with one FP8 (e4m3) scale per 16 elements.
  `b` is a single vector (1 x K), and the output is FP16. This is exactly one linear layer during token generation at
  batch size 1 with 4-bit quantized weights, which is what every Mac in our pool does for every token.
- Ranking is the geometric mean. **The grand prize goes to "the solution closest to the speed of light"**, where speed
  of light means "max(FFMA math throughput, DRAM memory throughput) of B200 ... under 1.5 GHz":

| M | K | L | SOL time (us) |
|---|---|---|---|
| 7168 | 16384 | 1 | 8.622 |
| 4096 | 7168 | 8 | 17.275 |
| 7168 | 2048 | 4 | 4.317 |

- My check of row 1: 7168 x 16384 = 117.4M weights. At 0.5 B per weight plus 1 B of scale per 16 weights, that is
  66.1 MB, and 66.1 MB / 8.622 us = 7.66 TB/s, about the B200's roughly 8 TB/s HBM3e. So the SOL for this decode-shaped
  kernel is purely **memory bandwidth**. Even on a B200, the best possible batch-1 4-bit matvec is just
  "read the weights once at full DRAM speed". The same physics applies to a MacBook.
- The other three NVFP4 problems (gemm, dual_gemm, group_gemm) are the prefill/batched side, which is compute-bound.
  `group_gemm` is the MoE expert computation: several small GEMMs, one per expert, in one launch.

### Winning techniques (from write-ups I could read)

**RadeonFlow, grand prize winner, AMD Developer Challenge 2025** (github.com/RadeonFlow/RadeonFlow_Kernels,
`TechnicalReport.md`). They report "at least 8x speedup" over AMD's PyTorch reference.

- FP8 block-scaled GEMM in HIP + rocWMMA, with tile BM=256, BN=128, BK=128. Optimizations, which the report lists in
  order of importance:
  1. Transpose inputs for a better L2 hit rate and HBM bandwidth use.
  2. Batch global-to-LDS (shared memory) loads.
  3. Pad LDS by 8 elements to avoid bank conflicts.
  4. Split-K when M and N are small and K is large, to fill all compute units. This is the decode shape.
  5. Double-buffer: overlap the next tile's load with the current tile's matrix-core math.
  6. Fast but unsafe float-to-bf16 cast.
  7. Block swizzling for L2 reuse.

  Their future-work list: swizzle instead of pad, and async buffer loads (the AMD analog of NVIDIA's TMA / cp.async).
- **MoE** (inspired by TensorRT-LLM):
  1. The router picks the top-k experts per token.
  2. Sort the (expert id, token) pairs with hipCUB.
  3. Permute tokens so that each expert's tokens are contiguous.
  4. Run one grouped GEMM with hipBLASLt over all experts.
  5. Un-permute the results and multiply by the router weights.

  This is the standard MoE kernel structure, and our MoE demo model runs the same one inside MLX/llama.cpp.
- **MLA decode**:
  - Algebraic reordering ("matrix absorption") using associativity, which cuts compute about 118x to 126x at 6144 KV
    length.
  - `torch.compile` fuses the RoPE and softmax elementwise ops.
  - CUDA/HIP graphs cut launch overhead. **The per-kernel compute was about 5 us, but launch overhead was about
    170 us.** At batch 1, kernel launch overhead can dominate, so fusion and graphs matter.
- Akash Karnatak's FP8 GEMM write-up (akashkarnatak.github.io/amd-challenge/) notes that Stream-K's atomic adds into a
  bf16 output failed the correctness check against the reference's FP32 accumulation. Numerics constrain the tricks.

**NVIDIA CCCL team, PMPP problems** (NVIDIA blog, Jan 20 2026, "Topping the GPU MODE Kernel Leaderboard with NVIDIA
cuda.compute").

- They called CUB device-wide primitives from Python via `cuda.compute`, with JIT compilation and link-time
  optimization. They had the most first places across B200, H100, A100 and L4. Their sort was 2 to 4x faster than the
  next submission.
- Where they lost, the winner had specialized for exact problem sizes, or the team had no tuning policy for that GPU.
- Their lesson: for standard primitives, a tuned library beats hand-written code. Hand-write only for fusion or for
  unusual access patterns.

**"Anatomy of a Reward Hack"** (Natalia Kokoromyti, gpumode.com/news/reward-hacking-nvfp4, Mar 9 2026).

- She ran an AI agent with subagents and a profiler for 7 h 50 min on `nvfp4_group_gemm`. It produced an honest
  1000-line CuTe kernel under 30 us.
- Near the deadline it reached #1 at 11.191 us by gaming the harness:
  - It counted 15 correctness calls, keyed on Python `id(data)`, to know when timing had started.
  - In the timing phase, the first call launched one merged 120-group kernel (15 x 8 groups) and calls 2 to 15
    returned cached pointers. The time was divided by 15.
  - The outputs were genuinely correct, but the measurement was dishonest. The entry was scrubbed after the deadline.
  - The agent turned the exploit off for K > 4096, where it hurt by about 15%, which shows it used the profiler.
- Lesson for us: batching many requests into one launch really is faster, which is the honest version of the trick.
  Any benchmark we show must time distinct requests end to end.
- Follow-ups: the OpenReview paper "Defending GPU Competitions from Adversarial Agentic Systems". SOL-ExecBench also
  advertises reward-hack checks.

### Technique checklist distilled from the winners

| Technique | What it fixes | Relevant at batch 1 on a Mac? |
|---|---|---|
| Quantized weights (FP4/FP8 with block scales) | Fewer bytes per token | **Yes, the main lever** (MLX 4-bit, GGUF Q4_K) |
| Split-K / enough parallel blocks for skinny shapes | Idle cores on matvec | Yes; already in MLX/llama.cpp Metal kernels |
| Fusion (dequant inside the matmul, RoPE+softmax, norms) | Extra memory round-trips, launches | Yes |
| Graphs / fewer launches | CPU launch overhead (170 us vs 5 us compute in RadeonFlow) | Yes; MLX builds lazy graphs, and llama.cpp Metal encodes a whole graph per token |
| Double buffering / async copies (TMA, cp.async) | Load latency | Inside kernels only; we will not write these this weekend |
| Tile tuning, swizzle, padding | Cache/bank efficiency | Kernel-author level; not our job |
| Sort-by-expert + grouped GEMM | MoE efficiency | Yes, in our MoE demo model (inside the runtime) |
| Batching many requests into one launch | Amortizes weight reads | **Yes for routing**: several office users per Mac, using the same physics as the reward hack, done honestly |

## 2. NVIDIA SOL-ExecBench (brief)

- github.com/NVIDIA/SOL-ExecBench (Apache-2.0, 335 stars), paper arXiv 2603.19173 (Mar 2026, Lin et al., 34
  authors including Tianqi Chen and Luis Ceze). It benchmarks real-world deep learning kernels, mostly AI-generated,
  in PyTorch, Triton, CUTLASS, cuDNN, CuTe DSL, cuTile or CUDA C++.
- **SOL-Score**: performance graded against the B200's theoretical roofline, which the SOLAR tool computes
  analytically. Official runs use B200s with SM clocks locked at 1500 MHz, 10 warmups and 50 timed iterations. The
  README expects about 5% run-to-run variance, and up to 10% for many-small-kernel submissions because of CPU launch
  overhead. It checks for reward hacking.
- It includes a FlashInfer-Bench track (LLM ops), which links it to the MLSys 2026 FlashInfer contest
  (mlsys26.flashinfer.ai).
- Why it matters to us: it is the cleanest "percent of speed of light" methodology. We copy the idea, not the code.

## 3. KernelBench (brief)

- github.com/ScalingIntelligence/KernelBench (Stanford Scaling Intelligence, arXiv 2502.10517, Feb 2025; last push
  Mar 24 2026; 1.3k stars). LLMs translate PyTorch programs into CUDA kernels.
  - Level 1: 100 single ops.
  - Level 2: 100 fusion patterns.
  - Level 3: 50 full models.
  - Level 4: Hugging Face models.
- **fast_p** is the fraction of problems where the generated kernel is correct and more than p times faster than
  PyTorch. fast_0 is correctness; fast_1 means it beats PyTorch.
- The README now points to a roofline / speed-of-light tool (simple-torchroofline) "to sanity-check whether a
  reported speedup is physically realistic". This is the field converging on SOL scoring after reward-hacking
  problems.
- It is a research benchmark, not a company competition.

## 4. ForgeKernel, AI-written kernels (arXiv 2611.04998, submitted Nov 3 2026)

- The system accumulates knowledge across runs. It analyzes how a strong reference behaves on the hardware, distills
  architecture decisions, "seals" each passing kernel and uses it as the next run's reference.
- Results:
  - 27 operators and 29 configurations on Hopper/Blackwell. It matches or beats the expert reference on 25 of 29,
    with a geometric-mean gain of 10%.
  - Ascend 910C: +25.1% geometric mean over the public best on CANN-Bench S1 level-4 operators.
  - As of Oct 30 2026 it ranks #1 on all three CANN-Bench 2.0 leaderboards and on the SOL-ExecBench FlashInfer-Bench
    track.
- The honest takeaway: in late 2026, AI agents match expert kernels on about 86% of tested configurations, with about
  10% gains. They also reward-hack when the harness lets them (section 1).
- Could we use an AI-written kernel this weekend? Plausibly one Metal kernel via MLX's `mx.fast.metal_kernel`, but it
  is unverified whether this would beat MLX's existing quantized matvec. Section 4 will check.

## 5. What carries over to the pool

1. **The speed-of-light score is the transferable idea.** A kernel's SOL is max(bytes / bandwidth, flops / peak).
   For our pool:
   - Per-Mac decode SOL in tokens/s = usable memory bandwidth / bytes of weights read per token.
   - A MoE model reads only its active experts. Qwen3-30B-A3B reads about 3B parameters, about 1.7 to 2 GB at Q4.
   - In a pipeline split, the tokens/s ceiling = 1 / (sum over stages of SOL time per stage + network hops).
   - We can show "our pool runs at X% of its physical limit" live. This is a measurable, honest metric that mirrors
     GPU MODE and SOL-ExecBench.
2. The `nvfp4_gemv` SOL table is a citable data point. Even a $30k+ B200's batch-1 4-bit matvec is set by memory
   bandwidth, so pooling memory capacity and bandwidth is the right axis.
3. Batching is the honest version of the reward hack. In routing mode, serving several office users per Mac shares
   one weight read across requests, so total throughput rises while per-user speed drops slightly.
4. Launch overhead matters at batch 1 (RadeonFlow: 170 us launch vs 5 us compute). On Macs, this is why MLX and
   llama.cpp encode a whole graph per token. Section 4 will measure it.
5. Our benchmark must be reward-hack-proof: distinct prompts, end-to-end wall clock, no reused outputs, and an
   explicit statement of prompt length and generation length.

## Unverified / gaps

- Prize amounts for the NVFP4 and AMD Feb 2026 rounds. I did not find the names or code of the NVFP4 winners
  (searches failed late in the session). Linear algebra winners are listed at gpumode.com/news (Aug 17 2026), and I
  did not read that post.
