# 04 Serving and UX (EXO 1.0)

Commit `a60b956` (2026-11-06). Paths relative to `C:\Users\aayoa\Desktop\hackathon\exo-src`.

## API surface (port 52415 on every node)

- OpenAI-compatible: `POST /v1/chat/completions` (stream or not, `usage` returned), `/v1/responses`, Claude
  `/v1/messages`, Ollama `/ollama/api/chat|generate|tags|show|ps`, `GET /v1/models`, `POST /v1/cancel/{command_id}`.
  Route table: `src/exo/api/main.py` lines 546 to 622. Docs: `docs/api.md`.
- Instances: `GET /instance/previews?model_id=`, `GET /instance/placement`, `POST /place_instance`
  (`{model_id, sharding, instance_meta, min_nodes}`), `POST /instance` (`{instance, constraints:{min_nodes,
  pinned_nodes}}`), `GET /instance/await?model_id=&timeout_seconds=` (SSE until ready), `DELETE /instance/{id}`.
- State: `GET /state`, `/state/{path}` (e.g. `/state/topology`, `/state/downloads`), `/membership`, `/events`,
  `/node_id`, `/v1/traces*`.
- Downloads: `POST /download/start`, `/download/cancel`, `DELETE /download/{node_id}/{model_id}`.
- Internal, cluster-only: `/_exo/serve`, `/_exo/deliver`, `/_exo/serving` (`src/exo/worker/serving.py` lines 59
  and 60, `src/exo/api/cluster_routes.py`). Callers are checked with `is_member_address` (IP is a cluster member),
  `cluster_routes.py` line 61.

## Request flow, end to end (after #2597, 2026-11-06)

1. Client → any node's `POST /v1/chat/completions` (`api/main.py` line 1275). The chat adapter
   (`api/adapters/chat_completions.py`) converts the request to a `TextGenerationTaskParams`.
2. `_try_instances()` (line 1089) → `candidate_instances()` (`src/exo/shared/serving.py` line 110): ready instances of
   that `model`, reachable from this API node, sorted by fewest in-flight requests from this API node, then
   not-busy runners, then random. This is the routing.
3. `ServingClient.open()` (`src/exo/api/serving_client.py` line 179) → `POST /_exo/serve` on the instance's output
   device (rank 0 for text).
4. The output device hands the task to every rank (its own runner; others via `POST /_exo/deliver`), answers
   Accepted once all ranks hold it, then streams tokens back as NDJSON on the same HTTP response
   (`worker/serving.py` `serve()` line 219, `deliver()` line 539).
5. Runner process (`src/exo/worker/runner/*`, one subprocess per instance per node) runs MLX; ranks exchange
   hidden states over the MLX `ring` (section 3).
6. The API node converts the stream to OpenAI SSE chunks. Closing the HTTP stream cancels on every rank.

The master is out of the request path: "no request, token or request input enters the cluster's log, so a frozen,
dead or changing master ... no longer stops or ends a chat" (commit message of #2597). The master only places
instances.

## Instance lifecycle for the demo

```bash
API=http://<any-mac>:52415
M=mlx-community/Qwen3.6-35B-A3B-4bit
curl "$API/instance/previews?model_id=$M" | jq '.[] | {sharding, instance_meta, error, memory_delta_by_node}'
curl -X POST $API/place_instance -H 'content-type: application/json' \
  -d "{\"model_id\":\"$M\",\"sharding\":\"Pipeline\",\"instance_meta\":\"MlxRing\",\"min_nodes\":3}"
curl -N "$API/instance/await?model_id=$M&timeout_seconds=300"
curl $API/v1/chat/completions -H 'content-type: application/json' \
  -d "{\"model\":\"$M\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"stream\":true}"
```

The enum spellings ("Pipeline", "MlxRing") come from `PlaceInstanceParams` in `src/exo/api/types/api.py` line 272
and need checking against `/instance/previews` output. Changes are acknowledged once in state; 503 after about 45 s.

A placed model becomes a **deployment** (`src/exo/master/deployments.py`) that the master keeps placed: it is
re-placed with the user's constraints, only on nodes that already have the files, and paused after 3 crashes in
10 minutes (`CRASH_LOOP_LIMIT`, `CRASH_LOOP_WINDOW`, `shared/types/deployments.py` lines 26 and 27).
`WAIT_LIMIT` = 1 h. Deployments are saved in `~/.exo/deployments.json`, so they survive restarts.

## One Mac holding a routing instance and a pool shard at once

- Allowed. Each instance has its own runner process per node. `memory_once_loaded()` (`master/placement.py` line 260)
  exists so "the next placement ... could put a second model in it" without double-counting; nothing forbids two
  instances on one node.
