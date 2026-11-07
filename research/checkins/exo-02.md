## Check-in exo 02: Cluster formation

**Found (most important first, max 8 bullets, each with a source):**
- **Discovery uses IPv6 link-local multicast only.** Hellos go to group `ff12::e0a1:de89` on UDP port 52413, joined on every interface (`rust/networking/src/discovery.rs` line 24 and lines 72 to 135). After discovery, peers connect over Zenoh TCP on port 52414 to the address the Hello came from. The node with the larger id dials the other (`rust/networking/src/lib.rs`, `open()`). Each worker then probes `GET http://<ip>:52415/node_id` every 10 s to build topology edges (`src/exo/worker/main.py` around line 546).
- **There is no working static-peer fallback at this commit.** `--bootstrap-peers` raises "Bootstrap peers has been temporarily removed" (`src/exo/main.py` line 490). `EXO_TESTKIT_DISCOVERY_ANNOUNCE_TO` accepts only IPv6 loopback addresses (`discovery.rs` lines 43 and 380). `ZENOH_CONNECT` has no effect (issue #2257, Aug 2026). If venue Wi-Fi blocks multicast or isolates clients, EXO will not form a cluster.
- **A backup patch is small but unverified.** Adding `connect/endpoints` from an env var in `cfg()` (`rust/networking/src/lib.rs`) is about 5 lines. Membership runs on Zenoh liveliness, and gossip multihop is already on. The catch: it needs a source build with Rust on every Mac, because the app/DMG can't be patched.
- **macOS Local Network permission is required.** Without it, a process "can't send multicast at all" (`discovery.rs` comment, lines 36 to 43). Click Allow on every Mac during setup.
- **EXO works as a black box behind our router.** Any node serves the full API on port 52415:
  - inference: `/v1/chat/completions`, `/v1/responses`, `/v1/messages` and Ollama routes;
  - placement: `/instance/previews`, `/place_instance`, and `POST /instance` with `constraints.pinned_nodes`;
  - status: `/state`, `/membership` and `/events`, for our dashboard.
  Sources: `src/exo/api/main.py` lines 546 to 622, `docs/api.md`, `src/exo/api/types/api.py` lines 272 to 290.
- **EXO already does request routing.** `candidate_instances()` (`src/exo/shared/serving.py` line 110) sends each request to the ready instance of that model with the fewest requests in flight, then to a non-busy one, then picks at random. One exo cluster can therefore run single-Mac instances of a small model (routed) plus one multi-Mac instance of a big model (pooled), and the `model` field picks which. All traffic should go to one API node, because each node balances only its own requests.
- **There is no node-to-node auth or encryption.** Zenoh runs over plain TCP, and node ids are random on every start (`router.py` line 288; persistence is a TODO). `--namespace` is a hash prefix, not a secret. Any Mac on the LAN with the same build joins. That gap matters for the "private" pitch.
- **Master election** is bully-style on (clock, seniority, id): 3 s election timeout, 5 s heartbeat, 30 s master-silence timeout (`src/exo/shared/election.py` lines 20 to 71). Pin the master with `--force-master`. State is event-sourced and spread over 7 Zenoh topics (`src/exo/routing/topics.py`).

**Surprises / things that change the plan:**
- Bringing our own router or switch is now required for an EXO demo, not just recommended, because EXO has no static-peer fallback today.
- Our "router" may not need to route. EXO's least-in-flight selection across instances could cover the "route" must-have. Our own layer could shrink to the UI, policy and audit log, plus the roadmap story for Windows machines through llama.cpp.

**PRD suggestions:**
- Demo hardware > Network: add "EXO discovery = IPv6 link-local multicast `ff12::e0a1:de89` UDP 52413, plus TCP 52414 and HTTP 52415 between Macs. No static-peer option (`--bootstrap-peers` disabled). Use Ethernet on our own switch with Wi-Fi off, and allow macOS Local Network access."
- Must have (Route): add "candidate: EXO itself (one cluster, several single-Mac instances of the small model, least-in-flight routing in `shared/serving.py`), with our router as a thin layer or fallback."
- Later: add "node authentication and encryption (EXO has none: plain Zenoh TCP, random node ids)."
- Open question 2: add "EXO is usable as a black box over HTTP (OpenAI API plus placement and state endpoints)."

**Questions for Aayo:**
1. Is someone confirmed to bring a router or switch and USB-C Ethernet adapters? If not, I should test whether the Zenoh `connect/endpoints` patch builds on a Mac, as a backup.
2. Should "route" use EXO's built-in routing (less code, Macs only) or our own router (more control, and the same router can front Windows/llama.cpp in the roadmap story)? This sets what I stress in section 4.

**Next section:** Section 3, splitting the model: the partition algorithm in `master/placement.py` and `placement_utils.py`, pipeline vs tensor sharding (`sharding/pipeline.py`, `tensor.py`, `auto_parallel.py`), and how activations move between ranks (`ring` vs `jaccl`). I will add three items: whether one Mac can hold a single-node instance and a pooled shard at the same time; which MoE models (Qwen3-30B-A3B and similar) EXO's model registry supports; and the memory each model needs per node.
