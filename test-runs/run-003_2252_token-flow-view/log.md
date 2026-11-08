# run-003: live token-flow view on the run-002 pool — started Sat Nov 07 22:52:31 CST

Pool: run-002 (M5 main .174, M3 Air .113 L0-6, M5 Air .204 L7-15, M5 L16-23+out). New: web/flow.html (token path animation) + flowstats.py :8001 (nettop bytes/s per link).

## Steps
- 22:52:34 flowstats.py started (pid 70647) for llama-server pid 70000: {"links": {}, "at": 0}
- 22:54:33 ❌→fix: nettop -L 1 takes 30 s when piped (buffering); flowstats now streams nettop -L 0 -s 1 through a pty → 1 sample/s. Restarted (pid 70867).
- 22:55:15 flow.html tested headless (prompt "Name three planets..."): real 39.6 tok/s, 25 ms/token; replays tokens in order through M5→.113→M5→.204→M5.
- Measured during generation (nettop, 1 s): main→.113 ↑340 KB/s ↓194 KB/s; main→.204 ↑252 KB/s ↓151 KB/s → ~6-9 KB out + 4-5 KB in per token per link (hidden state 896×f32 = 3.5 KB + RPC graph commands).
- Fix: dot stops at box edges (was covering main's label); default slow-motion 10×.
- 22:55:23 RESULT ✅ page works; left running: flowstats.py :8001 (pid 70867)
