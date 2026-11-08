# cppneed 02: The coordinator we would write

Worker `cppneed`, Sat Nov 7 2026. Source: `Desktop\hackathon\llama-src` at `e9a2a44`. Fits the spacetime-02 design:
one TS worker process (`worker.ts`) on the host Mac runs the agent and polls the engine every 2 s; the other Macs have
no Spacetime client (`research/spacetime/02-architecture.md` lines 10-31).

## 0. Facts from the source that shape the design

| Fact | Source | Consequence for the coordinator |
|---|---|---|
| A remote rpc-server that dies or answers badly makes llama-server abort: `RPC_STATUS_ASSERT(x) ... GGML_ABORT("Remote RPC server crashed or returned malformed response")` | `ggml/src/ggml-rpc/ggml-rpc.cpp:31, 433, 685-688` | A leave = llama-server process exit. Detect by child-process exit, not by parsing errors |
| A remote that is unreachable at launch also aborts: `GGML_ABORT("Failed to connect to %s")`, handshake failure too | `ggml-rpc.cpp:653, 656` | Probe every worker before launch; never put a dead IP in `--rpc` |
| rpc-server serves one client at a time | cppcode-01 (`RPC.cpp:2985-2993`) | Exactly one llama-server in the pool. A TCP probe while llama-server is connected only reaches the listen backlog, so it proves "Mac is up", not "rpc-server is free" |
| `--tensor-split` values are proportions of **layer count**, not bytes. Layer `il` goes to the first device whose cumulative fraction is `> il/(n_layer+1)`; the output layer is slot `n_layer` | `src/llama-model.cpp:1733-1789` | Pass integer layer counts (`k1,k2,k3`, summing to n_layer+1). Nemotron layers differ a lot in size (23 MoE layers hold most bytes), so the coordinator needs per-layer bytes |
| Input embeddings always stay on the head's CPU memory (unified on a Mac) | `src/llama-model.cpp` "assign the input layer" (after line 1777) | Count `token_embd` against the head's budget |
| Device order is `rpc` servers (in `--rpc` order), then local GPU | `src/llama.cpp:275-279` | Head always holds the last range plus the output layer |
| With mmap (default), each weight tensor is uploaded whole via `ggml_backend_tensor_set(cur, data, 0, n_size)`, so the hash covers the whole tensor | `src/llama-model-loader.cpp:1597-1600, 1749` | The `-c` cache works per whole tensor. Do not pass `--no-mmap` |
| Cache file name = `%016llx` of FNV-1a 64 (basis `0xcbf29ce484222325`, prime `0x100000001b3`) of the raw tensor bytes; file content = raw bytes | `ggml-rpc.cpp:313-320, 1760-1770, 1838-1856` | A cache can be **built offline from the GGUF** and copied to every worker by USB (section 4) |
| Default `--reasoning-format` is `auto` = thinking returned in `message.reasoning_content`, also in stream deltas | `common/common.h:429-434` | Agent reads `reasoning_content`; no flag needed |

## 1. Responsibilities (one module inside worker.ts: `engine-llamacpp.ts`)

The module exposes the same shape as the EXO adapter in spacetime-02 (`getState()` -> nodes with `layerStart`,
`layerEnd`, memory, status; chat on localhost), so `sync_devices` and the `device` table stay unchanged. Switching
engines is a one-file swap.

1. **Config** (`pool.json`, written by hand at setup):
   `{ head: {name, chip, budgetGb, bwGBs}, workers: [{name, chip, ip, port:50052, budgetGb, bwGBs}], model: "/Users/.../Nemotron-3-Nano-30B-A3B-Q4_0.gguf", layers: "layers.json", ctx: 32768, parallel: 2 }`.
   IPs come from DHCP reservations on our own router.
2. **Layer table** (`layers.json`, made once at setup): bytes per `blk.N` plus `token_embd` and `output`, from the
   GGUF tensor list (`gguf-py` in `llama-src/gguf-py`, ~20-line script). Also records which layers are attention,
   for KV sizing.
