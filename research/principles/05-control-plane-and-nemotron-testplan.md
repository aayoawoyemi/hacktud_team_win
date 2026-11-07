# 05: Nemotron re-target of the test plan, and the control-plane logic

Worker: principles. Written Sat Nov 7, 2026, after the 4:10pm change (NVIDIA track: Nemotron agent; Spacetime track:
Spacetime is the control-plane store; third Mac confirmed M5 24 GB). Spacetime tables are the spacetime worker's job;
this file covers the logic and how it talks to EXO.

## Part A. Test plan re-targeted to Nemotron

### A1. Engine choice: EXO by default

- **Parallax has no Nemotron support.** No `nemotron` entry in `src/backend/server/static_config.py` (its model list),
  no `nemotron_h` file under `src/parallax/models/` (repo tree, Nov 7 2026), and GitHub search finds no Parallax
  issue or PR mentioning Nemotron. Nemotron-3-Nano is a hybrid Mamba-Transformer MoE, so Parallax would need a new
  model file and new layer slicing for Mamba state. Not a hackathon job. **Parallax is out.**
- **EXO 1.0 has it built in:** `resources/inference_model_cards/nemotron_h/` lists Nemotron-3-Nano-30B-A3B MLX 4/5/6/8-bit,
  MXFP4, NVFP4, BF16; Nemotron-3.5-Lightning-30B-A3B 4/8-bit/bf16; Nemotron-Nano-9B-v2 4/6-bit; Nemotron-3-Ultra-550B.
  The 6-bit card: `n_layers = 52`, `hidden_size = 2688`, `supports_tensor = true`, `storage_size 25,667,611,392` bytes,
  `context_length = 262144` (read via GitHub API Nov 7 2026).
- **EXO chat completions pass tools through:** `src/exo/api/adapters/chat_completions.py` sends `tools=request.tools`
  (L166) and streams `ToolCallChunk` as OpenAI `tool_calls` with `finish_reason="tool_calls"` (L249-276). Whether
  Nemotron's tool-call format is parsed correctly by the runner is **unverified**; `tool_test.py` checks it.
- **Fallback: llama.cpp RPC with GGUF.** `unsloth/Nemotron-3-Nano-30B-A3B-GGUF` sizes (HF API, Nov 7 2026): Q4_0
  18.2 GB, UD-Q4_K_XL 22.8 GB, Q4_K_M 24.6 GB, Q5_K_M 26.1 GB, UD-Q5_K_XL 27.5 GB, Q8_0 33.6 GB. Also
  `bartowski/nvidia_Nemotron-3-Nano-30B-A3B-GGUF` and `lmstudio-community/...-GGUF` (Dec 2025). Splitting Mamba layers
  over RPC needs the Metal backend's SSM ops (`SSM_CONV`, `SSM_SCAN`) to work through the RPC server: likely, since RPC
  forwards whatever ops the remote backend supports, but **untested**. llama-server needs `--jinja` for tool calls.

### A2. Why Nemotron-3-Nano suits a memory-starved pool (from its config.json)

`nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16/config.json`: `model_type = nemotron_h`, 52 layers,
`hybrid_override_pattern = MEMEM*EMEMEM*...` = **23 Mamba-2 layers, 23 MoE layers, 6 attention layers**; 128 routed
experts, 6 per token; 2 KV heads; `max_position_embeddings = 262144`.

- KV cache exists only in the 6 attention layers: 6 × 2 heads × 128 dim × 2 (K, V) × 2 bytes = **6 KiB per token**
  (calc; head dim 128 assumed). 32K tokens of context = 192 MB; 128K = 768 MB. Compare Llama 3.1 8B at 128 KiB/token.
  Mamba layers keep a fixed-size state per sequence (tens of MB, guess). Long legacy-code files cost almost no memory.
- Pipeline hop per token: 2,688 × 4 bytes = **10.5 KiB** (FP32) (calc), smaller than Llama 8B's 16 KiB.
- Active parameters ~3B (A3B). Decode reads ~1.8 GB per token at 4-bit, ~2.6 GB at 6-bit (calc), the same as the
  Qwen3.6 estimates in section 3.

### A3. Model choice and expected numbers (3 × M5/M3/M5, travel router, EXO pipeline)

