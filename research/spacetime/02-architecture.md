# 02: Architecture: Spacetime as the pool's control plane and the agent's backend

Worker: spacetime. Sat Nov 7 2026, ~4:30pm CT. Builds on `01-what-spacetime-is.md`.
Constraints from FEEDBACK: there are 3 pieces only (EXO pool serving Nemotron-3-Nano-30B, Spacetime, one 8090
legacy-modernization agent). Spacetime is hosted on one M5. Agent tools are our own TS wrappers. QR join is stretch.
API shapes below come from the docs in the cloned repo (`docs/docs/`, commit 39bf853). Code is a sketch and has not
been compiled.

## 1. Picture

```
 Browser dashboard (React + TS SDK)            M5 "host" Mac
 ─ subscribes: device, task, step, file        ┌──────────────────────────────────────────────┐
 ─ calls: create_task, add_file,               │ spacetime start  (0.0.0.0:3000)               │
          approve_plan, cancel_task  ── ws ──► │   module "officepool" (TypeScript)            │
                                               │   tables: device task step file (+admin)      │
                                               │        ▲ ws (TS SDK, owner token)             │
                                               │ worker.ts (Bun/Node), ONE process             │
                                               │   a) every 2 s: GET localhost:52415/state     │
                                               │      -> reducer sync_devices                   │
                                               │   b) agent loop: claim task -> LLM -> tools    │
                                               │      -> reducers (steps, files)                │
                                               │        │ HTTP localhost:52415/v1/chat/...      │
                                               │ EXO node ◄── pipeline ──► EXO on M3, M5 #2     │
                                               └──────────────────────────────────────────────┘
```

- There is **one worker process, on the host Mac**. The other two Macs run only EXO. EXO's `GET /state` already
  reports every node (memory, topology, instances, and `runner_to_shard[*].start_layer/end_layer`; see
  `research/exo/04-serving-and-ux.md` l.150-152), so the other Macs need no per-Mac Spacetime client. Fewer moving parts.
- The worker reaches EXO at `localhost:52415`. That is allowed, because the worker is an ordinary process and not a
  Spacetime procedure (procedures cannot reach private IPs; see check-in 01).

## 2. Where the agent loop runs: in the TS worker. Why not in a procedure?

| Requirement | Procedure | TS worker (client) |
|---|---|---|
| Reach EXO at localhost/LAN | **No**: `is_blocked_ip` refuses 127/8 and 192.168/16 (`instance_env.rs` l.1089-1272) | Yes |
| Stream tokens to the UI while generating | No: the body is collected whole (`BodyExt::collect`) | Yes: SSE from EXO, flushed to Spacetime every ~150 ms |
| Calls longer than 180 s (planning with thinking on) | No: clamped to 180 s | No limit |
| Run local tools (compile/test legacy code) | No | Yes |

The worker is a thin adapter: LLM I/O plus tool execution. Every state transition, every validation, the queue, the
history and the fan-out to viewers live in the module (section 6).

## 3. Smallest table set: 4 public tables + 1 private

All tables use `public: true` except `admin`. Timestamps are `t.timestamp()`.

### `device`: one row per EXO node (written only by `sync_devices`)

| column | type | note |
|---|---|---|
| nodeId | string, PK | EXO node id |
| name | string | "Aayo M5" (from EXO, or a friendly-name map in the worker) |
| chip | string | M5 / M3 |
| ramTotalGb, ramFreeGb, gpuWorkingSetGb | f32 | from `/state` node memory |
| layerStart, layerEnd | i32 | from `runner_to_shard`; -1 if no shard |
| status | string | `online` / `offline` / `loading` |
| tokPerSec | f32 | last measured decode speed (set by worker after each LLM call; pool-wide value goes on the host row or a `pool` view) |
| lastSeen | timestamp | |

Heartbeats, stats and layer assignment are all columns of this one row. There are no separate heartbeat or stats
tables. A device missing from `/state` for 2 syncs gets `status = 'offline'` (the worker sends the full node list, and
the reducer marks the absent ones).

