# run-006: Checkpoint B, Gemma 4 12B Q4_K_M on 3 Macs via SpacetimeDB + coordinator (Sun ~01:10 CT)

Main = M3 Air .113 (SpacetimeDB `pool`, coordinator, llama-server :8080, main agent `m3-air-113`, started by P4 from Terminal.app after the Local Network fix).
Helpers = M5 Air .174 (`m5-air-174`, P1) and M5 Air .204 (`p5-air-204`, P5); agent-started `rpc-server -H 0.0.0.0 -p 50052 -d MTL0 -c`.

## Result: ✅ SERVING + real answers
- cluster_status: STOPPED → LAUNCHING (plan-1794121655-1) → **SERVING in ~35 s** (event 17 `llama_server_ready`), endpoint `http://192.168.8.113:8080/v1`, /health 200.
- .174 rpc-server: ESTABLISHED from .113; received ~2.61 GB from main (its layer share of the 6.6 GiB model), sent ~31 MB.
- Asked from .174 (`ask.sh`, cache_prompt false):

| ask | gen tok/s | tokens | prompt tok/s |
|---|---|---|---|
| 1 | 11.2 | 128 (all reasoning) | 27 |
| 2 | 11.5 | 128 (all reasoning) | 53 |
| 3 | 10.4 | 128 (all reasoning) | 51 |
| 4 (max_tokens 400) | 11.3 | 356, finish=stop | |

Answer 4: "Pooling laptops optimizes hardware utilization and reduces costs by allocating shared devices based on real-time demand rather than permanent individual assignments."

## Learned
- **Gemma 4 thinks first**: tokens go to `message.reasoning_content`; `content` stays empty until thinking ends. With max_tokens 128 the answer is empty. For bench + dashboard `ask`: raise max_tokens or turn thinking off (VERIFY in b8665: `--reasoning-budget 0`, or request `chat_template_kwargs: {"enable_thinking": false}`). TTFT measured on `content` would include all thinking time.
- The blocker before this run was macOS Local Network privacy on the M3 (see run-005), not the helpers.
- Layer split not captured here (it's in llama-server's log on the M3, P4).
