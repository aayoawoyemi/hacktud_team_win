# 01: The physics of LLM inference on one device and across devices

Worker: principles. Written Sat Nov 7, 2026. Numbers marked (measured) come from a cited source; (calc) are my
arithmetic from stated inputs; (guess) are estimates that need a test on our hardware.

## 1. What limits one device

### Two phases with different limits

- **Prefill** (reading the prompt): all prompt tokens go through the model at once as a matrix multiply. Cost is
  about 2 × parameters FLOPs per token: 16 GFLOP/token for an 8B model, 140 GFLOP/token for 70B (calc). This is
  **compute-bound**. It sets time-to-first-token (TTFT).
- **Decode** (writing the answer): one token at a time. Every token reads all weights from memory once (plus the KV
  cache). This is **memory-bandwidth-bound**: tokens/sec ≤ effective bandwidth ÷ model bytes.
- **Memory capacity** decides whether the model fits at all: weights + KV cache + runtime overhead.

### Model sizes used below

| Model | Layers | Hidden size d | KV heads × dim | Q4_K_M file | KV cache per token (fp16) | KV at 8K ctx |
|---|---|---|---|---|---|---|
| Llama 3.1 8B | 32 | 4096 | 8 × 128 | ~4.9 GB | 2·32·1024·2 B = 128 KiB | 1 GiB |
| Llama 3.3 70B | 80 | 8192 | 8 × 128 | ~42.5 GB | 2·80·1024·2 B = 320 KiB | 2.5 GiB |

(Architecture numbers from the Llama 3 model configs; file sizes from common bartowski GGUF uploads, approximate.)

### Measured single-device speeds (Apple Silicon)

