# 03 Splitting the model (EXO 1.0)

Commit `a60b956` (2026-11-06). Paths relative to `C:\Users\aayoa\Desktop\hackathon\exo-src`.
Hardware: MacBook M5 24 GB (Aayo), MacBook M3 16 GB, third MacBook 24 GB (probably M5, unconfirmed). Personal
machines with apps open and limited free disk.

## How placement works (exact code path)

`POST /place_instance` or `/instance/previews` -> `place_instance()` in `src/exo/master/placement.py` line 298:

1. `memory_once_loaded()` subtracts memory of instances still loading.
2. `usable_memory()` (line 223): each node's `ram_available` becomes `min(ram_available, gpu_working_set)`.
   `ram_available` = psutil `virtual_memory().available` (`src/exo/shared/types/profiling.py` `from_psutil`), or
   the env var `OVERRIDE_MEMORY_MB` if set (`src/exo/utils/info_gatherer/info_gatherer.py` line 521).
   `gpu_working_set` = Metal recommended working set, comment says "about 75% of the RAM".
3. `topology.get_cycles()`: candidate node groups. `min_nodes` filter. `required_nodes` (from
   `constraints.pinned_nodes`, `src/exo/master/deployments.py` line 151) filter.
4. `filter_cycles_by_memory()` (`placement_utils.py` line 21): the group's summed usable memory must be >= the
   model's `resident_size` (`memory_size` if the card has one, else `storage_size`; `model_cards.py` line 207).
   **No KV-cache or activation headroom is added.** The docstring says a runner whose shard needs more "aborts while
   loading".
5. Tensor only: `hidden_size % n == 0`, kv heads `% n == 0` (if the family splits kv heads), and every node must hold
   `resident_size / n` (even split).
6. **`get_smallest_cycles()`** (`placement_utils.py` line 40): keep only groups with the fewest nodes. EXO uses the
   fewest Macs that fit, so a model that fits on 2 Macs is placed on 2 unless `min_nodes=3`.
7. Backend filter (MlxMetal etc.), RDMA filter if `MlxJaccl`, prefer cycles with a leaf node, then pick
   by (already-downloaded score, total free memory).
8. Single node forces Pipeline + Ring.
9. `get_shard_assignments()` -> pipeline: `allocate_layers_proportionally()` (line 47): layers ∝ usable memory,
   largest-remainder rounding, **minimum 1 layer per node in the group**. `_allocate_and_validate_layers()` (line 96)
   checks `resident_size * layers / n_layers <= usable` per node.

### Mixed chips (M3 + M5): what EXO does

- Layer share depends **only on free memory**. There is no compute or bandwidth term:
  `SystemPerformanceProfile` has `# TODO: flops_fp16` (`profiling.py`). An M3 with 7 GB free gets the same per-GB
  share of layers as an M5.
- **A node inside the group cannot get zero layers** (min 1 per node). There are three ways to keep a slow Mac
  light or out:
  1. Leave it out of the group: pin the other two (`POST /instance` with `constraints.pinned_nodes`), or rely on
     smallest-cycle selection when 2 Macs suffice.
  2. `OVERRIDE_MEMORY_MB=<n>` on that Mac before starting exo: it reports less free memory and gets proportionally
     fewer layers. This is the knob for "M3 contributes a little".
  3. `--no-worker` on that Mac: it joins as coordinator/API only.
- prima.cpp's bad case (M1 with 9/32 layers at 263 ms/token vs 15 ms) is not expected here, for two reasons. EXO
  caps each share at Metal's working set, so there is no mmap paging unless other apps grow after load. And for an
  A3B MoE, the M3's added time per token is about (its share of ~1.7 GB active weights) / 100 GB/s ≈ 4 ms. That is
  an estimate, not measured. The risk that remains is memory pressure from open apps after placement, which shows
  up as swap and stalls.

## How the model is split and activations move

