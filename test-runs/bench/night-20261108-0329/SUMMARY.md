# Night bench 20261108-0329

Main: 192.168.8.174 (m5-air-174), single Mac, no helpers. llama-server: /Users/preston/llama-b8665/llama-b8665/llama-server
Warmup 1, repeats 3, ctx 4096, prompts bench.toml (use = bench).

| Run | What | Status | Load s | RPC devices in log |
|---|---|---|---|---|
| B1 | 12B, M5 Air (.174) alone | OK | 10 | 0 |
| B1s | 12B, M5 Air (.174) alone, ngram speculative | OK | 5 | 0 |

## B1: 12B, M5 Air (.174) alone

Flags: none. Load 10s.

| scenario | model | machines | prompt | ok/runs | TTFT ms (median) | gen tok/s (median, min-max) | total ms (median) | size ratio |
|---|---|---|---|---|---|---|---|---|
| single_fit | gemma-4-12b-it-q4_k_m | 1 | short | 3/3 | 998 | 7.2 (7.1-7.4) | 18910 | n/a |
| single_fit | gemma-4-12b-it-q4_k_m | 1 | long | 3/3 | 1418 | 7.2 (7.2-7.3) | 72422 | n/a |
| single_fit | gemma-4-12b-it-q4_k_m | 1 | edit | 3/3 | 5184 | 7.2 (7.1-7.3) | 48548 | n/a |

## B1s: 12B, M5 Air (.174) alone, ngram speculative

Flags: --spec-type ngram-simple. Load 5s.

| scenario | model | machines | prompt | ok/runs | TTFT ms (median) | gen tok/s (median, min-max) | total ms (median) | size ratio |
|---|---|---|---|---|---|---|---|---|
| single_fit | gemma-4-12b-it-q4_k_m | 1 | short | 3/3 | 922 | 7.4 (7.4-7.5) | 18284 | n/a |
| single_fit | gemma-4-12b-it-q4_k_m | 1 | long | 3/3 | 1406 | 7.0 (6.9-7.0) | 74847 | n/a |
| single_fit | gemma-4-12b-it-q4_k_m | 1 | edit | 3/3 | 5023 | 12.4 (12.3-12.4) | 30366 | n/a |


## Notes (Agent P2)

- **Label fix:** bench-night.sh hard-codes "M3 alone" for B1/B1s; these ran on **Preston's M5 Air (.174)** (the M3 was unreachable for agents tonight). Edited above.
- **Speculative decoding (ngram-simple) works where output copies input:** `edit` 7.2 → **12.4 tok/s (1.7x)**, total 48.5 s → 30.4 s; draft acceptance **79%** (178/224). On `short`/`long` (fresh text) no gain: acceptance 3.6%, 7.4 vs 7.2 and 7.0 vs 7.2 tok/s.
- ⚠ **7.2 tok/s alone is lower than the 3-Mac pool's ad hoc 10.8-11 tok/s** (P1/P2, 1:10-1:45am). Don't quote "pool faster than one Mac" from this: the M5 was not idle (P1's `nettop` at ~125% CPU, browser with the animated dashboard, the HF download of the 26B over Wi-Fi), it is fanless and had been running for hours, and the pool numbers were not `bench` runs. Re-run B1 with the M5 idle (or on the M3) before comparing with B2.
- Thinking off (`enable_thinking=false`), warmup 1 + 3 repeats, ctx 4096, b8665, flash attention auto → enabled, all 49 layers on MTL0.
