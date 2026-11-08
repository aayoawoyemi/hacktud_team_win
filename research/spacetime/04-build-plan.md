# 04: Build plan: go/no-go, module skeleton, clients, PR #18 triage, version pin

Worker: spacetime. Sat Nov 7 2026, ~6:30pm CT. Machine: this Windows box, Rust 1.93.1 MSVC. Nothing was installed:
no new toolchain, no target, no spacetime CLI.

## 1. Go/no-go build test: GO

| Test | Command | Result |
|---|---|---|
| Client SDK on Windows | `research/spacetime/sdk-test/`: `spacetimedb-sdk = "=2.10.1"`, `cargo +1.93.1 build` | **Builds. 48 s cold**, 0 errors (tokio-tungstenite 0.27, native-tls, url, idna...). |
| Module crate (host check) | `research/spacetime/module-test/`: `spacetimedb = "=2.10.1"`, full skeleton below, `cargo +1.93.1 check` | **Compiles with 0 errors and 0 warnings** after replacing the deprecated `ctx.identity()` with `ctx.database_identity()`. This checks the macros, types and reducer signatures natively. |
| wasm32 target | `rustup target list --installed` | Only `x86_64-pc-windows-msvc`. **wasm32-unknown-unknown is NOT installed here.** |

Does the module need wasm32? **Yes, on the one machine that publishes.** `spacetime publish`/`build` runs
`cargo build --target=wasm32-unknown-unknown` (`crates/cli/src/tasks/rust.rs` l.12-19). If the target is missing,
the CLI runs `rustup target add wasm32-unknown-unknown` itself (l.33-39). So publishing needs two installs on
**main only**:
- the `spacetime` CLI, which is also the server, so main needs it anyway to run `spacetime start`;
- the wasm32 target, a ~30 MB rustup component.

Helpers (the Dell and the other Macs) need neither: they build the agent with the plain host toolchain plus the
committed `module_bindings/`. The "nobody installs anything" rule therefore needs one exception, on main: the
Spacetime CLI and the wasm32 target. Say so to the team.

Not tested here (not allowed to install): the actual wasm build, `spacetime start` and publish, and a live connection.
Do these on main first (§6).

## 2. Module: `crates/stdb-module/src/lib.rs`

The full compiled skeleton is at **`research/spacetime/module-test/src/lib.rs`** (≈470 lines). Copy it as is.
`Cargo.toml`:

```toml
[package]
name = "stdb-module"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[lib]
crate-type = ["cdylib", "rlib"]   # cdylib = the wasm that gets published; rlib lets `cargo check`/tests run natively

[dependencies]
spacetimedb = "=2.10.1"
log = "0.4"
```

Contents:

| Kind | Items |
|---|---|
| Tables (public) | `node` (§6.1 + latest §6.2; PK `node_id`, unique `identity`), `command` (§6.3 + result; btree on `node_id`), `model` (§6.4), `plan` (§6.5), `cluster` (1 row: §5.6 state + §6.6 LaunchSpec JSON + §6.7 top-level), `event` (§6.10, autoInc `seq`), `bench_result` (§6.8) |
| Tables (private) | `admin` (publisher identity), `sweep_schedule` (`scheduled(sweep)`, interval 500 ms) |
| Lifecycle | `init` (admin row, cluster row `EMPTY`, sweep schedule), `client_disconnected` (node → LOST + `node_lost` + cluster SERVING → DEGRADED) |
| Agent reducers | `register_node(node_id, role, hostname, os_family, llama_build, rpc_protocol, usable_bytes, rpc_addr, caps_json)`, `heartbeat(seq, rpc_state, cpu_util_pct, last_error_code, heartbeat_json)`, `ack_command(id)`, `complete_command(id, ok, result_json, error_code)` |
| Coordinator reducers (admin) | `send_command(node_id, kind, args_json)`, `upsert_model(...)`, `create_plan(plan_id, model_id, strategy, fits, plan_json)`, `set_cluster_state(state)`, `llama_ready(pid, endpoint, launch_spec_json, active_node_ids)`, `record_bench(...)` |
| Either | `report_event(kind, node_id, message, data_json)` (for placement_mismatch, request_failed, llama_server_exited, netcheck_result) |
| Scheduled | `sweep`: nodes with `now - last_heartbeat_at > 6000 ms` → LOST + `node_lost` + DEGRADED. Guarded by `ctx.sender() == ctx.database_identity()`. |
| Transition checks | `cluster_transition_ok(from,to)` and `node_transition_ok(from,to)` encode §5.6. Illegal moves return `Err("E_INTERNAL: illegal cluster transition …")`, so the transaction rolls back and the caller sees the error. |
| Extras | `version_mismatch` event when the llama.cpp builds of non-LOST nodes differ; `node_id` bound to the first identity that registers it. |

