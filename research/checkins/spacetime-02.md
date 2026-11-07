## Check-in spacetime 02: Architecture (Spacetime as control plane + agent backend)

**Found (most important first, max 8 bullets, each with a source):**
- The agent loop runs in **one TS worker (Bun/Node) on the host M5**. A procedure can't do it for three reasons: it cannot reach localhost or the LAN, it cannot stream, and it is capped at 180 s (check-in 01, `instance_env.rs`). The worker subscribes to `task`, claims queued tasks through a reducer, calls EXO at `localhost:52415`, and writes every step back through reducers. Notes: `research/spacetime/02-architecture.md` §2, §6.
- **There are no per-Mac Spacetime clients.** EXO's `GET /state` already lists every node's memory, status and `start_layer/end_layer` (`research/exo/04-serving-and-ux.md` l.150-152). The same worker polls it every 2 s and calls `sync_devices`, so the other two Macs run only EXO.
- **The smallest table set is 4 public tables + 1 private:**
  - `device`: heartbeat, stats and layer range on one row
  - `task`: the job queue and state machine
  - `step`: plan items, thinking, messages, tool calls, results and the answer, as live rows
  - `file`: legacy code in, generated code and reports out
  - `admin`: the owner identity, written by `init`
  There are no separate heartbeat, metrics, layer, plan, tool-call or job tables.
- **There are 13 reducers and 2 lifecycle hooks**, about 200-250 lines of TS:
  - Dashboard: `create_task`, `add_file`, `approve_plan`, `cancel_task`.
  - Worker, admin only: `sync_devices`, `claim_task` (atomic claim), `set_plan`, `start_step`, `append_step_text` (server-side concat), `finish_step`, `write_file`, `add_usage`, `finish_task`.
  - `clientDisconnected` re-queues a dead worker's tasks. That gives crash recovery with no lease timer or schedule table.
- **The agent's file tools live in Spacetime.** `list_files`, `read_file` and `search` read the worker's subscription cache with no I/O, and `write_file` and `note` are reducers. Only `run_check` (py_compile and pytest in a temp dir) touches the OS. The dashboard shows generated files appearing live, and the legacy code never leaves the building.
- **Token streaming is cheap.** The worker flushes every 150 ms, which is at most 7 reducer calls/s at 13-37 tok/s. The cost is about 28 KB/s per tab for a 4 KB step, because subscribers get the whole updated row. That is trivial on a LAN. Thinking-on output for planning goes into a `thinking` step and thinking-off tool turns into `tool_call` steps. The worker should handle both `reasoning_content` and inline `<think>` tags, because which one EXO emits is unverified.
- **The "majority of backend on Spacetime" table (§7) is clear.** State, API, auth, queue, crash recovery, state machine, realtime push, code storage and agent file tools are all in Spacetime. The only other code is a worker that is a Spacetime client and an adapter to EXO and pytest. There is no Express, Postgres, Redis or websocket server of our own.
- **Gotcha: reducers return no values** (my reading of the docs, untested). The worker therefore picks the `step`/`file` ids itself and passes them in, instead of relying on autoInc ids coming back.

**Surprises / things that change the plan:**
- One worker on one Mac is enough. This drops the per-Mac agent from the plan.
- A human "Approve plan" step (`awaiting_approval` → `running`) costs one reducer and fits both NVIDIA ("plans then executes") and 8090 (a safe modernization workflow). I recommend it.

**PRD suggestions:**
- Replace the Spacetime bullet in Must have with: "Spacetime module `officepool` (TS) on the host M5. Tables: device, task, step, file (+ private admin). One worker.ts on the host polls EXO `/state` into `device` and runs the Nemotron agent loop. The plan, thinking, tool calls and results are `step` rows, and the code in and out is `file` rows. The dashboard (React + TS SDK) talks only to Spacetime."
- Add to the agent: "Tools: list_files, read_file, search, write_file, note (all Spacetime), run_check (pytest in a temp dir). Demo input: ~300-line COBOL loan-amortization program → Python + pytest + MIGRATION_NOTES.md. Plan needs human approval before execution."
- Add to Risks: "Verify the EXO streaming response carries `tool_calls` for Nemotron. If not, run tool turns non-streamed."

**Questions for Aayo:**
- Demo legacy input: is COBOL loan amortization (fits the mortgage-broker story) OK, or does the team have a real legacy file in mind (VB6, old PHP, Fortran)? This decides the system prompt and the `run_check` tool.
- Plan approval: should the human approve the plan (recommended), or should the agent auto-run?

**Next section:** Section 3, ScrabbleBot. I will only check the rules, the API, and how a bot connects, then give a yes or no on "under 1 hour" with a 10-line skeleton if yes. That should take about 5 tool calls. If Aayo has already decided to skip it, I will go straight to section 4: the exact module file layout, compilable TS for the tables and reducers above, the React subscription setup, worker.ts, and the build order.
