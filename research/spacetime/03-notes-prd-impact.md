# 03: What entering the Spacetime track does to the notes-branch PRD

Worker: spacetime. Sat Nov 7 2026, ~6:15pm CT. Input: `git show origin/notes:PRD.md` (1,322 lines), read §3, §5, §6
and WP-2/3/4/7/9. Sections 01-02 of these notes targeted the old EXO + TypeScript plan, which is superseded. The
Spacetime facts from 01 still hold (private-IP block on procedures, 180 s cap, `clientDisconnected`, schedule tables,
standalone on `0.0.0.0:3000`).

## 0. Answer first

- **Recommendation: replace the gRPC control plane with Spacetime, decided by 7:00pm.** One Rust module on main holds
  the registry, heartbeats, commands, the event log, the cluster state machine, failure detection and status. The
  agent and coordinator become Rust Spacetime clients that supervise llama.cpp processes. WP-2 keeps the serde
  contract types and the llama.cpp output parsers, and drops `.proto`/tonic/protox.
- This is better for the product, and not only for the prize. Several pieces we were about to hand-write go away:
  - the status API (WP-7)
  - inbound control ports on every helper
  - the event log storage
  - reconnect and registry code
  Section 3 has the reasons.
- **Blocker found: PR #17 pins Rust 1.93.1. `spacetimedb` and `spacetimedb-sdk` 2.12.0 to 2.14.1 require Rust 1.97**
  (crates.io `rust_version`). There are two fixes. (a) Bump `rust-toolchain.toml` to `1.97` or newer and add
  `targets = ["wasm32-unknown-unknown"]`. rustup then installs it on every machine at the first `cargo` call, about
  2-3 min. (b) Pin Spacetime 2.10.1 everywhere: it is the last version with `rust_version 1.93.0`, released Sep 15
  2026, and needs `spacetime version install 2.10.1` plus crates `=2.10.1`. Prefer (a).
- If the team says no by 7:00pm, keep gRPC and do not bolt Spacetime on later. A half-Spacetime backend fails the
  "majority" rule and costs more than either option alone.

## 1. Facts: Rust module, Rust client, platforms, offline

| Item | Fact | Source |
|---|---|---|
| Rust module crate | `spacetimedb` 2.14.1 (Nov 4 2026), compiled to `wasm32-unknown-unknown`, published with `spacetime publish` | crates.io API; `docs/.../00500-rust.md` |
| Rust client crate | `spacetimedb-sdk` 2.14.1 (Nov 4 2026), ~45k recent downloads | crates.io API |
| MSRV | **1.97** for 2.12.0+; 1.96 for 2.10.2-2.11.0; 1.93.0 for 2.0.0-2.10.1 | crates.io `/versions` |
| Native deps of the client | `tokio`, `tokio-tungstenite`, `native-tls =0.2.14` (SChannel on Windows, Security.framework on macOS), brotli, flate2. No C++ and no protoc. | `sdks/rust/Cargo.toml` |
| Windows / macOS client | Nothing in `sdks/rust/src` is gated by OS; only `wasm32` is cfg-gated. It should build on MSVC and on macOS. **Unverified on our machines**: build it on the Dell first (§6 step 1). | `rg cfg\(target_os` in `sdks/rust/src` |
| Client threading | `conn.run_threaded()` (own thread), `run_async().await` (tokio), or `frame_tick()` (manual) | `sdks/rust/src/db_connection.rs` l.656-696 |
| Codegen | `spacetime generate --lang rust --out-dir crates/agent/src/module_bindings --module-path crates/stdb-module` → commit the output. Helpers then need only `cargo`, not the `spacetime` CLI. | CLI reference l.442 |
| Rust module unstable bits | Procedures and HTTP handlers need `features=["unstable"]`. **The plan needs neither**: tables, reducers, lifecycle hooks and schedule tables are stable. | `00400-procedures.md` l.68-72 |
| Offline | `spacetime start` on main, no internet. Agents connect with no token (`with_token(None)`). The server mints an identity and the client saves the token to a file. Main publishes with a server-issued login. Never run `spacetime login` on main after the first publish (that loses owner rights; troubleshooting doc). | check-in 01 |
| Ports | Only main listens (TCP 3000). Agents dial out. Helpers need inbound only for rpc-server (50052), not for the control plane. | `standalone/src/subcommands/start.rs` l.33-36 |
| Disconnect detection | `clientDisconnected` fires on WS close or reset. For a silent drop (Wi-Fi gone) the server gives up after the WS idle timeout: ping 15 s, idle 30 s by default, set in `config.toml [websocket]`. A heartbeat-age sweep is still needed for a 6 s timeout. | `crates/standalone/config.toml` |
| Status with no frontend (D-06) | `spacetime subscribe "SELECT * FROM node"` and `spacetime sql` print live rows in a terminal, so O-10 is solved with zero code. Both are marked UNSTABLE in the CLI reference. | CLI reference l.369, l.726 |

