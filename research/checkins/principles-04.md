## Check-in principles 04: Mixed devices, failures, test plan

Full notes: `research/principles/04-mixed-devices-failures-testplan.md`. New tool: `research/principles/bench_openai.py` (stdlib Python; measures time to first token, decode tok/s and concurrent throughput on any OpenAI-compatible endpoint, writes CSV).

**Found (most important first, max 8 bullets, each with a source):**
- On Parallax, the M5 24 + M3 16 pair may not hold the 4-bit model. Parallax budgets weights at 0.5 × total RAM (`param_mem_ratio = 0.5`, src/scheduling/node.py L180), so that pair gets 12 + 8 = 20 GB, under the 20.4 GB model. Test 2-Mac Parallax on the two 24 GB Macs (24 GB budget). The 6-bit model (~28 GB) fits only on all three Macs (32 GB budget).
- Parallax maps `Qwen/Qwen3.6-35B-A3B` to `mlx-community/Qwen3.6-35B-A3B-4bit` (src/backend/server/static_config.py L92). The 6-bit build is not mapped, so the wow run may need a one-line config edit.
- If a node leaves, Parallax waits up to 30 s, then drops the whole pipeline. A node silent for 30 s is removed (`heartbeat_timeout = 30.0`, scheduler.py L45, L269-275). Then "if any member leaves, the whole fixed pipeline becomes invalid": the rest go to standby, layers are cleared and in-flight counts reset (node_management.py L161-182). Expect the in-flight request to fail and the pool to need a reload.
- Slow nodes: Parallax routes using measured per-layer latency plus a load term (node.py L379-387). Rebalancing for load imbalance looks off by default (`rebalance_threshold = float("inf")`, scheduler.py L43; my reading). EXO splits by free memory once and never adapts; `OVERRIDE_MEMORY_MB` on the M3 gives it fewer layers (exo-03). llama.cpp uses a fixed `--tensor-split`.
- No tool can survive losing a Mac that holds layers of a model too big for the rest. The 4-bit model can reload on M5 + M5; the 6-bit cannot reload on any 2 of our Macs. The request's KV cache is lost either way, so recovery means re-sending the prompt plus the text generated so far.
- Windows (roadmap): only llama.cpp RPC lets a Windows CPU PC join a Mac pool, using the same GGUF file on Metal and CPU. Parallax on Windows means NVIDIA via WSL, and EXO cannot run on Windows. Because of section 1's numbers, an old PC should hold zero or few layers and serve a small model or the router.
- Demo-day failure prevention: power adapters, `caffeinate -dims`, lids open, Low Power Mode off, apps quit, `pmset -g therm` check, Macs within a few metres of the 5 GHz travel router.

**Test plan (summary; exact commands in the notes):** prep 30 min (sysctl, ping matrix, `df -h`, download `mlx-community/Qwen3.6-35B-A3B-4bit` on every Mac). Then Parallax on 2 Macs and 3 Macs (`parallax run -m Qwen/Qwen3.6-35B-A3B -n 2 -u --host 0.0.0.0`; `parallax join -s <addr> -u`) for 45 min. Then EXO on the same Macs (`/instance/previews`, `POST /instance` with `min_nodes` 2 then 3) for 45 min. Each run gets the same 3 bench runs: a 200-token prompt, a 2,000-token prompt, and 4 concurrent requests.

**Pass bar:**
1. Answers within 15 min of starting.
2. Single-user decode of at least 10 t/s on 2 Macs.
3. Time to first token of 15 s or less on a 2,000-token prompt.
4. 8 of 8 concurrent requests complete, with aggregate throughput at least 1.3x single-user.
5. The 3-Mac run works.
6. Recovers within 5 min after one node process is killed.
7. Works with the router's internet unplugged.

**Pick rule:** if both pass, choose the faster 3-Mac decode unless the other wins on concurrency or recovery. If neither passes, fall back to llama.cpp RPC with Q4_K_M GGUF and our router. Total time is about 2.5 hours, including a 6-bit try on the winner.

**Surprises / things that change the plan:**
- Pulling a laptop out of the pool would be a bad demo moment on every tool. Don't plan it. Show a join instead: pool on 2 Macs, the third joins, and the bigger 6-bit model becomes loadable.
- The test plan's 2-Mac pair should be M5 + M5, not Aayo's M5 + the M3, because of Parallax's 0.5 × RAM budget.

**PRD suggestions:**
- Must have, First 2 hours: replace with "Run research/principles/04 §3 test plan; pass bar §3.4; results into a table in the PRD; use bench_openai.py for every candidate."
- Must have: "Client-side retry: if a stream fails, re-send the messages plus the partial answer once (the user sees a pause, not an error)."
- Demo hardware: add "power adapters for all 3 Macs; `caffeinate -dims`; Low Power Mode off; lids open."
- Demo script: "show a join (2 Macs, then the third joins and unlocks the 6-bit model), never a leave."
- Customer story/roadmap: "Old Windows PCs join through llama.cpp RPC with the same GGUF file; they get few or zero layers and serve a small model, embeddings, or the router."

**Questions for Aayo:**
1. Which machine runs the test tonight, and is there internet at the venue for the ~20 GB per Mac model download? If not, someone should download `mlx-community/Qwen3.6-35B-A3B-4bit` (and 6-bit) at home and bring it on a USB SSD.

**Next section:** Section 5, the control plane: discovery, join flow (join code / QR), auth on an office LAN, health checks and placement, drawing on Tailscale, k3s join tokens and what Parallax and EXO already provide. I would narrow it to what we would build on top of the winning tool in about 10 hours: a join page with a QR code, a token check, a dashboard reading Parallax's or EXO's state, speed-aware placement, and the retry router. That keeps the general survey short.
