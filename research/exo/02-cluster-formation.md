# 02 Cluster formation (EXO 1.0)

Commit studied: `a60b956` (2026-11-06). Paths relative to `C:\Users\aayoa\Desktop\hackathon\exo-src`.
Roadmap note (Dell): a Mac+Linux mix is refused by `build_id()` (`src/exo/routing/wire.py` line 67); setting the same
`EXO_BUILD_ID` on both sides bypasses the check, but whether mixed Metal/CPU ranks then work is unverified. Not pursued.

## Summary

| Layer | What | Where |
|---|---|---|
| Discovery | UDP Hello/WhatsUp on **IPv6 link-local multicast group `ff12::e0a1:de89`, UDP port 52413**, joined on every interface | `rust/networking/src/discovery.rs` line 24 (GROUP), lines 72 to 135 (socket, join per interface), `announce()` line 323, `respond()` line 189 |
| Gate | 8-byte cluster key in each Hello: blake3 prefix of namespace + wire version + build id. Mismatch = "device on another version", no connect | `rust/networking/src/wire.rs` (`ClusterKey::new` ~line 93) |
| Session | Zenoh 1.9 (forked), `mode=router`, listens `tcp/[::]:52414`, Zenoh's own multicast scouting OFF, gossip multihop ON | `rust/networking/src/lib.rs` `cfg()` |
| Dial | On each discovered peer, the node with the **larger** zid dials the smaller one over TCP to the address the Hello came from | `lib.rs` `open()` loop (`if discovered.zid > runtime.zid() { continue }`) |
| Membership | Zenoh liveliness tokens `live/<zid>` + probes; lease 10 s, probe timeout 2 s, stall 5 s, hold-off 12 s, forget after 5 min | `rust/networking/src/membership.rs` lines 64 to 161, `swarm.rs` ~line 300 |
| Pub/sub | 7 JSON topics over Zenoh: global_events, local_events, state_snapshots, commands, election_messages (high priority), membership (local only), download_commands | `src/exo/routing/topics.py`, `router.py` |
| Master | Bully-style election: (clock, seniority, node id); timeout 3 s, heartbeat 5 s, master silence 30 s, retries 15 to 300 s. `--force-master` = seniority 1,000,000 | `src/exo/shared/election.py` lines 20 to 71, `src/exo/main.py` ~205 |
| State | Event-sourced `State` (master orders events, broadcasts snapshots); includes topology, node memory, network, thunderbolt, instances, runners | `src/exo/shared/types/state.py`, `src/exo/master/main.py` |
| Topology | `rustworkx.PyDiGraph[NodeId, SocketConnection or RDMAConnection]`; socket edges found by each worker doing `GET http://<ip>:52415/node_id` on every IP of every node every 10 s | `src/exo/shared/topology.py` line 33; `src/exo/worker/main.py` `_poll_connection_updates` ~line 546; `src/exo/utils/info_gatherer/net_profile.py` |

## Identity

- Node id = Zenoh id, **random each start**: `get_node_zid()` returns `os.urandom(16).hex()` (`src/exo/routing/router.py`
  line 288). Persistence is commented out: "TODO(evan): bring back node id persistence once we figure out how to deal
  with duplicates". So a restarted Mac is a new node; placements re-plan.
- No authentication or encryption between nodes: plain TCP Zenoh, no TLS config in `cfg()`. Anyone on the LAN with
  the same namespace + build joins. The only isolation is `--namespace` (`EXO_ZENOH_NAMESPACE`), which is a hash
  prefix, not a secret. Relevant to the "private" pitch: mention, or set a non-default namespace for the demo.

## Joining, as a user sees it

1. Install the same exo build on each Mac (app or `uv run exo`), same network.
2. Within a few seconds of Hello exchange, nodes connect; one wins the election; dashboard at `http://<any-node>:52415`
   shows all nodes. Wrong build shows up at `GET /v1/devices-on-other-versions` and in logs, not as a member.
3. macOS asks for **Local Network** permission; without it a process "can't send multicast at all"
   (comment in `discovery.rs` lines 36 to 43). Click Allow on every Mac, or discovery silently fails.

## Transports

- Control plane: Zenoh over TCP 52414 (IPv6 link-local in practice, since peers dial the Hello source address).
- Request serving: HTTP on 52415 (each node's API/cluster routes; as of #2597 a request streams from the device that
  runs it).
- Tensor traffic between ranks: MLX distributed, `ring` (TCP) or `jaccl` (RDMA over Thunderbolt 5), set up per
  instance (`src/exo/worker/engines/mlx/utils_mlx.py` lines 133, 156). Detail in section 3.