- Pipeline (default, `Sharding.Pipeline`, `InstanceMeta.MlxRing`): `src/exo/worker/engines/mlx/sharding/pipeline.py`.
  Rank r runs layers [start_layer, end_layer); receives the hidden state from rank r-1 with
  `mx.distributed.recv_like` (line 66), sends to (r+1) % size with `mx.distributed.send` (line 140), and the last
  rank's output is `all_gather`ed (line 166) so every rank has the logits. During prefill, sends are queued
  (`_pending_prefill_sends`, line 13). Per decode token, each hop carries one hidden vector (hidden 2048 × bf16 ≈ 4 KB),
  so the cost is latency, not bandwidth.
- Tensor (`Sharding.Tensor`): `sharding/tensor.py` + `auto_parallel.py`; every layer is split evenly, with
  all-reduce per layer. Useful only with RDMA (`MlxJaccl`, Thunderbolt 5, macOS 26.2+). Our base M5/M3 MacBooks
  have no TB5 and we use Ethernet, so **use Pipeline**. Also, Qwen3.6-35B-A3B has 2 kv heads, so a 3-way tensor split
  is likely rejected.
- Transport: `mx.distributed.init(backend="ring")` over TCP with a hostfile EXO writes
  (`utils_mlx.py` lines 125 to 133). `find_ip_prioritised()` (`placement_utils.py` line 339) picks the link:
  for ring, thunderbolt > maybe_ethernet > ethernet > wifi. With Wi-Fi on and Ethernet plugged in, ring uses
  Ethernet.
- Engine: MLX only (Metal on Mac). There is no tinygrad and no llama.cpp. Non-Mac devices cannot share an instance
  (section 1).

## Disk: every Mac downloads the whole model

The worker sends `StartDownload(shard_metadata=shard)` (`src/exo/worker/main.py` line 417), but the download code
never reads `start_layer`/`end_layer` (they appear only in `src/exo/download/tests/harness.py`). Each Mac therefore
downloads the full repo (`whole_model_shard`, `shards.py` line 99). This is my reading of the code, not tested.
**Free disk needed = full model size on every Mac**, and pulling 3 copies over venue internet would be slow:
pre-download at home, or copy `~/.exo/models/<model>` between Macs over Ethernet (`--offline` mode exists).

## Memory budget for our three Macs

Metal working set (GPU budget):
- **16 GB Mac:** about 10.7 GB (2/3 of RAM), from llama.cpp logs for 16 GB Macs (unverified on the M3).
- **24 GB Mac:** about 16 to 18 GB.
- **One measured data point:** a 32 GiB M2 Max reports 78% (modelpiper.com, 2026, "iogpu.wired_limit_mb on Mac").
- **How to measure:** check `gpu_working_set` per node in exo's `GET /state`, or run
  `swift -e 'import Metal; print(MTLCreateSystemDefaultDevice()!.recommendedMaxWorkingSetSize)'`.
- **Can be raised:** `sudo sysctl iogpu.wired_limit_mb=N`. The cost is less headroom for macOS itself.

Two scenarios:
- **A "apps open" (assumed):** usable M5 12 GB, M3 7 GB, third 12 GB, total 31 GB.
- **B "clean, apps quit":** usable 16 / 10.7 / 16, total ≈ 42.7 GB.

All models below are in EXO's built-in cards (`resources/inference_model_cards/`). Size is `storage_size` from the
card (= disk per Mac = resident size). The split is EXO's proportional rule, calculated by hand.

