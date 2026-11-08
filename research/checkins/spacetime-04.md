## Check-in spacetime 04: Build plan (go/no-go, module skeleton, clients, PR #18 triage, version pin)

**Found (most important first, max 8 bullets, each with a source):**
- **GO on this Windows machine with Rust 1.93.1 MSVC and nothing installed.** `spacetimedb-sdk =2.10.1` builds cold in 48 s with 0 errors (`research/spacetime/sdk-test/`).
- **The full module skeleton compiles.** `spacetimedb =2.10.1` with 9 tables, 13 reducers, `init`, `client_disconnected`, a scheduled `sweep` and the §5.6 transition checks passes `cargo check` with 0 errors and 0 warnings (`research/spacetime/module-test/src/lib.rs`, 475 lines). Copy it to `crates/stdb-module/`.
- **wasm32-unknown-unknown is NOT installed here** (`rustup target list --installed` shows only x86_64-pc-windows-msvc). Only the publishing machine (main) needs it. `spacetime publish` builds with `--target=wasm32-unknown-unknown` and runs `rustup target add` itself if the target is missing (`crates/cli/src/tasks/rust.rs` l.12-39). Helpers need only their host toolchain and the committed bindings.
- **"Nobody installs anything" needs one exception, on main:** the Spacetime CLI, which is also the server, and the wasm32 target.
- **Version: pin Spacetime 2.10.1 on Rust 1.93.1.** It is verified to build, needs no change to PR #17, and needs no toolchain download on 4 machines. Pin the crates with `=2.10.1` and the CLI/server on main to 2.10.1 too, because codegen must match the SDK. The cost is ~7 weeks of releases (2.11 to 2.14), whose notes I have not checked.
- **PR #18: keep all `message`/`enum` definitions in `contracts.proto`.** The 47 serde types are generated from them, so deleting the .proto deletes the types. Keep:
  - `build.rs` with its protox and serde attributes
  - `json.rs` and `parsers.rs`
  - the llama samples and all their tests
  - the fixtures, which now guard the `*_json` column payloads
  Drop:
  - both `service` blocks
  - `RegisterRequest`, `RegisterResponse` and `HeartbeatStreamSummary`, plus their 3 fixtures
  - `tests/grpc_loopback.rs`
  - the `tonic`, `tonic-prost` and `tonic-prost-build` deps (switch `build.rs` to `prost_build::Config` + `compile_fds`, or set client/server to false)
  - `tonic::include_proto!` → `include!(concat!(env!("OUT_DIR"), "/contracts.v1.rs"))`
  This is about 30-45 min of work. The full list is in notes §4.
- **Client snippets** (notes §3, API names checked against the sdk-2.10.1 source):
  - Agent: `with_uri`/`with_database_name`/`with_token(credentials::File)`. In `on_connect` it calls `register_node` and subscribes to `command WHERE node_id = me`. `on_insert` runs `ack` → execute on a thread → `complete_command`. A 1 s `heartbeat` loop runs alongside.
  - Coordinator: runs on main with the publisher token and drives §5.5 through `create_plan`, `set_cluster_state`, `send_command` and `llama_ready`.
  - WP-7 status: `spacetime subscribe pool "SELECT … FROM node"`.
- **Rust SDK 2.10.1 has no auto-reconnect** (grep finds only a comment, `subscription.rs` l.52). The agent needs an outer reconnect loop; the snippet includes one.

**Surprises / things that change the plan:**
- The .proto cannot be dropped: it is the source of the types. Only gRPC goes.
- The module stores §6 payloads as JSON strings, so it does not depend on `contracts`, and prost never compiles to wasm.

**PRD suggestions:**
- O-03 → DECIDED: "Control plane = Spacetime 2.10.1 standalone on main (`ws://<main>:3000`, db `pool`), module `crates/stdb-module`, shared bindings `crates/stdb-bindings`. gRPC dropped. `.proto` kept as the type IDL only."
- O-14-style pin: "Spacetime CLI, server and crates `=2.10.1`; Rust stays 1.93.1."
- §6.12 config: add `spacetime_url`, `spacetime_db`; coordinator env `SPACETIME_TOKEN` (publisher token, never committed).
- WP-7 → "`spacetime subscribe` status + optional 60-line `status` subcommand". WP-9 detection → "`sweep` scheduled reducer (500 ms, 6 s timeout) + `client_disconnected`; recovery action stays in WP-4."
- Exception to "nobody installs": main installs the Spacetime CLI 2.10.1 + the wasm32 target.

**Questions for Aayo:**
- Which machine is main (O-06) and who owns `crates/stdb-module` + `crates/stdb-bindings`? Main runs `spacetime start`/publish/generate, and one owner keeps schema changes and regenerated bindings in sync.

**Next section:** Brief section 4 is done. If asked, I can do any of these next:
- pair on the first publish on main (§6 steps 1-5 of notes 04, about 30 min)
- fix the skeleton after the first `spacetime generate` shows the exact binding names
- the ScrabbleBot check (15 min)

Note: `research/spacetime/sdk-test/target` (686 MB) and `module-test/target` (133 MB) are build output. I added `.gitignore` files; do not commit `target/`.
