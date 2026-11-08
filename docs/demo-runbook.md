<!--
CONCEPT: the demo runbook (WP-13). Cold start → Part 1 on the pool → a machine joins → Part 2 (bigger than any one
         machine) → benchmark numbers → clean stop, using the merged Rust tooling (agent, coordinator, bench, netcheck)
         and SpacetimeDB on main.
WHY: Checkpoint C = "the full demo runs 3 times in a row from cold, following docs/demo-runbook.md". D-14: the demo
     shows machines joining, never leaving. D-13: live status comes from SpacetimeDB subscriptions, no frontend.
DEPENDS ON: docs/network.md (wiring, firewall, IPs, netcheck), docs/runbook-manual.md (PR #26: by-hand fallback and
     install), crates/{agent,coordinator,bench,netcheck} at main 2c3e491, contracts::defaults (ports, O-26 timings).
AFFECTS: WP-14 video script follows section 4. Change this file when a command or config key changes.
STATUS: DRAFT (Agent P2, Sat 9:30pm). Not yet run end to end on the real machines. Lines marked VERIFY are unrun.
-->
# Demo runbook

Open decisions this file waits on: **Part 2 model** (O-09, Aayo deciding 9:15pm), **what the audience sees** (O-10;
section 4 assumes two terminals + llama-server's built-in web page), **project name** (O-18).

## 1. Machines and roles

| Machine | Agent | Role | Runs | GPU budget (MTL0, measured) |
|---|---|---|---|---|
| M3 MacBook Air 16 GB | P4 | **main** | `spacetime start`, `coordinator`, llama-server (started by the coordinator), `bench`; holds both model files | 12124 MiB |
| Preston's M5 24 GB | P1/P2 | helper | `agent` (starts rpc-server on command) | 18186 MiB |
| M5 Air 24 GB | P5 | helper, **the one that joins live** | `agent` | 18186 MiB |
| Dell (Windows) | Aries | optional helper | `agent` | Vulkan, ~3-7 GiB |

Pool ≈ 47 GiB of GPU budget across the three Macs; the largest single machine has ≈ 17.8 GiB. No `.gguf` on helpers:
llama-server on main streams their layers over RPC, and `rpc_cache = true` keeps them for the next load.

## 2. One-time setup (before the first rehearsal)

1. Wiring, IPs, firewall, service order: docs/network.md sections 2, 4-6. Record wired IPs here:
   main `________`  M5 `________`  M5 Air `________`  Dell `________`
2. Every machine: llama.cpp **b8665** official binaries (`llama-server --version` → `8665 (b8635075f)`), Rust 1.99.1,
   repo at main ≥ 2c3e491.
3. Build (from the repo root):
   - main: `cargo build --release -p coordinator -p bench -p netcheck`
   - helpers: `cargo build --release -p agent -p netcheck`
4. main only: SpacetimeDB 2.14.1 (`spacetime version install 2.14.1 --use`), wasm32 target (rustup installs it on
   the first publish), both models in `~/models/` (never in the repo).
5. Configs (copy the examples, edit, keep outside git or in `~/pool/`):
   - helper `agent.toml` (from `crates/agent/agent.example.toml`): unique `node_id` (`m5`, `m5-air`, `dell`),
     `role = "helper"`, `spacetime_uri = "http://<main IP>:3000"`, `llama_bin_dir`, `rpc_bind = "0.0.0.0"`,
     `rpc_cache = true`.
   - main `part1.toml` and `part2.toml` (from `crates/coordinator/coordinator.example.toml`):
     `spacetime_uri = "http://127.0.0.1:3000"`, `llama_server_path`, `model_path`, `model_id`, `ctx_size = 4096`,
     `host = "0.0.0.0"`, `port = 8080`, `ready_timeout_s = 300`.
   - main `bench-part1.toml` / `bench-part2.toml` (from `crates/bench/bench.example.toml`): `endpoint =
     "http://127.0.0.1:8080"`, `scenario`, `model_id`, `nodes`, `size_ratio`.
6. Pre-warm the helper caches once (a full Part 1 and Part 2 load). After that a reload skips most network transfer.

## 3. Cold start (T-15 min). Every step has a check; don't continue past a failed check.

All Macs: plugged in, Low Power Mode off, other apps quit, `caffeinate -dims &` in a spare terminal.

| # | Where | Command | Check |
|---|---|---|---|
| 1 | main | `spacetime start` (leave running) | log says listening on `0.0.0.0:3000` |
| 2 | main | `spacetime publish -s local -p crates/control pool -y` (only if the DB is new; republishing an existing DB keeps its data, VERIFY) | `spacetime sql -s local pool "SELECT cluster_state FROM cluster_status"` prints one row |
| 3 | each helper | `target/release/netcheck check main=<main IP>:3000` | `ok` from every helper |
| 4 | main | `target/release/coordinator part1.toml` (leave running) | prints `coordinator identity <hex>` and the `add_admin` command, then `waiting for helpers...` |
| 5 | main, 2nd terminal | paste the printed `spacetime call -s local pool add_admin '"0x<hex>"'` (first run only; the identity is saved, VERIFY) | no error |
| 6 | main, 3rd terminal (audience status) | `spacetime subscribe -s local pool "SELECT node_id, role, state, online FROM node" "SELECT cluster_state, model_id, endpoint FROM cluster_status"` (VERIFY multi-query syntax) | live rows |

**Do not start P5's agent yet**: it is the join in section 4.

## 4. The demo (≤ 2 min on video; live can be longer)

1. **"Two laptops become one AI server."** On the M5: `target/release/agent agent.toml`.
   Status terminal: `m5` appears REGISTERED → READY. Coordinator: plans, starts the M5's rpc-server, launches
   llama-server over main + M5 → `SERVING`. Part 1 (Gemma 4 12B) answers: open `http://<main IP>:8080` (llama-server's
   built-in chat page) on a phone or the projector and ask a demo prompt from `prompts/bench.toml` (`use = "demo"`).
2. **"A third laptop joins."** On the M5 Air: `target/release/agent agent.toml`. Status: `m5-air` appears; the
   coordinator re-plans (`re-planned, SERVING with [...]`, ~2.5 s on the M5 test plus model reload time). Ask again.
3. **"Now a model none of these laptops can hold."** Coordinator terminal: press **Enter** (clean stop: helpers'
   rpc-servers stop, cluster → STOPPED). Then `target/release/coordinator part2.toml`. Agents stay up; it brings up
   Part 2 over all three Macs (cold load can take minutes; pre-warmed caches make it faster). Ask the demo prompt.
   Say the honest ratio: Part 2 size ÷ largest single machine's budget (≈ 1.4-1.7x with the candidates in #11),
   and that the internet is off (router WAN unplugged, D-02).
4. **Numbers** (pre-recorded is fine for the video): `target/release/bench bench-part2.toml` → markdown table with
   TTFT and tokens/s; compare with `single_fit` on one machine (section 5).

## 5. Benchmarks for the write-up

Run each with 1 warm-up + 3 repeats (the defaults); keep the `.md` files for docs/findings.md and Devpost.

| Scenario | Pool | Model | Config |
|---|---|---|---|
| `single_fit` | main only: `llama-server -m <part1> -ngl 99 --port 8080` by hand | Part 1 | `bench-single.toml` |
| `split_2` | coordinator part1 + M5 | Part 1 | `bench-part1.toml`, `scenario = "split_2"` |
| `pool_part1` | coordinator part1 + M5 + M5 Air | Part 1 | `scenario = "pool_part1"` |
| `pool_part2` | coordinator part2 + both helpers | Part 2 | `bench-part2.toml` |

The Air is fanless: never measure on its first run (warm-ups exist for this) and note throttling if tokens/s drifts.

## 6. Stop and reset (between rehearsals)

1. Coordinator: **Enter** (not Ctrl-C: Ctrl-C skips stopping the helpers' rpc-servers).
2. Each helper agent: Ctrl-C (it stops rpc-server first; no process left: `pgrep rpc-server` prints nothing).
3. Leave `spacetime start` running between rehearsals. For a fully cold run, stop it too.

## 7. When something fails

| Symptom | Likely cause | Fix |
|---|---|---|
| helper never appears in `node` | wrong `spacetime_uri`, firewall on main:3000, Wi-Fi/LAN mix-up | `netcheck check main=<main IP>:3000` from the helper; docs/network.md §6 |
| coordinator stuck before SERVING, then `E_NOT_READY_TIMEOUT` | cold Part 2 load slower than 300 s | pre-warm caches; raise `ready_timeout_s` in the config |
| `E_RPC_UNREACHABLE` | rpc-server bound to loopback or blocked | `rpc_bind = "0.0.0.0"`; macOS firewall prompt for rpc-server; `netcheck check h=<IP>:50052` from main |
| llama-server exits during load or a request | a helper dropped (llama.cpp aborts on a lost RPC peer) | out of demo scope (D-14): stop, restart the agent, start the coordinator again |
| `node_id ... owned by another identity` | agent token mismatch after a reinstall | the agent saves a token per node_id; use a new `node_id` or restore its token file |
| main runs out of memory on Part 2 | the 16 GB Air maps the whole Part 2 file while streaming helpers' layers (VERIFY on the first Part 2 load) | quit other apps; if it still fails, pick the smaller Part 2 candidate |

**Fallback if the tooling fails on stage:** the by-hand path in docs/runbook-manual.md (rpc-server on helpers,
`llama-server --rpc ...` on main) gives the same answer without SpacetimeDB.
