## Check-in exo 06: Fast answers + verdict and bring-up

**Found (most important first, max 8 bullets, each with a source):**
- **Q1, Xcode: yes, main needs full Xcode as written, but there is a safe way around it.** `pyproject.toml` lines 83 to 85 pin MLX as a darwin git source, so `uv sync --extra mlx` compiles it with CMake. That needs the Metal compiler, which ships with full Xcode plus the Metal Toolchain; the README says the same. My estimates: about 3 to 4 GB download / 10+ GB installed, plus 10 to 20 min MLX build on a base M5. The way around it: GitHub compare `v0.32.4...36969c09` = **identical** (0 ahead, 0 behind). So the pinned rev is MLX 0.32.4, which is on PyPI as cp313 macOS arm64 wheels (`mlx` 0.7 MB + `mlx-metal` 45 to 78 MB, uploaded 2026-10-14). Removing that 3-line source block (a tested Python patch is in the bring-up doc) installs identical code without Xcode; only the Command Line Tools + Rust are needed. **All 3 Macs must apply it, or none**, because `build_id` includes the MLX version string (`src/exo/routing/wire.py` line 80).
- **Q2, app v1.0.71 (tag `fd707de`, 2026-04-23): the #2515 crash should not trigger for our splits.** I read the code; I did not run it.
  - The crash needs a shard with no attention layer. Nemotron's attention layers sit at block indices 5, 12, 19, 26, 33 and 42, so any contiguous shard of 10 or more layers has one. Our splits (3-way 20/12/20, 2-way 26/26) all do.
  - v1.0.71 restores SSM snapshots with `deepcopy(snapshots[-2])` (`generate.py` line 373), which keeps the `make_mask` patch on the copy. Main's restore into new objects is what broke.

  So **a 2-way or 3-way Nemotron-3-Nano 5-bit pipeline on the app should work tonight.** It lacks the float32 fidelity fixes, the Lightning cards and #2597.
- **The app uses libp2p networking, not Zenoh** (v1.0.71 `Cargo.toml` line 45). An app networking test validates the router, LAN, memory and model, but not main's discovery (main uses IPv6 multicast UDP 52413; libp2p uses mDNS UDP 5353).
- **Verdict:** reuse exo main as the pooling engine and its HTTP API as our backend contract:
  - inference: chat + tools + `enable_thinking`;
  - placement: previews / place / await / delete;
  - status: state / membership;
  - built-in least-in-flight routing.

  Copy as patterns: placement preview, a layer map from `/state`, `OVERRIDE_MEMORY_MB` as the contribution knob, `min_nodes` constraints, and the version gate.

  Avoid: tensor parallel, bf16 on a 3-node pipeline (#2196), pulling a pool Mac live, the 9B model for tool calls, relying on `tool_choice`, and venue Wi-Fi.
- **Copy-paste bring-up written** (`research/exo/06-verdict-and-bringup.md`), steps T1 to T9:
  - T1: toolchain (CLT, brew, uv, node, rustup);
  - T2: clone `a60b956` + no-Xcode patch + `npm run build` + `uv sync --extra mlx` + version check;
  - T3: download 5-bit once via `/download/start`;
  - T4: `rsync` over the router;
  - T5: `iogpu.wired_limit_mb` + `EXO_OFFLINE=true uv run exo --namespace hacktud-pool [--force-master]`;
  - T6: `place_instance` with `min_nodes:3` + await;
  - T7: smoke tests (chat + speed, one tool call, thinking on), plus a correctness check: 3-way vs 2-way at temperature 0;
  - T8: optional 9B routing model;
  - T9: 6-bit upgrade.
- **API contract table for the Spacetime worker** is at the end of the same file: state polling, membership, layer map, previews, place, await, rebalance (delete + place), streaming agent steps, cancel.

**Surprises / things that change the plan:**
- The Xcode blocker disappears with a 3-line patch, because the pinned MLX rev is exactly the 0.32.4 release. This makes the main source build realistic tonight (about 30 to 45 min per Mac, mostly downloads).
- The app is a workable fallback for the demo, not just for testing. The Nemotron crash fixed in #2515 doesn't hit our layer splits.

**PRD suggestions:**
- Install: "exo main `a60b956` from source with the no-Xcode patch (PyPI MLX 0.32.4 = pinned rev). All 3 Macs identical. Fallback: app v1.0.71 on all 3 (works for 2-way/3-way Nemotron-3-Nano pipeline per code reading; libp2p networking; no Lightning)."
- First hour: "follow `research/exo/06-verdict-and-bringup.md` T5 to T7; correctness check 3-way vs 2-way at temp 0."
- Spacetime backend: "exo API contract = table at the end of 06-verdict-and-bringup.md."

**Questions for Aayo:**
1. Which build did the teammates install right now: the app or source? If the app, do we keep it for the demo (lowest risk) or switch to main tonight (better fidelity, Lightning, master-free serving)?
2. Does COORD have about 50 GB free (5-bit + 6-bit), or do we download only the 5-bit (22 GB)?

**Next section:** My brief's 6 sections are done. Useful follow-ups if wanted:
- (a) dry-run the T2 patch + `uv lock` resolution on a Mac, or with `uv lock --python-platform` from here, to confirm `mlx==0.32.4` resolves;
- (b) write the `join.sh` for the Spacetime join flow;
- (c) a short community survey of exo 1.0 field reports.
