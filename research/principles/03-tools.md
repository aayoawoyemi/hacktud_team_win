# 03: The tools that exist, and which one pools a MoE across our 3 Macs

Worker: principles. Written Sat Nov 7, 2026. (measured) = from the cited source; (calc) = my arithmetic;
(guess) = estimate to test on our machines. EXO code is covered by the exo worker; here only EXO benchmarks and fit.

Our hardware (from Aayo): M5 24 GB, M3 16 GB, third Mac 24 GB (likely M5). Personal machines with other apps open.
Travel router likely; venue Wi-Fi as fallback. No Thunderbolt 5 (base M5 and M3 MacBooks are TB4/USB4, so no RDMA).

## 1. Usable GPU memory on a non-clean 16 / 24 GB Mac

- macOS caps how much unified memory the GPU may wire. llama.cpp logs it as `recommendedMaxWorkingSetSize`.
  On a 16 GB M1: `recommendedMaxWorkingSetSize = 10922.67 MB` (2/3 of RAM) (abetlen/llama-cpp-python #687, Sep 2023).
  A 2026 M5 report logs `16384.02` MB (withcatai/node-llama-cpp #549, Feb 6 2026), consistent with 2/3 of 24 GB
  (my inference that it was a 24 GB M5).
- Raise it with `sudo sysctl iogpu.wired_limit_mb=N`, N "larger than the size of the model in megabytes but smaller
  than the memory size of the machine" (mlx-lm README). It resets on reboot.
- The limit is a ceiling, not a reservation. Other apps still hold their RAM. If the model plus apps exceed RAM, macOS
  compresses and swaps, and generation stalls (mlx-lm prints "[WARNING] Generating with a model that requires ...").

Realistic numbers for our Macs (calc + guess; measure with Activity Monitor "Memory Used" before the demo):

| Mac | RAM | Default GPU cap | Typical other apps (browser, Slack, IDE) | Usable for model + KV, apps open, default cap | Apps quit, `wired_limit_mb` raised to RAM − 5 GB |
|---|---|---|---|---|---|
| M5 24 GB | 24 GB | ~16 GB | 6 to 10 GB | ~12 to 14 GB | ~18 to 19 GB |
| M3 16 GB | 16 GB | ~10.7 GB | 5 to 8 GB | ~7 to 9 GB | ~10 to 11 GB |
| M5 24 GB (3rd) | 24 GB | ~16 GB | 6 to 10 GB | ~12 to 14 GB | ~18 to 19 GB |
| **Pool total** | 64 GB | ~43 GB | | **~31 to 37 GB** | **~46 to 49 GB** |

Demo-day rule: quit browsers/Docker/Xcode on every Mac, set `sudo sysctl iogpu.wired_limit_mb=19456` on the 24 GB
Macs and `11264` on the 16 GB Mac, keep KV context at 4K to 8K.

## 2. Which MoE model is "too big for any one of our Macs"

Model file sizes from Hugging Face API (Nov 7 2026):

| Model | Format | Size | Fits one 24 GB Mac? | Fits our pool? |
|---|---|---|---|---|
| Qwen3-30B-A3B-Instruct-2507 | GGUF Q4_K_M | 18.6 GB | Only with apps quit and limit raised | Yes, easily |
| Qwen3.6-35B-A3B | GGUF UD-Q4_K_M | 22.1 GB | No (over 19 GB) | Yes |
| Qwen3.6-35B-A3B | GGUF UD-Q6_K | 29.3 GB | No (over the Mac's RAM) | Yes, with limits raised |
| Qwen3.6-35B-A3B | GGUF Q8_0 | 36.9 GB | No | Tight: only with all apps quit, short context |
| Qwen3.6-35B-A3B | MLX 4bit / 6bit / 8bit (mlx-community) | ~20 / ~28 / ~37 GB (calc from bits; check HF) | No | 4 and 6 bit yes; 8 bit tight |
| Qwen3-Next-80B-A3B | MLX 4bit | ~45 GB (calc) | No | No (KV and apps push it over) |
| gpt-oss-120b | MXFP4 | ~63 GB | No | No |

Recommendation: **Qwen3.6-35B-A3B at 6-bit** for the wow moment. At ~28 to 29 GB, it is bigger than the RAM of the
biggest Mac, so no judge can say "one laptop could run that". It still reads only ~3B active parameters per token.
Fallback: the 4-bit version (~20 to 22 GB), which needs 2 of the 3 Macs.

## 3. Tool by tool

### Parallax (GradientHQ/parallax)

- What it is: decentralized serving with pipeline parallel, a scheduler that places layers and routes each request,
  continuous batching and paged KV cache on Mac (README). Mac backend = MLX LM; GPU backend = SGLang/vLLM.
  Apache-2.0; 1,421 stars; last push Jul 1 2026; tagged releases stop at v0.1.2 (Dec 2 2025), so install `main`.
- How it reaches remote compute: P2P over libp2p ("Lattica"); `parallax run` starts the scheduler with a web UI on
  :3001, each Mac runs `parallax join` (LAN) or `parallax join -s <scheduler-peer-id>` (quick_start.md). OpenAI API at
  `http://<scheduler>:3001/v1/chat/completions`.
- OS: macOS from source (`./install.sh`, Python 3.11 to 3.13, auto-installs `mac` extras); Linux GPU; Windows via an
  installer app (GPU, runs through WSL; not a CPU path for old Dells) (install.md).
- Our model: Qwen3.6-35B-A3B MLX MoE support was added (issue #473, closed Jun 2 2026); `Qwen/Qwen3.5-35B-A3B` maps to
  `mlx-community/Qwen3.5-35B-A3B-4bit`, plus Qwen3-30B-A3B, Qwen3-Next-80B-A3B, gpt-oss-20b/120b in
  `src/backend/server/static_config.py`.
- Placement code (read Nov 7 2026, `src/scheduling/node.py`, `layer_allocation.py`, `src/parallax/server/server_info.py`):
  - Each node's parameter budget = `memory_gb × param_mem_ratio` with `param_mem_ratio = 0.5` and KV budget
    `kvcache_mem_ratio = 0.3` (node.py L180-181, L281-306). On Mac `memory_gb` = **total** RAM from psutil
    (server_info.py L87-108, L209-216), not free RAM. So our pool gets 12 + 8 + 12 = **32 GB for weights**, regardless
    of what else is open. A 28 GB 6-bit model fits that budget; the 37 GB 8-bit does not.
  - Speed model: roofline from a chip TFLOPS table (M3 7.1, M5 9.37 TFLOPS FP16; server_info.py L72-82) and a
    hard-coded Mac bandwidth estimate of 100 GB/s (`est_bandwidth = 100.0`, L210), replaced by measured per-layer
    latency once nodes report it (`avg_layer_latency_ms`, node.py L379-387). Load-aware: overloaded nodes get
    infinite latency in routing.
  - Allocators: greedy and dynamic-programming (`GreedyLayerAllocator`, `DynamicProgrammingLayerAllocator`), water-
    filling rebalance per pipeline, dynamic join of new nodes "to the lightest layers", and global rebalance when load
    imbalance is high (layer_allocation.py L193-340).
- Known issues: libp2p mDNS unreliable on macOS (issue #460, May 29 2026, closed) → pass the scheduler address
  explicitly. Reasoning streams dropped in the dashboard (#480, open, Jun 3 2026) → disable thinking with
  `"chat_template_kwargs": {"enable_thinking": false}`. Privacy: first run sends "code version info" unless `-u`;
  "remote mode" uses a public relay server that sees IPs. For a "nothing leaves the building" pitch, run LAN mode
  with `-u` and no internet (models pre-downloaded).
- Benchmarks: paper (Sep 2025) on RTX 5090/4090 only: up to 3.6x throughput vs HexGen. A 2026 write-up cites 3.1x
  throughput and 5.3x lower inter-token latency vs Petals on Qwen 72B/235B (alphasignal.ai). **I found no published
  Mac-cluster tokens/sec for Parallax.** Unmeasured on Macs.

### llama.cpp RPC

- What it is: `ggml-rpc-server` exposes a machine's devices (Metal on Mac, CUDA, or CPU) over TCP; the main host's
  llama-server/llama-cli adds them with `--rpc host:port,...` and runs one model across all of them (tools/rpc/README.md,
  read Nov 7 2026). The README warns it is "proof-of-concept", "fragile and insecure. Never run the RPC server on an
  open network". That is another reason for the travel router.
- Placement: "By default, llama.cpp distributes model weights and the KV cache across all available devices ... in
  proportion to each device's available memory." Override with `--tensor-split` (e.g. `--tensor-split 9,4,9`). Same
  memory-ratio weakness prima.cpp measured; we would set the split by measured speed ourselves.
- Load time: weights are sent over the network to each rpc-server at load. `rpc-server -c` caches tensors locally
  under `~/.cache/llama.cpp/rpc` so later loads skip the transfer. First load of ~10 GB over 30 MB/s Wi-Fi takes ~6
  minutes (calc). Pre-warm the cache before the demo.
- New since my section 1: RDMA transport, negotiated automatically; on macOS it needs TB5 + macOS 26.2 + `rdma_ctl
  enable`. Not usable on our TB4 Macs.
- OS: macOS (Metal), Linux, Windows (CPU, CUDA, Vulkan). **It is the only tool here that lets an old Windows CPU
  machine join a Mac pool** (customer story).
- Measured: RPC overhead 24 to 28% even on localhost (r/LocalLLaMA, Jan 2026); 2.5 GbE adds ~5 ms/token per node
  (Geerling, Dec 2025); "throughput drop from 20 tok/s to 2 tok/s just by moving from wired gigabit to WiFi 6" on two
  M1 Max MacBook Pros (SharedLLM blog, Apr 11 2026, anecdote, no logs). Qwen3-Omni 30B MoE at 42 to 44 t/s across 5
  GPUs on 3 nodes (llama.cpp discussion #18273).
- Routing is separate: run one `llama-server --parallel N` per Mac (continuous batching) and put our own router in
  front. llama.cpp has no multi-machine request router.

### EXO 1.0 (benchmarks and fit only; code is the exo worker's)

- MLX only, Apple Silicon only; pipeline and tensor parallel; discovery over the local network; Thunderbolt or plain
  Wi-Fi/Ethernet ("Macs that share a Wi-Fi or Ethernet network don't need it", README, Nov 2026).
- Measured: Qwen3 235B on 4× M3 Ultra over TB5 RDMA, 19.5 t/s on 1 node → 31.9 t/s on 4 (Geerling, Dec 18 2025).
  I found no 2026 user numbers for EXO 1.0 on base MacBooks over Wi-Fi. Unmeasured for our case.
- Placement by memory (exo-01 notes). For M5 vs M3 the bandwidth gap is 154 vs 100 GB/s, so a memory split is less
  harmful than prima.cpp's mixed GPU/CPU testbed: the M3's ~1/4 share runs ~1.5x slower per layer (calc).
- No Windows (exo worker). No built-in multi-replica routing that I know of (check with exo worker).

### distributed-llama

- Tensor parallel, CPU (Linux/macOS/Windows). "You can run Distributed Llama only on 1, 2, 4... 2^n nodes" (README).
  **With 3 Macs it can use only 2.** Over Wi-Fi it measured ~150 ms per all-reduce (prima.cpp). Drop for our demo.

### prima.cpp

- llama.cpp fork with speed-aware Halda placement, built for Wi-Fi home clusters (section 2). Repo now
  `OpenCPIL/prima.cpp` (129 stars, last push Sep 6 2026). MoE support for Qwen3.6 not verified; a fork of an older
  llama.cpp may lack the Qwen3.6 architecture. Its placement idea is worth copying; the code is a demo risk.

### MLX distributed directly (`mlx.launch`, ring over TCP)

- What EXO and Parallax build on. Using `mx.distributed` by hand means writing our own pipeline code and launcher.
  Not worth it in 20 hours when Parallax and EXO already do it.

### vLLM / Ray

- No Metal backend; Ray multi-node does not work on Windows (team context). Out.

## 4. Expected tokens/sec for our mix (Qwen3.6-35B-A3B, pipeline over 3 Macs)

Inputs: ~3B active parameters per token. At 4-bit about 1.7 to 2 GB read per token; at 6-bit about 2.5 to 2.8 GB
(calc). Effective bandwidth M5 ~114 GB/s, M3 ~76 GB/s (section 1). MoE routing overhead and MLX kernels make real
decode ~60 to 70% of the bandwidth bound (guess).

| Setup | Compute per token | Network per token | Expected decode |
|---|---|---|---|
| 4-bit, one M5 (if it fit) | ~17 ms bound → ~25 to 30 ms real | 0 | ~35 to 40 t/s |
| 4-bit, 3 Macs, travel router (Wi-Fi to one AP, 2 to 4 ms RTT) | ~30 ms | 2 to 3 hops × 5 to 10 ms | **~20 to 28 t/s** |
| 6-bit, 3 Macs, travel router | ~40 to 45 ms | 2 to 3 hops × 5 to 10 ms | **~15 to 20 t/s** |
| 6-bit, 3 Macs, USB-C Ethernet + switch | ~40 to 45 ms | 2 to 3 hops × ~5 ms | ~18 to 22 t/s |
| 6-bit, 3 Macs, venue Wi-Fi (if peers can see each other) | ~40 to 45 ms | 2 to 3 hops × 10 to 30 ms | ~8 to 15 t/s; worst case ~2 (SharedLLM anecdote) |

Prompt reading (prefill): M5 prefill is fast (~720 t/s for 7B Q4_0 in llama.cpp #4167); for a 3B-active MoE expect a
few hundred to ~1,000 t/s, so a 2,000-token document takes ~2 to 8 s (guess).

The headline: a model bigger than any of the three laptops, at roughly reading speed (15 to 25 t/s), on a travel
router. These are estimates; the first-hour benchmark decides.

## 5. Which tool for our demo

| Need | Parallax | EXO 1.0 | llama.cpp RPC |
|---|---|---|---|
| Pool Qwen3.6-35B-A3B across M5 + M3 + M5 | Yes (MLX; #473 support) | Likely (MLX; check model card with exo worker) | Yes (GGUF; Qwen3.6 arch support assumed in current llama.cpp, verify) |
| Placement by speed | Partly (TFLOPS table + measured layer latency) | No (memory) | No (memory; manual `--tensor-split`) |
| Routing many users + batching | Built in (per-request chain selection, continuous batching) | Not built in (verify) | Per-Mac llama-server batching; router is ours |
| Join flow | `parallax join`, web UI | Automatic discovery | Manual `--rpc` list |
| Windows CPU node (customer story) | No | No | Yes |
| Mac numbers published | None found | Mac Studio + TB5 only | Some (2-Mac, Ethernet) |
| Risk | Young project, Mac path unbenchmarked, mDNS flaky | Discovery on Wi-Fi, identical builds required | Wi-Fi slowdown, insecure RPC, weights transfer at load |

My ranking for the hackathon: test **Parallax first** (it already has pooling + routing + join + dashboard, which is
our whole Must-have list), **EXO 1.0 second** (the exo worker knows its internals), and keep **llama.cpp RPC as the
fallback and the Windows story**. Spend the first 2 hours getting the 4-bit model pooled on 2 Macs with Parallax and
EXO side by side, then pick one.
