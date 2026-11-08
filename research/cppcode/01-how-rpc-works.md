# cppcode 01: How llama.cpp RPC works, mechanically

Worker `cppcode`, pass B (bottom-up from the source). Sat Nov 7 2026.
Source: `Desktop\hackathon\llama-src`, commit `e9a2a44` (Nov 7 2026 15:03 PST). All file:line refs are to that commit.
Abbreviation: `RPC.cpp` = `ggml/src/ggml-rpc/ggml-rpc.cpp` (3365 lines), `BE.cpp` = `ggml/src/ggml-backend.cpp`.
Partner `cppneed` 01 was read first; reconciliation is at the end.

## 1. The pieces

| Piece | File | Role |
|---|---|---|
| RPC backend (client side) | `RPC.cpp` | Makes each remote machine look like a local ggml device named `RPC<dev>[host:port]` (`RPC.cpp:1327`) |
| RPC server | `RPC.cpp:1396-2600` (`rpc_server` class), `rpc_serve_client` `RPC.cpp:2602`, accept loop `RPC.cpp:2930-2999` | Runs ggml ops on its local backend (Metal on a Mac) on behalf of one client |
| `rpc-server` binary | `tools/rpc/rpc-server.cpp` (252 lines) | CLI wrapper: `-H` host (default `127.0.0.1`, `:79`), `-p` port (default 50052, `:80`), `-t` threads, `-d` devices, `-c` cache (`:90-141`); prints a warning if host != 127.0.0.1 (`:208-211`); cache dir = `$LLAMA_CACHE` or `~/Library/Caches/llama.cpp/rpc` on macOS (`:48-66`, `:227`) |
| Transport | `ggml/src/ggml-rpc/transport.cpp` (814 lines), `transport-apple.cpp` (486 lines) | Plain TCP with `TCP_NODELAY` (`transport.cpp:703-722`, `:762`). Optional RDMA (Linux ibverbs, or Apple RDMA-over-Thunderbolt per Apple TN3205), auto-negotiated in HELLO (`RPC.cpp:427-447`), compiled in only if `librdma`/`libibverbs` is found (`ggml/src/ggml-rpc/CMakeLists.txt:13-27`) |
| Scheduler | `BE.cpp` (`ggml_backend_sched_*`) | Splits one model graph into per-device sub-graphs ("splits") and copies tensors between devices |
| Layer placement | `src/llama-model.cpp:1733-1789` | Maps layer index to device using `--tensor-split` or free memory |

## 2. The wire protocol

- Version 8.0.1 (`ggml/include/ggml-rpc.h:9-11`). Client refuses a server with a different major or higher minor
  (`RPC.cpp:435`).
- Request: `| cmd (1 byte) | size (8 bytes) | payload |` (`RPC.cpp:375-389`).
  Response, only for commands that have one: `| size (8 bytes) | payload |` (`RPC.cpp:391-407`).
- No authentication, no encryption, no request IDs. First message must be HELLO (`RPC.cpp:2606-2611`).
- 25 commands (`RPC.cpp:57-83`). The ones that matter:

| Command | Response? | Used when |
|---|---|---|
| HELLO (=14, fixed) | yes | connect; exchanges version and transport caps (24 bytes, `transport.h:11`) |
| DEVICE_COUNT, GET_DEVICE_MEMORY, GET_ALIGNMENT, GET_MAX_SIZE, GET_ALLOC_SIZE | yes | load: discover remote device |
| ALLOC_BUFFER / FREE_BUFFER / BUFFER_GET_BASE / BUFFER_CLEAR | yes / no | load: allocate weight buffer and compute buffer on the remote |
| INIT_TENSOR | no | load |
| SET_TENSOR_HASH | yes | load: "do you already have bytes with this FNV-1a hash?" |
| SET_TENSOR | **no** | load (weights) and per token (activations, inputs) |
| GET_TENSOR | yes | per token: read an activation back |
| COPY_TENSOR | yes | copy inside one server only |
| GRAPH_COMPUTE | **no** | first run of a sub-graph: sends the whole serialized sub-graph |
| GRAPH_RECOMPUTE | **no** | later runs of the same sub-graph: sends 12 bytes (device + uid) |
| COMM_INIT / COMM_ALLREDUCE / COMM_FREE | yes | tensor-parallel mode only (`--split-mode tensor`), servers talk to each other directly. Not used in layer split |

