## Check-in exo 04: Serving and UX

**Found (most important first, max 8 bullets, each with a source):**
- **The request path no longer goes through the master** (#2597, 2026-11-06, commit `a60b956`). The flow:
  1. Any node's `/v1/chat/completions` (`src/exo/api/main.py` line 1275) calls `candidate_instances()`, which does least-in-flight routing (`src/exo/shared/serving.py` line 110).
  2. The request goes as `POST /_exo/serve` to the instance's rank-0 device, which passes it to the other ranks with `/_exo/deliver` (`src/exo/worker/serving.py` lines 59, 219, 539).
  3. Tokens stream back as NDJSON and become OpenAI SSE.
  Closing the stream cancels the request on all ranks.
- **One Mac can hold a routing instance and a pool shard at the same time.** `memory_once_loaded()` (`src/exo/master/placement.py` line 260) explicitly supports a second model on a node. Risk, from reading the code: the cap uses the Mac's total Metal budget (`gpu_working_set`), not what is left of it. Two MLX processes can therefore over-commit. Keep it to an 8 GB pool shard plus a 2 to 5 GB small model on the 24 GB Macs, and give the M3 no routing model. Place the big model first.
- **A Mac leaving mid-request ends the request.** The client gets "A device serving this model can't be reached right now" (`device_unreachable`, 503), detected within 10 s (lease) to 30 s (`WEDGE_TIMEOUT`, `rust/networking/src/membership.rs` lines 71 and 145). The deployment is re-placed only on Macs that already have the files. With `min_nodes=3` it **waits up to 1 h** for the third Mac and does not shrink (`src/exo/master/deployments.py`; `WAIT_LIMIT`, `CRASH_LOOP_LIMIT=3` per 10 min in `shared/types/deployments.py`).
- **A new Mac joining does not rebalance anything.** The reconciler only places deployments that have no instance. Our UI needs a "Rebalance" button that runs `DELETE /instance/{id}` and then `POST /place_instance` with `min_nodes=<count>`. Demo advice: show "add a laptop → Rebalance → big model loads" and routing failover. Do not pull a laptop out of the pool live.
- **Model cache:** `~/.exo/models/<org>--<name>/` with a `.exo-download.json` manifest (`src/exo/download/store.py` lines 1 to 34; `shared/constants.py` lines 7 to 34).
  - A folder copied by hand counts as complete: `is_complete()` (`store.py` line 139) accepts a manifest, or an index plus all its shards.
  - Run `--offline` / `EXO_OFFLINE=true` so nothing tries the internet.
  - There is no peer-to-peer model transfer between nodes.
  - `EXO_MODELS_READ_ONLY_DIRS` can point at a shared USB SSD.
  Exact tonight/venue steps (download via the exo dashboard, `rsync` from `<mac>.local`, start offline) are in `research/exo/04-serving-and-ux.md`.
- **Dashboard:** a SvelteKit app at `:52415` with a topology graph (per-device memory, GPU %, temp, power), chat, model picker and downloads (`dashboard/src/routes/+page.svelte`, `TopologyGraph.svelte`). It **does not show which layers each device runs**; `start_layer` appears nowhere in `dashboard/src`. Our dashboard can read it from `GET /state` → `instances[*].shard_assignments.runner_to_shard`.
- **The join experience today has no approval, token or login.** You install the same build, join the same LAN, allow Local Network, and the Mac "shows up in the dashboard within a few seconds" (README "Adding another Mac"). The cluster name (`EXO_ZENOH_NAMESPACE`, app Settings › Cluster) is a 3-byte hash label, not a secret (`rust/networking/src/wire.rs`). The app has no URL scheme or deep link (no `onOpenURL`/`CFBundleURLSchemes` in `app/`). It does accept custom env vars in Settings, which is where `OVERRIDE_MEMORY_MB` would go (`app/EXO/EXO/Models/ExoLaunchSettings.swift`).
- **A QR join we can build on top, with no EXO patch** (about 1 to 2 h):
  1. The coordinator screen shows a Wi-Fi QR code (`WIFI:T:WPA;S:..;P:..;;`) and a join QR code (`http://<coord>:8080/join?code=123456`).
  2. The join page, after checking the code, serves a `join.sh` that installs the pinned exo build from the LAN, sets the cluster name and the `OVERRIDE_MEMORY_MB` slider value, rsyncs the model and starts exo offline.
  3. Our UI shows "M3 joined, 7 GB contributed" and offers Rebalance.
  Caveat to say out loud: EXO doesn't enforce the code, so real admission control is roadmap.

**Surprises / things that change the plan:**
- EXO's built-in dashboard already covers most of the "dashboard" stretch goal (devices, memory, live GPU use). Our UI only has to add three things: layer ranges per device, the join/QR flow, and Rebalance.
- With `min_nodes=3`, losing a Mac stalls the pooled model until that Mac returns, which is a demo-day risk. Keep a "re-place on 2 Macs" button (no `min_nodes`) ready as a fallback, with the 17.6 GB `Qwen3-30B-A3B-4bit` if 20.4 GB doesn't fit on 2.

**PRD suggestions:**
- Must have: add "Rebalance button (delete + re-place with `min_nodes`=current Mac count); EXO does not rebalance when a Mac joins."
- Must have > Route: "routing model `Meta-Llama-3.1-8B-Instruct-4bit` (4.6 GB) on each 24 GB Mac, none on the M3; place the pooled model first; send all traffic to one API node."
- Stretch > Dashboard: "EXO's dashboard already shows devices, memory and GPU. We add per-device layer ranges from `GET /state` `runner_to_shard`."
- Stretch > One-step join: replace with the 4-step QR flow above, and note "EXO does not enforce the join code (no auth); signed join tokens are roadmap."
- Demo script: "show add-a-laptop → Rebalance, and kill a routing Mac to show failover. Don't unplug a pool Mac mid-answer (request fails; with `min_nodes=3` the model waits for it)."

**Questions for Aayo:**
1. Is pinning everyone to the EXO **app** v1.0.71 (DMG/brew) OK, instead of source builds? The app is easier to install and its Settings take our env vars, but app and source builds don't join each other, so all three Macs must use the same one.
2. For the join demo, which do you want: a real one-line `join.sh` that installs and configures exo, or a guided page that shows the steps? The script is the better demo; the page is safer.

**Next section:** Section 5, what people say: Reddit (r/LocalLLaMA, r/MacStudio), HN and GitHub issues about exo 1.0. I will look for real tokens/s on MacBooks over Ethernet/Wi-Fi (not Mac Studio RDMA), common failures (discovery, version mismatch, OOM while loading), and what was abandoned (tinygrad, Linux GPU, bootstrap peers). I'd focus on 2026 reports of 2 to 4 MacBook clusters running Qwen3 30B/35B A3B pipelines, since that matches our demo.
