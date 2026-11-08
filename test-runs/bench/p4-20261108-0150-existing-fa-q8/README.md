<!-- CONCEPT: measured existing 12B pool. WHY: WP-8 reproducible results. DEPENDS ON: bench ebc1863 and serving plan-1794121655-1. AFFECTS: performance claims. -->
# P4 three-Mac 12B benchmark

Completed November8 01:56CT. Bench commit ebc1863. Model gemma-4-12b-it-Q4_K_M, ctx4096, thinking=false, one warmup then three measured repetitions per short/long/edit prompt. Main M3.113; M5 helpers.174/.204. All9 measured requests succeeded.

**Existing pool configuration:** flash_attn=on, KV q8_0/q8_0, no speculative decoding. These are NOT the driver plain-no-flags B2 baseline; compare only against an equivalently configured baseline. Plan1794121655-1. No26B/livejoin claim.

| scenario | model | machines | prompt | ok/runs | TTFT ms (median) | gen tok/s (median, min-max) | total ms (median) | size ratio |
|---|---|---|---|---|---|---|---|---|
| pool_part1 | gemma-4-12b-it-q4_k_m | 3 | short | 3/3 | 587 | 10.9 (10.8-11.0) | 12380 | n/a |
| pool_part1 | gemma-4-12b-it-q4_k_m | 3 | long | 3/3 | 749 | 11.2 (11.2-11.8) | 46348 | n/a |
| pool_part1 | gemma-4-12b-it-q4_k_m | 3 | edit | 3/3 | 4650 | 11.2 (10.9-11.9) | 32711 | n/a |


Raw per-request JSONL and CSV retain warmups and timings. The table excludes warmups. This captures the original successful3Mac service before the later helper-disconnect/crash. The server is no longer that healthy run; see issue11 for current deployment state. Reported only in the morning due to a stopped P4 session.