- Each tensor on the wire is a packed `rpc_tensor` struct: id, type, buffer, shape `ne[4]`, strides `nb[4]`, op,
  op_params, flags, src ids, view info, remote data pointer, 64-byte name (`RPC.cpp:36-52`). Roughly 300 bytes
  (my count from GGML_MAX_* constants; unverified exact size).
- Client side: one `rpc_dispatcher` per endpoint (`RPC.cpp:711-730`), one background thread that drains a message
  queue in order onto one socket (`RPC.cpp:663-700`). Commands with no response are fire-and-forget: the caller
  returns as soon as the bytes are queued (`send_async`, `RPC.cpp:577-585`). `synchronize()` only waits for the local
  queue to drain (`RPC.cpp:624-630`); it is not a network round trip. Optional busy-spin to cut wakeup latency
  (`RPC.cpp:664-677`).
- Server side: compute is async (`ggml_backend_graph_compute_async`, `RPC.cpp:2205`); any command that reads data
  first calls `sync_all_backends()` (`RPC.cpp:2236-2240`, used in `get_tensor` `RPC.cpp:1943`). So a GET_TENSOR
  response doubles as "compute finished".
- The server handles **one client at a time**: `accept` then `rpc_serve_client` runs to completion before the next
  `accept` (`RPC.cpp:2985-2993`). Two llama-server heads cannot share one rpc-server.

## 3. What crosses the network at load

1. The **head machine must have the whole GGUF**. It reads every weight from its disk and pushes the bytes over the
   socket with SET_TENSOR (`ggml_backend_rpc_buffer_set_tensor`, `RPC.cpp:852-876`). Workers need no model file.
2. With `rpc-server -c`, for each weight tensor > 10 MiB (`HASH_THRESHOLD`, `RPC.cpp:118`; only weight buffers,
   `rpc_use_hash_cache` `RPC.cpp:847-849`) the client first:
   - computes FNV-1a 64-bit over the tensor bytes, one byte at a time (`fnv_hash`, `RPC.cpp:323-331`);
   - sends SET_TENSOR_HASH and **waits** for the answer (synchronous, `RPC.cpp:863`, comment "TODO: make this async");
   - hit: nothing else sent. Miss: sends the data with `cache_flag=1` and the server writes it to
     `<cache_dir>/<hash hex>` (`RPC.cpp:1762-1773`). Lookup reads that file back into memory (`RPC.cpp:1838-1856`).
   - Tensors <= 10 MiB are always resent. Hashing still reads the full GGUF on the head on every restart.
3. So a warm restart costs: head reads ~18-26 GB from SSD + byte-wise FNV over the remote share + 1 round trip per
   large tensor + worker reads its cached files from disk into Metal memory. A cold load sends the remote share of
   the GGUF over Wi-Fi. (Timing not measured yet; byte-wise FNV is likely ~1 GB/s per core, a guess.)

## 4. How the graph is split across local Metal + remote RPC devices

### Layer to device (`--tensor-split`)

- Device order: RPC servers are inserted **at the front** of the device list, in `--rpc` order, local GPU last
  (`src/llama.cpp:275-276`, comment "to minimize network transfers").
- Split weights: `--tensor-split` values in that device order; if absent or all zero, each device's free memory as
  reported (`src/llama-model.cpp:1734-1753`). For an RPC device "free memory" is what the remote reports via
  GET_DEVICE_MEMORY.
- Cumulative fractions; layer `il` goes to the first device whose cumulative fraction exceeds `il / n_layers`
  (`std::upper_bound`, `src/llama-model.cpp:1776`). Contiguous blocks of layers per device.
- The token-embedding (input) layer always stays on the head's CPU (`src/llama-model.cpp:1784-1786`). The output
  layer (norm + LM head) is treated as layer `n_layer` and lands on the last device, the head's own Metal GPU
  (`src/llama-model.cpp:1793-1794`).
