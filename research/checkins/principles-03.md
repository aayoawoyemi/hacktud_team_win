## Check-in principles 03: The tools

Full notes: `research/principles/03-tools.md`

**Found (most important first, max 8 bullets, each with a source):**
- Usable GPU memory is lower than RAM. macOS lets the GPU use 2/3 of RAM by default: a 16 GB Mac logs `recommendedMaxWorkingSetSize = 10922.67 MB` (llama-cpp-python #687), and a 2026 M5 logs `16384.02` MB (node-llama-cpp #549), which matches 2/3 of 24 GB. With normal apps open, our pool has about 31 to 37 GB usable. With apps quit and `sudo sysctl iogpu.wired_limit_mb` raised to RAM minus 5 GB (method in the mlx-lm README; resets on reboot), it has about 46 to 49 GB (my math).
- The model to demo is Qwen3.6-35B-A3B. Hugging Face sizes: Q4_K_M 22.1 GB, Q6_K 29.3 GB, Q8_0 36.9 GB, and MLX 4/6/8-bit builds exist (mlx-community). The 6-bit build (~28-29 GB) is bigger than the RAM of our biggest Mac, so no single laptop can run it, yet it reads only ~3B parameters per token.
- Parallax supports this model on Mac (issue #473, "Support Qwen3.6 35B MLX MoE", closed Jun 2 2026). It ships a scheduler web UI (`parallax run`), `parallax join` on each Mac, an OpenAI API, per-request routing and continuous batching on Mac (GradientHQ/parallax README and quick_start.md, main pushed Jul 1 2026). That covers most of our Must-have list.
- Parallax caveats from its code: each Mac's weight budget is 0.5 × total RAM, not free RAM (`param_mem_ratio = 0.5`, src/scheduling/node.py L180-306), so our pool gets 12 + 8 + 12 = 32 GB. It estimates speed from a chip TFLOPS table (M3 7.1, M5 9.37) and a fixed 100 GB/s bandwidth for every Mac (server_info.py L72-82, L210), then switches to measured per-layer latency. Its mDNS discovery is unreliable on macOS (#460), so pass the scheduler address. The first run sends version info unless you add `-u`.
- llama.cpp RPC splits by available memory by default (override with `--tensor-split`). Weights cross the network at load unless `rpc-server -c` caches them. The README says it is "fragile and insecure. Never run the RPC server on an open network". It now supports RDMA, but only on Thunderbolt 5 Macs, and ours are TB4 (tools/rpc/README.md, read Nov 7 2026). It is the only tool that lets a Windows CPU machine join a Mac pool.
- Wi-Fi can wreck llama.cpp RPC. On two M1 Max MacBook Pros, one user saw 20 t/s on wired gigabit drop to 2 t/s on Wi-Fi 6 (SharedLLM blog, Apr 11 2026; anecdote, no logs).
- distributed-llama only runs on 1, 2, 4... nodes (README), so 3 Macs means it uses 2. Ruled out. prima.cpp moved to OpenCPIL/prima.cpp (129 stars), and its support for the Qwen3.6 architecture is unverified. Too risky.
- None of the three has published numbers for base MacBooks pooling a MoE over Wi-Fi. EXO's published numbers are Mac Studios on TB5 only, and Parallax's are NVIDIA only. Our tokens/sec figures are estimates until we measure.

**Expected numbers for our mix (my estimates):** Qwen3.6-35B-A3B across M5 + M3 + M5. 4-bit on the travel router: about 20 to 28 t/s. 6-bit on the travel router: about 15 to 20 t/s. 6-bit on venue Wi-Fi: about 8 to 15 t/s, possibly far worse. Reading a 2,000-token prompt: about 2 to 8 s.

**Surprises / things that change the plan:**
- Parallax already does pooling, routing, join, and a web UI on Macs. If it runs on our Macs, the hackathon build becomes the office product layer on top of it (onboarding, dashboard, speed-aware placement, the privacy story), not distributed inference from scratch.
- For the privacy pitch, Parallax must run in LAN mode with `-u` and models downloaded in advance. Its "remote mode" uses a public relay server.

**PRD suggestions:**
- Must have, Pool: name the model as "Qwen3.6-35B-A3B 6-bit (~28 GB, bigger than any one of our Macs' RAM); fallback 4-bit (~20-22 GB)", with an expected 15 to 25 t/s on the travel router.
- Demo hardware: add a demo-day memory checklist: quit apps on every Mac; run `sudo sysctl iogpu.wired_limit_mb=19456` on the 24 GB Macs and `11264` on the 16 GB Mac; keep context at 4K to 8K.
- Open question 2: add "Test order: Parallax first, EXO 1.0 second, llama.cpp RPC as fallback and the Windows customer path. Drop distributed-llama (2^n nodes only) and prima.cpp (stale fork)."
- Must have: "First 2 hours: pool the 4-bit model on 2 Macs with Parallax and EXO side by side, record t/s, pick one."
- Should have, Fully local: "Parallax: LAN mode, `-u` flag, pre-downloaded models, no relay."

**Questions for Aayo:**
1. Is the third Mac an M5? With 2 M5s plus the M3, the M3 is the slow node, and speed-aware placement (giving it fewer layers) becomes a visible feature.
2. Can each owner quit their apps and run one `sudo sysctl` command on demo day? Without that, the 6-bit model probably will not fit and we demo the 4-bit one.

**Next section:** Section 4, mixed devices: how a Windows/Linux CPU machine joins a Mac pool with each tool (llama.cpp RPC is the only real path), quantization compatibility, weighting work by device speed, and what happens when a device is slow or leaves mid-generation. I would shorten the Windows part, since it is roadmap only, and spend the time on failure handling and on a hands-on Parallax test plan for our three Macs (exact commands, what to measure). That is what the team needs tonight.
