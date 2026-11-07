# How EXO works (functionally)

EXO 1.0 is open-source software that pools Apple Silicon Macs on one local network into a single AI server. One model
too big for any one Mac is split by layers across several Macs, and any Mac answers requests through an
OpenAI-compatible API. This file summarizes the `exo` research session's notes (`research/exo/01` to `06`) as one
functional walkthrough.

- **Code studied:** exo-explore/exo main at commit `a60b956` (Nov 6 2026), cloned to `Desktop\hackathon\exo-src`.
  File paths below are relative to that folder.
- **Version note:** "EXO" today means exo 1.0, a full rewrite. Most Reddit and Hacker News posts describe the 2024
  version (tinygrad, gRPC, Linux/NVIDIA), which no longer exists.

## The five parts

| Part | What it does | Built with | Main code |
|---|---|---|---|
| 1. Discovery | Finds other Macs on the network and checks they run the same build | Rust, IPv6 multicast | `rust/networking/src/discovery.rs`, `wire.rs` |
| 2. Cluster | Connects the Macs, elects a leader ("master"), keeps one shared view of the cluster | Rust + Zenoh (pub/sub library), Python | `rust/networking/src/lib.rs`, `membership.rs`, `src/exo/shared/election.py` |
| 3. Placement | Decides which Macs run a model and which layers each one holds | Python | `src/exo/master/placement.py`, `placement_utils.py` |
| 4. Inference | Runs the model's layers on each Mac and passes the token's state between Macs | MLX (Apple's ML library), Metal GPU | `src/exo/worker/engines/mlx/sharding/pipeline.py` |
| 5. API and routing | Accepts chat requests on any Mac and sends each to the least-busy copy of the model | Python HTTP server | `src/exo/api/main.py`, `src/exo/shared/serving.py` |

A Swift menu-bar app and a SvelteKit dashboard sit on top. Size: about 116k lines of Python (with tests and model
code), 6.2k lines of Rust, 10.7k of Swift, about 30k in the dashboard.

## 1. Discovery: how Macs find each other

1. Every Mac running exo sends a "Hello" to the IPv6 multicast group `ff12::e0a1:de89` on UDP port 52413, on every
   network interface.
2. Each Hello carries an 8-byte **cluster key**: a hash of the cluster name ("namespace") + wire version + build id.
3. A Mac connects only to Macs with the same key. Different builds show up as "device on another version" and never
   join. This is why every Mac must run the exact same exo build, and why a Mac and a Linux machine never join
   (different MLX builds).
4. macOS must grant exo **Local Network** permission, or multicast silently fails.

What breaks it: venue and guest Wi-Fi that blocks multicast or isolates devices. Our venue test showed 100% packet
loss between Macs. There is **no manual "connect to this IP" fallback** at this commit (`--bootstrap-peers` raises
"temporarily removed"). Fix: our own router.

## 2. Cluster: connection, membership, leader, shared state

- **Connection:** for each discovered peer, the Mac with the larger id dials the other over TCP port 52414 (Zenoh).
- **Membership:** Zenoh "liveliness" tokens. A Mac is considered gone after a 10 s lease; a stuck Mac after 30 s.
- **Identity:** a random id each time exo starts. A restarted Mac counts as a new node.
- **Leader election:** bully-style on (clock, seniority, node id). `--force-master` makes one Mac the leader.
- **Shared state:** event-sourced. The master orders events and broadcasts snapshots over 7 Zenoh topics. The state
  holds the topology (a graph of Macs and links), each Mac's memory, models placed, and runners.
- **Links between Macs:** every 10 s each Mac calls `GET http://<ip>:52415/node_id` on every address of every other
  Mac to learn which network paths work.
- **Security:** none. No authentication or encryption between Macs. Any Mac on the network with the same cluster name
  and build joins. The cluster name is a label, not a secret.

## 3. Placement: who holds which layers

Triggered by `POST /place_instance` (or previewed with `GET /instance/previews`):

1. **Usable memory per Mac** = `min(free RAM, Metal GPU budget)`. The GPU budget is about 2/3 to 3/4 of RAM.
   `OVERRIDE_MEMORY_MB` on a Mac replaces its free-RAM number (how much it lends to the pool).
2. **Candidate groups:** cycles in the topology graph, filtered by `min_nodes` and pinned nodes.
3. **Memory check:** a group's total usable memory must be at least the model size. No extra room is reserved for
   the KV cache.
4. **Fewest Macs wins:** EXO keeps only the smallest groups that fit. A model that fits on 2 Macs goes on 2 unless
   `min_nodes=3`.
5. **Layer split (pipeline):** layers in proportion to usable memory, at least 1 layer per Mac. Speed is ignored
   (`# TODO: flops_fp16`), so a slow Mac gets the same per-GB share as a fast one.
6. **Deployment:** the placed model is kept as a deployment. If a Mac drops, the master re-places it only on Macs
   that already have the files. With `min_nodes=3` it waits up to 1 hour for the missing Mac rather than shrinking.
   After 3 crashes in 10 minutes it pauses.
7. **No rebalance on join:** a new Mac gets no layers until the model is deleted and placed again.

Example, Nemotron-3-Nano-30B-A3B 5-bit (21.7 GB, 52 layers) on M5 24 GB / M3 16 GB / M5 24 GB with apps open
(budgets 12 / 7 / 12 GB): layers 20 / 12 / 20, about 7.7 / 5.5 / 8.5 GB per Mac.

## 4. Inference: what happens for one token

Pipeline mode (the one we use):

```
Mac A: layers 1-20  ->  Mac B: layers 21-32  ->  Mac C: layers 33-52  ->  every Mac gets the result  ->  next token
```

- Each Mac runs its layer range with MLX on its GPU.
- Between Macs, only the token's hidden state moves: one vector of 2,688 numbers, about 5 KB. The model weights never
  move after loading.
- Transport: MLX distributed `ring` backend over TCP. EXO picks the link in this order: Thunderbolt > Ethernet > Wi-Fi.
- Per token there are about 4 network waits in a row (2 sends + 2 ring steps of the final all-gather), and the
  network and GPU work don't overlap. So **latency per hop** sets the network cost, not bandwidth.
- Tensor mode (every Mac does part of every layer) needs Thunderbolt 5 with RDMA. Our MacBooks don't have it.
- Nemotron specifics: 52 layers = 23 Mamba + 23 MoE + 6 attention. Mamba state stays on the Mac that owns the layer.
  `fix_pipeline_shard` patches shards with no attention layer (fixed in #2515, Nov 1). Memory for context is tiny:
  about 6 KB per token plus 48 MB of Mamba state per conversation.
- Open risk: PR #2264 reports garbled output for hybrid models in pipeline mode. Day-1 check: same prompt at
  temperature 0 on 3 Macs vs 1 Mac.

## 5. API and routing: what a request does

1. A client sends `POST /v1/chat/completions` to any Mac on port 52415.
2. `candidate_instances()` picks a ready copy of the requested `model`: fewest requests in flight from this Mac,
   then idle runners, then random. **This is the routing.** One cluster can hold small single-Mac copies (routed)
   and one big multi-Mac copy (pooled); the `model` field picks which.
3. The request goes to the first Mac of that copy (`POST /_exo/serve`), which hands it to the other Macs
   (`/_exo/deliver`).
4. Tokens stream back as NDJSON and become OpenAI-style streaming chunks. Closing the stream cancels on every Mac.
5. The master is not in the request path (#2597, Nov 6), so a leader change doesn't break a chat.

If a Mac in the copy leaves mid-answer, the request fails with `device_unreachable` (HTTP 503). There is no partial
recovery; the client must re-send.

API endpoints we use:

| Purpose | Endpoint |
|---|---|
| Chat (with `tools`, `enable_thinking`, streaming) | `POST /v1/chat/completions` |
| Preview where a model would go | `GET /instance/previews?model_id=...` |
| Place a model | `POST /place_instance` (`model_id`, `sharding`, `instance_meta`, `min_nodes`) |
| Wait until loaded | `GET /instance/await?model_id=...` (server-sent events) |
| Remove a model (for Rebalance) | `DELETE /instance/{id}` |
| Cluster state, layer map, memory | `GET /state`, `/membership`, `/node_id` |
| Download a model | `POST /download/start` |

Tool calls: EXO passes `tools` to the model and returns OpenAI `tool_calls`. Nemotron 3 Nano and 3.5 Lightning use
the `qwen3_coder` parser in mlx-lm. `tool_choice` is ignored. Nemotron-Nano-9B-v2 has no tool parser. Untested in
EXO: try one tool call on day 1.

Thinking: `enable_thinking` true/false works for both 30B Nemotron models; reasoning streams as `reasoning_content`.

## Models and disk

- Model cards live in `resources/inference_model_cards/` (built-in list with sizes and layer counts).
- **Every Mac downloads the whole model**, not just its layers. Cache: `~/.exo/models/<org>--<name>/`. A hand-copied
  folder counts as complete. `--offline` / `EXO_OFFLINE=true` stops internet access. No Mac-to-Mac model transfer.

## Install options

| Option | Nemotron | Notes |
|---|---|---|
| App v1.0.71 (Apr 23 2026) | Works for our 2-way and 3-way splits (code reading, not run) | libp2p networking instead of Zenoh; no Nov fixes |
| Source build of main `a60b956` | Best support (fixes from #2515, #2597) | Normally compiles MLX with full Xcode. Avoid that by using the PyPI MLX 0.32.4 wheel (identical to the pinned git commit); steps in `research/exo/06-verdict-and-bringup.md` T1-T2 |

All 3 Macs must use the same option. App and source builds don't join each other.

## Limits that matter to us

- Mac only. No Windows, and Mac+Linux mixes are refused. The "old office PCs" story can't run on EXO.
- No authentication or encryption between Macs.
- Needs multicast; no manual peer list.
- Placement ignores speed; no rebalance on join; a Mac leaving fails the request.
- Every Mac stores the full model.

## Full notes

| File | Covers |
|---|---|
| `research/exo/01-map-the-repo.md` | Layout, languages, versions, platforms |
| `research/exo/02-cluster-formation.md` | Discovery, membership, election, Wi-Fi failure modes |
| `research/exo/03-splitting-the-model.md` | Placement algorithm, pipeline vs tensor, disk, memory |
| `research/exo/04-serving-and-ux.md` | API, request flow, failures, downloads, dashboard, join |
| `research/exo/05-nemotron-on-exo.md` | Nemotron layers, memory per Mac, tool calls, thinking |
| `research/exo/06-verdict-and-bringup.md` | Verdict, copy-paste install (T1-T9), API contract for Spacetime |