## 2. Mapping: §6 contracts and the WPs onto Spacetime

Rule: the scalar fields that people query or that drive logic become columns. Nested blobs stay as a `String` column
holding the existing serde JSON (`caps_json`, `data_json`), so the WP-2 types remain the single source of truth and
the module needs no nested `SpacetimeType` for every struct.

### Tables (module `crates/stdb-module`, all `public` except `admin`)

| Table | Key columns | Replaces | Written by |
|---|---|---|---|
| `node` | `node_id` PK, `identity` (unique), `role`, `state` (§5.6 node states), `hostname`, `os`, `llama_build`, `rpc_protocol`, `usable_bytes`, `rpc_addr`, `rpc_state`, `last_heartbeat_at`, `seq`, `caps_json` (6.1) | 6.1 registration, the node registry, the node half of 6.7 | `register_node`, `heartbeat`, the sweep |
| `heartbeat_latest` (or fold into `node`) | `node_id` PK, `cpu_util_pct`, `relay_bytes_in/out`, `power_json`, `last_error_code` | 6.2 Heartbeat | `heartbeat` reducer (upsert, no history) |
| `command` | `command_id` PK autoInc, `node_id` (btree index), `kind`, `args_json`, `status` (`pending`/`running`/`ok`/`error`), `result_json`, `error_code`, `created_at`, `finished_at` | 6.3 Command + CommandResult | coordinator: `send_command`; agent: `ack_command`, `complete_command` |
| `model` | `model_id` PK, `file_bytes`, `n_layers`, `info_json` | 6.4 ModelInfo | coordinator after parsing the GGUF on main |
| `plan` | `plan_id` PK, `model_id`, `strategy`, `status`, `plan_json` (6.5), `created_at` | 6.5 SplitPlan | `create_plan` reducer (planner runs **inside** the module, see below) |
| `cluster` | 1 row: `state` (§5.6 cluster states), `plan_id`, `endpoint`, `llama_server_pid`, `ready`, `launch_spec_json` (6.6), `last_request_json` | 6.6 + top level of 6.7 | coordinator reducers `set_cluster_state`, `llama_ready` |
| `event` | `seq` PK autoInc, `at`, `kind`, `node_id`, `plan_id`, `message`, `data_json` | 6.10 Event log | every reducer that changes state appends here, inside the same transaction |
| `bench_result` | `id` PK autoInc, `run_id` (index), `scenario`, the numbers as columns, `raw_json` | 6.8 | WP-8 runner (`record_bench`) |
| `netcheck_result` | `id` PK, `from_node`, `to_node`, `rtt_p50_ms`, `raw_json` | 6.9 | agent, as the result of a `netcheck` command |
| `sweep_schedule` | `scheduled_id`, `scheduled_at` = `ScheduleAt::Interval(500ms)` | the timer in WP-9 | `init` |
| `admin` (private) | `identity` PK | auth | `init` (owner) |

6.11 error codes: keep them as the Rust enum in contracts and store them as `error_code: String` columns.
6.12/6.13 config: keep TOML/.env per O-24. Only add `spacetime_url` and `spacetime_db` keys.

### Reducers (about 15)

`register_node(caps_json, …)` · `heartbeat(seq, …)` · `send_command(node_id, kind, args_json)` (admin) ·
`ack_command(id)` · `complete_command(id, ok, result_json, error_code)` · `upsert_model(...)` (admin) ·
`create_plan(model_id, strategy)` · `set_cluster_state(state, …)` (admin) · `llama_ready(pid, endpoint, actual_layers_json)` ·
`report_event(kind, …)` · `record_bench(...)` · `record_netcheck(...)` · `stop_cluster()` ·
`sweep()` (scheduled) · `init` · `client_connected` · `client_disconnected`.

Each reducer checks the legal §5.6 transitions and returns `Err(String)` on an illegal one. The transition table
therefore lives in the database.

### Per WP

