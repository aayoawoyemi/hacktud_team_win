# 04: Mixed devices, failure handling, and tonight's test plan

Worker: principles. Written Sat Nov 7, 2026. Parallax code read via GitHub API on Nov 7 2026 (main branch, last push
Jul 1 2026). EXO facts come from the exo worker's notes (research/exo/02, 03), not re-read here.

Assumptions (orchestrator): Mac A = M5 24 GB (Aayo), Mac B = M3 16 GB, Mac C = M5 24 GB. Owners quit apps and run
the sysctl. Travel router confirmed.

## 1. Windows / old office PCs joining a Mac pool (roadmap only, short)

| Tool | Can a Windows CPU PC join a Mac pool? | How |
|---|---|---|
| llama.cpp RPC | **Yes** | Windows PC runs `rpc-server` (CPU backend, `-DGGML_RPC=ON`); the Mac's `llama-server --rpc <pc-ip>:50052` includes it. Same GGUF file on all devices, no conversion. |
| Parallax | No (CPU) | Windows support is an installer for NVIDIA GPUs via WSL (install.md). Mac nodes use MLX; no CPU node type. |
| EXO 1.0 | No | POSIX-only startup code, no MLX on Windows (exo-01). |
| distributed-llama | Yes, but CPU-only for every node and 2^n nodes | Not a Mac-GPU pool. |

- Quantization formats: GGUF (llama.cpp) runs the same file on Metal, CUDA and CPU. MLX 4/6/8-bit runs only on Apple
  Silicon (and MLX-CUDA/CPU on Linux). A mixed office pool therefore implies GGUF + llama.cpp.
- Physics (section 1): a Windows laptop CPU decodes at 25 to 55 GB/s effective vs ~114 GB/s for an M5. In a pipeline
  it adds 2 to 4x more time per GB it holds. The product answer for old PCs: give them **zero or few layers** and use
  them as routed workers for a small model (a 4B to 8B model at 5 to 15 t/s), embeddings for document search, or as
  the always-on router/dashboard box.
- Pitch line: "Today the pool is Macs. The same GGUF format and llama.cpp backend run on Windows, so office PCs join
  as workers in the next version." Do not demo it.

## 2. Failure handling

### 2a. A slow node (thermals, other apps, the M3)

- Pipeline time per token is the sum of every stage. A slowed stage slows every token for every user.
- Causes on demo day: a fanless MacBook Air throttling after minutes of load (unverified which models are Airs),
  Spotlight/Photos indexing, a browser tab with video, Low Power Mode, battery power.
