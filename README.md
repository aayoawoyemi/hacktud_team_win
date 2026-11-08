# Oasis

Oasis pools the laptops an office already owns into one private LLM server. A model too big for any one machine
runs split across several of them over a local network, with no cloud and no internet.

A small company that deals in law or healthcare could use the computers lying around to build a mini home lab using our protocol.

## Results (measured, Sun Nov 8 2026)

<!-- CONCEPT: the numbers Devpost and the pitch quote. WHY: Aries 13:47Z; every row links its raw bench files.
DEPENDS ON: crates/bench (`bench`), test-runs/bench/. AFFECTS: Devpost, video. Add a row per new bench run. -->

Generation speed of Gemma 4 on three pooled MacBook Airs and on one Air alone:

| Run | Model | Machines | Flags | Gen tok/s (median) | TTFT | Raw files |
|---|---|---|---|---|---|---|
| **B3** | **Gemma 4 26B-A4B Q8_0 (25.0 GiB)** | **3 Airs** | FA, q8 KV, `--no-mmap` | **12.5** (12.1-12.6), flat for 5 min | **0.6 s** (short/long) | `test-runs/bench/p4-26b-final/` |
| B2-fa | Gemma 4 12B Q4_K_M (7.1 GB) | 3 Airs | FA, q8 KV | 11.2 (10.8-11.9), flat for 5.5 min | 0.6-0.7 s | `test-runs/bench/p4-20261108-0150-existing-fa-q8/` |
| B1 | Gemma 4 12B Q4_K_M | 1 M5 Air | none | ~16 cold, **8-9 sustained** (throttles after ~1.5 min) | 0.3-0.9 s | `test-runs/bench/night-20261108-0408/` |
| B1s | Gemma 4 12B Q4_K_M | 1 M5 Air | `--spec-type ngram-simple` | rewrite task: **1.6-1.7x** (9 → 14.5, 7.2 → 12.4); new text: no gain | | `test-runs/bench/night-20261108-0329/`, `-0408/` |

Every run on the hardware is listed in [test-runs/INDEX.md](test-runs/INDEX.md).

### ? What this table shows

- Hardware: three MacBook Airs on a gigabit switch (M3 16 GB as main, two M5 24 GB as helpers), llama.cpp b8665
  over RPC, SpacetimeDB as the control plane, no internet.
- Method: every number comes from `bench` with thinking off and ctx 4096. Each prompt gets one warm-up and 3
  measured runs (short 128, long 512, edit 512 tokens). Values are medians.
- Gen tok/s: generated tokens per second.
- TTFT: time to first token.
- FA, q8 KV: flash attention and an 8-bit KV cache.
- `--no-mmap`: main reads only its share of the weights into memory instead of mapping the whole file.
- `--spec-type ngram-simple`: speculative decoding that drafts tokens from text already in the context, with no
  draft model.

Findings:

- The 26B runs on hardware where no single machine can hold it. Its weights are 25.0 GiB, 1.4x the biggest Air's
  GPU budget of 17.8 GiB. On the B3 run the M3 held 5718 MiB, each M5 held 9941 MiB, and all 31 layers ran
  on GPU.
- Sustained, the pool is faster than one laptop. Three fanless Airs hold ~11-12.5 tok/s for minutes. One Air alone
  drops from ~16 to 8-9 tok/s as it heats up. Cold, one Air is faster on the 12B. The pool runs used flash
  attention and q8 KV; B1 did not.
- N-gram speculative decoding speeds up rewrite tasks 1.6-1.7x (79% draft acceptance) and gives no gain on new text.

## How it works

Oasis has two layers.

- Control plane: a SpacetimeDB 2.14.1 module, the Oasis protocol (`crates/control`). Its tables hold node
  registration, heartbeats, commands, split plans, cluster status, events and model requests. Agents and the
  coordinator change state through reducers and read it through subscriptions.
- Data plane: llama.cpp RPC (build 8665). `llama-server` runs on the main Mac with `--rpc` pointing at `rpc-server`
  on each helper. llama.cpp splits the model by layers and sends activations between machines for every token.

```
            browser: web/oasis-v2.html  (reads SpacetimeDB :3000 and llama-server :8080)
                                  |
 main Mac  +----------------------+-----------------------+
           | SpacetimeDB :3000  <-- coordinator --> llama-server :8080
           +--------^------------------------------------|-------------+
                    | reducers + subscriptions           | llama.cpp RPC
 helper Macs  agent -> rpc-server :50052   agent -> rpc-server :50052
```

- Discovery: each machine runs an agent. The agent reads its GPU devices and free memory
  (`llama-server --list-devices`), registers a `node` row and sends a heartbeat every second.
- Placement: the coordinator writes a split plan and sends each helper a `command` to start `rpc-server`. It then
  launches `llama-server` with one `--rpc` address per helper. llama.cpp places layers by each device's free memory;
  `tensor_split` in the config sets layer counts by hand. The planner crate that will check fit before launch is
  not wired in yet.
- Live join: a new helper registers and starts `rpc-server`, and the coordinator re-plans the model across all
  helpers.
- Model switch: the dashboard calls the `request_model` reducer. The coordinator loads only the default `model_id` and the
  models listed under `[models]` in its config.
- Cluster states: `PLANNED`, `STARTING_HELPERS`, `LAUNCHING`, `SERVING`. A lost helper moves the pool to
  `DEGRADED`. A stop moves it to `STOPPED`.

## Run it