Source: llama.cpp discussion #4167 "Performance of llama.cpp on Apple Silicon M-series"
(https://github.com/ggml-org/llama.cpp/discussions/4167, summary table, read via GitHub API Nov 7 2026). Llama 2 7B,
Q4_0 (3.56 GB), PP = prompt processing at 512 tokens, TG = text generation of 128 tokens.

| Chip (GPU cores) | Bandwidth GB/s | Q4_0 PP t/s | Q4_0 TG t/s | Effective BW in TG (TG × 3.56 GB) |
|---|---|---|---|---|
| M1 (8) | 68 | 118 | 14.2 | 50 GB/s (74%) |
| M2 (10) | 100 | 180 | 21.9 | 78 GB/s (78%) |
| M3 (10) | 100 | 187 | 21.3 | 76 GB/s (76%) |
| M3 Pro (18) | 150 | 342 | 30.7 | 109 GB/s (73%) |
| M4 (10) | 120 | 221 | 24.1 | 86 GB/s (72%) |
| M4 Pro (20) | 273 | 440 | 50.7 | 180 GB/s (66%) |
| M4 Max (40) | 546 | 886 | 83.1 | 296 GB/s (54%) |
| M5 (10) | 154 | 723 | 31.9 | 114 GB/s (74%) |
| M5 Pro (20) | 307 | 1621 | 66.3 | 236 GB/s (77%) |

Takeaways:
- Decode tracks bandwidth closely: llama.cpp reaches 55 to 78% of spec bandwidth (calc).
- Prefill tracks GPU compute. M5 prefill is ~3x M4 at the same core count (M5 has matmul units in the GPU).
- **Llama 3.1 8B Q4_K_M (4.9 GB) decode estimate** (calc, effective BW ÷ 4.9 GB): M1 ~10 t/s, M2/M3 ~15.5,
  M4 ~17.5, M5 ~23, M4 Pro ~37, M5 Pro ~48.
- **70B Q4_K_M (42.5 GB) decode estimate** if it fit on one chip (calc): M4 ~2.0 t/s, M4 Pro ~4.2, M4 Max ~7.

### Memory capacity on MacBooks

- macOS lets the GPU wire only part of unified memory by default (roughly 2/3 on small-RAM Macs, 3/4 on larger;
  can be raised with `sudo sysctl iogpu.wired_limit_mb=<MB>`). (Commonly reported in llama.cpp/MLX issues;
  exact defaults to verify on our Macs.)
- A 16 GB MacBook gives ~10 to 11 GB to the GPU by default, a 24 GB one ~16 GB, a 36 GB one ~27 GB (calc from the
  ratios above, guess on exact values).
- So: 8B Q4 fits any MacBook. 70B Q4 (42.5 GB + 2.5 GB KV at 8K) fits no MacBook under 64 GB. That gap is the only
  reason to split one model across machines.

### The Windows Dell (no discrete GPU)

- CPU decode is bandwidth-bound too. Dual-channel DDR4-3200 = 51.2 GB/s spec, DDR5-5600 = 89.6 GB/s spec; many
  cheap laptops ship single-channel (half that). Expect 25 to 55 GB/s effective (guess).
- A 2026 CPU-tuning guide reports Q4_K_M 8B at "~5-8 tok/s on AVX2 desktop, ~10-15 on Zen 4 AVX-512" and states
  token rate scales linearly with memory bandwidth (bigiron.cc, "llama.cpp tuning for CPU-only homelab inference",
  https://www.bigiron.cc/guides/llama-cpp-tuning-for-cpu-only-homelab-inference, 2026).
- Laptop CPU prefill is slow: tens of tokens/sec on 8B (guess; measure with `llama-bench` on the Dell).
- Per GB of model it holds, the Dell is 2 to 3x slower than a base M4 and 4 to 6x slower than an M4 Pro (calc).

### Prefill is the hidden cost for office use

Office prompts are long (a contract, a client file, RAG context of 2,000 to 8,000 tokens).
- 8B, 2,000-token prompt: M4 ~10 s TTFT, M4 Pro ~5 s, M5 ~3 s (calc from PP numbers, scaled 7B→8B).
- 70B pooled across base M4 MacBooks: prefill ~8.75x slower than 8B → ~23 t/s → **~85 s TTFT for 2,000 tokens**
  (calc). Splitting layers across machines does not make one request's prefill faster (stages run in sequence).

## 2. What limits a multi-device setup

### Network numbers

| Link | Usable bandwidth | Round-trip latency | Source |
|---|---|---|---|
| Venue Wi-Fi (client → AP → client) | ~10 to 40 MB/s, shared, both hops use airtime | 3 to 5 ms typical, spikes 15 to 30 ms | gamerhardware.org Wi-Fi vs Ethernet (Wi-Fi 6E avg 3-5 ms, spikes 15-30 ms); bandwidth is a guess |
| 1 GbE through a switch | ~112 MB/s | ~0.2 to 0.5 ms with macOS/Windows TCP | standard; latency a guess incl. OS stack |
| 2.5 GbE | ~280 MB/s | similar | Geerling cluster tests |
| Thunderbolt bridge, TCP (TB4/TB5) | TB5 "50-60 Gbps real-world" | "300 μs" memory-access latency over TCP | Geerling, Dec 18 2025 (below) |
| Thunderbolt 5 RDMA (macOS 26.2+) | same link | "< 50 μs" | Geerling, Dec 18 2025; Apple TN3205 (Mar 19 2026) |

- RDMA over Thunderbolt needs macOS 26.2+, Thunderbolt 5 on both ends, and `rdma_ctl enable` run from Recovery OS
  (Geerling blog; MLX PR #2808 comments: "it's Tb5 only so it won't work on M2", awni, Dec 2025). TB5 MacBooks are
  the M4 Pro/Max (and later Pro/Max) MacBook Pros. MacBook Airs and base-chip MacBook Pros are TB4/USB4: TCP only.
- TB5 has no switches; RDMA clusters are full-mesh cables, max 4 Macs today (Geerling; Apple TN3205).
- Sources: https://www.jeffgeerling.com/blog/2025/15-tb-vram-on-mac-studio-rdma-over-thunderbolt-5/ (Dec 18 2025),
  https://developer.apple.com/documentation/technotes/tn3205-low-latency-communication-with-rdma-over-thunderbolt
  (Mar 19 2026), https://github.com/ml-explore/mlx/pull/2808 (JACCL backend, merged Dec 2025, in mlx 0.30.1).

### Software overhead is as big as the network

- llama.cpp RPC on **localhost** (no network at all) cut GPT-OSS 20B TG to 72% and pp2048 to 63% of direct
  (u/StorageHungry8380, Jan 11 2026); another user saw 50 → 38 t/s putting one GPU behind RPC on the same PC, and
  10 Gb → 50 Gb Ethernet changed nothing (37 → 38 t/s). vLLM+Ray on the same 10 Gb network: 69 t/s.
  (https://www.reddit.com/r/LocalLLaMA/comments/1q9yd1w/llamacpp_rpc_experiment/, Jan 11-13 2026)
- One user saw only ~50 Mbps between 2 RPC nodes: the link is idle most of the time; per-message latency dominates,
  not bandwidth (same thread, u/TableSurface).
- Geerling, 2× M3 Ultra over 2.5 GbE, llama.cpp RPC (build 7315), Llama 3.2 3B Q4_K_M: **171.16 → 93.40 t/s** going
  from 1 to 2 nodes (Dec 10 2025, https://github.com/geerlingguy/beowulf-ai-cluster/issues/17). That is
  1/93.4 − 1/171.2 = **4.9 ms added per token for one extra node on wired Ethernet** (calc). I use 5 ms/hop wired
  as the llama.cpp RPC cost below; Wi-Fi I estimate at 10 to 30 ms/hop (guess: several RPC round trips per token ×
  Wi-Fi RTT).
- Geerling, Qwen3 235B: llama.cpp RPC got slower as nodes were added; EXO with RDMA tensor parallel went from
  19.5 t/s (1 node) to 31.9 t/s (4 nodes) (blog Dec 18 2025; GIGAZINE summary Dec 22 2025).

## 3. Ways to share work, bytes per token, expected effect

Notation: d = hidden size, L = layers, N = devices, RTT = network round trip.

### a) Request routing (each device runs a whole model; a router sends each request to a free device)
- Network per token: only the streamed text back to the user (a few bytes). Prompt sent once.
- Single-user speed: same as the best device. No speedup and no slowdown.
- Multi-user throughput: ~N× (calc). Works on any Wi-Fi.
- Limit: every device must hold the whole model. Caps model size at the smallest device used.

### b) Batching (one device serves several users in one forward pass)
- Decode reads weights once per step for the whole batch, so 4 to 8 concurrent users cost little more than 1 until
  compute becomes the limit. llama-server does this with `--parallel N` (continuous batching).
- Cost: KV cache per user (128 KiB/token for 8B: 8 users × 8K ctx = 8 GiB).
- For an office of 5 to 20 people, routing + batching on whole-model copies gives the most total throughput.

### c) Pipeline / layer parallel (each device holds a block of layers; activations pass device to device)
- Network per token per boundary: one hidden-state vector, d × 4 bytes (llama.cpp passes f32): **16 KiB for 8B,
  32 KiB for 70B** (calc). Bandwidth is irrelevant for decode; latency per hop matters.
- Prefill per boundary: T × d × 4 bytes. 2,000-token prompt: 32 MB (8B), 64 MB (70B) (calc). ~1 to 5 s per hop on
  Wi-Fi, ~0.3 to 0.6 s on 1 GbE, negligible on Thunderbolt (calc from bandwidth table).
- Single-stream decode: **no speedup**. Time per token = sum of each slice's (bytes ÷ its bandwidth) + hop costs.
  The model just gets slower devices' bandwidth mixed in.
- Throughput can rise with several requests in flight (each stage works on a different request) but llama.cpp RPC
  does not overlap stages across requests this way, as far as I know (unverified; check in section 3).
- The win is **capacity**: run a model no single machine can hold.
- Weighting: give each device layers in proportion to its memory and bandwidth. A slow device (the Dell) should
  hold few layers or none.

### d) Tensor parallel (every layer split across devices; devices sum partial results)
- Megatron-style split needs **2 all-reduces per layer** (after attention, after MLP) (Shoeybi et al. 2019,
  arXiv:1909.08053). Per token: 64 all-reduces for 8B, 160 for 70B; each carries d × 2 bytes = 8 KiB / 16 KiB.
- Latency-bound: time added per token ≈ 2L × all-reduce latency.

| Link | all-reduce latency (2 nodes) | 8B: added ms/token | 70B: added ms/token |
|---|---|---|---|
| Wi-Fi | ~5 ms (guess) | 320 | 800 |
| 1 GbE TCP | ~0.3 to 0.5 ms (guess) | 19 to 32 | 48 to 80 |
| TB TCP | ~0.3 ms (Geerling "300 μs") | ~19 | ~48 |
| TB5 RDMA | < 0.05 ms (Geerling) | ~3 | ~8 |

- Compute and bandwidth split N ways, so decode can be ~N× faster, but only when added latency is much smaller
  than per-token compute time. That is true only on TB5 RDMA. On Wi-Fi tensor parallel is 5 to 20x slower than one
  machine (calc).
- Prefill all-reduces carry T × d × 2 bytes each: 2,000 tokens × 8B = 16 MB × 64 = 1 GB per prompt (calc). Only
  Thunderbolt carries that in reasonable time.

### e) Expert parallel (MoE models; experts live on different devices)
- Per token per MoE layer: send the hidden state to the device(s) holding the chosen experts and get results back,
  2 messages per layer. Same latency profile as tensor parallel. Not viable over Wi-Fi.
- MoE matters for us for a different reason: decode reads only active weights. Qwen3-30B-A3B (~3.3B active,
  ~18.6 GB at Q4) and gpt-oss-20b (~3.6B active, ~13 GB MXFP4) decode at small-model speed while needing big-model
  memory (sizes approximate). Pipeline-splitting a MoE across two 16 GB MacBooks gives a model neither laptop holds
  alone, at roughly 30 to 50 t/s compute-side on M4-class chips (guess; ~2 GB read per token ÷ 86 GB/s = 23 ms).
  Network cost per hop is the same 16 KiB as dense pipeline.

### f) Speculative decoding across devices
- A small draft model proposes k tokens; the big model checks all k in one forward pass. In bandwidth-bound decode,
  checking k tokens costs about the same as producing 1.
- Network per round: k token IDs (4 bytes each) plus the accepted count back. One round trip per k tokens.
- Typical speedup 1.5 to 3x depending on acceptance rate (Leviathan et al. 2023, arXiv:2211.17192, reported 2-3x).
- Fits pipeline split well: the hop cost is paid once per k tokens instead of once per token. Draft model can run
  on the coordinator (or the Dell). Whether llama.cpp's `-md` draft works together with RPC splitting is unverified.

## 4. Back-of-envelope: 8B and 70B on 2 to 4 MacBooks

Assumptions: base M4 MacBook (86 GB/s effective, 16 to 24 GB) unless noted; llama.cpp RPC for pipeline at 5 ms/hop
wired and 10 to 30 ms/hop Wi-Fi; tensor parallel latencies from the table above; decode only, short context.

### Llama 3.1 8B Q4_K_M (4.9 GB)

| Setup | Wi-Fi | 1 GbE switch | Thunderbolt (TCP) | TB5 RDMA (M4 Pro MBPs) |
|---|---|---|---|---|
| 1 Mac, no network | 17.5 t/s | same | same | 37 t/s (M4 Pro) |
| Routing, N Macs | 17.5 t/s per user, N users at once | same | same | same |
| Pipeline, 2 Macs | 11 to 15 t/s | ~16 t/s | ~16 to 17 t/s | n/a (no RDMA in llama.cpp) |
| Pipeline, 4 Macs | 7 to 11 t/s | ~14 t/s | ~15 t/s | n/a |
| Tensor parallel, 2 Macs | ~3 t/s | 16 to 21 t/s | ~21 t/s | ~45 to 55 t/s (M4 Pro) |

Conclusion for 8B: splitting never beats one Mac except tensor parallel over TB5 RDMA on M4 Pro machines. Routing is
the right mode for models that fit.

### Llama 3.3 70B Q4_K_M (42.5 GB + 2.5 GB KV at 8K)

Fit: needs ~45 GB of GPU-wired memory. 4 × 16 GB MacBooks (~43 GB default) does not fit without raising
`iogpu.wired_limit_mb`; 3 × 24 GB (~48 GB) barely fits; 4 × 24 GB fits (calc, guess on defaults).

| Setup | Wi-Fi | 1 GbE switch | Thunderbolt (TCP) | TB5 RDMA |
|---|---|---|---|---|
| Pipeline, 4 base M4 Macs | ~1.7 to 1.9 t/s | ~2.0 t/s | ~2.0 t/s | n/a |
| Pipeline, 2 to 4 M4 Pro Macs | ~3.5 to 4 t/s | ~4 t/s | ~4 t/s | n/a |
| Tensor parallel, 4 M4 Pro Macs | < 1 t/s | ~7 to 9 t/s (guess) | ~8 t/s (guess) | ~12 to 15 t/s (guess) |
| TTFT, 2,000-token prompt, base M4 pool | ~90 s | ~85 s | ~85 s | n/a |

Conclusion for 70B: network barely matters for pipeline decode (500 ms of compute per token swamps 15 to 90 ms of
hops), so pipeline over Wi-Fi "works", but 2 t/s and 85 s TTFT on base MacBooks is a poor product experience. The
70B demo only looks good on M4 Pro/Max machines (4 t/s pipeline) or TB5 RDMA tensor parallel.

## 5. What this means for the plan

1. Pool (pipeline) is for capacity, not speed. Route for speed. Use both: route requests that fit one machine,
   pool only for the model nobody can hold alone.
2. The best "wow" per watt is a MoE model split across two or three laptops (e.g. Qwen3-30B-A3B, ~18.6 GB at Q4):
   too big for one 16 GB MacBook, still 20+ t/s pooled (guess). A dense 70B pooled on base MacBooks is 2 t/s.
3. Wired beats Wi-Fi mostly for reliability (latency spikes), not throughput. Bring a gigabit switch and USB-C
   Ethernet adapters; Thunderbolt cables for a 2-Mac link.
4. Venue Wi-Fi risk: campus and event Wi-Fi often isolates clients from each other and blocks mDNS/multicast, which
   would stop machines seeing each other at all (general knowledge; not verified for HackUTD's network). A
   private router removes the risk.
5. Tensor parallel is only worth it with TB5 RDMA (M4 Pro/Max MacBook Pros, macOS 26.2+, Recovery-mode enable).
   Probably out of scope unless the team has 2+ such machines.
6. The Dell is a poor compute node for pooling (2 to 6x slower per GB). Better roles: router / web front end /
   control plane, a small whole model for routing (8B at 5 to 10 t/s), embeddings for RAG, or a speculative-decoding
   draft model. Section 4 covers whether it can join a Metal pool.
7. Measure, do not trust these numbers: run `llama-bench` on every demo machine (8B Q4_K_M, pp512/tg128) and
   `ping` between machines on the venue network in the first hour.
