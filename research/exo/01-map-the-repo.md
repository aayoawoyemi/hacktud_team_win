# 01 Map the repo (EXO)

Studied: `exo-explore/exo` main at commit `a60b95698ca81737d367443674dd5563cbc9559b`, 2026-11-06 15:00 -0700
("Serve requests without the master: each request streams over HTTP from the device that runs it (#2597)").
Clone: `C:\Users\aayoa\Desktop\hackathon\exo-src` (depth 50, covers 2026-11-01 to 2026-11-06).
Latest tag: `v1.0.71` (git ls-remote). Python package version in `pyproject.toml`: `0.3.70`.

## Which generation

Current main is **exo 1.0**, a complete rewrite of the 2024 "legacy exo".

| | Legacy exo (2024 to mid 2025) | exo 1.0 (current main) |
|---|---|---|
| Language | Python only | Python core + Rust networking (PyO3 bindings `exo_rs`) + Swift macOS app + Svelte dashboard |
| Engines | MLX and tinygrad (tinygrad gave NVIDIA/Linux) | MLX only (Metal on Mac; `mlx-cpu`, `mlx-cuda-12/13` on Linux). tinygrad removed "in the v1 prep" |
| Networking | gRPC between peers, UDP broadcast discovery | Zenoh (pinned fork `evanev7/zenoh`, branch `exo`) + own IPv6 multicast discovery (`rust/networking/src/discovery.rs`) |
| Splitting | Pipeline "ring", memory-weighted layer split | Pipeline AND tensor parallel via `mx.distributed` (backends `ring` = TCP, `jaccl` = RDMA over Thunderbolt 5) |
| Control plane | Peer-to-peer, each node equal | Elected master + workers, event-sourced shared `State` |
| Install | `pip install` | macOS app (DMG / `brew install --cask exo`), or `uv run exo` from source |