| WP | With gRPC (current) | With Spacetime | Size change |
|---|---|---|---|
| **WP-2 contracts** | serde types + `.proto` + tonic/protox codegen + parsers | serde types + parsers (unchanged). Add `crates/stdb-module` (tables and reducers) and committed `module_bindings`. Drop `.proto`, tonic and protox. | smaller transport part; the module is new work (~2.5-3 h, §5) |
| **WP-3 agent** | gRPC server (inbound port) or client streams; reconnect logic; register; heartbeat timer; command handler; rpc-server supervisor; probe | Spacetime client: `register_node` on connect, a 1 s `heartbeat` loop, `on_insert` on `command WHERE node_id = me` → run → `complete_command`. **Unchanged**: rpc-server supervision, banner parsing, probe, speed probe, netcheck. | transport code drops from ~250 to ~100 lines (estimate); supervision unchanged |
| **WP-4 coordinator** | gRPC server; registry in memory; state machine in memory; command fan-out; launch llama-server; poll `/health`; parse the load log | Spacetime client on main: subscribe to `node`/`cluster`/`command`. On READY nodes it calls `create_plan`, then `send_command(start_rpc_server)` per helper, waits for `command.status = ok` rows, builds the LaunchSpec and **launches llama-server as a child process (stays outside)**, polls `/health`, then calls `llama_ready`. Registry and state live in tables. | smaller: no registry or state persistence code; launcher unchanged |
| **WP-5 planner** | Rust crate called by the coordinator | Same crate. If it is pure Rust with no I/O, it compiles to wasm and `create_plan` runs it **inside the module**, so the plan is computed in the DB. GGUF parsing (file I/O) stays on main and uploads `model` rows. | none; better "majority" story |
| **WP-7 status API** | HTTP/JSON server producing 6.7 | **Mostly removed.** Status = subscriptions. The demo shows `spacetime subscribe "SELECT node_id,state,rpc_state,last_heartbeat_at FROM node"` in a terminal. If a 6.7 JSON blob is still wanted (for WP-8 or the write-up), a ~60-line CLI `status` subcommand subscribes and prints it. | ~3 h → ~0.5 h |
| **WP-9 failure detection** | coordinator timer + heartbeat ages + events | `sweep` scheduled reducer every 500 ms marks `LOST` when `now - last_heartbeat_at > node_timeout_ms` and appends `node_lost`. `client_disconnected` marks `LOST` at once on a clean close. A returning node calls `register_node` → `REGISTERED` + `node_recovered`. **Unchanged**: the recovery *action* (coordinator re-plans and restarts llama-server; agent kills orphans, R9.6). | detection becomes ~40 lines in the module; recovery unchanged |
| WP-8 bench | writes JSON lines | also calls `record_bench` (optional; JSON lines stay the primary output) | +20 lines |
| WP-10 netcheck | standalone tool + optional command | unchanged; results also go to `netcheck_result` | +20 lines |

What stays outside Spacetime no matter what:
- The data plane (llama.cpp RPC TCP, llama-server).
- Spawning and supervising rpc-server and llama-server.
- Reading the llama.cpp logs and banners.
- Hardware probes and the GGUF parse.
- The bench client.

Procedures can't reach LAN or localhost (check-in 01), and modules have no process or file I/O, so this split is
forced anyway.

## 3. Better for the product, or only for the prize?

Better, on these points:
1. **The registry, the event log and the state survive a coordinator crash.** Today, if the coordinator restarts, it
   loses the registry and the plan. With Spacetime, the coordinator reconnects and re-reads the `cluster` and `node`
   rows (the commitlog is on disk).
2. **There is no inbound control port on helpers.** Agents dial out to main:3000, which fits campus/Windows firewall
   reality (§4.2). With gRPC, either agents serve (inbound port on the Dell) or we write bidirectional streaming.
3. **WP-7 mostly disappears.** Every consumer gets live status by subscribing, and anything can read it: a terminal,
   a later dashboard (roadmap), or the WP-8 runner.
4. **The event log is free and consistent.** Events are written in the same transaction as the state change, so the
   log can never disagree with the state.
5. **The state-machine rules sit in one place** (reducers) instead of being split across coordinator code paths.

Worse or riskier:
1. **Toolchain bump** (1.93.1 → 1.97+) and a wasm target. This costs 5 min, but it touches PR #17.
2. **A new process on main** (`spacetime start`) that must be running before anything else, and one more thing to
   install (`curl -sSf https://install.spacetimedb.com | sh`). Main is already a single point of failure (llama-server
   and the coordinator run there), so this adds no new failure domain.
3. **Learning curve at 6pm.** Nobody on the team has shipped a Spacetime module (assumption). The pattern needed is
   the simplest one (tables + reducers + one schedule table). Budget 1 h of ramp for the module owner.
4. **Schema changes need a republish and regenerated bindings** that every crate must pull (`git sync`). One owner
   for `crates/stdb-module` and the committed bindings avoids the churn.
5. **Command latency**: an insert row → push to the subscribed agent. On a LAN this should be in the low milliseconds
   (unmeasured). Commands are start/stop/probe, which take seconds anyway, so this is irrelevant for the demo.
6. **Windows build of `spacetimedb-sdk` is unverified.** It is pure Rust plus native-tls, so the risk is low, but
   test it first.
7. **Silent drops** still need the heartbeat sweep, because `client_disconnected` alone waits up to the 30 s WS idle
   timeout.

Net: for a 3-machine control plane whose main cost is state, registration, status and events, Spacetime removes more
code than it adds. It does nothing for the data plane, and it should not try to.

