# cppneed 01: Requirements to llama.cpp components

Worker `cppneed`, pass A (top-down from the product). Sat Nov 7 2026.
Source read: `Desktop\hackathon\llama-src` at commit `e9a2a44` (Nov 7 2026 15:03 PST). File:line references are to that
commit. Partner folder `research/cppcode/` was empty when this section started, so there is nothing to reconcile yet.

## What the product needs (from PRD.md, EXO-HOW-IT-WORKS.md, BRIEF-cppneed.md)

| # | Requirement | Demo / roadmap |
|---|---|---|
| R1 | Run one model too big for one Mac (Nemotron-3-Nano-30B-A3B GGUF, 18.2 to 26.1 GB) across 3 Macs (24 + 16 + 24 GB) | Demo |
| R2 | One OpenAI-compatible endpoint for several users at once | Demo |
| R3 | Tool calls + thinking on/off per request for the Nemotron agent (8090 track) | Demo |
| R4 | A Mac joins or leaves the pool | Demo |
| R5 | Place layers by measured speed, not only by memory | Demo (the "smart" part) |
| R6 | Dashboard data: which device holds what, tokens/s, busy slots, queue | Demo |
| R7 | Fully offline on our own router | Demo |
| R8 | Windows / old Dell PC joins | Roadmap (pitch) |
| R9 | Model files on every device, first load time acceptable | Demo |
| R10 | Health and failure reporting (a node died) | Demo |

## Requirements to components (the one table)

