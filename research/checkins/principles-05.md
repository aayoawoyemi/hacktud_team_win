## Check-in principles 05: Nemotron test plan and control-plane logic

Full notes: `research/principles/05-control-plane-and-nemotron-testplan.md`. New script: `research/principles/tool_test.py` (stdlib Python; runs a 2-step tool loop, read_file then answer, against any OpenAI-compatible endpoint and reports how many of N trials pass).

**Found (most important first, max 8 bullets, each with a source):**
- Parallax cannot run Nemotron. Its model list (`static_config.py`) has no Nemotron entry, `src/parallax/models/` has no `nemotron_h` file, and GitHub search finds no Parallax issue or PR mentioning Nemotron (Nov 7 2026). EXO is the engine. llama.cpp RPC is the fallback.
- EXO's Nemotron card for the 6-bit build lists 52 layers, hidden size 2688, `supports_tensor = true`, 25,667,611,392 bytes and 262,144-token context (resources/inference_model_cards/nemotron_h/...-MLX-6Bit.toml). EXO's chat-completions adapter passes `tools` through and streams OpenAI `tool_calls` (chat_completions.py L166, L249-276). Whether Nemotron's tool-call format parses correctly in EXO is unverified; `tool_test.py` checks it.
- Nemotron-3-Nano suits a memory-starved pool. Its config has 23 Mamba layers, 23 MoE layers (128 experts, 6 active) and only 6 attention layers with 2 KV heads (HF config.json). So the KV cache is about 6 KiB per token: 128K tokens of context needs about 0.75 GB (Llama 8B needs 128 KiB per token). Each pipeline hop is 10.5 KiB per token. Long legacy-code files cost almost no memory (my math).
- GGUF fallback sizes (unsloth, HF API): Q4_0 18.2 GB, UD-Q4_K_XL 22.8, Q4_K_M 24.6, Q5_K_M 26.1, Q8_0 33.6 GB. Splitting the Mamba layers over llama.cpp RPC is untested.
- Expected decode on 3 Macs over the travel router (my estimates): 6-bit 15 to 20 t/s, 5-bit 17 to 23, 4-bit 20 to 28. Nano-9B-v2 4-bit as a routing model: about 20 t/s per M5.
- Speed-aware placement barely matters for our three Macs. EXO's memory split gives the M3 about 22% of layers, which costs about 2.5 ms of a 45 to 60 ms token (~5%). It matters for the customer story (old PCs, M1s), so treat it as a dashboard explanation and build it last.
- Join pattern borrowed from kubeadm (id.secret token, 24 h TTL, CA hash), k3s (token embeds the CA hash) and Tailscale (one-off, expiring, pre-approved keys). Ours is a single-use, 10-minute secret stored as a hash in Spacetime. A QR code links to an installer, `claim_join` returns the EXO namespace, and pool-agent starts EXO with `--namespace`. EXO has no auth, so the real trust boundary is the router's WPA2 password.

**Test plan update:** EXO only, with llama.cpp RPC as fallback.
1. 2-Mac pool (M5 + M5) with the 4-bit model.
2. 3-Mac pool with the 6-bit model (`min_nodes: 3`).
3. Nano-9B routing instances pinned to each M5.

Each run gets `bench_openai.py` (200- and 4,000-token prompts, 4 concurrent requests) and `tool_test.py --trials 10`, with thinking both off and on.

**Pass bar:**
1. The 6-bit model on 3 Macs answers within 15 min.
2. Decode is at least 10 t/s.
3. Time to first token is 20 s or less on a 4,000-token prompt.
4. At least 9 of 10 tool loops pass.
5. 8 of 8 concurrent requests complete.
6. Recovers within 5 min after a node restart.
7. Works offline.

If the tool loop fails, try Nemotron-3.5-Lightning-4bit, and test llama-server `--jinja` on one M5 to tell a model problem from an EXO parser problem.

**Surprises / things that change the plan:**
- Nemotron's thinking mode is on by default, and an agent makes many calls. Thinking tokens multiply the time per step, so measure agent steps with thinking off and on and default to off for tool-calling steps.
- Agent steps re-send long code context, so the time to read the prompt on each step will dominate. The pass bar now uses a 4,000-token prompt.

**PRD suggestions:**
- Open question 2: "Parallax ruled out (no Nemotron/nemotron_h support). Engine = EXO; fallback = llama.cpp RPC with unsloth Nemotron-3-Nano GGUF Q4_0 18.2 GB."
- Must have, test plan: replace with the 05 §A4/A5 plan and pass bar, including `tool_test.py` at 9/10 or better.
- Must have, gateway: "per-step model choice: pooled 6-bit Nano-30B for planning and long context, Nano-9B routing instances for cheap steps; retry on 503/`device_unreachable` after `/instance/await`; never re-run a logged tool call."
- Build order (control plane): gateway + step log, then heartbeat + `/state` mirror + dashboard, then join token/QR, then placement panel with a Rebalance button, then speed-aware steering (only if time remains).
- Pitch: "Nemotron-3-Nano needs only 6 KiB of memory per token of context, so the pool can read whole legacy codebases" (6 attention layers of 52).

**Questions for Aayo:**
1. Should the agent's model calls run inside Spacetime (procedures making HTTP calls to EXO) or in a small gateway process that Spacetime drives? This depends on what the spacetime worker finds about outbound HTTP and on how strictly the track reads "majority of backend".

**Next section:** Section 6, the recommendation: final architecture (EXO + Spacetime + gateway + Nemotron agent), the build plan by hour until 12pm Sunday, the demo script (join moment, the 6-bit model across 3 Macs, the agent modernizing a COBOL file privately), realistic on-screen numbers, and the top risks with fallbacks. I would align it with the spacetime worker's table design and the exo worker's findings before writing it.
