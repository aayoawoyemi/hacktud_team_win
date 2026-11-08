<!-- CONCEPT: observed three-Mac 12B acceptance evidence. WHY: D-08 and Checkpoint A/B. DEPENDS ON: main 66c5637, b8665, SpacetimeDB 2.14.1. AFFECTS: pool bring-up and demo acceptance. -->
# P4 Gemma 12B three-Mac answer — PASS

Main: MacBook Air M3 .113 (SpacetimeDB, coordinator, llama-server). Helpers: M5 Air .174 and M5 Air .204. Date: November 8, 2026; model became healthy around 07:08 UTC. Source snapshot: 66c56373c72af4399e8f3e675396975fb069118b; all 196 Git blobs verified.

## Observed acceptance

- Coordinator: SERVING on 8080 with m5-air-174 and p5-air-204.
- cluster_status: SERVING, endpoint http://192.168.8.113:8080/v1.
- GET /health: {"status":"ok"}.
- All three node rows online READY before launch; both helper start commands ok.
- RPC model buffers: .174 2474.58 MiB; .204 2290.47 MiB; M3 MTL0_Mapped 6776.85 MiB. Model and KV/compute buffers on all three physical Macs appear in raw load log.
- POST /completion, prompt "Question: What is 2 + 2? Answer with only the number.\nAnswer:" returned 4, with Gemma thought markers in raw output.
- POST /v1/chat/completions, user "What is the capital of France? Reply with only the city name.", max_tokens=128, temperature=0, chat_template_kwargs.enable_thinking=false: assistant content **Paris**, finish_reason stop.
- Chat usage 27 prompt tokens, 2 completion tokens. Tiny smoke-request timing is not a benchmark.

## Original failure and repair

Coordinator admin-only failure was fixed by passing the existing local publisher login directly into its environment; no token included here.

Coordinator preflight then failed E_RPC_UNREACHABLE before llama-server spawned. Python returned errno 65 No route to host while Apple nc connected. Direct llama-server RPC device probe also failed both peers. Route was correctly en7, including explicit source binding. P1/P5 independently confirmed agent-owned wildcard RPC listeners, no competing clients, and P1-to-P5 Python connectivity.

Apple Network.framework gave the decisive result: unsatisfied (Local network prohibited), interface en7; unsatisfiedReason localNetworkDenied. UI automation could not alter settings because assistive access was denied.

A named Pool Coordinator.app with NSLocalNetworkUsageDescription was launched through macOS normally. Its probe changed from localNetworkDenied to satisfied/ready, then started the coordinator and llama-server children. No privacy database editing, SSH workaround, or preflight bypass was used. The Swift compiler defaulted to minimum macOS28 while this machine runs27.2; building launcher with -target arm64-apple-macosx15.0 resolved LaunchServices -10825.

Local launcher/config paths: p4-runtime/Pool Coordinator.app, run-coordinator.py, coordinator.toml. Runtime source is unchanged. min_helpers=2, ctx_size=4096, ready_timeout_s=300, flash_attn=on, extra_args -ctk q8_0 -ctv q8_0, public_host=.113. Model: p4-runtime/downloads/gemma-4-12b-it-Q4_K_M.gguf.

## Limits / next work

This records one real three-Mac model answering through coordinator tooling. It does not establish repeated cold-start acceptance, a live-join test, benchmark results, 26B readiness, or a second teammate independently reproducing the run. Helpers should remain stable while the serving endpoint is verified remotely. Earlier helper registration success alone was insufficient; the successful response and raw placement evidence are included here.