## 4. Does "majority of backend on Spacetime" hold honestly?

Yes, if the claim is framed as the **control plane**, and the module is made fat on purpose:
- llama.cpp is a third-party inference engine (D-05 says it is used as shipped). It is the compute, not our backend,
  in the same way that a game backend on Spacetime doesn't count the GPU driver.
- Our backend = registry, heartbeats, commands, the plan, cluster state, the event log, failure detection, status,
  and bench/netcheck records. All of it lives in Spacetime tables and reducers, including the planner (if it compiles
  to wasm) and the transition rules.
- Outside it: two thin Rust clients whose job is OS process supervision and log parsing. That is like a game client.
- Weak spot: by line count, agent + coordinator + parsers could exceed the module (estimate: module 500-700 lines;
  agent + coordinator supervision 600-900). Mitigations:
  - put the planner and all transition rules in the module;
  - keep the clients free of business logic;
  - add a README table ("responsibility → where it lives"), as in 02 §7.
- A judge who counts llama.cpp's C++ as backend would say no. Nobody can fix that, and every Spacetime-track AI
  project calls an external model.

## 5. WP-2 right now: what to do, with hours

**Recommendation: thin hybrid, decided by 7:00pm.** Keep `crates/contracts` serde types + parsers as is. Stop writing
`.proto`, tonic and protox now. Spacetime is the transport and the state. Hours (one person each, estimates):

| Task | Hours | Owner |
|---|---|---|
| Bump `rust-toolchain.toml` to ≥1.97 + wasm32 target; remove tonic/prost/protox from workspace deps | 0.25 | WP-12 (PR #17) |
| Install `spacetime` on main; `spacetime start`; publish a hello module; connect a Rust client from the **Dell** and from a Mac | 0.75 | module owner |
| `crates/stdb-module`: 10 tables + ~15 reducers + sweep + transition checks | 2.5-3 | module owner (former WP-2 transport person) |
| Commit `module_bindings`; a small `stdb` helper in contracts (connect, token file, `run_threaded`) | 0.5 | module owner |
| WP-3 transport swap (register, heartbeat, command subscription) | 1-1.5 | WP-3 |
| WP-4 transport swap (subscribe, `send_command`, wait on results) | 1-1.5 | WP-4 |
| WP-7 reduced to a `status` subcommand + the demo `spacetime subscribe` line | 0.5 | WP-7 |
| WP-9 detection in the module (sweep) | included above | module owner |
| **Total extra vs the gRPC plan** | about +2 h for the module, roughly −2 to −3 h saved across WP-2 transport, WP-7 and registry code. Net about zero, with the risk moved to the first 45 min (the hello-module test). |

Decision deadline: **7:00pm**, because WP-3 and WP-4 start writing transport code on tonic after WP-2 lands.
Checkpoint B (9:30pm) needs the control plane working. Go/no-go test before committing: by 7:45pm a Rust client on
the Dell calls a reducer on main and receives an `on_insert`. If that fails, fall back to gRPC. At that point only
~1 h is lost, because contracts and parsers are shared by both plans.

If the team does NOT enter the Spacetime track: keep gRPC as planned. None of this is needed for the NVIDIA/8090
tracks.

## 6. First steps if it's a yes (in order)

1. On the Dell: create a fresh crate (outside the workspace, Rust ≥1.97) with `spacetimedb-sdk = "=2.14.1"` and run
   `cargo build`, about 5-10 min cold (estimate). This verifies the Windows build before anything else.
2. On main: install the CLI, `spacetime start`, `spacetime init --lang rust stdb-module`, add the `node` table and
   `register_node`/`heartbeat` reducers, `spacetime publish -s local pool`, `spacetime generate --lang rust ...`.
3. Dell client connects to `ws://<main-ip>:3000`, calls `register_node`, and sees its row via
   `spacetime sql pool "SELECT * FROM node"`. This is the go/no-go.
4. Then add the rest of the module tables and hand the bindings to WP-3/WP-4.
5. Check the toolchain bump, the published module and the bindings into the repo (by the module owner; the
   orchestrator ships).

## 7. Open items / unverified

- Rust SDK automatic reconnect: the TS SDK has `withAutomaticReconnect()`, but I did not confirm a Rust equivalent.
  The agent may need its own reconnect loop (~30 lines).
- The exact Rust module syntax for schedule tables in 2.14 (`#[table(scheduled(sweep))]` + `ScheduleAt::Interval`),
  per the docs example at `00500-schedule-tables.md` l.138. Check it in step 2.
- Whether the WP-5 planner's dependencies compile to `wasm32-unknown-unknown`. If not, run the planner on the
  coordinator and only store the plan.
- Push latency on the venue LAN: measure it in step 3 (insert → `on_insert` round trip).