3. **Probe loop** (every 2 s, same tick as the existing `/state` poll):
   - each worker: TCP connect to `ip:50052`, 500 ms timeout. 2 misses in a row = `offline`.
   - head: `GET http://127.0.0.1:8080/health` (200 ready, 503 loading) and child-process alive.
   - `GET /slots` and, if started with `--metrics`, `GET /metrics` for busy slots and token counters.
4. **Plan** (section 2): pick devices, compute layer counts, emit `--rpc` and `--tensor-split`.
5. **Launch / restart**: spawn `llama-server` (args in section 3), capture stderr, parse
   `load_tensors: <dev> model buffer size = X MiB` lines (`src/llama-model.cpp:2117`) to record real bytes per
   device, wait for `/health` 200 (timeout 300 s). Kill with SIGTERM, then SIGKILL after 10 s.
6. **Drain**: the agent loop is in the same process, so drain = a mutex. The agent takes it around each LLM call;
   restart waits for it. No HTTP-level drain needed.
7. **Report**: `getState()` returns one node per Mac with `layerStart/layerEnd` from the plan, `status`
   (`online` / `offline` / `loading` / `standby` = reachable but not in the pool), budget, and pool-level
   `engineStatus` + `lastRestartMs`.

The other Macs run only `rpc-server` (section 3), started once by hand or by `ssh` from the host (macOS Remote
Login works offline on our router). No other code runs on them.

## 2. How it computes `--tensor-split` (speed-aware)

Decode in a layer pipeline is serial: one token passes through every device in turn, and llama.cpp does not overlap
devices during decode. Time per token is

`T = sum over devices i of (active bytes on i / memory bandwidth of i) + (number of remote devices) x RTT`

