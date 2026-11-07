## Check-in kernels 03: What dominates LLM inference speed (batch 1, our Macs)

**Found (most important first, max 8 bullets, each with a source):**
- **The wired limit does not constrain us; free RAM does.**
  - macOS caps GPU-wired memory at `recommendedMaxWorkingSetSize`. With the default `iogpu.wired_limit_mb=0`, that cap is about 2/3 to 78% of RAM. ModelPiper measured 78% on a 32 GB Mac under macOS 26.5.2. The 2/3 figure (11,453 MB on 16 GB Macs) comes from my memory of llama.cpp logs and is not re-verified.
  - Resulting ceilings: about 16 to 18.7 GiB on the 24 GB Macs and 10.7 to 12.5 GiB on the M3 16 GB. The exo worker's budgets with apps open (12/7/12 GB) are below every ceiling.
  - Pool need: 20.4 GB weights + about 0.3 GB of lm_head copied to each Mac + under 1 GB of cache + buffers, so about 22 to 23.5 GB against 31 GB available. It fits with about 8 GB to spare.
  - Do not raise the sysctl on personal Macs. It would buy nothing here and can freeze a Mac (modelpiper.com 2026; mlx-lm README "Large Models").
- **"Too big for any one Mac" holds by default.** 20.4 GB = 19.0 GiB, which is above even the 78% ceiling of a 24 GB Mac (18.7 GiB) and far above the ~12 GB actually free. Honest caveat: with every app quit and the sysctl raised, an M5 24 GB might just hold it. So say "no laptop as people actually use it".
- **What one Qwen3.6-35B-A3B token costs, from its config.json** (40 layers: 30 Gated DeltaNet + 10 full attention; 256 experts, 8 routed + 1 shared):
  - 2.95B active parameters, so **1.66 GB of weight reads per token at MLX 4-bit**. 17% of that is the lm_head (248k vocabulary).
  - KV cache is only **20 KB per token**, versus 96 KB for the Qwen3-30B-A3B fallback. A 32k context costs 0.66 GB versus 3.1 GB. Long documents and several users are cheap on Qwen3.6.
- **At 4-bit on a Mac, MoE decode is about half fixed overhead and half bandwidth.**
  - I fit time per token = bytes / effective bandwidth + fixed cost to the mlx-lm maintainers' M4 Max numbers (`mlx_lm/BENCHMARKS.md`). Qwen3-30B-A3B gives 474 GB/s effective (87%) + **5.2 ms per token fixed**; dense 4B gives 2.9 ms. The fit predicts q8, q6 and q5 within 3%.
  - At q4, 59% of the MoE token time is overhead (dispatch, expert routing, sampling), not weight reads.
  - Qwen3.6 numbers from other Macs agree (llmcheck.net, Oct 2026: Mac mini M6 at 170 GB/s 64 tok/s; M5 Max 118 to 126): about 1.79 GB effective per token and about 5.1 ms fixed.
- **Single-Mac estimate if it fitted** (a guess): base M5 about 50 to 60 tok/s, M3 about 37 to 42.
- **Pool estimate, 3 Macs** (16/9/15 layers, assuming the third Mac is an M5; full table in notes section 4):
  - Speed of light from weight reads alone is about **77 tok/s**.
  - Realistic: Ethernet via the router about **38 to 45 tok/s**; Wi-Fi about **14 to 28 tok/s**.
  - Each token crosses the network 3 times (2 pipeline sends + 1 all_gather in mlx-lm `qwen3_5.py`). Each crossing carries only 4 KB, so latency matters, not bandwidth.
- **Speculative decoding is not available for Qwen3.6 in MLX.**
  - mlx-lm needs a trimmable cache, and the DeltaNet `ArraysCache` returns `is_trimmable()==False` (`mlx_lm/models/cache.py` line 164).
  - Qwen3.6 ships an MTP head (`mtp_num_hidden_layers: 1`), but mlx-lm's `qwen3_5.py` has no MTP code.
  - It would have helped us most: k tokens verified per pass means k times fewer network round-trips per token.
- **Batching works for routing.**
  - The mlx-lm server batches when every cache supports `merge`, and `ArraysCache` does. It turns batching off when a draft model or `--kv-bits` is set (`server.py` line 475; SERVER.md).
  - For MoE, each extra user activates more experts, so the gain is smaller than for dense models. I found no measured number; we measure it with `mlx_lm.benchmark -b 4`.

**Surprises / things that change the plan:**
- **Our pitch line needs one more clause.** "Decode is bandwidth-bound" is right for the weight reads. But for small-active MoE at 4-bit, a fixed cost of about 5 ms per token is as large as the reads, and in a pool the network hops add to it. The speed-of-light meter will show about 50 to 60% of SOL on Ethernet and 20 to 40% on Wi-Fi. That is an honest, explainable gap, not a failure.
- **Ethernet roughly doubles pool tok/s versus Wi-Fi** in my estimate. Confirming the USB-C Ethernet adapters is worth more than any software optimization we could do.
- **A strong demo moment:** predict the pool's tok/s from per-Mac measurements, then run it and show prediction against measurement. Two `mlx_lm.benchmark` runs (4B at q4 and bf16) per Mac give each Mac's effective bandwidth and fixed overhead.

**PRD suggestions:**
- Under Demo hardware, add: "Usable GPU memory = free RAM with apps open, not the wired ceiling. Default ceilings (`recommendedMaxWorkingSetSize`): about 16 to 18.7 GiB on 24 GB Macs and 10.7 to 12.5 GiB on 16 GB, all above our 12/7/12 GB budgets. Do not change `iogpu.wired_limit_mb` on personal Macs."
- Under Open question 3, add expected tok/s for Qwen3.6-35B-A3B-4bit on 3 Macs: speed-of-light about 77 tok/s; estimate 38 to 45 on Ethernet and 14 to 28 on Wi-Fi; one M5, if it could hold the model, about 50 to 60.
- Under Why pooling works, add: "MoE gives speed, the pool gives capacity. Qwen3.6 holds 20 GB but reads only 1.66 GB per token, and its hybrid attention keeps KV at 20 KB per token (32k context = 0.66 GB)."
- Under First hour, add: "On each Mac, run `mlx_lm.benchmark` with Qwen3-4B q4 and bf16 (`-p 512 -g 256 -n 3`) and once with `-b 4`. The results feed the speed-of-light meter and the pool prediction."
- Under Roadmap, add: "Multi-token prediction (Qwen3.6 has an MTP head) to cut network round-trips per token. It is not supported in mlx-lm today."

**Questions for Aayo:**
1. What chip is the third Mac (M5, M4 Pro, M3 Pro...)? It changes the pool estimate by up to about 30%, and with M4 Pro bandwidth (273 GB/s) the SOL ceiling moves to about 90 tok/s.
2. Confirm the USB-C Ethernet adapters if possible. Per my estimate they are the single biggest speed factor for the pool.

**Next section:** Section 4 covers Apple Silicon specifically: chip bandwidth from M1 to M5 (verified spec pages), how close MLX and llama.cpp Metal get to peak, what is inside the fixed cost of about 5 ms per token (one profiling pass with `mx.metal` capture or timing, if feasible), whether any MLX op has headroom for the custom Metal-kernel stretch (pending Aayo), and the network side: whether EXO/mlx-lm overlap send/recv with compute, and the real per-hop latency of MLX distributed over TCP (ring) on Wi-Fi vs Ethernet. Change from the brief: activation compression is pointless here (4 KB per hop; latency dominates), so I will replace it with "reducing round-trips per token".