- For our 3 Macs, `--rpc A,B` on head C gives: CPU(embed) -> A (first block) -> B (second block) -> C Metal (last
  block + output). Nemotron's 52 layers would be cut by layer index only; the cut does not know Mamba vs MoE vs
  attention layers (their costs differ; relevant to section 3).

### Graph to splits (`ggml_backend_sched`)

- Every op is assigned the backend that holds its weight; consecutive ops on the same backend form one split. Each
  split gets a uid (`BE.cpp:2017-2020`).
- `compute_splits` (`BE.cpp:2199-2284`), per split in order:
  1. copy the split's inputs to its backend (`ggml_backend_sched_copy_input`, `BE.cpp` ~`:2100-2150`);
  2. launch the split's graph async (`BE.cpp:2236`);
  3. record an event.
- Copy between two RPC servers: `ggml_backend_rpc_buffer_cpy_tensor` only works inside one server; for different
  servers it returns false (`RPC.cpp:933-944`, log "falling back to a host copy"). The fallback
  `ggml_backend_tensor_copy` (`BE.cpp:832-853`) does GET_TENSOR from server A to the head, then SET_TENSOR to
  server B. **Activations always bounce through the head; servers never talk to each other in layer mode.**
- Graph caching: the first time a split runs, the client serializes it (`serialize_graph`, `RPC.cpp:1219-1248`,
  ~300 bytes x tensors in the split, so tens to hundreds of KB) and sends GRAPH_COMPUTE. Later runs with the same uid
  send GRAPH_RECOMPUTE, 12 bytes (`RPC.cpp:1250-1277`). Cache holds up to 1024 graphs per device on both sides
  (`RPC.cpp:122`, `:2159-2164`). llama.cpp reuses the graph between decode steps when shapes match
  (`src/llama-context.cpp:1692`), so steady-state decode sends only GRAPH_RECOMPUTE.
- Pipeline parallelism (`n_copies = 4`, `BE.cpp:1171-1172`, `:2304`) is on when >1 device, `-ngl` > n_layer,
  layer split, no tensor overrides, all non-CPU devices support async + events (`src/llama-context.cpp:581-607`).
  RPC devices report async=true, events=true (`RPC.cpp:3031-3036`). This overlaps successive ubatches during
  prompt processing; it does not help a single decode token.

## 5. Per token: round trips and payloads (decode, 3 Macs, head = C)

Sequence for one decode step (from `BE.cpp:2199-2284` + `RPC.cpp` handlers):

| Step | Network | Blocking? |
|---|---|---|
| Head CPU computes embedding (1 row) | none | - |
| SET_TENSOR embedding -> A, plus per-split user inputs (positions, masks, recurrent-state copy indices) | head -> A | no (fire and forget) |
| GRAPH_RECOMPUTE -> A (12 B) | head -> A | no |
| GET_TENSOR A's last hidden state | A -> head | **yes: round trip 1**, also waits for A's compute (`RPC.cpp:1943`) |
| SET_TENSOR hidden -> B, inputs, GRAPH_RECOMPUTE -> B | head -> B | no |
| GET_TENSOR B's last hidden state | B -> head | **yes: round trip 2**, waits for B's compute |
| Head Metal runs last block + LM head + sampling | none | - |

- **Blocking network waits per token = number of remote devices** (here 2). Each wait is one RTT plus that device's
  compute. Total per token ~ `2 x RTT + t_A + t_B + t_C`; compute is strictly serial (each Mac idle ~2/3 of the time
  for one stream).
- Payload per hop (decode): one hidden-state row in F32 = `n_embd x 4` bytes. For Nemotron-3-Nano n_embd = 2688
  (from model card memory, **unverified**, check GGUF in section 2) -> ~10.5 KB per hop, plus a few small inputs and
  12-byte recompute commands. Bandwidth is irrelevant for decode; latency (RTT + wakeups) is what costs.
- Prefill: same pattern per ubatch (default 512 tokens), payload `n_tokens x n_embd x 4` = ~5.5 MB per hop per
  512-token ubatch. With pipeline parallelism, ubatch k+1 on A overlaps ubatch k on B.