- Thunderbolt: the macOS app's `io.exo.networksetup` service gives each TB port its own network service and disables
  Thunderbolt Bridge; topology has `get_thunderbolt_bridge_cycles` / `get_rdma_cycles` (`src/exo/shared/topology.py`
  lines 226 to 274).

## Venue Wi-Fi: what breaks and the fallbacks

What discovery needs:
- IPv6 enabled with link-local addresses on the interface (default on macOS).
- Multicast to `ff12::e0a1:de89` UDP 52413 delivered between clients (ff12 = link-local scope, so it never crosses a
  router anyway).
- Unicast TCP 52414 and HTTP 52415 between clients (client isolation on campus/guest Wi-Fi blocks this even if
  multicast passes).
- macOS Local Network permission granted.

Static-peer fallbacks at this commit:
- `--bootstrap-peers` / `EXO_BOOTSTRAP_PEERS`: **disabled**. `main.py` line 490: `raise ValueError("Bootstrap peers has
  been temporarily removed")`; flag hidden from help (line 639).
- `EXO_TESTKIT_DISCOVERY_ANNOUNCE_TO` (`ANNOUNCE_TO_ENV`, `discovery.rs` line 43): sends Hellos to listed addresses
  instead of multicast, but `parse_announce_to` (line 380) **keeps only IPv6 loopback**, so it cannot reach another Mac.
- `ZENOH_CONNECT` env: no effect (issue #2257, Aug 2026, which also confirms the bootstrap-peers error).
- Related open reports: #2257 (Hellos seen in tcpdump, but no TCP session over a multi-homed IPv6 link-local link,
  Linux), #2271 (two Ubuntu nodes on one LAN don't discover each other; links #520, #1471, #1682). Both Linux;
  Macs are Tier 1, but the failure class is "silent, no errors".

Patch options if multicast is blocked (requires source builds on every Mac, since `exo_rs` is compiled Rust; unverified):
1. Zenoh static connect: in `rust/networking/src/lib.rs` `cfg()`, add
   `cfg.insert_json5("connect/endpoints", <from env, e.g. ["tcp/192.168.8.10:52414"]>)`. Membership runs on Zenoh
   liveliness, and gossip multihop is already on, so one seed node may be enough. This bypasses the version gate
   (it lives only in the Hello), so pin one build manually.
2. Unicast Hellos: drop the loopback-only filter in `parse_announce_to` and list each Mac's address. Keeps the version
   gate. Socket is IPv6; IPv4 peers would need v4-mapped addresses (unverified on macOS).
3. No patch: our own travel router/switch + USB-C Ethernet, Wi-Fi off on the Macs (avoids multi-homing), Local Network
   allowed. This is the plan; patches are a backup.

Test on arrival: run exo on two Macs, check `GET http://localhost:52415/membership` and `/state` list both nodes;
if not, `sudo tcpdump -i en0 udp port 52413` to see if Hellos arrive.

## EXO as a black-box pooling engine behind our router

Yes, the HTTP API on port 52415 (any node) is enough (`src/exo/api/main.py` lines 546 to 622, `docs/api.md`):
- Inference: `POST /v1/chat/completions` (OpenAI), `/v1/responses`, `/v1/messages` (Claude), `/ollama/api/chat` and
  other Ollama routes, `GET /v1/models`. Streaming, `usage` returned, `POST /v1/cancel/{command_id}`.
- Placement: `GET /instance/previews?model_id=` (all candidate placements, with per-node memory delta),
  `GET /instance/placement` (sharding Pipeline or Tensor, instance_meta MlxRing or RDMA, `min_nodes`),
  `POST /place_instance`, `POST /instance` with `constraints: {min_nodes, pinned_nodes:[NodeId]}`
  (`src/exo/api/types/api.py` lines 272 to 290), `GET /instance/await?model_id=` (SSE until ready),
  `DELETE /instance/{id}`. Changes ack once in state, 503 after about 45 s.
- Observability for our dashboard: `GET /state` (topology, node memory, instances, runners), `/membership`, `/events`,
  `/node_id`, `/v1/traces`.
- **EXO already routes**: `candidate_instances()` in `src/exo/shared/serving.py` line 110 picks, among ready
  instances of the requested model, the one with fewest requests in flight from this API node, then not-busy runners,
  then random. So "route + pool" can be done inside one exo cluster: several single-Mac instances of a small model
  (routed) plus one multi-Mac instance of a big model (pooled), chosen by the `model` field. Caveat from the
  docstring: "several API nodes balance less evenly than one router would", so send all traffic to one API node.
- Open for section 3/4: whether one Mac can hold a single-node instance and a shard of the pooled instance at once
  (memory check in placement), and how fast an instance comes back after a node drops.