| Model (EXO model_id) | Size | Fits on | Expected decode (my estimate) |
|---|---|---|---|
| `mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-6Bit` (wow) | 25.7 GB | 3 Macs only (bigger than any Mac's 24 GB RAM) | ~15 to 20 t/s |
| `...-MLX-5Bit` | 21.7 GB | 2 or 3 Macs | ~17 to 23 t/s |
| `...-MLX-4Bit` (fallback) | 17.8 GB | one M5 with apps quit, or 2 Macs | ~20 to 28 t/s pooled; ~30 to 40 on one M5 |
| `mlx-community/NVIDIA-Nemotron-Nano-9B-v2-4bits` (routing) | 5.0 GB | each M5 | ~20 t/s per M5 (guess: 9B dense at 4-bit ≈ 5 GB per token ÷ 114 GB/s ≈ 23 t/s bound) |

Memory check for the 6-bit pool, wired limits 19 / 11 / 19 GB = 49 GB: 25.7 GB model + KV (tiny) + EXO runner
overhead. If Nano-9B routing instances also run on both M5s (+5 GB each), total ~36 GB of 49 GB. This fits, but place
the pooled model **first** (exo-04).

### A4. Updated test plan (EXO primary, llama.cpp RPC fallback)

Prep (unchanged from 04 §3.0): sysctl `19456` / `11264`, `caffeinate -dims`, power, ping matrix, `df -h`.
Disk: EXO puts the whole model on every Mac, so the 6-bit needs ~26 GB free per Mac and 4-bit ~18 GB.

```bash
API=http://<mac-a>:52415
M6=mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-6Bit
M4=mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-4Bit
# 1. 2-Mac pool, 4-bit (M5 + M5)
curl "$API/instance/previews?model_id=$M4" | jq '.[] | {sharding, instance_meta, error, memory_delta_by_node}'
curl -X POST $API/place_instance -H 'content-type: application/json' \
  -d "{\"model_id\":\"$M4\",\"sharding\":\"Pipeline\",\"instance_meta\":\"MlxRing\",\"min_nodes\":2}"
curl -N "$API/instance/await?model_id=$M4&timeout_seconds=600"
U=$API/v1/chat/completions
python3 bench_openai.py --url $U --model $M4 --label exo-n4-2mac-short --prompt-tokens 200  --runs 3 --out results.csv
python3 bench_openai.py --url $U --model $M4 --label exo-n4-2mac-doc   --prompt-tokens 4000 --runs 2 --out results.csv
python3 bench_openai.py --url $U --model $M4 --label exo-n4-2mac-c4    --concurrency 4 --runs 2 --out results.csv
python3 tool_test.py    --url $U --model $M4 --trials 10
# 2. Delete, then the wow: 3-Mac pool, 6-bit
curl -X DELETE $API/instance/<id-from-/state>
#    place $M6 with "min_nodes":3, await, then the same bench + tool_test with labels exo-n6-3mac-*
# 3. Routing instances: Nemotron-Nano-9B-v2-4bits pinned to each M5 (POST /instance with pinned_nodes), bench c4
```

`bench_openai.py` sends `chat_template_kwargs.enable_thinking=false`. Nemotron-3-Nano reasons by default; agent steps
should be measured both ways (`tool_test.py --thinking`), because thinking tokens multiply time per step.

### A5. Pass bar (replaces 04 §3.4)

EXO passes for the demo if, on the travel router with models on disk:

1. 6-bit pooled over **3 Macs** loads and answers within **15 min** of placement.
2. Single-user decode **≥ 10 t/s** (6-bit, 3 Macs).
3. TTFT **≤ 20 s** for a 4,000-token prompt (agents send long code context).
4. **Tool loop: ≥ 9/10** `tool_test.py` trials pass (valid `tool_calls` JSON, answer uses the tool result).
5. 4 concurrent requests all complete (8/8 over 2 runs).
6. After killing one node's exo process, the pool serves again within **5 min** of restarting it.
7. Works with the router's WAN unplugged.

If 1 or 2 fail: drop to 5-bit or 4-bit on 3 Macs (`min_nodes: 3` keeps the "all three" story). If 4 fails: try
Nemotron-3.5-Lightning-30B-A3B-4bit (same size, EXO card exists) and llama.cpp `llama-server --jinja` on one M5 with
the 4-bit GGUF to see whether the model or EXO's parser is at fault. If EXO cannot pool at all: llama.cpp RPC with
`Nemotron-3-Nano-30B-A3B-Q4_0.gguf` (18.2 GB) and `--tensor-split`.

## Part B. Control-plane logic (store = Spacetime; engine = EXO HTTP API)

Pieces: **Spacetime** (state + live subscriptions for the dashboard), **pool-agent** (a small daemon on every Mac),
**gateway** (one process on the coordinator Mac: the agent loop's model client and the retry router). EXO stays a
black box behind its HTTP API on port 52415.

### B1. Join (token + QR)

Patterns used:
- kubeadm bootstrap tokens: short `[a-z0-9]{6}.[a-z0-9]{16}` id.secret, 24 h TTL by default, plus a CA hash so the
  joining node can verify the server.
- k3s: the node token embeds the server CA hash (`K10<hash>::server:<secret>`).
- Tailscale: auth keys that are one-off or reusable, expire, and can be pre-approved and tagged.
(From each project's documentation, not re-fetched today.)

Our flow:
1. Dashboard "Add a computer" calls reducer `create_join_token(ttl=10 min, uses=1)` → a row with a random 128-bit
   secret (store a hash, show the secret once). The dashboard renders a QR code of
   `http://<coordinator-ip>:<port>/join?t=<secret>` plus the same 8-character code for typing.
2. The new Mac opens the link. The page serves a one-line installer that downloads pool-agent and runs
   `pool-agent join <secret>`.
3. pool-agent calls reducer `claim_join(secret, device_info)`. The reducer checks hash, expiry and remaining uses,
   then creates the `device` row bound to the caller's Spacetime Identity and returns the pool settings: EXO
   namespace, model list, coordinator address.
4. pool-agent applies the sysctl (asks for the admin password once), starts EXO with
   `--namespace <pool-namespace>` (or `EXO_ZENOH_NAMESPACE`), runs the join benchmark (B3), and reports.
- Honest limit: EXO has no auth. Any Mac with the same build on the LAN can join (exo-02). The namespace only prevents
  accidental cross-talk. The trust boundary for the demo is the travel router's WPA2 password. Roadmap: WireGuard mesh
  between devices (the Tailscale pattern) and signed device keys.

### B2. Health checks

- pool-agent → reducer `heartbeat(device_id, …)` every 2 s with: free memory (`vm_stat`), wired limit
  (`sysctl iogpu.wired_limit_mb`), thermal state (`pmset -g therm`), on AC (`pmset -g batt`), EXO node_id
  (`GET localhost:52415/node_id`), exo process alive.
- gateway polls EXO `GET /state` and `/membership` every 2 s and writes instance status, runner status and per-node
  memory to Spacetime. EXO's own membership lease is 10 s (exo-02).
- A Spacetime scheduled reducer every 5 s marks a device `stale` after 10 s without a heartbeat and `gone` after 60 s.
  The dashboard shows red. The gateway stops routing to instances on stale nodes. EXO's `candidate_instances`
  already skips unreachable ones (exo-04).

### B3. Speed-aware placement: inputs and the honest result for our trio

Inputs per device: usable memory (EXO `/state` `ram_available`, `gpu_working_set`), a decode benchmark at join
(64 tokens of Nemotron-Nano-9B-4bit, or a 5-second MLX memory-read test → effective GB/s), RTT to peers (ping),
thermal state.

Rule (prima.cpp-style): minimise Σ layers_i × t_layer_i + hops, subject to memory. Fill the fastest Macs first, and
give a device zero layers if it would hold ≤ 1 to 2 layers (unless `min_nodes` forces it in for the demo).

EXO lever: EXO places by free memory only, ≥ 1 layer per node (exo-03). Our placement steers it by
1. choosing the node set (`pinned_nodes` / `min_nodes` in `POST /instance`), and
2. starting a slower Mac's EXO with `OVERRIDE_MEMORY_MB=<target>` so its proportional share equals our target.

Honest result for M5 + M3 + M5 with the 6-bit model: EXO's memory split gives the M3 about 11 / 49 ≈ 22% of
layers. Per token the M3 then reads ~0.57 GB at ~76 GB/s ≈ 7.5 ms, versus ~5 ms if an M5 held those layers (calc).
That is ~2.5 ms of a ~45 to 60 ms token, about 5%. **Speed-aware placement barely changes speed for our three Macs.**
It matters for the customer story (an old Windows PC at 25 to 55 GB/s, or an M1 next to an M5). For the demo it is a
dashboard explanation ("M3: 11 layers, 7 ms/token"), not a speed win. Build it last.

### B4. Gateway: model choice + retry router + agent-step log

- Model choice per agent step: the planner and long-context steps use the pooled Nemotron-3-Nano (6-bit). Cheap steps
  (classify, extract, summarise one file) use Nano-9B routing instances. Both go through any node's
  `POST :52415/v1/chat/completions`; EXO picks the least-busy ready instance of the requested model (exo-04).
- Retry: on HTTP 503 before streaming, or an in-stream error `device_unreachable` (exo-04), wait for
  `GET /instance/await?model_id=…` (≤ 5 min), then re-send the original messages plus the partial assistant text as
  a prefix with a short "continue" instruction. Retry the model call at most twice. **Never re-run a tool call** whose
  result is already logged. Tool calls are idempotent by `step_id`.
- Agent-step log: every model call and tool call is a row (`job_id`, `step_id`, `kind`, `model`, `device(s)`,
  `prompt_tokens`, `completion_tokens`, `ttft_ms`, `tps`, `status`). The dashboard subscribes and shows the agent
  working live, plus which Macs served each step.
- Where it runs: a process on the coordinator Mac that subscribes to Spacetime's `job` table and calls EXO. If
  Spacetime procedures can make outbound HTTP calls to LAN addresses, the model call could move into Spacetime
  (spacetime worker to confirm; the "majority of backend on Spacetime" rule may favour that).

### B5. What to build, in order (for ~10 hours)

1. Gateway with retry + step log (needed by the agent; ~2 h).
2. pool-agent heartbeat + `/state` mirror into Spacetime + dashboard of devices/instances (~2 to 3 h).
3. Join token + QR + `claim_join` + EXO start with namespace (~2 h).
4. Placement panel: layers per Mac, measured ms/token, "Rebalance" button (`DELETE /instance` + `POST
   /place_instance`) (~1 to 2 h).
5. Speed-aware `OVERRIDE_MEMORY_MB` steering (only if time remains).