- Risk (from reading the code, unverified): `usable_memory()` caps each node at `min(ram_available, gpu_working_set)`.
  `gpu_working_set` is the Mac's total Metal budget, not what remains of it, so a second instance can be placed
  while the Metal budget is already partly used by the first. Two MLX processes then compete for one wired limit.
  Stay well under: on the 24 GB Macs, a pool shard of about 8 GB plus a routing model of about 2 to 5 GB is fine.
  Skip the routing model on the M3 (16 GB).
- Routing model candidates from EXO's cards: `Llama-3.2-3B-Instruct-4bit` 1.9 GB, `Qwen3.5-9B-4bit` 6.0 GB,
  `Meta-Llama-3.1-8B-Instruct-4bit` 4.6 GB, `gemma-4-e4b-it-4bit` 5.2 GB.
- Demo layout: one instance of `Llama-3.1-8B-4bit` (or `Qwen3.5-9B-4bit`) on each 24 GB Mac (two instances,
  routed by least-in-flight), plus the pooled `Qwen3.6-35B-A3B-4bit` across all 3. Place the pooled model **first**
  (it needs the most memory), then the small ones. Send all traffic to one API node so routing counts are global.

## When a Mac leaves mid-request

- Detection: Zenoh lease 10 s; a wedged peer after `WEDGE_TIMEOUT` 30 s (`rust/networking/src/membership.rs` lines 71
  and 145).