Design notes:
- Nested §6 objects are stored as the WP-2 serde JSON in `*_json: String` columns. The module therefore does **not**
  depend on the contracts crate, and prost never has to compile to wasm. Scalars used in logic or queries are columns.
- Durations: `ctx.timestamp.time_duration_since(t)` → `TimeDuration::to_micros()`.
- Timeouts are constants (`NODE_TIMEOUT_MS = 6000`, `SWEEP_EVERY_MS = 500`, O-26). Move them to a config table only
  if needed.
- `create_plan` takes the plan computed by WP-5 on the coordinator. If the planner crate is pure Rust, call it inside
  the reducer instead. Check this with `cargo check --target wasm32-unknown-unknown -p planner` on main.

## 3. Client snippets (spacetimedb-sdk 2.10.1; API names checked in the crate source)

Bindings: on main, after each schema change:

```
spacetime generate --lang rust --out-dir crates/stdb-bindings/src/module_bindings --module-path crates/stdb-module
```

Commit the generated code. Put it in a small crate `crates/stdb-bindings` (lib.rs: `pub mod module_bindings; pub use
module_bindings::*;`, deps: `spacetimedb-sdk = "=2.10.1"`) so the agent and coordinator share it. The exact generated
names (`DbConnection`, `RemoteTables`, `RemoteReducers`, `register_node`, ...) come from codegen. The code below
follows the 2.x Rust SDK docs and must be adjusted to the generated output.

### Agent (WP-3)

```rust
use stdb_bindings::*;                      // DbConnection, tables, reducers (generated)
use spacetimedb_sdk::{credentials, DbContext, Table};

fn connect(cfg: &Config) -> DbConnection {
    let creds = credentials::File::new("pool-agent");          // token file in ~/.spacetime_client_credentials
    let token = creds.load().ok().flatten();
    DbConnection::builder()
        .with_uri(cfg.spacetime_url.as_str())                 // "http://192.168.8.10:3000"
        .with_database_name(cfg.spacetime_db.as_str())       // "pool"
        .with_token(token)
        .on_connect(move |conn, _identity, token| {
            let _ = credentials::File::new("pool-agent").save(token);
            let caps = probe();                                // contracts::NodeCapabilities
            conn.reducers.register_node(
                caps.node_id.clone(), role(), caps.hostname.clone(), os_family(),
                caps.llama_cpp.build, caps.llama_cpp.rpc_protocol.clone(),
                usable_bytes(&caps), rpc_addr(), serde_json::to_string(&caps).unwrap(),
            ).ok();
            conn.subscription_builder()
                .on_error(|_ctx, e| log::error!("subscription: {e}"))
                .subscribe([format!("SELECT * FROM command WHERE node_id = '{}'", caps.node_id)]);
        })
        .on_disconnect(|_ctx, err| log::warn!("disconnected: {err:?}"))   // outer loop reconnects
        .build()
        .expect("connect to spacetime")
}

fn run(cfg: Config) {
    loop {                                                    // 2.10.1 Rust SDK has NO auto-reconnect
        let conn = connect(&cfg);
        conn.db.command().on_insert(|ctx, cmd| {
            if cmd.status != "pending" { return; }
            ctx.reducers.ack_command(cmd.command_id).ok();
            let (ok, data, code) = execute(&cmd.kind, &cmd.args_json); // start/stop rpc-server, probe, netcheck
            ctx.reducers.complete_command(cmd.command_id, ok, data, code).ok();
        });
        let handle = conn.run_threaded();
        let mut seq = 0u64;
        while conn.is_active() {
            seq += 1;
            let hb = heartbeat_snapshot(seq);                  // contracts::Heartbeat
            conn.reducers.heartbeat(seq, hb.rpc_state(), hb.cpu_util_pct(), hb.last_error_code(),
                                    serde_json::to_string(&hb).unwrap()).ok();
            std::thread::sleep(std::time::Duration::from_millis(1000));   // heartbeat_interval_ms
        }
        let _ = handle.join();
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
```

