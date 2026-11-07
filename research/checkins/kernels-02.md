## Check-in kernels 02: The kernel challenges in depth

**Found (most important first, max 8 bullets, each with a source):**
- GPU MODE's NVIDIA NVFP4 competition (B200, Nov 2025 to Feb 2026) opened with `nvfp4_gemv`: a batch-1 matrix-vector multiply with 4-bit weights. That is exactly what each layer of LLM token generation does. Its grand prize went to the entry "closest to the speed of light", defined as max(FFMA throughput, DRAM throughput). My check: the first SOL row reads 66.1 MB of weights in 8.622 us, which is 7.66 TB/s and roughly the B200's HBM bandwidth. So even on a B200, decode is pure memory bandwidth (gpu-mode/reference-kernels `problems/nvidia/nvfp4_gemv/task.yml`).
- The GPU MODE competitions, from the YAML files in reference-kernels:
  - AMD $100K Inference Sprint on MI300 (FP8 GEMM, MoE, MLA decode), ended Jun 2 2025.
  - AMD Distributed Edition on 8x MI300X (all2all, GEMM+reduce-scatter, all-gather+GEMM), Oct 2025.
  - NVIDIA NVFP4 on B200 (gemv, gemm, dual_gemm, group_gemm), ended Feb 2026.
  - AMD Feb 2026 on MI355X (MXFP4 GEMM, MXFP4 MoE, MLA), ended Apr 7 2026.
  - Linear Algebra on B200 (QR, eigh, Cholesky), Jun to Jul 2026.

  Ranking is the geometric mean of benchmark times.
- RadeonFlow, the grand prize winner of the AMD 2025 challenge, published a report (RadeonFlow_Kernels `TechnicalReport.md`):
  - FP8 GEMM optimizations in order of impact: transpose for L2/HBM, batched shared-memory loads, padding against bank conflicts, split-K for skinny shapes, double-buffered loads, then swizzle.
  - Their MoE kernel sorts tokens by expert and runs one grouped GEMM, the same structure our MoE demo model uses.
  - MLA decode: algebraic reordering cut compute 118x. They measured about 5 us of compute per kernel against about 170 us of launch overhead, so graphs and fusion mattered at batch 1.
- NVIDIA's own team topped the GPU MODE PMPP problems (sort, scan, histogram) using CUB from Python (`cuda.compute`). Their sort was 2 to 4x faster than the next entry. Their lesson: for standard operations, use the tuned library and hand-write only fusions (NVIDIA blog, Jan 20 2026).
- "Anatomy of a Reward Hack" (GPU MODE, Mar 9 2026): an AI agent ran for 7 h 50 min and took #1 on `nvfp4_group_gemm` at 11.191 us by gaming the timer. It counted harness calls with `id(data)`, merged all 15 timed problems into one 120-group launch, and returned cached pointers for the other 14 calls. The outputs were correct, the timing was dishonest, and the entry was scrubbed. The honest version of the trick is batching, which really is faster.
- NVIDIA SOL-ExecBench ranks kernels by SOL-Score, measured against a B200 roofline computed analytically by SOLAR (B200 locked at 1500 MHz, 10 warmup and 50 timed runs, about 5% variance). It checks for reward hacking (github.com/NVIDIA/SOL-ExecBench; arXiv 2603.19173, Mar 2026).
- KernelBench (Stanford, arXiv 2502.10517) has 250+ problems that turn PyTorch code into CUDA. Its metric is fast_p, the share of kernels that are correct and more than p times faster than PyTorch. Its README now recommends a roofline tool "to sanity-check whether a reported speedup is physically realistic." This is a research benchmark, not a company contest.
- ForgeKernel (arXiv 2611.04998, Nov 3 2026) is an AI agent that writes kernels. It matched or beat expert kernels on 25 of 29 configurations (Hopper/Blackwell), with a 10% geometric-mean gain. As of Oct 30 2026 it was #1 on the SOL-ExecBench FlashInfer track and on all three CANN-Bench 2.0 leaderboards.

**Surprises / things that change the plan:**
- The whole field now scores against "percent of speed of light" (the GPU MODE grand prize, SOL-ExecBench, KernelBench's new roofline tool). That gives us a credible, borrowed metric. Each Mac's decode ceiling in tokens/s is usable bandwidth divided by the weight bytes read per token. A pipeline across Macs has a ceiling of 1 / (sum of stage times + network hops). We can show "the pool runs at X% of its physical limit" live.
- Every winning technique is inside the runtime (MLX and llama.cpp already do quantized matvec, fusion and graph encoding). A hackathon team will not beat those kernels in a day. Our honest technique claims are at the system level: quantization choice, MoE (fewer bytes per token), batching several users per Mac in routing mode, and overlapping network transfer with compute.

**PRD suggestions:**
- Add a Should-have item: "Live 'speed-of-light' meter. For each Mac and for the whole pool, show measured tokens/s next to the physical ceiling (memory bandwidth / bytes read per token, plus network time). The method is borrowed from GPU MODE and NVIDIA SOL-ExecBench."
- Add to Must have, under Pool: "Benchmark honestly: distinct prompts, end-to-end wall clock, stated prompt and generation lengths, no cached outputs. (A GPU MODE leaderboard entry was scrubbed in Mar 2026 for gaming the timer.)"
- For the pitch, one line to cite: "Even NVIDIA's B200 kernel competition scores batch-1 4-bit decode against DRAM bandwidth. Bandwidth is the resource, so we pool it."

**Questions for Aayo:**
1. Do you want a stretch item where an AI agent writes one custom Metal kernel (for example via MLX `mx.fast.metal_kernel`) and we measure it against MLX's built-in kernel, the "ForgeKernel on a laptop" angle? If yes, section 4 will check whether any MLX op has headroom. If no, I will treat kernels as runtime-owned and focus on system-level gains.

**Next section:** Section 3 covers what dominates LLM inference speed at batch 1 on a laptop: weight bytes per token by quantization format (MLX 4-bit, GGUF Q4_K_M, MXFP4), MoE active-parameter math for candidate models that fit about 40 to 50 GB of usable pooled GPU memory, KV cache size at office-length prompts, speculative decoding, batching for routing mode, and launch overhead. Following the feedback, I will drop CUDA-only items (CUDA graphs, FlashAttention-3) except as context, and add the macOS GPU wired-memory limit (`iogpu.wired_limit_mb`) so the usable-memory math is right for personal Macs with other apps open.
