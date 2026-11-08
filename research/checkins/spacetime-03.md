## Check-in spacetime 03: Spacetime track impact on the notes-branch PRD (Rust, llama.cpp RPC)

**Found (most important first, max 8 bullets, each with a source):**
- **Blocker: PR #17 pins Rust 1.93.1, but `spacetimedb` and `spacetimedb-sdk` 2.12.0 to 2.14.1 require Rust 1.97.** The last versions that build on 1.93 are 2.10.1 (Sep 15 2026). Fix: bump `rust-toolchain.toml` to ≥1.97 and add `targets = ["wasm32-unknown-unknown"]`. The alternative is to pin Spacetime CLI and crates to 2.10.1 everywhere. Source: crates.io `/api/v1/crates/spacetimedb-sdk/versions` `rust_version`; `gh pr view 17`.
- **The Rust client SDK should work on Windows and macOS.** Its native deps are tokio, tokio-tungstenite and native-tls (SChannel / Security.framework), with no protoc and no C++, and only wasm32 is cfg-gated. I have not built it on our machines; build it on the Dell first. A Rust agent can be a Spacetime client: `register_node` on connect, a 1 s `heartbeat` reducer, `on_insert` on `command WHERE node_id = me`, then `complete_command`. Threading options are `run_threaded`, `run_async` or `frame_tick`. Source: `sdks/rust/Cargo.toml`, `sdks/rust/src/db_connection.rs` l.656-696.
- **Offline works.** `spacetime start` runs on main only (TCP 3000), and agents dial out with no token (the server mints an identity and the client saves the token to a file). Helpers need no inbound control port, only rpc-server's 50052. The module needs only stable features: tables, reducers, lifecycle hooks and schedule tables. No procedures or HTTP handlers, which are "unstable" in Rust.
- **Mapping of §6** (notes 03 §2):
  - Spacetime tables: 6.1 NodeCapabilities, 6.2 Heartbeat, 6.3 Command/Result, 6.4 ModelInfo, 6.5 SplitPlan, the 6.6/6.7 cluster row, the 6.10 event log, 6.8/6.9 results.
  - Scalar fields are columns; nested blobs stay serde-JSON strings, so the WP-2 types remain the source of truth.
  - About 15 reducers enforce the §5.6 state transitions.
  - Events are written in the same transaction as each state change.
- **Effect on each WP:**
  - WP-7 mostly disappears: status becomes `spacetime subscribe "SELECT … FROM node"` in a terminal, which also satisfies D-06 "no frontend".
  - WP-9 detection becomes a 500 ms scheduled `sweep` reducer plus `client_disconnected`. A sweep is still needed because a silent drop otherwise waits for the 30 s WS idle timeout (`crates/standalone/config.toml [websocket]`).
  - WP-3/WP-4 lose their registry, state and transport code but keep all process supervision. llama-server launch, rpc-server supervision, log parsing, probes and the GGUF parse stay outside, because modules have no process or file I/O and procedures can't reach the LAN.
- **It is better for the product, not only the prize.**
  - State and the event log survive a coordinator restart.
  - Helpers need no inbound firewall rule for the control plane.
  - Every reader gets live status for free.
  - The state-machine rules sit in one place.
  The costs are a toolchain bump, one more process on main, a 1 h learning ramp, and bindings to regenerate after each schema change.
- **"Majority of backend" holds honestly if it is framed as the control plane and the module is made fat.** Put the WP-5 planner inside the module (if it compiles to wasm) and keep the clients free of business logic. Weak spot: by line count, the agent and coordinator supervision code may rival the module. llama.cpp is the engine, not our backend.

**Surprises / things that change the plan:**
- The toolchain conflict with PR #17 has to be resolved before PR #17 merges, whatever the decision.
- Half-and-half (gRPC plus Spacetime bolted on) is the worst option: it fails the "majority" rule and costs more than either option alone.

**PRD suggestions:**
- Add an O-27 decision: "Control-plane transport and state: gRPC (O-03) or Spacetime standalone on main (Rust module `crates/stdb-module` + `spacetimedb-sdk` clients). Decide by 7:00pm, with a go/no-go at 7:45pm: a Rust client on the Dell calls a reducer on main and gets `on_insert`."
- If yes: WP-2 drops `.proto`/tonic/protox and keeps serde types + parsers. A new module owner builds the tables and reducers (2.5-3 h). WP-7 shrinks to a `status` subcommand + `spacetime subscribe` (0.5 h). WP-9 detection moves into a scheduled `sweep` reducer. Add `spacetime_url`/`spacetime_db` to §6.12 config.
- WP-12/PR #17: toolchain ≥1.97 and the wasm32 target (needed for Spacetime 2.12+), or pin Spacetime 2.10.1.
- Risks: "Spacetime server is a hard dependency on main (the WP-1 manual runbook stays the fallback)" and "Rust SDK auto-reconnect unverified; the agent may need its own reconnect loop."

**Questions for Aayo:**
- Go or no-go on the Spacetime track by 7:00pm? It decides whether WP-2 keeps writing tonic/protox right now.
- Who owns `crates/stdb-module` and the committed bindings? One owner avoids schema churn.

**Next section:** If yes: write section 4 as a build plan for this Rust design. That means the exact `crates/stdb-module/src/lib.rs` skeleton (tables, reducers, sweep schedule, transition checks), the agent/coordinator client snippets (connect, token file, subscribe, call), and the Dell build test commands. First I will run the go/no-go myself on this Windows machine: a fresh crate with `spacetimedb-sdk 2.14.1` on Rust ≥1.97, plus the `spacetime` CLI, to verify the Windows build. If no: I stop, and offer a 15-minute check of ScrabbleBot as a separate side entry.
