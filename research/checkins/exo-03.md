## Check-in exo 03: Splitting the model

**Found (most important first, max 8 bullets, each with a source):**
- **Best wow model: `mlx-community/Qwen3.6-35B-A3B-4bit`.**
  - Size: 20.4 GB on disk and in memory, 40 layers, an EXO built-in card (`resources/inference_model_cards/qwen3/`).
  - It is above any one Mac's GPU budget (~16 GB on a 24 GB Mac) and fits across all three even with apps open. My assumed budgets of 12/7/12 GB give EXO's split M5 16 layers (8.2 GB), M3 9 (4.6 GB), third Mac 15 (7.7 GB).
  - Fallback: `Qwen3-30B-A3B-4bit` (17.6 GB, 48 layers; 7.0/4.0/6.6 GB).
  - Truly needs all three: `Qwen3-30B-A3B-8bit` (33.3 GB). It only fits if the apps are quit (~12.5/8.3/12.5 GB).
  - Too big for 64 GB total: Qwen3-Next-80B-A3B (47 GB), gpt-oss-120b (70.7 GB), and every "Flash" variant (DeepSeek-V4-Flash 155 GB, V4.1-Flash 510 GB, GLM-5.3-Flash 204 GB, Step-3.5-Flash 115 GB).
- **EXO uses the fewest Macs that fit.** `get_smallest_cycles()` (`src/exo/master/placement_utils.py` line 40) runs after the memory filter. A 20 GB model that fits on M5 + third Mac would go on 2 Macs, so the demo must pass `min_nodes=3` (or pin all three) to use every Mac.
- **The layer split follows free memory only.** Each node's share is `min(psutil available, Metal working set)` (`placement.py` `usable_memory()` line 223). Layers are given out in proportion to it, with **at least 1 layer per node** (`allocate_layers_proportionally()`, line 47). There is no compute term (`profiling.py`: `# TODO: flops_fp16`). So EXO cannot give a device in the group zero layers. A Mac can still be kept light or out three ways:
  - set `OVERRIDE_MEMORY_MB` on it (`info_gatherer.py` line 521), so it reports less memory and gets fewer layers;
  - pin the others with `constraints.pinned_nodes`;
  - run it with `--no-worker`.
- **The prima.cpp slowdown on mixed devices is unlikely here.** Each share is capped at Metal's working set, so nothing pages. For an A3B MoE, the M3's share costs about 4 ms per token (~0.4 GB / 100 GB/s), which is my estimate, not a measurement. The real risk is apps growing after the model loads, which leads to swap.
- **Placement adds no KV-cache headroom.** It only checks that the summed free memory is at least the model size (`filter_cycles_by_memory`, line 21). A runner that runs out "aborts while loading" (`placement.py` line 228 docstring). Quit heavy apps and keep prompts short.
- **Pipeline is the right mode for us.** Each rank runs its layer range, does `recv_like` from rank r-1, `send`s to r+1, and the last rank `all_gather`s the logits (`src/exo/worker/engines/mlx/sharding/pipeline.py` lines 66, 140, 166). That is about 4 KB per hop per token over MLX `ring` (TCP), so latency matters, not bandwidth. Tensor parallel needs RDMA (TB5, macOS 26.2+) and is pointless on our MacBooks. Qwen3.6-35B-A3B's 2 kv heads also likely block a 3-way tensor split.
- **Every Mac downloads the whole model.** The download path never reads `start_layer`/`end_layer`; they appear only in a test harness, and the worker sends `StartDownload` with the shard but the coordinator fetches the full repo (`src/exo/worker/main.py` line 417, `src/exo/download/*`). I read this from the code and have not tested it. So each Mac needs about 20.4 GB of free disk for the primary pick, and 3 downloads over venue internet are risky. Pre-download at home, or copy `~/.exo/models/...` between Macs and run `--offline`.
- **Link choice:** `find_ip_prioritised()` (`placement_utils.py` line 339) ranks thunderbolt > ethernet > wifi for the ring, so plugging in Ethernet is enough even with Wi-Fi left on. Speed guess: a few to 10 tokens/s over Wi-Fi, and perhaps 20 to 40 over wired Ethernet for an A3B model across 3 Macs (my estimate; measure it on day 1).

**Surprises / things that change the plan:**
- Disk is now a blocker on par with RAM: about 20 GB free on all 3 personal Macs, and the model files must be pre-staged before the venue.
- The pitch "no single laptop can run it" holds for Qwen3.6-35B-A3B-4bit at default macOS settings. "Needs all three" is honest only for the 8-bit 30B, or when we force `min_nodes=3`.
- `OVERRIDE_MEMORY_MB` is a demo-friendly knob for "this machine lends only X GB to the pool", and it could back a "how much do you contribute" slider in our UI.

**PRD suggestions:**
- Must have > Pool: change the model to "`mlx-community/Qwen3.6-35B-A3B-4bit` (20.4 GB, EXO built-in), Pipeline, `min_nodes=3`. Fallback `Qwen3-30B-A3B-4bit` (17.6 GB). Stretch `Qwen3-30B-A3B-8bit` (33.3 GB, needs all 3 with apps quit)."
- Demo hardware: add "EXO downloads the whole model to every Mac: about 20.4 GB free disk per Mac. Pre-download at home and copy over Ethernet. GPU budget per Mac ≈ 2/3 to 3/4 of RAM (16 GB Mac ≈ 10.7 GB, 24 GB ≈ 16 to 18 GB); read the real value from EXO `/state` `gpu_working_set`."
- First hour: add "quit heavy apps; check per-node `ram_available` and `gpu_working_set` in `GET /state`; place with `/instance/previews` before the demo."
- Stretch: "Contribution slider: per-Mac `OVERRIDE_MEMORY_MB` sets how much memory it lends to the pool."
- Open question 3: add the speed guess above, marked unverified.

**Questions for Aayo:**
1. How much free disk does each Mac have? If any has under ~25 GB, the primary pick changes to `Qwen3-30B-A3B-4bit` (17.6 GB) or smaller.
2. Can someone pre-download the model at home tonight on one Mac (about 20 GB) so we copy it to the others over Ethernet? If yes, I will write the exact copy and `--offline` steps in section 4.

**Next section:** Section 4, serving and UX: the request flow from `/v1/chat/completions` to the runner and back (including #2597, which serves without the master), the instance lifecycle (`/instance/previews` → `/instance` → `/instance/await`), model download and caching (exact `~/.exo/models` layout, `--offline`, `scripts/download_model_to_cluster.py`), the dashboard and what a joining user sees. I would add: whether one Mac can hold a routing instance and a pool shard at the same time, and what happens to an in-flight request when a Mac leaves the pool (for a "pull a laptop" demo moment, or to avoid one).
