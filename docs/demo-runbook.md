<!--
CONCEPT: the demo runbook (WP-13): cold start → 12B on 3 MacBook Airs → switch to the 26B from the dashboard →
         live join → numbers → clean stop. Everything here was run on the real machines Sat night / Sun morning,
         except lines marked VERIFY.
WHY: Checkpoint C = "the full demo runs from cold, following docs/demo-runbook.md". D-14: the demo shows joins.
     Aayo 7:00am Sun: final scope = B3 + rehearsal of model switch and live join (B5 dropped, Aries 8:00am).
DEPENDS ON: main ≥ a4cb62f (#49 rejoin, #51 model switch, #52 recovery, #53/#55 stuck rpc-server self-heal),
     docs/network.md (wiring, firewall), web/oasis.html (dashboard, v1), scripts/bench-night.sh (bench), #11 logs.
AFFECTS: the video script (WP-14) follows section 4. Change this file when a command, config key or IP changes.
STATUS: Agent P2, Sun 7:10am CT. Replaces the Sat 9:30pm draft (which assumed the M5 as main and Part 2 unknown).
-->
# Demo runbook

## 1. Machines (all wired to the Opal router + switch, 192.168.8.0/24, ~0.7 ms per hop)

| IP | Machine | Role | Runs | GPU budget (MTL0) |
|---|---|---|---|---|
| 192.168.8.113 | MacBook Air M3, 16 GB | **main** | SpacetimeDB `pool` :3000, `coordinator`, llama-server :8080 (started by the coordinator), holds the GGUFs | 12124 MiB |
| 192.168.8.174 | MacBook Air M5, 24 GB (Preston) | helper | `agent` (starts rpc-server :50052), dashboard server :8000 | 18186 MiB |
| 192.168.8.204 | MacBook Air M5, 24 GB (P5) | helper | `agent` | 18186 MiB |

Pool ≈ 47 GiB of GPU budget; the biggest single Mac has 17.8 GiB. Models on main (`~/models` or P4's
`p4-runtime/downloads/`): **Gemma 4 12B Q4_K_M** (7121861440 B, sha256 `0a270ec9…`) and **Gemma 4 26B-A4B Q8_0**
(26859861728 B, sha256 `5f7cbd0f…`, 25.0 GiB: fits only on the pool). Helpers hold no GGUF; rpc-server `-c` caches
the layers they receive.

## 2. Rules that cost us hours (read before anything)

1. **Start every pool program from Terminal.app, never from inside Codex.** macOS Local Network privacy blocks LAN
   connections from binaries launched by an app without the permission (`errno 65 No route to host`,
   Network.framework `localNetworkDenied`) while `/usr/bin/nc` still works. P4 uses a small launcher app; Terminal
   works too. System Settings > Privacy & Security > Local Network: Terminal ON.
2. **`caffeinate -dims &` on all three Macs, lids open, power in.** A sleeping helper kills the pool (llama.cpp
   aborts when an RPC peer vanishes); .204 slept at 2:35am and the pool stayed down for 4 h.
3. **Stop the coordinator with Enter**, not Ctrl-C or `kill` (Enter stops llama-server and the helpers' rpc-servers).
4. **Don't start rpc-server by hand on a helper.** The agent owns it. A hand-started one blocks the agent's
   (`E_PORT_IN_USE`); the coordinator now recovers from that once (#53), but don't rely on it on stage.
5. **Never `--delete-data` on publish.** Republishing `pool` keeps the data (#33).

## 3. Cold start (T-20 min). Don't continue past a failed check.

| # | Where | Command | Check |
|---|---|---|---|
| 1 | all 3 | `caffeinate -dims &` (Terminal) | prints `[1] <pid>` |
| 2 | main | `spacetimedb-cli start -l 0.0.0.0:3000` (leave open) | `curl -s -o /dev/null -w '%{http_code}' http://192.168.8.113:3000/v1/ping` → `200` from a helper |
| 3 | main | `spacetimedb-cli publish -s local -p crates/control pool -y` (or `--bin-path control.wasm` if main has no cargo) | `spacetimedb-cli sql -s local pool "SELECT cluster_state FROM cluster_status"` prints a row |
| 4 | each helper | `target/release/netcheck check main=192.168.8.113:3000` | `ok` |
| 5 | each helper | `target/release/agent agent.toml` (Terminal, leave open) | `SELECT node_id, state, online FROM node` shows it `READY`/`REGISTERED`, online |
| 6 | main | `SPACETIME_TOKEN=<publisher token> target/release/coordinator coordinator.toml` (Terminal or P4's launcher app) | `SERVING on 8080: [...]` in ~35 s (12B, warm caches) |
| 7 | any Mac | from the repo root: `python3 -m http.server 8000` | the dashboard URL below loads |

**Configs:**
- helper `agent.toml`: `node_id` (`m5-air-174` / `p5-air-204`), `role = "helper"`,
  `spacetime_uri = "http://192.168.8.113:3000"`, `spacetime_db = "pool"`, `rpc_bind = "0.0.0.0"`, `rpc_cache = true`.
- main `coordinator.toml` (from `crates/coordinator/coordinator.example.toml`): `spacetime_uri =
  "http://127.0.0.1:3000"`, `model_id`/`model_path` = the 12B, `public_host = "192.168.8.113"`, `min_helpers = 2`,
  `ready_timeout_s = 900` (the 26B cold load took 303 s), **`extra_args = ["--no-mmap"]`** (plus `flash_attn = "on"`,
  `"-ctk","q8_0","-ctv","q8_0"` as in B3; without `--no-mmap` the 26B stalls on the 16 GB M3), and a `[models]` table with the 12B and 26B ids → paths.
  First run prints an `add_admin` command: run it once.

**Dashboard (v1, Aayo Sun 8:25am):** `http://192.168.8.174:8000/web/oasis.html?stdb=http://192.168.8.113:3000&llama=http://192.168.8.113:8080`
(or the IP of whichever Mac serves :8000). Live mode: topology, events, token flow, `ask` with TTFT and tok/s. v1 has no
model menu: switch models with the `request_model` call below (or the menu in `oasis-v2.html`, same reducer).

**Pre-warm before recording:** switch to the 26B once and back (section 4.2), so the helpers' `-c` caches hold its
layers. VERIFY how much a warm switch saves (cold 12B → 26B = 303 s).

## 4. The demo (≤ 2 min on video)

1. **"Three laptops, one AI."** Dashboard: three nodes connected, SERVING, Gemma 12B. Ask in the chat; the answer
   ends with `computed on 3 machines: m3-air-113 + m5-air-174 + p5-air-204` and its tok/s (~11).
2. **"A model none of them can hold."** Switch to the 26B: v2's model select, or (works with any dashboard, v1
   included, since it reads `cluster_status` live):
   `curl -X POST http://192.168.8.113:3000/v1/database/pool/call/request_model -H 'Content-Type: application/json' -d '["gemma-4-26B-A4B-it-Q8_0"]'`
   (back: `'["gemma-4-12b-it-Q4_K_M"]'`). Status: LAUNCHING → SERVING; cold ~5 min (film the start and the SERVING
   moment, cut the middle). Ask again (~12.5 tok/s). Say the honest ratio: 25.0 GiB model, biggest Mac holds 17.8 GiB (1.4x), and the
   internet is off (router WAN unplugged, D-02). The load is minutes cold: cut it in the video, or pre-warm.
3. **"A machine leaves, the pool heals; it comes back, the pool takes it in."** (P1 rehearsal, Sun 9:00am, 12B):
   Ctrl-C .174's agent → **the request in flight fails** (llama.cpp aborts on a lost RPC peer) → DEGRADED → the
   pool recovers onto the 2 remaining Macs and answers again in **~30 s** (#52). Start the agent again → the pool
   **keeps answering**, then re-plans to 3 in **~18 s** (#49) → SERVING with 3. Total ~54 s. **Don't claim answers
   continue during a leave.**
4. **Numbers** (pre-recorded): section 5.

## 5. Numbers we can quote (all `bench`, thinking off, warmup 1 + 3 repeats; files in `test-runs/bench/`)

| What | Result | Source |
|---|---|---|
| 12B, one M5 Air alone | ~16 tok/s cold, **~8-9 tok/s sustained** (fanless; throttles after ~1.5 min) | night-20261108-0408 |
| 12B, 3-Mac pool (FA + q8 KV) | **~11 tok/s, flat over 5.5 min** (TTFT 0.6-0.7 s short/long) | p4-20261108-0150-existing-fa-q8 (#54) |
| ngram speculative decoding, rewrite task, one Air | **1.6-1.7x** (edit: 7.2 → 12.4, 9 → 14.5 tok/s; 79% draft acceptance); no gain on new text | night-20261108-0329 / -0408 |
| **26B on the 3-Mac pool (B3)** | **12.5 tok/s (12.1-12.6), flat over 5 min; TTFT 0.6 s**; 9/9 ok; FA + q8 KV + `--no-mmap`; M3 5718 MiB, each M5 9941 MiB | test-runs/bench/p4-26b-final (#57) |

Honest framing: cold, one Air is faster on the 12B; sustained, three Airs beat one throttled Air (~11 vs 8-9), with
the caveat that the pool's main was the M3 and had FA + q8 KV on. The 26B is the point: no single Mac can hold it.

## 6. Stop and reset

1. Coordinator: **Enter**. 2. Each agent: Ctrl-C (stops its rpc-server first; `pgrep rpc-server` prints nothing).
3. Leave SpacetimeDB running between takes. For a fully cold run, stop it too.

## 7. When something fails

| Symptom | Cause | Fix |
|---|---|---|
| `E_RPC_UNREACHABLE` / errno 65 while `nc` works | Local Network permission (rule 1) | run from Terminal; toggle Terminal in Local Network, Cmd-Q, reopen |
| llama-server `Remote RPC server crashed`, `recv failed (bytes_recv=0)` at start | helper's rpc-server still held by a dead client (its Mac slept through the last crash) | coordinator ≥ #53 restarts it through the agent; by hand: Ctrl-C + restart that agent |
| pool down, helper `LOST`, `ping` says "Host is down" | helper asleep | wake it, `caffeinate -dims &`, restart its agent (rule 2) |
| chat answer blank | Gemma 4 thinks first; small max_tokens | dashboard sends `enable_thinking: false` (#47); keep it |
| a stale model loads after a coordinator restart | old `model_request` row | fixed in #51 review (requests before start are ignored); VERIFY on main |
| `E_PORT_IN_USE` on a helper | hand-started rpc-server (rule 4) | stop it; the agent starts its own |
| 26B launch times out | cold load > `ready_timeout_s` | `ready_timeout_s = 900`, pre-warm |
| 26B SERVING but no answer ever completes; kernel log `IOGPUCommandDescriptor::prepare … 0xE00002BE` | main mmaps the whole 25.6 GB file on a 16 GB Mac; Metal stalls | `--no-mmap` in main's `extra_args` (B3 fix, P4 + P1) |

**Fallback if the tooling fails on stage:** by hand, as in run-002 (`test-runs/`): `rpc-server -H 0.0.0.0 -p 50052
-d MTL0 -c` on each helper (Terminal), then on main `llama-server -m <gguf> --rpc 192.168.8.174:50052,192.168.8.204:50052
-ngl 99 -c 4096 --host 0.0.0.0 --port 8080`. Same answer, no SpacetimeDB. The dashboard's `?direct` mode shows it.
