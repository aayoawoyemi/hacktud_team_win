<!--
CONCEPT: main-Mac model inventory with measured facts and unresolved selections.
WHY: WP-11 R11.1/R11.4/R11.5/R11.7; D-07 requires both models locally loaded before demo.
DEPENDS ON: approved O-09 files, WP-5 inspection/fit, WP-1 real loads.
AFFECTS: coordinator paths, download scheduling, demo size claims.
-->
# Models on main

P4 inventory, 2026-11-08 UTC. Main is locally verified M3 Air / 16 GB RAM.
Free space at inspection: approximately 385 GiB. Recheck before each download.

| Role | Requested model | Exact file/source | Bytes / SHA-256 | Quant / layers / KV bytes per token | Load and fit |
|---|---|---|---|---|---|
| Small bring-up | Team suggested Qwen2.5 0.5B or Qwen3 0.6B | Not downloaded or selected by P4 | Unknown | Unknown | Not tested |
| Part 1 | Gemma 4 12B, per P2's #11 assignment | Exact GGUF URL/path pending | Unknown | Unknown | Not tested |
| Part 2 | About 3x largest machine's usable capacity | Not selected | Unknown | Unknown | Not tested |

No inference-model GGUF was found in the accessible Documents/Downloads/Desktop and
searched home folders. Hugging Face cache contains a 731357568-byte Parakeet speech-model
blob; it is not the requested language model and has not been used as a substitute.

models/ and *.gguf are already ignored; verified with git check-ignore.
Shell DNS currently blocks GitHub and SpacetimeDB installer hosts, so pinned runtime
installation and subsequent model download remain blocked. No verified tokens/s exists.

Before acceptance: record the approved source URL/revision, exact file size and SHA-256
(compared to published checksum when available), quantization, layers, KV bytes/token,
chosen context, measured memory budgets, and standalone/pool fit from WP-5.
Check helper free disk for tensor caches and warm each with a successful full load.
A backup copy on USB/another machine remains pending. Never commit the GGUF.