| Req | llama.cpp piece that covers it | Verified where | Gap | Who builds the gap |
|---|---|---|---|---|
| R1 pool | `rpc-server` on each worker Mac (flags only `-H/--host`, `-t`, `-d/--device`, `-p/--port`, `-c/--cache`, `tools/rpc/rpc-server.cpp:101-142`). Head Mac runs `llama-server -m X.gguf --rpc ip1:50052,ip2:50052 -ngl 99` (`common/arg.cpp:2651`, `:2781`). Default `--split-mode layer` = pipelined layer split (`arg.cpp:2799`) | source | Nemotron-H ops (`SSM_CONV`, `SSM_SCAN`, MoE) through RPC to a Metal backend are untested; arch `nemotron_h_moe` exists (`src/llama-arch.cpp:105`) | Test on day 1 (cppcode / bring-up) |
| R1 order | RPC devices go first in the device list, in `--rpc` order, local GPU last (`src/llama.cpp:275-279`) | source | `--tensor-split` values must be written in that order: `rpc1,rpc2,local` | Coordinator |
| R2 many users | `llama-server --parallel N` (`-np`, `arg.cpp:2515`), continuous batching on by default (`cont_batching = true`, `common/common.h:577`); `/v1/chat/completions`, `/v1/responses`, `/v1/messages` (`tools/server/server.cpp:180-190`) | source | One pooled model = one llama-server = one endpoint. No queueing across instances, no per-user auth beyond one `--api-key` (`arg.cpp:3511`) | Coordinator (thin proxy, or none: agent calls head's llama-server directly) |
| R3 tools | `--jinja` is already the default (`use_jinja = true`, `common.h:656`). Nemotron-3-Nano template is bundled (`models/templates/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16.jinja`) and routes to the Qwen3-Coder XML tool-call parser (`common/chat.cpp:1412`) | source | Real tool-call reliability on our quant: untested | Agent team tests with `principles/tool_test.py` |
| R3 thinking | Per request: `"chat_template_kwargs": {"enable_thinking": false}` (`server-common.cpp:1487-1495`) or `"reasoning_effort": "none"` (`:1498-1502`). Cap with `reasoning_budget_tokens` per request or `--reasoning-budget` (`arg.cpp:3746`). Split output via `--reasoning-format` (`arg.cpp:3705`) | source | `enable_thinking` must be a JSON boolean; a string `"false"` throws (`server-common.cpp:1493`) | Agent |
| R4 join/leave | Nothing dynamic. The `--rpc` list is fixed at start; a dropped rpc-server makes graph compute fail (principles/04, untested) | source (no reload path found) | Detect join/leave, rebuild `--rpc` + `--tensor-split`, restart llama-server; drain requests first | Coordinator, fed by Spacetime device rows |
| R4 restart cost | `rpc-server -c` caches tensors under `$HOME/.cache/llama.cpp/rpc` (`tools/rpc/README.md:86-96`, `LLAMA_CACHE` overrides) | source | Restart still re-reads the GGUF on the head and re-hashes; first load of ~8 GB per worker over Wi-Fi is minutes (principles/03: ~6 min for 10 GB at 30 MB/s) | Coordinator pre-warms: first launch at setup time, not on stage |
| R5 speed placement | `--tensor-split N0,N1,...` proportions (`arg.cpp:2823`). Default without it: proportional to each device's free memory (`tools/rpc/README.md:83`). `--fit on` (default, `common.h:485`) only fits memory | source | No speed measurement, nothing adaptive. Must also respect memory: the 16 GB M3 caps near 10.7 GB GPU-wired (2/3 rule, principles/03) | Coordinator computes split = speed-weighted, clipped by each device's memory cap; speed numbers stored in Spacetime |
| R6 dashboard | `GET /health` (no key), `GET /slots` (on by default, `endpoint_slots = true`, `common.h:689`), `GET /props`, `GET /metrics` Prometheus (off by default, needs `--metrics`, `common.h:691`, route `server.cpp:173`). Per-response `timings` give tokens/s | source | No per-device view (which layers on which Mac, per-node load). llama-server logs the per-device buffer sizes at load only. `GGML_RPC_DEBUG=1` on rpc-server logs graph computes (`tools/rpc/README.md:114-122`) | Coordinator records the split it chose + polls `/slots` and `/metrics`; Spacetime stores it; dashboard reads Spacetime |
| R7 offline | `--offline` (`arg.cpp:3958`) blocks network model downloads; load with `-m /path/file.gguf`, never `-hf` | source | None, if GGUF is pre-copied | Setup script |
| R8 Windows | `rpc-server` builds on Windows with `-DGGML_RPC=ON` (CPU or CUDA backend); same GGUF; joins with `--rpc dell-ip:50052` (principles/04) | doc | Speed: a CPU node in a layer pipeline slows every token by its share; give it a small split or none. Untested on our Dell | Coordinator (small or zero split for slow nodes); roadmap only |
| R9 model files | Only the head Mac needs the GGUF; workers receive tensors over RPC, cached with `-c` | doc | Copy one GGUF (18 to 26 GB) to the head by USB/SSD, not Wi-Fi | Setup |
| R10 health | `GET /health` returns 503 while loading, 200 when ready (server README). No rpc-server health endpoint | partly source | Probe rpc-server port by TCP connect; restart on failure | Coordinator / TS worker on each Mac |

## What we use (smallest set)

1. `rpc-server` (one per worker Mac, `-H 0.0.0.0 -p 50052 -c`).
2. `llama-server` (head Mac only) with `-m`, `--rpc`, `-ngl 99`, `--tensor-split`, `-c <ctx>`, `--parallel N`,
   `--metrics`, `--host 0.0.0.0`, `--offline`, optionally `--api-key`, `--reasoning-format`.
3. Endpoints: `/v1/chat/completions`, `/health`, `/slots`, `/metrics`, `/props` (GET).
4. Bundled Nemotron-3-Nano jinja template (default, nothing to pass).

## Drop list (do not use, and why)

| Piece | Why drop it |
|---|---|
| `--split-mode row` / `tensor` (`arg.cpp:2799`) | Tensor-parallel needs a fast link; over Wi-Fi every layer becomes a network round trip. `tensor` is marked EXPERIMENTAL in the help string |
| RDMA transport (`tools/rpc/README.md:98-107`) | Needs Thunderbolt 5 + macOS 26.2 + `rdma_ctl enable` from Recovery. Not our Wi-Fi setup |
| Router mode: `--models-dir`, `--models-preset`, `--models-max`, `--models-memory-margin` (`arg.cpp:3661-3690`, proxies in `server.cpp:490-514`) | Multi-model swapping on ONE machine. It is not a multi-machine request router and adds a process layer in front of the pooled server |
| `llama-connect` / `--connect` (`tools/connect/CMakeLists.txt`, `server-connect.h`) | Downloads a prebuilt binary from GitHub at build time and exposes the server to a remote browser over WebRTC. Breaks the offline and "data never leaves the building" story |
| `/cors-proxy`, `/tools` routes (`server.cpp:268-303`), MCP (`server-mcp.cpp`) | Server-side tool execution and web proxying. Our agent runs its own tools; these widen the attack surface |
| `-hf` model download | Needs internet; use `-m` with a local path |
| Built-in web UI as the product dashboard | Single-model chat UI with no device view. Fine for a sanity check, not the demo dashboard |
| `/v1/systemone`, `/tts`, `/v1/audio/*`, `/v1/realtime`, embeddings, rerank, LoRA | Not needed for the demo |
| `--cpu-moe` / `--override-tensor` (`arg.cpp:2725-2731`) | Keeps experts in CPU RAM on the head; useful on a single machine, but on Macs unified memory makes it pointless and it complicates the split. Keep in pocket only if the 16 GB M3 cannot hold its share |
| Exposing `rpc-server` beyond our router | README: "fragile and insecure. Never run the RPC server on an open network". Our own router keeps it isolated |
| `prima.cpp` / `distributed-llama` forks | Fork of an older llama.cpp (prima) or CPU-only 2^n nodes (distributed-llama); see principles/03 |

## What we must build ourselves

| Piece | Owner | Size guess |
|---|---|---|
| Speed probe per device (tokens/s or GB/s once at join) | TS worker on each Mac -> Spacetime | small |
| Split calculator: speed-weighted, memory-clipped, ordered `rpc...,local` | Coordinator | ~50 lines |
| Launcher: start/stop rpc-server on workers, (re)start llama-server on head with `--rpc` + `--tensor-split`, wait for `/health` 200 | Coordinator + TS worker | ~150 lines |
| Join/leave handler: Spacetime device row change -> drain -> restart | Coordinator | ~80 lines |
| Stats poller: `/slots`, `/metrics`, chosen split -> Spacetime | TS worker on head | ~60 lines |
| Dashboard: device map, layer share per Mac, tokens/s, busy slots | Frontend reading Spacetime | separate |
| Agent: tool loop on `/v1/chat/completions` with `enable_thinking` toggle | Agent team | separate |

Sizes are guesses; section 2 will firm them up.

## Open risks (unverified)

1. Nemotron-H hybrid (Mamba SSM ops + MoE) across RPC to Metal workers. Not tested by anyone on the team. This is the
   deciding test for llama.cpp.
2. `/metrics` metric names: not confirmed in this pass (could not grep them quickly in `server-context.cpp:5764`);
   server README lists `llamacpp:prompt_tokens_total`, `llamacpp:tokens_predicted_total`, etc. (from memory, verify).
3. Whether Metal registers as GPU or IGPU type does not change order: either way the local device sits after RPC
   devices (`src/llama.cpp:276-285`).
4. A recurrent (Mamba) model's state per slot: `--parallel N` multiplies recurrent state memory; small for this model
   but check the load log.