Run `execute` off the callback thread, for example with `std::thread::spawn`. Starting rpc-server can take seconds,
and the callback would otherwise block every other message.

### Coordinator (WP-4), on main, with the **publisher's token** (admin)

```rust
// token: `spacetime login show --token` on main, saved to .env as SPACETIME_TOKEN (never committed)
let conn = DbConnection::builder()
    .with_uri("http://127.0.0.1:3000").with_database_name("pool")
    .with_token(Some(std::env::var("SPACETIME_TOKEN")?))
    .on_connect(|conn, _, _| {
        conn.subscription_builder().subscribe([
            "SELECT * FROM node", "SELECT * FROM cluster", "SELECT * FROM command", "SELECT * FROM plan",
        ]);
    })
    .build()?;
conn.run_threaded();

// startup sequence (§5.5), driven by the client cache:
let ready: Vec<Node> = conn.db.node().iter().filter(|n| n.state == "READY").collect();
let plan = planner::plan(&model_info, &ready_caps(&ready));         // WP-5
conn.reducers.create_plan(plan.plan_id.clone(), model_id, strategy, plan.fits, serde_json::to_string(&plan)?)?;
conn.reducers.set_cluster_state("STARTING_HELPERS".into())?;
for h in plan.helpers() { conn.reducers.send_command(h.node_id.clone(), "start_rpc_server".into(), args_json(h))?; }
wait_until(|| commands_for(&conn, &plan).all(|c| c.status == "ok"), timeout);   // poll the cache, or use on_update
conn.reducers.set_cluster_state("LAUNCHING".into())?;
let child = launch_llama_server(&launch_spec)?;                   // stays outside Spacetime
wait_health(&launch_spec, ready_timeout)?;
conn.reducers.llama_ready(child.id(), endpoint, serde_json::to_string(&launch_spec)?, plan.node_ids())?;
// on cluster.state == DEGRADED (on_update): re-plan + relaunch per O-11, then llama_ready again (logs recovery_succeeded)
```

Reducer calls are fire-and-forget: the call returns once the request is sent. To see a failure (an illegal
transition), use the generated `on_<reducer>` callbacks or watch the resulting rows.

### Status (WP-7) with zero code

```
spacetime subscribe pool "SELECT node_id, role, state, rpc_state, cpu_util_pct FROM node" "SELECT * FROM cluster"
spacetime sql pool "SELECT * FROM event"
```

The `spacetime subscribe` and `spacetime sql` commands are marked UNSTABLE. If an §6.7 JSON blob is needed, a ~60-line
`status` subcommand can subscribe and print it.

## 4. PR #18 triage (precise list for the PR author)

Fact: in PR #18 the 47 Rust types are **generated from `proto/contracts.proto`** by protox + tonic-prost-build, with
the serde derives added in `build.rs`. Dropping the `.proto` would delete the types. So keep the `.proto` as the
schema for the types (as an IDL, not for RPC) and drop only gRPC.

KEEP:
- `crates/contracts/proto/contracts.proto`: **all `message` and `enum` definitions**.
- `crates/contracts/build.rs`: protox compile, `NULLABLE_COLLECTIONS`, `ONEOFS`, the serde attributes, `btree_map`.
- `crates/contracts/src/json.rs` (null_as_empty, enum casing helpers).
- `crates/contracts/src/parsers.rs` + `tests/parsers.rs` + `tests/llama_samples/*` (Dell build 8665 samples).
- `fixtures/json/*.json` + `tests/fixtures.rs`. These are the JSON that goes into the `*_json` columns, so the fixture
  tests now guard the Spacetime payloads.
- Workspace deps: `prost`, `prost-types`, `protox`, `serde`, `serde_json`, `thiserror`.
- `CONTRACTS.md`, with a transport section rewritten (see DROP).

DROP or CHANGE:
- `.proto`: delete `service AgentService` and `service CoordinatorService` (around diff l.2362-2376).
- Messages used only by gRPC: `RegisterRequest`, `RegisterResponse`, `HeartbeatStreamSummary` and their fixtures
  (`register_request.json`, `register_response.json`, `heartbeat_stream_summary.json`). Registration is now the
  `register_node` reducer, and heartbeats are the `heartbeat` reducer.