Needs: Macs on one private wired network, llama.cpp build 8665 binaries on each Mac, a GGUF model on main, and Rust
(`rust-toolchain.toml` pins 1.99.1 and the wasm32 target; rustup installs both on the first `cargo` command).
Ports: 3000 (SpacetimeDB), 8080 (llama-server), 50052 (rpc-server). [docs/network.md](docs/network.md) covers
ports, firewalls and the `netcheck` preflight.

Start every program below from Terminal.app or over SSH. macOS Local Network privacy blocks LAN connections from
command-line tools launched by another app (errno 65, Apple TN3179).

1. Build, on every Mac: `cargo build --release`. No `protoc` is needed.
2. Control plane, on main. Install the `spacetime` CLI (`curl -sSf https://install.spacetimedb.com | sh`), pin it with
   `spacetime version install 2.14.1 --use`, then:

   ```
   spacetime start
   spacetime publish -s local -p crates/control pool -y
   ```

   Always pass `-s local`. The CLI's default server is the hosted maincloud.
3. Agent, on each helper. Copy `crates/agent/agent.example.toml` to `agent.toml` and set `node_id` (unique per
   machine), `role = "helper"`, `spacetime_uri = "http://<main-ip>:3000"` and `llama_bin_dir`. Keep the Mac awake
   and plugged in:

   ```
   caffeinate -dims &
   ./target/release/agent agent.toml
   ```

4. Coordinator, on main. Copy `crates/coordinator/coordinator.example.toml` to `coordinator.toml` and set
   `llama_server_path`, `model_path`, `model_id` and `min_helpers`. For a model bigger than main's RAM, add
   `extra_args = ["--no-mmap"]`. `SPACETIME_TOKEN` is the publisher's token from `~/.config/spacetime/cli.toml`.
   Press Enter to stop the pool.

   ```
   SPACETIME_TOKEN=<publisher token> ./target/release/coordinator coordinator.toml
   ```

5. Dashboard, from any machine that reaches main: `cd web && python3 -m http.server 8000`, then open
   `http://<main-ip>:8000/oasis-v2.html?stdb=http://<main-ip>:3000&llama=http://<main-ip>:8080`.
   `oasis-v2.html?fixture=1` shows sample data without a pool.
6. Chat: `llama-server` on main serves the OpenAI-style API at `http://<main-ip>:8080/v1/chat/completions`.
7. Bench: copy `crates/bench/bench.example.toml` to `bench.toml`, set `scenario`, `model_id` and `nodes` to match
   the launch, then run `cargo run -p bench --release -- bench.toml`.
8. Night bench (B1 to B5 in a row on main): copy `scripts/bench-night.env.example` to `scripts/bench-night.env`,
   fill in the paths, then run `open -a Terminal scripts/bench-night.command`. Results land in
   `test-runs/bench/night-<stamp>/SUMMARY.md`.

After changing `crates/control`, regenerate the committed client bindings:
`spacetime generate --lang rust --out-dir crates/contracts/src/control --module-path crates/control -y`.

## Known limits

- Main must load big models with `--no-mmap`. With mmap, the 26B mapped 25.6 GB on the 16 GB M3 and generation
  stalled in Metal (`IOGPUCommandDescriptor::prepare … 0xE00002BE`). With `--no-mmap`, main holds only its 5718 MiB
  share.
- Cold 26B load takes ~5 min while the weights stream to the helpers. `rpc-server -c` (`rpc_cache = true` in
  `agent.toml`) caches them for the next load.
- Launch from Terminal.app or over SSH, never from inside another app (macOS Local Network privacy, Apple TN3179).
- Sleep: a sleeping helper takes the pool down until it is back. Run `caffeinate -dims` on every helper and keep the
  Macs on power.
- Recovery: a lost helper moves the pool to `DEGRADED` and `llama-server` keeps the old plan. When the helper's agent
  comes back, the coordinator re-plans and the pool returns to `SERVING`. If `llama-server` exits, the coordinator
  makes one bring-up with the helpers still online; if that fails, the pool goes to `STOPPED`.
- Network: every token crosses the network once per helper. Use a wired network.
- Thermals: fanless MacBook Airs throttle after ~1.5 min of generation.
- Gemma 4 writes its thinking to `reasoning_content`. A low `max_tokens` (128) returns an empty answer.

## Repository layout

| Path | Contents |
|---|---|
| `crates/control` | The Oasis protocol: the SpacetimeDB module (tables and reducers) |
| `crates/contracts` | Shared types, llama.cpp output parsers and the generated SpacetimeDB client bindings |
| `crates/agent` | Per-machine agent: hardware probe, registration, heartbeats, `rpc-server` supervision |
| `crates/coordinator` | Main-Mac process: plans the split, launches `llama-server`, re-plans on join and on model requests |
| `crates/bench` | Benchmark runner: time to first token and tokens/s, written as JSONL, CSV and markdown |
| `crates/netcheck` | Port reachability, round-trip time and throughput between machines |
| `crates/planner`, `crates/relay` | Placeholders for the split planner and relay telemetry |
| `web/` | Dashboards: `oasis-v2.html` (live, token flow view) and `oasis.html` (v1) |
| `scripts/` | Team git commands and the night bench (`bench-night.sh`) |
| `test-runs/` | Logs and numbers from every run on the hardware |
| `prompts/` | Bench prompts (`bench.toml`) |
| `docs/` | Network setup ([network.md](docs/network.md)) and team workflow ([team-workflow.md](docs/team-workflow.md)) |
| `CONTRACTS.md` | Shared shapes: tables, reducers, routes, env vars, folder ownership |

Model files (`models/`, `*.gguf`) and `target/` are git-ignored. Team workflow (branches, `git ship`, `git sync`,
agent rules): [docs/team-workflow.md](docs/team-workflow.md).