Source for rewrite: maintainer AlexCheema in issue #1039: "exo 1.0 was a complete rewrite ... the new design does make it
easier to implement different inference engines"; reporter states "The tinygrad backend was removed in the v1 prep"
(https://github.com/exo-explore/exo/issues/1039, closed). Most Reddit/HN/blog content from 2024 describes the legacy
version; treat any tinygrad/gRPC/"works on Linux NVIDIA" claim as legacy.

## Size (git ls-files, this commit)

| Kind | Files | Lines |
|---|---|---|
| Python (`*.py`) | 552 | 116,387 |
| Rust (`*.rs`) | 16 | 6,226 |
| Svelte | 31 | 22,078 |
| TypeScript | 36 | 7,515 |
| Swift (macOS app) | 72 | 10,674 |
| Total tracked files | 1,338 | |

Python by package (incl. tests): `worker` 285 files / 54,890 lines (most of it per-model MLX code in
`worker/engines/mlx/models/*`), `api` 13,173, `shared` 10,166, `master` 9,116, `download` 6,100, `testkit` 5,946,
`utils` 4,522, `routing` 3,498, `main.py` 683.

## Top-level layout

- `src/exo/` the Python app (entry `exo = "exo.main:main"` in pyproject; `__main__.py` for `python -m exo`)
  - `main.py` builds a `Node` (lines 78 to 250): Router, EventRouter, Membership, DownloadCoordinator, Worker,
    Master, Election, API, DeploymentKeeper. Every node starts a Master object; election decides which is active
    (`--force-master` gives seniority 1,000,000, main.py ~line 205).
  - `routing/` Python side of networking: `router.py` (topics over Zenoh), `topics.py` (GLOBAL_EVENTS, LOCAL_EVENTS,
    STATE_SNAPSHOTS, COMMANDS, ELECTION_MESSAGES, MEMBERSHIP, DOWNLOAD_COMMANDS), `membership.py`, `wire.py`
    (wire version + `build_id()` line 67).
  - `master/` `main.py` (708 lines), `placement.py` (585), `placement_utils.py` (461): decides which devices run
    which shard of which model.
  - `worker/` `main.py` (590), `plan.py`, `serving.py`, `runner/` (runner subprocess: `bootstrap.py`,
    `supervisor.py`, `runner.py`, `llm_inference/batch_generator.py`), `engines/mlx/` (the inference engine).
  - `worker/engines/mlx/sharding/` `pipeline.py` (342), `tensor.py` (60), `blocks.py` (54);
    `auto_parallel.py` (170, `pipeline_auto_parallel`); `utils_mlx.py` lines 133 and 156 call
    `mx.distributed.init(backend="ring")` / `backend="jaccl"`.
  - `api/` FastAPI/Hypercorn HTTP API (`main.py` 2,726 lines), adapters incl. `chat_completions.py` (OpenAI
    style). Detail in section 4.
  - `download/` Hugging Face downloads, resumable, one state machine per model (refactored 2026-11-03, #2558).
  - `shared/` pydantic types, `State`, election, constants.
- `rust/networking/` `discovery.rs` (752, IPv6 multicast Hellos), `membership.rs` (2,418), `swarm.rs` (1,110),
  `wire.rs` (382). `rust/exo_rs/` PyO3 bindings + pidfile.
- `app/` Swift macOS menu-bar app (installs `io.exo.networksetup` service for Thunderbolt networking).
- `dashboard/` SvelteKit web UI, built with `npm run build`, served by the API.
- `bench/` `exo_bench.py`, `prefill_decode_bench.py`, `parallel_requests.py`, TOML scenarios.
- `docs/api.md`, `docs/benchmarks/` (Jeff Geerling 4x M3 Ultra images), `packaging/` (DMG, PyInstaller), `flake.nix`.

## Key dependencies (pyproject.toml)

fastapi, hypercorn, pydantic, msgspec, anyio 4.11.0 (pinned), rustworkx (topology graph), huggingface-hub,
transformers >= 5.6.2, psutil, loguru, python-daemon, exo-rs (local Rust workspace). Extras: `mlx` (mlx >= 0.32.3,
pinned on macOS to git rev `36969c09`, mlx-lm pinned git rev `6b572ab1`, mlx-vlm, mflux for images, torch 2.10 on
darwin/linux only), `mlx-cpu`, `mlx-cuda12`, `mlx-cuda13` (Linux only). Rust: zenoh 1.9.0 (forked), tokio, pyo3 0.28.

## Install and run

macOS (Apple Silicon, macOS Sequoia 15.1+; RDMA needs Tahoe 26.2+):
- App: download DMG or `brew install --cask exo`. Adding a Mac = install + same Wi-Fi/Ethernet or a Thunderbolt cable.
- Source: `brew install uv node`, Rust via rustup, `macmon` from a pinned git fork (Homebrew macmon 0.6.1 crashes on
  M5), then `cd dashboard && npm install && npm run build`, `uv sync` with the mlx extra, `uv run exo`.
  Flags: `--no-worker`, `--no-api`, `--force-master`, `--namespace <name>` (or `EXO_ZENOH_NAMESPACE`), `--offline`,
  `--legacy-daemon`.

Linux: source only, `uv run exo` with `--extra mlx-cpu` or `mlx-cuda12/13`. README line 298: "Currently, exo runs on
CPU on Linux. GPU support ... is under development." PLATFORMS.md lists Linux CUDA (DGX Spark) and Linux CPU as
*planned*, not supported; Tier 1 is only Apple Silicon (M3 Ultra Studio, M4 Pro mini, M5 / M4 Max MBP).

Windows: **not supported, will not run natively.** `src/exo/main.py` line 4 `import resource` and line 468
`resource.setrlimit(RLIMIT_NOFILE)` are POSIX-only; `python-daemon` (DaemonContext) is POSIX; torch extras exist only
for darwin/linux; MLX has no Windows wheels. WSL2 would be "Linux CPU" at best.

## Mac + non-Mac in one cluster: blocked by design

README line 142: "A Mac and a Linux machine don't join each other, even on the same commit, because they run
different builds of MLX." Enforced by `build_id()` (`src/exo/routing/wire.py` line 67 to 80): a node joins only peers
with an identical string `"exo <ver>, mlx <ver>, mlx-lm <ver>"`, or the same `EXO_BUILD_ID` env value. Setting the same
`EXO_BUILD_ID` on both sides would bypass the check; whether a Metal rank and a CPU rank can then run one model over the
MLX `ring` backend is **unverified** (section 3 will check). Likely fragile; not something to plan the demo around.

## Where the core logic lives

1. Control plane: `src/exo/main.py` (Node wiring), `src/exo/master/main.py` + `placement.py`, `src/exo/shared`
   (State/events), `src/exo/routing` + `rust/networking`.
2. Data plane (inference): `src/exo/worker/runner/*` (separate runner process per model instance) and
   `src/exo/worker/engines/mlx/*` (sharding, `mx.distributed`, per-model patches).
3. Serving: `src/exo/api/main.py`, `src/exo/worker/serving.py`; as of #2597 (2026-11-06) each request streams over
   HTTP from the device that runs it, not via the master.

## Unverified / to check later
- Exact date of the 1.0 rewrite (no old history in the shallow clone; web search did not give a date).
- Whether legacy exo is still installable (old PyPI/tag) as a Windows-friendly fallback: probably not worth it.
