# Test runs on the wired network (Agent P1, Preston's M5)

One folder per run: `run-NNN_HHMM_<what>/` with `log.md` (every command, output and timestamp) and the raw
llama.cpp logs. Newest last. Result: ✅ works · ❌ failed · ⏳ in progress · ⚪ prep only.

| Run | Started (CT) | What | Result | What we learned |
|---|---|---|---|---|
| run-001 | Sat 22:29 | Network scan + 0.5B solo baseline on M5 | ⚪ | 3 Macs wired on 192.168.8.x (M5 .174, .113, .204), RTT 0.6-0.7 ms; nothing listening on helpers yet. Solo 0.5B on M5: 33-53 tok/s (M5 under other load; earlier tonight ~180). |
| run-002 | Sat 22:33 | Checkpoint A: 0.5B split M5 (main) + .113 (M3 Air) + .204 (M5 Air) | ✅ | Layers on all 3 (7/9/8+out), 4/4 answers over LAN. Load cold 4.3 s, 2nd 3.2 s. Pool 31-45 tok/s vs solo 33-53 (tiny model: hops only cost). Helpers survive main restart. Default split (no -ts) works. Watch: zsh word-splitting in launch scripts. |

Shared files: `ask.sh <host:port> [label]` (one chat request → tok/s + answer), `web/` (dashboard test copy with real IPs, served on :8000).
| run-003 | Sat 22:52 | Live token-flow view (web/flow.html + flowstats.py) on the run-002 pool | ✅ | Every token visits all 3 Macs (M5→.113 L0-6→M5→.204 L7-15→M5 L16-23+out). Measured ~6-9 KB out + 4-5 KB in per token per link (~300 KB/s at 40 tok/s). nettop must run via a pty (piped = 30 s buffering). |
| run-004 | Sat 22:58 | One watched run (page triggered via web/trigger.json) | ✅ | Real generation ~2 s, peak ~480-560 KB/s up per link; links idle otherwise. Replay at 10× runs ~30-40 s behind real time. |
| run-005 | Sun 01:06 | Checkpoint B prep: netcheck RTT from .174 (main now M3 .113) | ✅ | All pairs < 1 ms p50 (.174→.113 0.94, .174→.204 0.44, .204→.113 0.70). Blocker found same time: macOS Local Network privacy on the M3 (`localNetworkDenied`, errno 65) stops coordinator/llama-server dialing helpers; launch from Terminal.app/.command or ssh. |
| run-006 | Sun 01:10 | **Checkpoint B: Gemma 12B on 3 Macs via SpacetimeDB + coordinator** (main M3 .113, helpers .174 + .204) | ✅ | LAUNCHING → SERVING ~35 s; ~11 tok/s gen (10.4-11.5); real answer. Gemma 4 puts thinking in `reasoning_content`; max_tokens 128 gives empty `content` → raise max_tokens or disable thinking for bench/dashboard. |