- Hybrid-model caveat: Mamba layers keep recurrent state on the device that owns the layer (to verify in section 2);
  if so, no state crosses the network per token.

### Compared with EXO (from `research/EXO-HOW-IT-WORKS.md:86`)

| | EXO (pipeline, 3 nodes) | llama.cpp RPC layer split (3 Macs, 1 head) |
|---|---|---|
| Sequential network waits per token | ~4 (2 sends + 2 ring steps of the final all-gather) | 2 (one GET per remote device); sends are one-way and pipelined in the TCP stream |
| Topology | ring of peers | star: every hop goes through the head |
| Path of the hidden state | node -> next node | A -> head -> B -> head |
| Messages per remote per token | (see exo notes) | ~3-5 one-way + 1 request/response |

Honest reading: in RTT count llama.cpp's star is ~2 RTT vs EXO's ~4 one-way-ish waits; a star actually costs the same
number of one-way legs as a ring with 2 remotes (head->A, A->head, head->B, B->head = 4 legs) but ring has 3 legs
(C->A->B->C) for the forward pass. The EXO all-gather adds legs on top. Net: comparable order of magnitude; the
difference is a few ms per token on Wi-Fi. Exact numbers need a measurement (`GGML_RPC_DEBUG` level 3 prints per
command ms, `RPC.cpp:681-693`).

## 6. `tools/connect` is not part of multi-machine inference

- `tools/connect/CMakeLists.txt`: compiles nothing; at build time downloads a prebuilt `llama-connect` binary from
  GitHub `ggml-org/llama-connect` (`:6-7`, `:63-73`), and building `llama-server` pulls it too (`:76-78`).
- `ggml-org/llama-connect` (Rust + TS, v0.0.1, 6 stars, checked Nov 7 2026): "Allow web UI to connect to a
  locally-hosted llama-server instance via WebRTC peer-to-peer connection." It proxies chat requests from a browser
  to a local llama-server. Nothing to do with pooling. Irrelevant to us; disable for offline builds
  (it needs internet at build time).

## 7. Other things found on the way

- Tensor-parallel over RPC now exists: `--split-mode tensor` with COMM_INIT/COMM_ALLREDUCE, servers open direct
  peer sockets (`RPC.cpp:251-272`, `:2277-2560`). `llm_arch_supports_sm_tensor` lists `LLM_ARCH_NEMOTRON_H` and
  `LLM_ARCH_NEMOTRON_H_MOE` among the architectures in the block starting at `src/llama-arch.cpp:1435` (lines
  1457-1458), and that list returns `false` (`src/llama-arch.cpp:1474`): **Nemotron-H cannot use tensor mode**
  (confirmed). Either way tensor mode is all-reduce per layer, which over Wi-Fi would be ~2 x 52 RTTs per token: wrong
  for us.
- Apple RDMA over Thunderbolt is built in (`transport-apple.cpp:16-27`): 128 KiB fixed strides, 16-frame ring.
  Needs Thunderbolt 5 cables between Macs; not our Wi-Fi demo, but a real "roadmap" line.

## Reconciliation with cppneed 01

Agree on: device order rpc...,local (`src/llama.cpp:275`); default split by free memory; only the head needs the GGUF;
`-c` cache; tensor/row mode and RDMA are drops; llama-connect is a drop.

Additions / small disagreements:
1. cppneed R2 says one pooled model = one llama-server. Stronger: each rpc-server serves one client connection at a
   time (`RPC.cpp:2985-2993`), so a second head (e.g. a second model, or a hot standby) cannot use the same workers
   at the same moment. Needs separate rpc-server ports/processes, and each one would allocate its own memory.
2. cppneed R4 restart cost: besides the head re-reading the GGUF, the head hashes every large tensor with byte-wise
   FNV and does one synchronous round trip per tensor > 10 MiB (`RPC.cpp:323-331`, `:863`). Small tensors (<= 10 MiB)
   are always resent.
3. cppneed drop list for `--split-mode tensor`: agree, and Nemotron-H is excluded from it in code anyway
   (`src/llama-arch.cpp:1457-1474`; `src/llama-model.cpp:398` rejects it).