- What each tool does:
  - **Parallax:** routing uses measured per-layer latency when nodes report it (`avg_layer_latency_ms`) plus a load term
    (`load_compensator × current_requests / max_requests`), and an overloaded node counts as infinite latency
    (`src/scheduling/node.py` L379-387, L270-272). Layer re-placement happens on join (`dynamic_join` "to the
    lightest layers") or on global rebalance (`layer_allocation.py` L193-276). The scheduler's
    `rebalance_threshold` defaults to `float("inf")` (`src/scheduling/scheduler.py` L43), which suggests rebalancing
    for load imbalance is off by default (my reading; not tested).
  - **EXO:** splits by free memory only, at placement time; nothing reacts to a slow node during serving (exo-03).
    Knob: `OVERRIDE_MEMORY_MB` on the M3 to give it fewer layers.
  - **llama.cpp RPC:** static `--tensor-split`; nothing adaptive.
- Our mitigation (cheap): plug every Mac into power, turn off Low Power Mode, run `caffeinate -dims &`, quit apps,
  check `pmset -g therm` before the demo. Product feature (if time): show per-stage ms/token on the dashboard and
  re-place layers when one stage is >1.5x its share.

### 2b. A node leaves mid-generation (lid closed, Wi-Fi drop, crash)

- Physics: a pipeline holding a model that is too big for the remaining Macs cannot continue. The leaving Mac had
  layers and that request's KV cache for them. Only two recoveries exist:
  1. Reload the model on the remaining Macs (works only if it fits there: the 4-bit 20.4 GB model fits on M5 + M5,
     the 6-bit ~28 GB does not fit on any two of our Macs).
  2. Wait for the Mac to come back and reload its layers.
  Either way the in-flight request loses its KV cache. It must be re-sent (re-read the prompt + text generated so far)
  to continue.
- **Parallax** (code): nodes heartbeat to the scheduler; a node silent for **30 s** (`heartbeat_timeout = 30.0`,
  scheduler.py L45, L269-275) is enqueued to leave. On leave, "if any member leaves, the whole fixed pipeline becomes
  invalid": remaining members go to STANDBY, their layer allocations are cleared and their in-flight request count is
  reset to 0 (`detach_on_member_leave`, node_management.py L161-182). A global rebalance and re-bootstrap runs only if
  `should_global_rebalance()` says so (scheduler.py L581-633). Expect the in-flight request to fail after up to 30 s,
  then the pool to need a reload. Not tested.
- **EXO** (exo-02): membership lease 10 s, master heartbeat 5 s, master silence 30 s. What happens to an in-flight
  request is on the exo worker's list (exo-03 "Next section").
- **llama.cpp RPC:** a dropped rpc-server makes the main llama-server fail the graph compute; restart needed
  (general behaviour of the RPC backend; not tested).
- Our mitigation for the demo:
  - Prevent: power adapters, `caffeinate -dims`, disable sleep on lid close is not possible on battery, so keep lids
    open; travel router 5 GHz with the Macs within a few metres.
  - Recover in the client: our router/front end retries a failed stream once by sending the original messages plus
    the text produced so far as a partial assistant turn ("continue"). The user sees a pause, not an error.
  - Do not plan a "pull a laptop out" demo moment. Every tool loses the request, and the 6-bit model cannot reload on
    two Macs.

## 3. Tonight's test plan: Qwen3.6-35B-A3B 4-bit on 2 Macs, Parallax vs EXO

### 3.0 Prep (all Macs, before anything else, ~30 min, needs internet)

```bash
# Memory and power
sudo sysctl iogpu.wired_limit_mb=19456      # 24 GB Macs; on the 16 GB M3 use 11264
caffeinate -dims &                          # no sleep while testing; keep on power adapter
pmset -g therm                              # note any thermal warning
# Network: all Macs on the travel router; note each Mac's IP
ipconfig getifaddr en0
ping -c 20 <other-mac-ip>                   # record avg and max RTT for every pair
# Disk: need ~25 GB free per Mac for the 4-bit model (EXO puts the whole model on every Mac)
df -h ~
```

Model download (once per Mac; copy over LAN with `rsync` if internet is slow). Parallax and EXO use the same MLX
repo, `mlx-community/Qwen3.6-35B-A3B-4bit` (Parallax maps `Qwen/Qwen3.6-35B-A3B` to it in
`src/backend/server/static_config.py` L92; EXO card is 20.4 GB per exo-03):

```bash
pip install -U "huggingface_hub[cli]"
huggingface-cli download mlx-community/Qwen3.6-35B-A3B-4bit    # goes to ~/.cache/huggingface/hub
```

Check whether EXO reads the HF cache or only `~/.exo/models` (exo worker); if only its own folder, let EXO download it
once on one Mac and `rsync -a ~/.exo/models/ <other-mac>:~/.exo/models/`.

Copy `research/principles/bench_openai.py` to Mac A.

### 3.1 Parallax on 2 Macs (Mac A = scheduler + node, Mac C = node) (~45 min)

Use the two 24 GB Macs first: Parallax budgets weights at 0.5 × RAM (`param_mem_ratio`, node.py L180), so
M5 24 + M3 16 gives 12 + 8 = 20 GB, which is under the 20.4 GB model. M5 + M5 gives 24 GB.

```bash
# Every Mac
git clone https://github.com/GradientHQ/parallax.git && cd parallax
./install.sh --extras mac && source .venv/bin/activate

# Mac A: scheduler, LAN mode, no telemetry upload, 2 nodes, our model
parallax run -m Qwen/Qwen3.6-35B-A3B -n 2 -u --host 0.0.0.0
#   note the scheduler address printed in the log

# Mac A (second terminal) and Mac C: join. Pass the address explicitly (mDNS is flaky on macOS, issue #460)
parallax join -s <scheduler-address> -u

# Smoke test from Mac A
curl -s http://localhost:3001/v1/chat/completions -H 'Content-Type: application/json' \
  -d '{"max_tokens":64,"messages":[{"role":"user","content":"Say hi"}],"stream":false,
       "chat_template_kwargs":{"enable_thinking":false}}'

# Measurements (same for every candidate)
U=http://localhost:3001/v1/chat/completions; M=Qwen/Qwen3.6-35B-A3B
python3 bench_openai.py --url $U --model $M --label px-2mac-short --prompt-tokens 200  --runs 3 --out results.csv
python3 bench_openai.py --url $U --model $M --label px-2mac-doc   --prompt-tokens 2000 --runs 2 --out results.csv
python3 bench_openai.py --url $U --model $M --label px-2mac-c4    --prompt-tokens 200  --concurrency 4 --runs 2 --out results.csv
```

Then repeat with 3 nodes (`-n 3`, M3 joins), labels `px-3mac-*`, and read the scheduler log's allocation snapshot to
see how many layers the M3 got.

### 3.2 EXO 1.0 on the same Macs (~45 min)

Stop Parallax first (both use MLX and GPU memory). Same exo build on every Mac (app DMG or `uv run exo`); allow the
local-network prompt on every Mac (exo-02).

```bash
# Every Mac: start exo, pin the master on Mac A
uv run exo --force-master        # Mac A
uv run exo                       # Mac C (and Mac B for the 3-Mac run)

# Check membership and free memory
curl -s http://localhost:52415/membership
curl -s http://localhost:52415/state | python3 -m json.tool | less     # ram_available / gpu_working_set per node

# Preview placements, then create a 2-node pipeline instance
curl -s "http://localhost:52415/instance/previews?model_id=mlx-community/Qwen3.6-35B-A3B-4bit" | python3 -m json.tool
# POST /instance with constraints {min_nodes: 2} (field names: check exo docs/api.md / exo worker notes)
curl -s "http://localhost:52415/instance/await?model_id=mlx-community/Qwen3.6-35B-A3B-4bit"   # waits until ready

U=http://localhost:52415/v1/chat/completions; M=mlx-community/Qwen3.6-35B-A3B-4bit
python3 bench_openai.py --url $U --model $M --label exo-2mac-short --prompt-tokens 200  --runs 3 --out results.csv
python3 bench_openai.py --url $U --model $M --label exo-2mac-doc   --prompt-tokens 2000 --runs 2 --out results.csv
python3 bench_openai.py --url $U --model $M --label exo-2mac-c4    --prompt-tokens 200  --concurrency 4 --runs 2 --out results.csv
```

Then 3 Macs with `min_nodes: 3`, labels `exo-3mac-*`.

### 3.3 What to record (one table per candidate, into the PRD)

| Metric | How |
|---|---|
| Time from start to first answer (setup) | stopwatch, from `run`/`exo` start with the model already on disk |
| Model load time | log timestamps until ready |
| Decode t/s, single user, 200-token prompt | `SUMMARY` line, median of 3 |
| TTFT, 2,000-token prompt | `SUMMARY` line |
| Aggregate t/s at 4 concurrent users | `run N: ... aggregate=` line |
| Failures | `ok=x/y` in SUMMARY |
| Per-Mac memory used during serving | Activity Monitor or `/state` |
| Layers per Mac | Parallax allocation log; EXO `/state` instance |
| Recovery | stop one node process (`Ctrl+C`), time until error, restart it, time until serving again |
| Pair RTT | `ping` avg/max from 3.0 |

### 3.4 Pass/fail bar

A candidate **passes** if, on the travel router, with the 4-bit model already on disk:

1. Loads across 2 Macs and answers within **15 min** of starting from scratch (install excluded).
2. Single-user decode **≥ 10 t/s** on 2 Macs (my estimate is 20 to 28 t/s; under 10 means something is wrong).
3. TTFT **≤ 15 s** for a 2,000-token prompt.
4. **4 concurrent requests all complete** (ok = 8/8 over 2 runs), aggregate ≥ 1.3x single-user t/s.
5. **3-Mac run works** (the wow needs all three for the 6-bit model).
6. **Recovers** after one node process is killed and restarted, within **5 min**, with no reboot of the Macs.
7. **No internet needed** once models are on disk (pull the router's WAN cable and re-run the short test).

Pick rule:
- Only one passes → use it.
- Both pass → pick the higher 3-Mac decode t/s unless the other wins on criteria 4 (concurrency) or 6 (recovery),
  which matter more for an office of many users than ~20% speed.
- Neither passes → llama.cpp RPC with GGUF `Qwen3.6-35B-A3B-UD-Q4_K_M` (22.1 GB), `rpc-server -c` on Macs B and C,
  `llama-server --rpc ... --tensor-split` set by measured speed, `--parallel 4`; our router in front.

Time budget: prep 30 min + Parallax 45 + EXO 45 + 6-bit try on the winner 30 = **~2.5 hours**.

### 3.5 After picking: the 6-bit wow run

- Download `mlx-community/Qwen3.6-35B-A3B-6bit` (~28 GB; check HF for the exact size).
- Parallax: weight budget 12 + 8 + 12 = 32 GB, so ~28 GB fits on 3 Macs; it does not fit on 2. Parallax's 6-bit model
  name mapping may need a code edit, since `static_config.py` maps `Qwen/Qwen3.6-35B-A3B` to the 4-bit build only.
- EXO: needs `min_nodes: 3` and enough free memory on each Mac after the sysctl.
- Expected 15 to 20 t/s (section 3). If memory pressure goes yellow/red in Activity Monitor, fall back to 4-bit with
  `min_nodes 3`.
