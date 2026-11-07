# 01: What Spacetime is, precisely

Worker: spacetime. Written Sat Nov 7 2026, ~4:00pm CT.
Sources: SpacetimeDB repo cloned at commit `39bf853` (Nov 6 2026, workspace version 2.15.0 in `Cargo.toml`), its
`docs/docs/` tree (the source of spacetimedb.com/docs, labeled "2.0.0"), GitHub releases (latest v2.14.1, Nov 4 2026).
Local clone path: `/tmp/SpacetimeDB` (Windows: `%TEMP%\SpacetimeDB`).

## 1. The model in one paragraph

Spacetime is a relational database that is also the app server. You write a "module" (TypeScript, Rust, C#, C++),
publish it into the database, and clients connect over WebSocket, call functions, and subscribe to SQL queries. Every
committed change to subscribed rows is pushed to clients. There is no separate API server.

## 2. Building blocks (docs `00200-core-concepts/`)

| Piece | What it is | Notes that matter for us |
|---|---|---|
| Table | Typed rows, primary key, indexes, `autoInc`, unique | Private by default. Only `public: true` tables can be subscribed to by clients (`00050-troubleshooting.md` "Tables not visible to client"). |
| Reducer | Transactional function, called by clients | Atomic, deterministic, **no network I/O**. `ctx.sender` = caller identity. |
| Procedure | Function that may do side effects, incl. outbound HTTP | Runs outside a transaction; opens them with `ctx.withTx(tx => ...)`. The `withTx` body may be re-run, so it must be deterministic (`00400-procedures.md` l.151-168). Return value goes only to the caller. **Stable in TypeScript**; "unstable" in Rust (`features=["unstable"]`), C#, C++. |
| View | Read-only function returning rows; subscribable like a table | Must be `public` with a name. `anonymousView` is much cheaper than per-caller `view` (`00500-views.md` l.292-303). |
| HTTP handler | Module exposes routes under `/v1/database/:name/route/...` | **Beta**. Sender is a random anonymous identity per request; Authorization header is not checked for you (`00600-HTTP-handlers.md` l.13-19). TS: `spacetimedb.httpHandler((ctx, req) => new SyncResponse(...))`. Useful for a QR "join" URL a phone or laptop can hit without the SDK. |
| Schedule table | Table with `scheduledAt: t.scheduleAt()` column; each row fires a reducer or procedure at a time or interval | TS: `onSchedule` option. A scheduled procedure must return `t.unit()` (`00500-schedule-tables.md` l.14, 53-55). Good for heartbeat expiry sweeps and polling. |
| Event table | Table that is empty outside a transaction; clients see inserts only | New in 2.0, replaces 1.0 reducer callbacks (`00600-migrating-to-2.0.md` l.15-20, 421-427). Good for "token chunk arrived" or "agent step event" broadcasts. |
| Subscription | Client sends SQL / query-builder queries; gets initial rows then deltas | Insert/update/delete callbacks; update only for tables with a primary key. |
| Lifecycle reducers | `init`, `clientConnected`, `clientDisconnected` | Disconnect hook gives free "device went offline" detection. |
| Inter-database communication | `00650-inter-database-communication.md`, `st_outbound_*` tables in `crates/core/src/host/idc_actor.rs` | Not needed for us; noted. |

## 3. Languages

- Module (server): TypeScript (runs in V8 inside the host), Rust, C#, C++ (all three compile to WASM)
  (`00300-language-support.md`).
- Client SDKs (official): TypeScript (browser, Node, Bun, Deno; React/Next/Vue/Svelte/etc. quickstarts), Rust, C#
  (Unity), Unreal C++/Blueprint.
- **No supported Python SDK.** `clockworklabs/spacetimedb-python-sdk` is archived on GitHub (`gh api` says
  `archived: true`, last push Oct 26 2026) and predates the 2.0 WebSocket protocol (v2). A Python process can still use
  the HTTP API (call reducers/procedures, run SQL) but cannot subscribe. So the node-side worker should be TypeScript
  (Node or Bun).
- Recommended stack for us: TypeScript module + React dashboard (TS SDK) + TS/Bun node worker. One language end to end.

## 4. Running it

- Install (Install page lists Windows, macOS, Linux):
  - macOS/Linux: `curl -sSf https://install.spacetimedb.com | sh` (confirmed in `00200-self-hosting.md` l.28).
  - Windows PowerShell: `iwr https://windows.spacetimedb.com -useb | iex` (from memory of the install page; the page
    is JS-rendered and I did not get the exact string. Verify at https://spacetimedb.com/install).
- Local server: `spacetime start`. Default listen address `0.0.0.0:3000` (`crates/standalone/src/subcommands/start.rs`
  l.33-36), so other machines on the LAN can connect to `ws://<host-ip>:3000` with no extra flag. Persistent override:
  `listen_addr` in `cli.toml`.
- `spacetime dev` watches the module, republishes and regenerates bindings on save.
- Offline: standalone runs with no internet. Auth gotcha: if you publish to local before `spacetime login`, the CLI does
  a "server-issued login" against the local server; a later `spacetime login` silently discards that token and you
  lose owner rights over the local DB (`00050-troubleshooting.md` l.25-46). Rule for the team: decide once. Either
  `spacetime login` (GitHub/Google) on the host Mac **before** first publish while online, or stay server-issued and
  never run `spacetime login` on that machine. Back up with `spacetime login show --token`.
- Maincloud: hosted (`spacetime publish --server maincloud`). Fine for the track, but it breaks "data never leaves the
  building", and see section 6: Maincloud cannot reach the LAN pool anyway.

## 5. MCP endpoint (`00300-resources/00150-mcp.md`, marked **unstable**)

- `spacetime mcp [db] --server local` bridges stdio MCP; HTTP: `POST /v1/mcp` (host-wide) or
  `POST /v1/database/<name>/mcp` (one DB). Bearer token, or none for an anonymous identity.
- Tools: `list_databases`, `ping`, `get_schema`, `sql` (optional `confirmed`), `call` (reducer, positional JSON args).
- Release notes say `/v1/mcp` is now live on Maincloud too.
- Use for the NVIDIA track: the Nemotron agent can get `get_schema` / `sql` / `call` as tools against our own DB with
  zero glue code. A legacy-data story for 8090 also fits (agent inspects schema, runs SQL).

## 6. Critical check: can a procedure call EXO at `http://192.168.x.x:52415`? **No.**

Code: `crates/core/src/host/instance_env.rs` (commit 39bf853).

- l.1089-1091: if the URL host is an IP literal and `is_blocked_ip`, return
  `"refusing to connect to private or special-purpose addresses"` (constant at l.1184).
- l.1093-1099: same check on every redirect hop.
- l.1186-1203: a custom DNS resolver (`FilteredDnsResolver`) drops resolved addresses that are blocked, so a hostname
  like `mac1.local` or a DNS name pointing to `192.168.x.x` is also refused.
- l.1221-1272 `is_blocked_ipv4`: blocks `10/8`, `127/8`, `172.16/12`, `192.168/16`, plus the other RFC 6890
  special-purpose ranges. l.1281+ does the same for IPv6 (`::1`, ULA, link-local, etc.).
- The only escape is the Cargo feature `allow_loopback_http_for_tests` (`crates/core/Cargo.toml` l.120,
  `crates/standalone/Cargo.toml` l.21) and it only un-blocks **loopback**, not `192.168/16`. It is a compile-time
  feature, not available in the released binary.
- There is no config switch. `crates/standalone/config.toml` `[module-http]` has only `enabled` (on/off for all
  outbound HTTP).
- Checked the released tags via `gh api ...contents/...?ref=`: the block is present in **v2.0.0 and v2.14.1**. It
  applies to standalone and Maincloud alike (same core code).

Consequences:
- A procedure cannot call EXO on the LAN, nor on the same Mac via `localhost`.
- Options: (a) **node-side worker** (TS client on each Mac) subscribes to a `job` table, calls EXO at
  `localhost:52415`, and writes results back through reducers. This is the normal Spacetime pattern and also gives
  live token streaming (write chunks as rows). (b) Build `spacetime` standalone from source with `is_blocked_ip`
  patched to return false: a one-line change, but a Rust build of the whole server on a laptop at a hackathon
  (guess 10-25 min, untested) and a forked binary. (c) Tunnel EXO to a public URL: defeats the privacy pitch. Pick (a).
- Docs do not mention this block anywhere in `00400-procedures.md` (grep for private/loopback found nothing).

## 7. HTTP timeout and streaming in procedures

- Default 30 s, max 180 s, clamped silently (`instance_env.rs` `HTTP_DEFAULT_TIMEOUT` / `HTTP_MAX_TIMEOUT` just above
  l.1184; docs `00400-procedures.md` l.919). Covers connect through full body read.
- **No streaming.** The host collects the whole body before returning (`http_body_util::BodyExt::collect` in the
  spawned task, ~l.1117-1121). TS API is synchronous: `ctx.http.fetch(url, init): SyncResponse`
  (`crates/bindings-typescript/src/server/http_internal.ts` l.37-42). SSE from an LLM would arrive as one blob at the
  end.
- `ctx.http.fetch` inside `withTx` fails with `WouldBlockTransaction` (`instance_env.rs` ~l.1020-1022). Do HTTP
  outside the transaction, then open a transaction to write the result.
- Procedure concurrency: pool of `procedure-instance-pool-size` per database, default = CPU cores (`config.toml`
  `[v8]`).

## 8. Auth and identity

- Every connection has an `Identity` derived from an OIDC JWT. With no token, the server issues an anonymous identity
  and token; SDK saves it (browser cookie/localStorage, Node file). Good enough for devices: each Mac's worker gets a
  stable identity that we map to a `device` row.
- Real login: SpacetimeAuth, Auth0, Clerk, BetterAuth integrations (`00500-authentication/`). Not needed for the demo.
- Row-level security exists (`00250-how-to/00400-row-level-security.md`) if we want "each office sees only its jobs".

## 9. 2.0 changes and gotchas

- 2.0 (vs 1.x): new WebSocket protocol v2; reducer callbacks removed (use event tables or per-call `_then()`);
  `light_mode` and `CallReducerFlags` removed. Old blog posts and the Python SDK are 1.x and will mislead.
- Releases move fast: 2.11 (Oct 7) to 2.14.1 (Nov 4), roughly weekly. Pin CLI and SDK to the same version on every
  machine; mismatch gives codegen type errors (`00050-troubleshooting.md` l.141-147).
- Unverified (from memory, not checked in this clone): `ctx.identity` is the database's identity, the caller is
  `ctx.sender`; scheduled reducers are not callable from clients; `onUpdate` fires only for tables with a primary key.
  Check these in section 4 when writing the skeleton.
- C#/Rust/C++ procedures and Rust HTTP handlers need unstable flags. TypeScript procedures need none. Another reason to
  use TypeScript.
