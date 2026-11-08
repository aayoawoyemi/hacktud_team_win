# Night bench 20261108-0408

Main: 192.168.8.174 (m5-air-174), single Mac, no helpers. llama-server: /Users/preston/llama-b8665/llama-b8665/llama-server
Warmup 1, repeats 3, ctx 4096, prompts bench.toml (use = bench).

| Run | What | Status | Load s | RPC devices in log |
|---|---|---|---|---|
| B1 | 12B, m5-air-174 alone | OK | 5 | 0 |
| B1s | 12B, m5-air-174 alone, ngram speculative | OK | 5 | 0 |

## B1: 12B, m5-air-174 alone

Flags: none. Load 5s.

| scenario | model | machines | prompt | ok/runs | TTFT ms (median) | gen tok/s (median, min-max) | total ms (median) | size ratio |
|---|---|---|---|---|---|---|---|---|
| single_fit | gemma-4-12b-it-q4_k_m | 1 | short | 3/3 | 330 | 16.1 (15.2-16.1) | 8266 | n/a |
| single_fit | gemma-4-12b-it-q4_k_m | 1 | long | 3/3 | 867 | 10.3 (9.3-15.0) | 50552 | n/a |
| single_fit | gemma-4-12b-it-q4_k_m | 1 | edit | 3/3 | 3740 | 9.3 (8.3-9.4) | 37503 | n/a |

## B1s: 12B, m5-air-174 alone, ngram speculative

Flags: --spec-type ngram-simple. Load 5s.

| scenario | model | machines | prompt | ok/runs | TTFT ms (median) | gen tok/s (median, min-max) | total ms (median) | size ratio |
|---|---|---|---|---|---|---|---|---|
| single_fit | gemma-4-12b-it-q4_k_m | 1 | short | 3/3 | 764 | 9.1 (9.1-9.4) | 14806 | n/a |
| single_fit | gemma-4-12b-it-q4_k_m | 1 | long | 3/3 | 1286 | 8.0 (7.9-8.4) | 65305 | n/a |
| single_fit | gemma-4-12b-it-q4_k_m | 1 | edit | 3/3 | 3889 | 14.5 (14.3-14.6) | 25511 | n/a |


## Notes (Agent P2): idle re-run, the M5 Air throttles

Re-run of night-20261108-0329 with `nettop` stopped (P1) and nothing else heavy on .174 (Preston's browser left as is; HF download over Wi-Fi still running). `offloaded 49/49 layers to GPU`, `using device MTL0 (Apple M5) - 18185 MiB free`. `pmset -g therm`: no thermal/performance warning recorded, before or after (macOS doesn't report GPU throttling there).

**Per request, in run order (gen tok/s):**
- B1, 10:08-10:14Z: short 16.2 / 16.1 / 16.1 / 15.2 → long 15.4 / 15.0 / **10.3 / 9.3** → edit 9.8 / 9.4 / 8.3 / 9.3
- B1s, 10:15-10:21Z: short 9.6 / 9.4 / 9.1 / 9.1 → long 8.2 / 8.4 / 7.9 / 8.0 → edit 14.1 / 14.5 / 14.3 / 14.6

**Reading:** the M5 Air (fanless) runs the 12B at **~16 tok/s cold** and drops to **~8-9 tok/s after ~1.5 min of sustained generation**, then stays there. The first run's flat 7.2 was the same throttled state plus `nettop`. The medians mix both states, so quote the two numbers separately:
- single M5 Air, cold: ~16 tok/s; sustained: ~8-9 tok/s
- ngram-simple on `edit`, sustained: 14.5 tok/s vs ~9 without (**~1.6x**); in the first run 12.4 vs 7.2 (1.7x)

Implication for "pool vs one Mac": compare sustained to sustained. A pool spreads each token's layers over 3 GPUs, so each runs ~1/3 of the time and may throttle less (VERIFY with B2 run for > 2 min).
