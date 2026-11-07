# 05: What we can honestly claim, and what to measure on day 1

## Three technique claims for the pitch

1. **The MoE model reads 1/9 of its weights per token, so it runs on laptops.**
   - Number: Nemotron-3-Nano-30B-A3B 5-bit holds 21.7 GB but reads 2.38 GB per token (3.46B of 31.8B parameters).
   - Claim: "The pool holds a model too big for any one of our Macs, and each token touches about 11% of it, which keeps it usable."
   - Backed by: section 04 math, plus measured pool tok/s.

2. **Memory-light context makes long legacy files cheap.**
   - Number: 6 KB of KV per token (only 6 of 52 layers use attention) + 49 MB of fixed Mamba state. A 32k-token file is 0.2 GB, versus 3.1 GB on a standard 30B MoE (Qwen3-30B-A3B).
   - Claim: "An agent can hold a whole legacy module in context on borrowed laptops."
   - Backed by: section 04 math, plus the memory reading before and after a long prompt.

3. **Physics-based networking: generating a token is limited by memory bandwidth and network delay, so we pool memory and wire the network.**
   - Number: the pool's ceiling is 55 tok/s (5-bit, 3 Macs). Estimates are 25 to 37 tok/s on Ethernet and 13 to 27 on Wi-Fi. Each token makes about 4 sequential network trips of 5.4 KB each, so per-trip delay is the cost and network bandwidth is not.
   - Claim: "Measured X tok/s on Ethernet vs Y on Wi-Fi, Z% of the physical ceiling."
   - Only say this after measuring both.

Do not claim kernel speedups. All the kernels are MLX's own; ours are system-level choices.

## Speed-of-light meter: worth it as a stretch, if it stays cheap

- Worth building **only** as one number on the dashboard: measured tok/s ÷ ceiling. That is about 30 lines.
  - The ceiling is a constant computed once per placement: 1 / Σ(stage bytes / bandwidth of that Mac) + slowest output-layer time.
  - Measured tok/s comes from EXO's response timing.
- It gives judges a reason to believe the numbers, using the same scoring method as GPU MODE and NVIDIA SOL-ExecBench.
- Skip it if the core demo is not done by Sunday 8am.
- A slide with the day-1 prediction vs measurement gives 80% of the value at zero code.

## Day-1 measurements (about 30 minutes, apps open as in the demo)

| # | Command / action | Backs |
|---|---|---|
| 1 | `sysctl hw.memsize`; `python3 -c "import mlx.core as mx;print(mx.device_info())"`; EXO `GET /state` free memory per Mac | 5-bit vs 6-bit choice; "too big for one Mac" |
| 2 | `ping -c 50 <each other Mac>` on Wi-Fi, then on Ethernet; record median and p95 | Claim 3 |
| 3 | On one M5, apps closed: `mlx_lm.benchmark --model mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-4Bit -p 512 -g 256 -n 3` | Single-Mac baseline; real fixed cost per token on the base M5 |
| 4 | Each Mac: `mlx_lm.benchmark --model mlx-community/NVIDIA-Nemotron-Nano-9B-v2-4bits -p 512 -g 256 -n 3`, then with `-b 4` | Routing speed and batching gain |
| 5 | Pool, 5-bit: 3 distinct 512-token prompts, 256 output tokens, wall clock, on Wi-Fi and on Ethernet | Claims 1 and 3; the meter |
| 6 | Pool: one 16k to 32k-token legacy file; read free memory per Mac before and after | Claim 2 |

Honest-benchmark rules: distinct prompts, fixed lengths stated on the slide, end-to-end wall clock, no cached outputs.