### `task`: one row per agent run (the job queue)

| column | type | note |
|---|---|---|
| id | u64, PK, autoInc | |
| title | string | "Modernize loan_calc.cbl to Python" |
| goal | string | user's instruction |
| status | string | `queued` → `planning` → `awaiting_approval` → `running` → `done` / `failed` / `cancelled` |
| claimedBy | option(identity) | worker identity |
| createdBy | identity | ctx.sender |
| createdAt, updatedAt | timestamp | |
| summary | string | final answer (filled by `finish_task`) |
| tokensIn, tokensOut | u32 | running totals, for the "all of this stayed on our 3 laptops" counter |

### `step`: the agent's plan, thinking, tool calls and results as live rows

| column | type | note |
|---|---|---|
| id | u64, PK, autoInc | |
| taskId | u64, `.index('btree')` | dashboard subscribes `step.where(taskId = X)` |
| seq | u32 | order within the task |
| kind | string | `plan_item` / `thinking` / `message` / `tool_call` / `tool_result` / `answer` / `error` |
| status | string | `pending` / `running` / `done` / `failed` |
| title | string | plan item text, or tool name |
| text | string | streamed content (thinking, message, or result, truncated to 4 KB; full content in `file`) |
| toolArgs | string | JSON args for `tool_call` |
| parentId | u64 | tool_call/result → plan_item it serves (0 = none) |
| startedAt, endedAt | timestamp | duration shown in UI |

### `file`: the legacy code in and the modernized code out

| column | type | note |
|---|---|---|
| id | u64, PK, autoInc | |
| taskId | u64, `.index('btree')` | |
| path | string | `src/LOANCALC.cbl`, `out/loan_calc.py` |
| role | string | `legacy` (uploaded) / `generated` (agent) / `report` (migration notes, test output) |
| content | string | full text |
| version | u32 | bump on each agent write, so the UI can flash a diff |
| updatedAt | timestamp | |

Code is stored in Spacetime itself, so it never touches a cloud. The agent's file tools read the worker's local
subscription cache and write through reducers, and the dashboard shows files being written live. This also moves most
of the agent's "tool" surface into Spacetime.

### `admin` (private, 1 row)

`identity` PK. `init` inserts `ctx.sender` (the module owner; docs `00500-lifecycle.md` l.117). Worker-only reducers
check `ctx.db.admin.identity.find(ctx.sender)`. The worker runs with the owner's token (`spacetime login show --token`
on the host Mac). This prevents a random browser on the LAN from writing steps. It costs 5 lines.

Dropped on purpose: separate heartbeat, metrics, layer-assignment, plan, tool_call and job tables (all folded into
the rows above); a join-code table (QR is stretch, see section 8); schedule tables; and event tables (the token stream
uses row updates, see section 5).

## 4. Reducers (all in the module)

Called by the dashboard (any identity):

| reducer | args | checks / effect |
|---|---|---|
| `create_task` | title, goal | insert task `queued` |
| `add_file` | taskId, path, content | only while task is `queued`/`awaiting_approval`; role `legacy` |
| `approve_plan` | taskId | `awaiting_approval` → `running` (human in the loop; strong for 8090 and NVIDIA "plans then executes") |
| `cancel_task` | taskId | any non-terminal → `cancelled`; worker sees the update and aborts its fetch |

Called by the worker (admin only):

| reducer | args | effect |
|---|---|---|
| `sync_devices` | nodes: array of {nodeId, name, chip, mem..., layerStart, layerEnd, status} | upsert each; mark absent ones offline |
| `claim_task` | taskId | atomic: only if `queued` and unclaimed → `planning`, `claimedBy = sender`. Two workers can never take the same task. |
| `set_plan` | taskId, items: string[] | delete old `plan_item` steps, insert new ones `pending`; task → `awaiting_approval` (or `running` if auto-approve) |
| `start_step` | stepId (worker-chosen), taskId, kind, title, toolArgs, parentId | insert `running` step; module computes seq; rejects duplicate id |
| `append_step_text` | stepId, delta | `text += delta` (server-side concat, so the worker sends only the new tokens) |
| `finish_step` | stepId, status, text? | set `done`/`failed`, endedAt; optional final text overwrite |
| `write_file` | taskId, path, content, role | upsert by (taskId, path); version++ |
| `add_usage` | taskId, tokensIn, tokensOut, tokPerSec | add to task totals; set host device `tokPerSec` |
| `finish_task` | taskId, status, summary | → `done`/`failed` |