(1 blocking round trip per remote per token: the head sends the hidden state and inputs without waiting, then
GET_TENSOR waits for the remote's compute and result; star through the head, cppcode-01 section 5).
Two consequences:

- **Fewer devices is faster.** Each extra remote adds ~1 RTT per token (Wi-Fi RTT ~2-5 ms on our own router, guess,
  measure). Use only as many Macs as the model needs to fit.
- **Put more layers on faster Macs, up to their memory cap.** Splitting in proportion to speed is right for a
  pipeline that overlaps requests; llama.cpp does not overlap, so the optimum is "fill the fastest Macs to their cap,
  give the slowest the remainder".

Algorithm (~60 lines TS):

```
budget_i  = min(free RAM, ~2/3 of RAM Metal wired limit) - reserve_i        // reserve: KV + Mamba state + compute buffer, ~1 GB
need      = sum(layer bytes) + token_embd + output                           // from layers.json
devices   = [head] + workers sorted by bwGBs desc, then by budget desc
pool      = smallest prefix of devices whose sum(budget) >= need             // head always in
order     = workers in pool sorted by bwGBs desc, head last                  // head must be last (llama.cpp order)
fill: walk layers 0..n-1, assign to the fastest device with room; since ranges must be contiguous in --rpc order,
      do it as: for each device in `order` except the head, take layers while bytes fit (fast ones first);
      head takes the rest + output; if head overflows, move the boundary back and push layers to the slowest worker.
--tensor-split = k_1,...,k_m,k_head   (k_head counts the output layer; sum = n_layer + 1)
--rpc          = ip_1:50052,...,ip_m:50052   (same order)
```

Check: llama-model.cpp sends layer `il` to device `j` iff `cum[j-1] <= il/(n+1) < cum[j]` with
`cum[j] = (k_1+...+k_j)/(n+1)`. Integer counts give exact boundaries.

Worked example (guesses: Nemotron Q4_0 18.2 GB, 52 layers per EXO card; budgets M5 24 GB ~14 GB, M3 16 GB ~9 GB
after apps; bandwidth M5 ~153 GB/s, M3 ~100 GB/s, unverified):

| Pool | Fits? | Plan |
|---|---|---|
| M5 head + M5 worker | 28 GB budget >= 18.2 + ~2 | 2 devices, ~half each; M3 is `standby` |
| M5 head + M3 + M5 | yes | Only if the demo must show 3 Macs: M5 worker filled first, M3 gets the remainder (smallest share), head last |
| Any 2 of the 3 | yes for Q4_0 (M3+M5 = 23 GB) | Survivor plan after a leave |
| UD-Q4_K_XL 22.8 GB | needs all 3 or two M5s tight | decide on day 1 from real `model buffer size` lines |

Demo choice: show 3 Macs with the M3 holding the fewest layers ("placed by speed"). Pure speed would leave the M3
out; that is an honest line for the pitch ("the slow Mac only joins when the model needs its memory").

## 3. Start / restart commands

Worker Macs (once, left running; rpc-server loops on `accept` by itself):

```
rpc-server -H 0.0.0.0 -p 50052 -c
```

Head (spawned by the coordinator):

```
llama-server -m <model.gguf> --rpc <ip1>:50052,<ip2>:50052 -ngl 99 --tensor-split <k1>,<k2>,<khead> \
  -c 32768 --parallel 2 --metrics --host 127.0.0.1 --port 8080 --offline
```

`--host 127.0.0.1`: only the worker.ts on the same Mac calls it; the dashboard goes through Spacetime. Exact flag
syntax gets verified against `common/arg.cpp` in section 3.

## 4. Join, leave, and what a restart costs

**Leave** (worker Mac sleeps, Wi-Fi drops, lid closed):
1. llama-server aborts on the next RPC call (`ggml-rpc.cpp:31`); if idle, the TCP probe catches it within 4 s.
2. The in-flight LLM call fails; the agent step is marked retry (Spacetime reducer, existing `step` status).
3. Coordinator re-plans with survivors. If they fit: relaunch. If not: `engineStatus = degraded`, dashboard says
   "model does not fit on remaining Macs".
4. Agent retries the step after `/health` 200.

**Join** (Mac starts rpc-server):
1. Probe sees the port; device row `standby`.
2. No automatic restart (avoid killing an agent run). Dashboard button "Rebalance" calls a reducer that sets a flag;
   the worker re-plans and restarts at the next idle point. Same as EXO's "no rebalance on join", but one click.

**Restart cost** (estimates, none measured):

| Step | Warm (all tensors cached on the worker) | Cold (cache miss) |
|---|---|---|
| Head reads GGUF via mmap | 18.2 GB at ~5 GB/s SSD, or from page cache: 0-4 s | same |
| Head hashes every remote tensor > 10 MiB with byte-wise FNV-1a (cppcode-01; `ggml-rpc.cpp:313-320, 853`) | Serial xor+multiply chain, ~4 cycles/byte, ~1 GB/s per core (guess). Remote share ~9-12 GB: **~9-12 s**, single-threaded, one tensor at a time | same |
| 1 synchronous round trip per tensor > 10 MiB | ~100-200 tensors x 3 ms: <1 s | same |
| Worker reads cache file, copies to Metal | ~6 GB per worker at ~3-5 GB/s: 1-2 s each, serial | n/a |
| Network transfer of remote share | 0 | ~9-12 GB over Wi-Fi at ~60 MB/s: **2.5-3.5 min**; over gigabit Ethernet ~110 MB/s: ~1.5-2 min |
| Small tensors (<= 10 MiB) always resent + server warmup | a few seconds | same |
| **Total** | **~20-30 s** | **~2-4 min** |

The trap: after a re-plan, a worker receives layers it never held, so those tensors are cache misses (cold) even with
`-c`. Two fixes:

- **Pre-warm** at setup: launch once for every plan the demo can reach (full 3 Macs, and each 2-Mac survivor plan:
  4 launches). Cost: ~4 cold loads, done once before judging.
- **Build the cache from the GGUF** (better): a 40-line C program on the host walks the GGUF tensor list, and for
  every tensor > 10 MiB writes the raw bytes to `<cache>/<%016llx FNV-1a 64>`. Copy the folder to each worker by USB
  (~16-20 GB). Then every plan is warm. Matches the server code exactly (`ggml-rpc.cpp:1760-1770` write,
  `1838-1856` read). Risk: any loader transform before upload would change the hash; with mmap there is none for
  plain Metal buffers (`llama-model-loader.cpp:1749`), but verify with `GGML_RPC_DEBUG=2` showing `cache: hit`.
- Cache path on the worker: cppcode-01 reads `~/Library/Caches/llama.cpp/rpc` on macOS from `rpc-server.cpp:48-66`,
  or `$LLAMA_CACHE`. Set `LLAMA_CACHE` explicitly to avoid doubt.

## 5. HTTP contract (agent and dashboard)

| Caller | Call | Notes |
|---|---|---|
| agent (worker.ts, same Mac) | `POST http://127.0.0.1:8080/v1/chat/completions` `{messages, tools, stream:true, chat_template_kwargs:{enable_thinking:true|false}, reasoning_budget_tokens?}` | JSON boolean for `enable_thinking` (`server-common.cpp:1487-1495`). Read `reasoning_content` and `tool_calls` from deltas. Per-response `timings` give tokens/s -> `add_usage` |
| coordinator | `GET /health`, `GET /slots`, `GET /metrics` | 2 s poll |
| dashboard | Spacetime subscriptions only (`device`, `task`, `step`, `file`) | No direct HTTP to the engine |
| dashboard -> worker | new reducer `request_rebalance()` (sets a flag the worker reads) | PRD/spacetime suggestion |
| Spacetime `device` row | existing columns: `layerStart/layerEnd` from plan, `status` adds `standby`; pool row or host row gets `engineStatus`, `lastRestartMs` | small schema addition |

Model name in requests is ignored by a single-model llama-server.

## 6. Size and time

| Part | Lines (TS unless noted) |
|---|---|
| config + layers.json loader | 40 |
| `layers.json` dump script (Python, gguf-py) | 20 |
| plan / split calculator | 60 |
| launcher, stderr parser, health wait, kill | 120 |
| probe loop + getState adapter | 80 |
| leave/join/rebalance handling + agent mutex | 70 |
| cache builder (C, optional) | 40 |
| **Total** | **~430** |

Time: ~3 h to write, ~3-4 h to test on the 3 Macs (cold loads alone take minutes each). **~6-7 h total**, one person.
The engine bring-up test (Nemotron-H over RPC works at all) comes before any of this.

## 7. Against EXO

| Area | EXO gives for free (EXO-HOW-IT-WORKS.md) | llama.cpp + our coordinator |
|---|---|---|
| Discovery | Multicast, automatic | Static IP list + TCP probe (more reliable on our router; no multicast needed) |
| Membership / leader | Zenoh leases, election | None needed: host Mac is fixed |
| Placement | By memory, fewest Macs, ignores speed (`# TODO: flops_fp16`) | Ours: fewest Macs, fastest first, memory-capped, exact layer counts. **Gain** |
| Rebalance on join | No (delete + re-place) | Button -> restart, ~20-30 s warm. Similar, **slightly better** |
| Leave | Re-places on Macs that have files; request fails 503 | Process abort -> re-plan -> restart; request fails, agent retries. Same outcome, **we write it** |
| Model files | Every Mac stores full model (21.7 GB each) | Only the head holds the GGUF; workers hold cached tensors (optional). **Gain** |
| Windows/Linux node | Refused | rpc-server on Windows CPU/CUDA. **Gain (roadmap story)** |
| Routing between copies | `candidate_instances()` across copies | One llama-server; `--parallel` slots. **Lose** multi-copy routing (not needed for one agent) |
| Tool calls / thinking | mlx-lm `qwen3_coder` parser, `enable_thinking` | Bundled template + Qwen3-Coder parser, `enable_thinking`. Equal |
| Dashboard data | `/state` with layer map | `/slots`, `/metrics`, plan + load log. Equal after ~80 lines |
| Network per token | Ring, ~4 waits | Star, 1 RTT per remote, 2 RTT for 3 Macs (cppcode-01). About equal |
| Security | None | rpc-server none ("never on an open network"). Equal; our router isolates both |
| Code we own | ~0 (adapter only) | ~430 lines + restart testing. **Lose ~6 h** |
| Open model risk | Hybrid garbling in pipeline (#2264) | Nemotron-H SSM ops over RPC untested. Both need the day-1 test |