- `build.rs`: `.build_client(true).build_server(true)` → `false`/`false`. Better: replace `tonic_prost_build::configure()`
  with `prost_build::Config::new()` (same `btree_map`/`message_attribute`/`field_attribute`, then
  `.compile_fds(fds)`). This drops the tonic codegen dependency.
- `src/lib.rs`: `tonic::include_proto!("contracts.v1")` → `include!(concat!(env!("OUT_DIR"), "/contracts.v1.rs"));`
- `Cargo.toml` (contracts + workspace): remove `tonic`, `tonic-prost`, `tonic-prost-build` (add `prost-build` if
  switching); remove `tokio` from dev-deps if only the gRPC test used it.
- Delete `tests/grpc_loopback.rs`.
- CONTRACTS.md transport section: "Transport = Spacetime module `pool` on main (`ws://<main>:3000`). Reducers and
  tables: see `crates/stdb-module`. §6 payloads travel as serde JSON in `*_json` columns."
- ADD: `crates/stdb-module/` (§2), `crates/stdb-bindings/` (§3), and the config keys `spacetime_url`, `spacetime_db`,
  `SPACETIME_TOKEN` (coordinator only).

The edit takes about 30-45 min for the PR author. The types, parsers and fixtures (most of the value in PR #18) are
untouched.

## 5. Version pin: **Spacetime 2.10.1 on Rust 1.93.1 (recommended)**

| | 2.10.1 on 1.93.1 | 2.14.1 on ≥1.97 |
|---|---|---|
| Builds on the pinned toolchain | **Yes, verified here** (SDK build 48 s, module check) | Needs a toolchain bump in PR #17 and a ~200 MB download on all 4 machines, against the "nobody installs" rule |
| PR #17 change | none | `channel = "1.97"`; re-test everything already built |
| Server/CLI | `spacetime version install 2.10.1` then `spacetime version use 2.10.1` on main (unverified syntax; or the installer's version flag) | latest install |
| What we lose | ~7 weeks of releases (2.11 to 2.14: Oct 7 to Nov 4). I did not check their notes for a fix we need. | none |
| Risk | Docs on the site are for the latest. A few APIs may differ (seen: `name=` → `accessor=` happened in 2.0, so it is already right in 2.10.1). Check against crate source, not docs. | toolchain churn at 6:30pm |

Pin **everything** to 2.10.1: the `spacetimedb` and `spacetimedb-sdk` crates with `=2.10.1`, and the CLI/server on
main. Codegen output must match the SDK version.

## 6. Order of work (first 90 min)

1. (main, 10 min) Install the Spacetime CLI at 2.10.1 and run `spacetime start` (listens on `0.0.0.0:3000`). Allow
   it through the macOS firewall.
2. (main, 10 min) Copy `research/spacetime/module-test/` → `crates/stdb-module/`, then
   `spacetime publish -s local pool -p crates/stdb-module`. This auto-adds the wasm32 target. Then run
   `spacetime sql pool "SELECT * FROM cluster"`, which should show 1 row in state `EMPTY`.
3. (main, 5 min) `spacetime generate --lang rust ...` into `crates/stdb-bindings`, and commit.
4. (Dell, 15 min) The minimal agent calls `register_node` + `heartbeat`. On main, `spacetime subscribe pool "SELECT * FROM node"`
   shows the Dell's row, and its `last_heartbeat_at` ticks. **This is the go/no-go for the whole design.**
5. Kill the agent on the Dell. Within 6 s, `node.state = LOST` and an `event` row `node_lost` appear (WP-9 detection
   is done).
6. In parallel: the PR #18 author applies §4; WP-3 adds command handling; WP-4 adds the startup sequence.

## 7. Known gaps / unverified

- **Rust SDK 2.10.1 has no automatic reconnect** (grep finds only a comment, `subscription.rs` l.52). The agent's
  outer loop in §3 handles it.
- Generated binding names and reducer-call signatures: confirm after the first `spacetime generate`.
- The wasm build and `spacetime publish` are untested here (no target, no CLI on this machine by rule).
- `sweep` runs every 500 ms forever. The cost is negligible (3 rows scanned), but it writes nothing unless a node goes
  stale.
- Is the `ctx.sender() == ctx.database_identity()` scheduler guard right for scheduled reducers in 2.10.1? If `sweep`
  errors with "scheduler-only", delete the guard. The doc pattern for scheduled reducers uses this check.