Lifecycle:
- `init`: insert the owner into `admin`.
- `clientDisconnected`: if the disconnecting identity has tasks in `planning`/`running`, set them back to `queued` and
  write an `error` step saying "worker disconnected, re-queued". This gives crash recovery with no lease timer and no
  schedule table.

That makes 13 reducers + 2 lifecycle hooks, about 200-250 lines of TypeScript. The module enforces every legal state
transition with a small `transition(task, from[], to)` helper that throws `SenderError` (`00600-error-handling.md` l.22-36).

Note: reducers do not return values to the caller (the docs describe return values only for procedures, which
"will be sent to the caller"; my reading, not tested). So `start_step` cannot
return the new id. Two options: (a) the worker generates the step id itself (pass a `u64` id and do not use autoInc on
`step`), which is simplest and deterministic; (b) match on `onInsert`. **Use (a)**: the worker keeps a counter seeded
from `max(id)` in its cache. Same for `file` by (taskId, path).

## 5. Token streaming: rate and cost

- Nemotron on the pool: PRD estimate is 13-37 tok/s (unmeasured).
- The worker buffers the SSE deltas and calls `append_step_text` every 150 ms or 64 chars, whichever is first. That
  is ≤7 reducer calls/s per running task, each with a payload of ~5-40 bytes of delta.
- Each call updates one `step` row. Subscribers receive the whole updated row, so cost grows with text length. At a
  4 KB step and 7 updates/s that is ~28 KB/s to each dashboard tab. This is trivial on a LAN. Long thinking steps
  (8-16 KB) give ~110 KB/s, still fine. If it ever matters: start a new `thinking` step every 2 KB (rotate rows).
- Reducer throughput on Spacetime is in the thousands of tx/s on a laptop (the marketing claims more; not measured
  by me). 7/s is nothing.
- Thinking: send `chat_template_kwargs: {enable_thinking: true}` for planning and `false` for tool turns
  (`research/principles/04-*.md` l.125). Route `reasoning_content` deltas into a `thinking` step and `content` deltas
  into `message`/`answer` steps. (Whether EXO splits reasoning into `reasoning_content` or inlines `<think>` tags in
  `content` is unverified. The worker should handle both: strip `<think>…</think>` into the thinking step.)

## 6. The agent loop (worker.ts, ~250 lines)

```
subscribe: task (all), step where taskId in active, file where taskId in active, device
on task insert/update with status=queued and no claimedBy -> claim_task(id)   (serial: 1 task at a time; the pool is one model instance)

run(task):
  1. PLAN  (thinking ON, no tools)
     messages = [system: "You modernize legacy code...", user: goal + file list (paths + sizes)]
     stream -> thinking step + message step; parse a numbered list -> set_plan(items)
     wait for task.status == running   (dashboard Approve button; or auto-approve flag)
  2. EXECUTE (thinking OFF, tools ON), per plan item:
     start_step(plan_item running)
     loop up to 8 turns:
       POST /v1/chat/completions {stream:true, tools:[...], chat_template_kwargs:{enable_thinking:false}}
       if tool_calls: for each -> start_step(tool_call, args) -> run tool -> start_step(tool_result) -> finish
                      append tool messages, continue
       else: finish plan_item with the message text; break
     check task.status == cancelled between turns -> abort
  3. ANSWER: final summary call (thinking OFF) -> answer step -> finish_task(done, summary)
  on any exception: error step + finish_task(failed)
```

Tools (OpenAI function schema; Nemotron uses the qwen3_coder parser in mlx-lm; `tool_choice` is ignored, per the PRD):

