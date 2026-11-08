## Check-in cppneed 01: Requirements to components

**Found (most important first, max 8 bullets, each with a source):**
- llama.cpp covers pooling, serving, tool calls, thinking toggle and stats with 2 binaries: `rpc-server` on each worker Mac and one `llama-server --rpc ip1:50052,ip2:50052 --tensor-split ... --parallel N --metrics` on the head (`common/arg.cpp:2651, 2823, 2515, 3611`; `tools/rpc/rpc-server.cpp:101-142`). Full table in `research/cppneed/01-requirements-to-components.md`.
- `--tensor-split` order is RPC servers first (in `--rpc` order), local GPU last: "add RPC servers at the front of the list" (`src/llama.cpp:275-279`). The coordinator must write `rpc1,rpc2,local`.
- Nemotron-3-Nano tool calls work out of the box in the source: `--jinja` is on by default (`common/common.h:656`), the template is bundled (`models/templates/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16.jinja`), and it maps to the Qwen3-Coder XML parser (`common/chat.cpp:1412`).
- Thinking toggle per request: `"chat_template_kwargs": {"enable_thinking": false}` or `"reasoning_effort": "none"`; the value must be a JSON boolean, a string throws (`tools/server/server-common.cpp:1487-1502`). Cap with `reasoning_budget_tokens`.
- Join/leave is not dynamic: `--rpc` is fixed at launch, so every join or leave means restart llama-server. `rpc-server -c` caches tensors in `~/.cache/llama.cpp/rpc` so restarts skip the network transfer (`tools/rpc/README.md:86-96`). First load must happen at setup, not on stage.
- Speed-aware placement does not exist: default split is by free memory (`tools/rpc/README.md:83`), `--fit` only fits memory (`common.h:485`). Our coordinator computes speed-weighted, memory-clipped proportions.
- Dashboard sources: `/health`, `/slots` (on by default), `/metrics` (off unless `--metrics`), `/props` (`tools/server/server.cpp:171-214`, `common.h:689-691`). No per-device view; the coordinator must record the split it chose.
- Drop: `--split-mode row/tensor`, RDMA, router mode (`--models-dir`, one machine only), `llama-connect` (downloads from GitHub, WebRTC out of the building), `/tools` + MCP + `/cors-proxy`, `-hf` downloads.

**Surprises / things that change the plan:**
- llama.cpp now has a "router server" mode (`--models-dir`, `--models-max`, `server.cpp:490-514`). It is multi-model on one machine, not multi-machine routing. Do not let anyone mistake it for our router.
- `llama-connect` is new (`tools/connect/`): it fetches a binary from GitHub during the build. Build with tools off or ignore it; it does not affect the offline demo unless enabled.
- Only the head Mac needs the GGUF file; workers get tensors over RPC. That saves copying 18 to 26 GB to every Mac.
- `research/cppcode/` was empty, so there was nothing to reconcile. No disagreements yet.

**PRD suggestions:**
- In the llama.cpp option, add: "Head Mac holds the GGUF; workers run `rpc-server -c`; `--tensor-split` order is workers first, head last; any join or leave restarts llama-server (cached weights, so seconds to a minute, not minutes; verify)."
- Add to the risk list: "Nemotron-H (Mamba + MoE) over RPC to Metal workers is untested. This is the go/no-go test for llama.cpp."

**Questions for Aayo:**
- Which Mac is the head (holds the GGUF and runs llama-server)? I would pick an M5 24 GB, with the M3 16 GB as a worker with the smallest share.
- For the demo, is "restart the pool when a Mac joins, ~30 to 60 s" acceptable, or does join have to be live with no downtime? If live, llama.cpp alone cannot do it.

**Next section:** Section 2, the coordinator: inputs from Spacetime, split formula (speed-weighted, clipped by the ~2/3-of-RAM Metal cap), launch/restart/drain sequence, health checks, HTTP contract for agent and dashboard, LOC/hour estimate, and a gain/lose table against EXO. I will also confirm the `/metrics` metric names from `server-context.cpp` (not verified this pass) and read cppcode's notes if they exist by then.