| Model (EXO card) | Disk per Mac, GB | Layers | Fits one Mac? | Split A (M5/M3/X layers → GB) | Split B |
|---|---|---|---|---|---|
| `mlx-community/Qwen3.6-35B-A3B-4bit` (qwen3_5_moe, 262k ctx) | 20.4 | 40 | No (> ~16 GB budget of a 24 GB Mac) | 16/9/15 → 8.2/4.6/7.7 | 15/10/15 → 7.7/5.1/7.7 |
| `mlx-community/Qwen3-30B-A3B-4bit` (qwen3_moe, 32k ctx) | 17.6 | 48 | Borderline no (17.6 > ~16) | 19/11/18 → 7.0/4.0/6.6 | 18/12/18 → 6.6/4.4/6.6 |
| `mlx-community/GLM-4.7-Flash-4bit` (glm4_moe_lite) | 19.3 | 47 | No | ≈18/11/18 → 7.4/4.5/7.4 | ≈18/11/18 |
| `mlx-community/Qwen3-30B-A3B-8bit` | 33.3 | 48 | No; needs all 3 | Does not fit (33.3 > 31) | 18/12/18 → 12.5/8.3/12.5 (≈1.5 to 3.5 GB headroom) |
| `mlx-community/Qwen3.6-35B-A3B-8bit` | 37.7 | 40 | No; needs all 3 | Does not fit | 15/10/15 → 14.1/9.4/14.1 (≈1.3 to 2 GB headroom, risky) |
| `mlx-community/Qwen3-Next-80B-A3B-Instruct-4bit` | 47.0 | 48 | No | Does not fit | Does not fit (47 > 42.7) |
| `mlx-community/gpt-oss-20b-MXFP4-Q8` | 12.0 | 24 | Yes (routing model) | n/a | n/a |
| `gpt-oss-120b-MXFP4-Q8` | 70.7 | | No | Does not fit | Does not fit |
| "Flash" giants: DeepSeek-V4-Flash 155 GB, V4.1-Flash 510 GB, GLM-5.3-Flash-4bit 204 GB, Step-3.5-Flash-4bit 115 GB, MiniMax-M3.1-Flash-3bit 186 GB, Qwen3.8-Flash-Next-4bit 112 GB | | | No | No | No |

KV cache comes on top (the placement check ignores it). Short demo prompts (2 to 4k tokens) for a 2-kv-head
A3B model add well under 1 GB.

### Pick

- **Primary wow: `mlx-community/Qwen3.6-35B-A3B-4bit` (20.4 GB), Pipeline, `min_nodes=3`.**
  - It is larger than any one Mac's GPU budget (~16 GB on a 24 GB Mac) with no tricks, and fits across all three
    even with apps open (A: 8.2/4.6/7.7 GB).
  - It is a newer model with long context.
  - Caveat: 2 Macs (M5 + X, 32 GB of budget) also hold it, so "needs all three" is true only with `min_nodes=3`
    or in scenario A, where M5 + X = 24 GB is tight. Phrase the claim as "no single laptop can run it".
- **Fallback: `Qwen3-30B-A3B-4bit` (17.6 GB).** It is the most widely run MoE on MLX, but it is only just over one
  Mac's budget, and someone could fit it on one 24 GB Mac by raising `iogpu.wired_limit_mb`.
- **"Truly needs all three" stretch: `Qwen3-30B-A3B-8bit` (33.3 GB).** This requires scenario B (quit apps) and
  33.3 GB of free disk on each Mac.
- **Not possible on 64 GB total:** Qwen3-Next-80B-A3B (47 GB) and every "Flash" variant.
- **Routing model on each Mac:** gpt-oss-20b (12 GB) is too big to run beside a pool shard on the M3. Use a small
  model, or route only on the two 24 GB Macs (check with `/instance/previews`; whether EXO lets one node hold two
  instances is not verified).

### Speed guess (unverified, to measure on day 1)

- **Decode, A3B 4-bit:** about 1.7 GB of weights read per token, split by layers. Approximate times: M5 share ≈
  4 ms (153 GB/s), M3 ≈ 4 ms (100 GB/s), third ≈ 4 ms, plus 3 hops × 1 to 3 ms on wired Ethernet. That gives a
  theoretical ceiling around 50 to 60 tokens/s; MLX overhead will put real numbers lower, perhaps 20 to 40.
- **Wi-Fi:** each hop costs 10 to 30 ms (principles-01), so expect a few to 10 tokens/s.

## Static-peer backup (one paragraph, not building)

If our router fails and discovery doesn't work, the smallest patch is in `rust/networking/src/lib.rs` `cfg()`:
add Zenoh `connect/endpoints` from an env var. It needs source builds (Rust + uv) on all 3 Macs; details in
02-cluster-formation.md.