- In-flight request on the pooled instance: it ends with the error "A device serving this model can't be reached
  right now. Try again in a moment." (code `device_unreachable`; HTTP 503 before streaming starts) at both the API
  device and the output device (#2597 commit message). No partial recovery: the client must resend.
- The instance is lost. The reconciler re-places the deployment only on nodes that have the files, with its
  constraints. With `min_nodes=3` and only 2 Macs left, **it waits for the third** (up to `WAIT_LIMIT` 1 h) rather
  than shrinking. Without `min_nodes`, it comes back on 2 Macs if they hold it. For Qwen3.6-35B-A3B-4bit (20.4 GB)
  on M5 + third Mac with apps open, that is tight.
- Joining does not rebalance. The reconciler places only deployments without an instance, so a new Mac does not
  get layers until the instance is deleted and placed again. Our UI needs a "Rebalance" button
  (`DELETE /instance/{id}` then `POST /place_instance` with `min_nodes=<count>`). The model reloads from local disk,
  so allow seconds to tens of seconds; not measured.
- Demo advice: don't pull a laptop live from the pool. A safer moment is "add the third laptop → Rebalance →
  the big model now fits / loads". Routing failover works better live: kill one small-model Mac and requests go to
  the other instance (`candidate_instances` skips unreachable ones).

## Model download, caching and the pre-download plan

- Location on macOS: `~/.exo/models/<org>--<name>/`, e.g. `~/.exo/models/mlx-community--Qwen3.6-35B-A3B-4bit/`
  (`src/exo/shared/constants.py` lines 7 to 34: `EXO_HOME` override, non-Linux default `~/.exo`;
  `ModelId.normalize()` replaces `/` with `--`). Override with `EXO_DEFAULT_MODELS_DIR`; extra dirs with
  `EXO_MODELS_DIRS` (colon list) or `EXO_MODELS_READ_ONLY_DIRS` (never written, e.g. an external SSD).
- Downloads come only from Hugging Face (or `HF_ENDPOINT`). There is no peer-to-peer model sharing. Each file is
  written as `<file>.partial` and renamed after its hash matches. The manifest `.exo-download.json` in the model
  dir marks it complete (`src/exo/download/store.py` lines 1 to 34).
- A copied folder is accepted: `is_complete()` (`store.py` line 139) trusts the manifest if present, otherwise it
  checks that the `*.safetensors.index.json` and every weight file it names exist ("copied by hand" case).
- `--offline` / `EXO_OFFLINE=true`: no internet checks; a model not fully on disk fails with "Offline, and <model>
  isn't fully downloaded." (`download/coordinator.py` line 181).
- `scripts/download_model_to_cluster.py <model> --host <api>` asks every node to download the whole model through
  `/download/start`. It downloads from HF on each node, so it is not useful on venue internet.

### Steps: pre-download tonight on one Mac, copy over LAN tomorrow

Tonight, on the Mac with the most free disk (home internet):
```bash
# 1. Pin one exo build for all three Macs (the app 1.0.71 everywhere is simplest; or a source checkout at the same tag)
# 2. Download through exo itself so the manifest is written
open -a EXO        # or: uv run exo
API=http://localhost:52415
curl -X POST $API/download/start -H 'content-type: application/json' -d '...'   # or click Download in the dashboard
#    simplest: in the dashboard pick Qwen3.6-35B-A3B-4bit and download it; also Llama-3.1-8B-Instruct-4bit
# 3. Check
ls -la ~/.exo/models/mlx-community--Qwen3.6-35B-A3B-4bit/ ; cat ~/.exo/models/mlx-community--Qwen3.6-35B-A3B-4bit/.exo-download.json | head -c 300
du -sh ~/.exo/models/*
```
Fallback without exo: `hf download mlx-community/Qwen3.6-35B-A3B-4bit --local-dir ~/.exo/models/mlx-community--Qwen3.6-35B-A3B-4bit`
(no manifest; the "copied by hand" rule accepts it if the index and all shards are present).

At the venue, on the travel router (Ethernet if adapters exist; 5 GHz Wi-Fi otherwise. 20.4 GB over 1 GbE takes
about 3 min, over Wi-Fi about 5 to 15 min):
```bash
# On the source Mac: System Settings > General > Sharing > Remote Login ON (or use a USB SSD)
# On each other Mac (quit exo first so it doesn't start a download):
mkdir -p ~/.exo/models
rsync -a --progress <user>@<source-mac>.local:.exo/models/mlx-community--Qwen3.6-35B-A3B-4bit ~/.exo/models/
# then start exo offline
EXO_OFFLINE=true open -a EXO   # app: Settings > Offline mode; source: uv run exo --offline
```
Faster alternative: one USB-C SSD, and point every Mac at it read-only with `EXO_MODELS_READ_ONLY_DIRS=/Volumes/SSD/models`
(set as a custom environment variable in the app's Settings). This uses no internal disk, but loading is slower over
USB; unverified.

## Dashboard (what users see)

- SvelteKit app served by the API at `http://<node>:52415` (`dashboard/src/routes/+page.svelte`, 8,371 lines;
  routes `/`, `/downloads`, `/advanced`, `/integrations`, `/traces`).
- `TopologyGraph.svelte` draws every device and link with memory, GPU use, temperature and power (from macmon).
  `ChatForm`/`ChatMessages` hold a chat with a model picker; `ModelPickerModal` launches models
  (`/place_instance`, `/instance`).
- Missing for our demo: there is **no per-device layer range display**; `start_layer` does not appear anywhere in
  `dashboard/src`. Our dashboard can read `GET /state` → `instances[*].shard_assignments.runner_to_shard[*]
  .start_layer/end_layer` + `node_to_runner` to draw "Mac A: layers 0 to 15".
- Onboarding is a one-time flag only (`GET/POST /onboarding` writes a file, `api/main.py` line 2720).

## Join experience today, and what a QR / log-in join needs on top

Today (README "Adding another Mac"): install EXO, same LAN, same version, allow Local Network. The Mac "shows up
in the dashboard within a few seconds". There is no approval, no token, no account. Joining is just being on the same
LAN with the same build and cluster name.

Levers EXO already has:
- Cluster name (`EXO_ZENOH_NAMESPACE`, app Settings › Cluster) separates clusters on one LAN. It is a 3-byte blake3
  prefix in the Hello (`rust/networking/src/wire.rs`), so it is a label and not a secret.
- The app's Settings accept custom environment variables, offline mode, `HF_ENDPOINT`, a models folder
  (`app/EXO/EXO/Models/ExoLaunchSettings.swift`). `OVERRIDE_MEMORY_MB` can be set there.
- The app has no URL scheme or deep link (no `onOpenURL`/`CFBundleURLSchemes` in `app/`). A QR code cannot
  configure the app directly.

QR join we could build in the hackathon (our layer, no EXO patch):
1. The coordinator Mac runs our small web app (e.g. port 8080) on the router LAN. Its screen shows a QR code with
   `http://<coordinator-ip>:8080/join?code=<6-digit>`. A second QR code in `WIFI:T:WPA;S:<ssid>;P:<pass>;;` format
   joins the router's Wi-Fi first; the macOS/iOS camera understands it.
2. The join page (opened on the new Mac) offers a one-line script served from the LAN:
   `curl -fsSL http://<coord>:8080/join.sh?code=123456 | bash`. The script:
   - installs the pinned exo build from the LAN (DMG copy or `uv` source tarball, no internet);
   - sets the cluster name and `OVERRIDE_MEMORY_MB` (the "contribution slider" value);
   - rsyncs the model folder from the coordinator, or mounts the shared SSD path;
   - starts exo with `EXO_OFFLINE=true`.
3. The new Mac appears in `GET /state` within seconds. Our UI shows "M3 joined, 7 GB contributed" and offers
   **Rebalance** (delete and re-place the pooled instance with `min_nodes=<new count>`).
4. "Log in" = the 6-digit code checked by our web app before it hands out the script and cluster name. Honest
   caveat: EXO itself does not enforce it. Any Mac on the LAN with the same build and cluster name joins anyway.
   Real admission control (signed join tokens, Zenoh TLS/auth) is roadmap.

Simplest demo version (1 to 2 h of work): QR → join page → shows the 3 commands (install, rsync, start), with
the cluster name prefilled and a memory slider that fills in `OVERRIDE_MEMORY_MB`. The macOS Local Network prompt
still needs one click.