| tool | runs where | implementation |
|---|---|---|
| `list_files()` | worker cache of `file` rows | no I/O |
| `read_file(path, startLine?, endLine?)` | worker cache | no I/O; returns ≤ 300 lines |
| `search(pattern)` | worker cache | regex over legacy files |
| `write_file(path, content)` | `write_file` reducer | the dashboard shows the new file appear |
| `run_check(path)` | worker, local temp dir | materialize `generated` files to `/tmp/task-<id>/`, run e.g. `python -m py_compile` + `python -m pytest -q` (timeout 30 s); the output becomes a `report` file + `tool_result` |
| `note(text)` | `write_file` to `MIGRATION_NOTES.md` | the migration report the 8090 judges read |

Four of the six tools are pure Spacetime reads/writes. Only `run_check` touches the OS, and it is sandboxed to a temp
dir. Never run untrusted legacy code, only the generated Python and its tests.

Demo input (suggestion): a ~300-line COBOL loan-amortization program, or a VB6/Classic ASP module, uploaded through
`add_file`. Output: Python + pytest tests that reproduce the COBOL's sample outputs + `MIGRATION_NOTES.md`. This fits the
PRD's regulated-office story (a mortgage broker's legacy code that cannot go to a cloud AI).

## 7. "Majority of the backend is on Spacetime": the case

| Backend responsibility | Where |
|---|---|
| Persistent state (devices, tasks, plan, steps, code files, usage) | Spacetime tables |
| API for the UI (create task, upload code, approve, cancel) | Spacetime reducers |
| Auth / who may write what | Spacetime identity + `admin` check |
| Job queue with atomic claim | `claim_task` reducer |
| Crash recovery (re-queue on worker drop) | `clientDisconnected` hook |
| Task state machine and validation | reducers |
| Realtime push to dashboards (live steps, tokens, files, device stats) | Spacetime subscriptions |
| Storage of the legacy code and generated code | `file` table |
| Agent file tools (list/read/search/write) | subscription cache + reducer |
| LLM inference | EXO (the compute, not the backend) |
| Calling EXO over HTTP + running `pytest` | worker.ts (an adapter that is a Spacetime client, like a game client) |

There is no Express, no Postgres, no Redis and no websocket server of our own. The dashboard is static files that talk
only to Spacetime. Say this in the README with this table. Also say in the README why the worker exists (the
private-IP block, no streaming in procedures, the 180 s cap), so it reads as a deliberate design.

## 8. Stretch items (only after the core works)

- QR join: HTTP handler `GET /v1/database/officepool/route/join?code=XYZ` (beta) that returns a small HTML page with the
  EXO install command and the host IP. A 1-row `join_code` table plus a `rotate_join_code` reducer. ~30 lines.
- Rebalance button: a `rebalance_requested` flag on a 1-row `pool` table. The worker sees it and calls EXO
  `DELETE /instance` + `POST /instance` with `min_nodes` = online count (`research/exo/04-*.md` l.45-47).
- MCP: point a second agent (or Claude Desktop) at `POST /v1/database/officepool/mcp` to query tasks with SQL. Zero code.
- ScrabbleBot: separate repo, not this module.

## 9. Risks and checks (do these first)

1. `spacetime start` + `spacetime publish` on the M5 and a browser on another Mac connecting to `ws://<m5-ip>:3000`
   (macOS firewall prompt for `spacetime`).
2. TS SDK in Bun/Node on the host: `DbConnection.builder().withUri('ws://localhost:3000').withDatabaseName('officepool')
   .withToken(ownerToken)` (`00300-nodejs.md` quickstart l.138-160).
3. Verify that a reducer called with the owner token sees `ctx.sender` == the `init` owner (needed for the `admin` check).
   Otherwise fall back to a shared-secret `register_worker(secret)` reducer.
4. Verify that `tool_calls` come back in EXO's streaming chunks for Nemotron (if only non-stream works, the tool turns
   are non-streamed; only plan/answer stream).
5. Row size: the largest legacy file is ≤ 100 KB, so a single string column is fine (no documented row limit found;
   unverified).
