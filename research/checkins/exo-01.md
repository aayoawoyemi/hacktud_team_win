## Check-in exo 01: Map the repo

**Found (most important first, max 8 bullets, each with a source):**
- Studied main at `a60b956` (2026-11-06), latest tag `v1.0.71`. Current main is **exo 1.0**, a full rewrite of the 2024 "legacy exo". Most Reddit/HN posts describe the old version (tinygrad, gRPC, runs on Linux/NVIDIA), so treat those claims as stale. Source: maintainer comment in issue #1039 (https://github.com/exo-explore/exo/issues/1039): "exo 1.0 was a complete rewrite"; tinygrad was "removed in the v1 prep".
- **The Windows Dell cannot join an exo cluster.** `src/exo/main.py` line 4 imports the POSIX-only `resource` module and calls `setrlimit` at line 468. `python-daemon` is POSIX-only too, and MLX has no Windows wheels. Native Windows fails at startup. WSL2 would at best count as "Linux CPU".
- **Macs and Linux machines are blocked from joining each other on purpose.** README line 142: "A Mac and a Linux machine don't join each other, even on the same commit, because they run different builds of MLX." The check is `build_id()` in `src/exo/routing/wire.py` lines 67 to 80, which compares `"exo X, mlx Y, mlx-lm Z"`. Setting the same `EXO_BUILD_ID` env var on both sides bypasses the check. Whether a Metal rank and a CPU rank can then share one model is unverified.
- Officially supported: Apple Silicon only (PLATFORMS.md Tier 1: M3 Ultra Studio, M4 Pro mini, M5/M4 Max MBP). Linux CPU and CUDA are listed as "planned". README line 298 says Linux runs on CPU only.
- Stack: Python core with 552 .py files and 116k lines, including tests and per-model code. Networking is in Rust: 6.2k lines, built on Zenoh 1.9 (a forked copy) and their own IPv6 multicast discovery in `rust/networking/src/discovery.rs`. There is also a Swift macOS menu-bar app (10.7k lines) and a SvelteKit dashboard (about 30k lines). The repo has 1,338 tracked files.
- Architecture: every node runs a Router, Membership, Worker and Master object, and an election picks the active master (`src/exo/main.py` lines 78 to 250). Shared state is event-sourced over Zenoh topics (`src/exo/routing/topics.py`). Placement code is in `src/exo/master/placement.py` (585 lines).
- Inference is MLX only. It supports both pipeline and tensor parallel (`src/exo/worker/engines/mlx/sharding/pipeline.py` and `tensor.py`) through `mx.distributed.init(backend="ring")`, which runs over TCP, or `backend="jaccl"`, which is RDMA over Thunderbolt 5 (`utils_mlx.py` lines 133 and 156).
- Mac install is easy: a DMG, or `brew install --cask exo`. Macs on the same LAN find each other automatically. The README warns that "guest Wi-Fi and some office networks stop devices from seeing each other" (it relies on multicast), so hackathon Wi-Fi is a real risk. Bring a router or switch, or Thunderbolt cables. Joining also requires the same exo build on every Mac.

**Surprises / things that change the plan:**
- EXO alone cannot meet the "Macs + Windows Dell" demo. Either the Dell joins through a different layer, such as llama.cpp RPC or our own router in front of exo's OpenAI API, or the Dell's role shrinks to something like gateway or dashboard host.
- The repo is active, with commits every day; #2597 on Nov 6 changed request serving. Pin one release (for example `v1.0.71`) on every Mac, because devices on different builds refuse to join.

**PRD suggestions:**
- Open question 4: add "EXO main (1.0) does not run on Windows and refuses Mac+Linux mixes (README line 142, `wire.py` `build_id`). The Dell needs a non-EXO path."
- Open question 2: note that "EXO" now means exo 1.0 (MLX-only, Apple-first). The 2024 tinygrad/gRPC version is gone.
- Demo hardware: add "a travel router or switch is strongly recommended. EXO discovery uses IPv6 multicast, which guest/venue Wi-Fi often blocks."

**Questions for Aayo:**
1. Is it acceptable for the Windows Dell to run its own model, or to act as the front door/router, instead of holding a slice of the same model as the Macs? This decides whether I dig into the `EXO_BUILD_ID` bypass with a Mac + Linux-CPU MLX ring in section 3.
2. How many Macs are confirmed, and do any have Thunderbolt 5 (M4 Pro/Max or M5) with macOS 26.2 or later? RDMA/tensor parallel depends on this.

**Next section:** Section 2, cluster formation: `rust/networking/src/discovery.rs`, `membership.rs` and `swarm.rs`, plus `src/exo/routing/*` and the election. I would add one item to it: exactly what blocks discovery on venue Wi-Fi (the multicast group and ports), and whether there is a manual or static-peer fallback (I spotted the `ANNOUNCE_TO_ENV` override in discovery.rs).
