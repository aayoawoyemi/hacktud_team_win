<!--
CONCEPT: Operator guide for an Oasis pool: what runs where, how to start it, how to switch models and add a machine,
         and exactly what the pool does when a machine fails. Every number here was measured on the team's three
         MacBook Airs on Sat 8 / Sun 9 Nov 2026 (sources: issue #11, test-runs/, PRs named below).
WHY: Preston (Sun 7:15am): production-ready. Operators and judges need one page that says how to run it and what
     breaks, instead of the scattered #11 thread.
DEPENDS ON: main at or after a4cb62f (#49 rejoin, #51 model switch, #52 recovery, #53/#55 stuck rpc-server).
AFFECTS: docs only.
-->
# Running the pool

Oasis pools ordinary laptops into one llama.cpp model that is too big for any one of them. No cloud: one private
router, a cable per laptop. This page is the operator's guide: setup, daily use, and what happens when something fails.

## Roles

| Machine | Runs | Holds the model file? |
|---|---|---|
| **main** (one) | SpacetimeDB (`pool` db, :3000), `coordinator`, `llama-server` (:8080), optionally an agent with `role = "main"` | yes: every GGUF you want to serve |
| **helper** (one or more) | `agent` (it starts and stops `rpc-server` :50052 on command) | no: layers arrive over the network and are cached on disk (`rpc-server -c`) |

All traffic is a star through main: each token goes main → helper 1 → main → helper 2 → main. Wired gigabit adds
about 0.4-1 ms per hop on our router (`netcheck`, run-005).

## Before you start (macOS)

- **Local Network privacy.** Start every process that dials other Macs (coordinator, llama-server) from **Terminal**
  or a named app that has Local Network permission, never from a sandboxed agent shell. A blocked process fails with
  errno 65 "No route to host" while `nc` still works (found Sat night on the M3).
- **Keep machines awake:** `caffeinate -dims &` on every Mac, plugged in. A helper that sleeps drops out of the pool
  (Sun 2:35am: .204 slept and took the pool down).
- **Copying big files between Macs:** `python3 -m http.server` ignores HTTP Range, so download with one plain
  `curl -fL -o file URL` and check size + sha256. Never resume (`curl -C -`) against it: it appends a second copy.

## Start order

1. **main:** `spacetime start -l 0.0.0.0:3000`, then `spacetime publish -s local -p crates/control pool -y`
   (never `--delete-data` on a live pool; adding tables is a safe migration).
2. **each helper:** `agent agent.toml` with `spacetime_uri = "http://<main>:3000"`, `rpc_bind = "0.0.0.0"`,
   `rpc_cache = true` (example: `crates/agent/agent.example.toml`). Do not start `rpc-server` by hand: the agent
   must own it, or the coordinator's start hits `E_PORT_IN_USE`.
3. **main:** `coordinator coordinator.toml` with `SPACETIME_TOKEN` set. First run prints an `add_admin` command for
   its identity: run it once. Key settings:

```toml
model_id = "gemma-4-12b-it-Q4_K_M"          # served at start
model_path = "/path/gemma-4-12b-it-Q4_K_M.gguf"
public_host = "192.168.8.113"                # main's LAN IP, shown as the endpoint
min_helpers = 2                              # wait for this many helpers before the FIRST load only
ready_timeout_s = 900                        # a cold 26B load streams ~19 GB to helpers
flash_attn = "on"
extra_args = ["-ctk", "q8_0", "-ctv", "q8_0", "--no-mmap"]

[models]                                     # what the dashboard may switch to
"gemma-4-26B-A4B-it-Q8_0" = "/path/gemma-4-26B-A4B-it-Q8_0.gguf"
"qwen2.5-0.5b-instruct-q4_k_m" = "/path/qwen2.5-0.5b-instruct-q4_k_m.gguf"
```

**`--no-mmap` is required when the model is bigger than main's RAM.** By default llama-server on main maps the
*whole* GGUF (25.6 GB for the 26B) even though main only computes its own share. On the 16 GB M3 that stalled the
GPU on page-ins and the 26B never answered. With `--no-mmap` main loaded only its 5.7 GB share and answered at once.

4. **dashboard** (any Mac on the router): serve the repo (`python3 -m http.server 8000`) and open
   `web/oasis-v2.html?stdb=http://<main>:3000&llama=http://<main>:8080`. It shows the machines, chat (each answer is
   labeled with the machines whose layers computed it), the event log and the model switch.

Stop the coordinator with **Enter** in its window (or close its stdin). That stops llama-server, then every helper's
rpc-server, and waits until the agents report them stopped.

## Everyday operations

| Action | How | Measured |
|---|---|---|
| Ask a question | dashboard chat, or any OpenAI client at `http://<main>:8080/v1` | 12B on 3 Macs ~11 tok/s; 26B on 3 Macs ~12 tok/s, first token 0.6-0.8 s (B3 short prompts, preliminary until the B3 table) |
| Switch model | dashboard model select (calls the `request_model` reducer). One switch per 30 s. A model that fails to load gives way to the previous one. | 12B → 26B cold ~5 min, warm ~2 min; 26B → 12B 21 s (`rpc-server -c` caches each model's layers on the helpers) |
| Add a machine | start its agent; the coordinator re-plans with it while the pool keeps serving until the new helper is ready | re-plan ~2-3 s on test hardware plus the model's load time |
| Remove a machine | stop its agent (Ctrl-C stops its rpc-server first) | the pool re-plans on the next request (see below) |

## When something fails

| What happens | What the pool does | Since |
|---|---|---|
| A helper's agent stops or loses the network | node goes LOST, cluster DEGRADED; the next request reaches the dead link, llama-server exits, and the coordinator **recovers by itself** onto the helpers still online (no `min_helpers` gate; zero helpers allowed) | #52 |
| That helper comes back | it rejoins as a normal join and the pool returns to SERVING with all machines | #49 |
| A helper's rpc-server crashes (agent alive) | same recovery; the agent restarts its rpc-server on command. Test: SERVING again in ~2 s | #52 |
| A helper slept through main's crash and its rpc-server is still held by the dead connection | at the next launch the coordinator sees it doesn't answer an RPC hello and restarts it through its agent | #53, #55 |
| The port is held by an rpc-server from a previous agent process | launch gets `E_PORT_IN_USE`, sends stop (the agent kills that leftover rpc-server, only if `ps` names it rpc-server) and start once | #53 |
| The coordinator restarts | a model request left in the db is ignored; the config's model loads | #51 |
| A model fails to load | the previous model is restored, reason in the event log (`launch_failed`) | #51 |

Every row has a test in `crates/coordinator/tests/full_cycle.rs` (steps 6b-6k), which runs real agents, rpc-servers
and llama-server against a scratch SpacetimeDB (`SPACETIME_DB=pool-smoke`; it refuses the live `pool`).

## Known limits

- **No capacity guard yet.** The planner's fit check (#20) is not wired, so a model too big for the machines left after
  a failure is attempted and fails in llama-server's load (logged; the pool stops). Example: if every helper is gone
  while the 26B serves, recovery tries main alone.
- **Killing the coordinator with `kill` (SIGTERM) leaves llama-server running** and holding :8080; a restart then
  reports `E_PORT_IN_USE`. Stop it with Enter instead.
- **Thermals:** a fanless M5 Air alone runs the 12B at ~16 tok/s cold and throttles to 8-9 after ~1.5 min; the pool
  held ~11 tok/s flat over 5.5 min (different main Mac and flags, so treat it as an observation, not a controlled test).
- `request_model` can be called by any device on the router (private network only); it can only pick ids from the
  coordinator's own `[models]`.
