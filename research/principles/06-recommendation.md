# 06: Recommendation (one page)

Worker: principles. Written Sat Nov 7, 4:15pm CT. Deadline: Sun 12:00pm (19 h 45 min). Devpost team registration by 11pm Sat.

## The build: 3 pieces

1. **Pool:** EXO 1.0 on the 3 Macs (M5 24, M3 16, M5 24), one pipeline instance of
   `mlx-community/NVIDIA-Nemotron-3-Nano-30B-A3B-MLX-6Bit` (25.7 GB, bigger than any one Mac's RAM), placed with
   `min_nodes: 3`. Fallback is the 5-bit build (21.7 GB, still over any Mac's default GPU budget of ~16 GB). Expect
   ~15 to 20 t/s decode.
2. **Spacetime backend** (design owned by the `spacetime` worker): devices, jobs, agent steps, token rows, live
   dashboard. A TypeScript worker on the coordinator Mac claims jobs, calls EXO at `localhost:52415`, and writes
   results back through reducers. Spacetime procedures cannot reach LAN addresses (spacetime-01).
3. **One Nemotron agent (8090 track):** modernizes one legacy program. Loop: plan → `list_files` / `read_file` →
   `write_file` → `run_tests` → fix → done. Tools run in a sandbox folder; every step is a Spacetime row. Thinking off
   for tool steps.

Stretch, only after a recorded good run exists: QR join, Rebalance button, routing model, speed-of-light meter.

## Build order (from 4:15pm Sat)

| When | Hours | Pool (1 person) | Spacetime (1 to 2 people) | Agent (1 person) |
|---|---|---|---|---|
| 4:15 to 6:15pm | 2 | Download 5- and 6-bit to every Mac (~47 GB each; copy over LAN). Run test plan 05 §A4 with `bench_openai.py` and `tool_test.py`. Pick 5- or 6-bit. | Module skeleton: `device`, `job`, `step`, `token_chunk` tables + reducers | Sample legacy repo (1 COBOL program ≤ 200 lines + golden-output tests); agent loop against EXO 4-bit on one M5 |
| 6:15 to 9:15pm | 3 | Pool soak: 20 agent-sized requests; fix discovery/memory issues | TS worker (claim → EXO → stream rows); dashboard: devices, layers per Mac (`/state`), live steps | Tools + plan/step prompts; 9/10 tool loops pass |
| 9:15 to 11:15pm | 2 | Help integrate | Agent writes every step via reducers | Agent runs as a Spacetime job on the pool |
| 11pm | | **Devpost registration** | | |
| 11:15pm to 1am | 1.75 | **Record a full good run** (backup video) | Fix | Fix |
| 1 to 8am | | Sleep in shifts; overnight loop of the agent job, log failures | | |
| 8 to 10am | 2 | Fix overnight failures. Stretch only if everything is green. **Feature freeze 10am.** | | |
| 10 to 11am | 1 | Record the 2-min video (2 takes) | | |
| 11 to 11:45am | 0.75 | Repo public, README, Devpost text (3 tracks) | | |
| 11:45am | | **Submit** (15 min buffer) | | |

## Demo script (2-minute video)

- **0:00 to 0:15. Problem.** "A mortgage office runs a 1990s COBOL interest calculator. Its code and client data can't
  go to a cloud AI. Every laptop here has 16 to 24 GB."
- **0:15 to 0:35. The pool.** Dashboard: 3 Macs, each one's memory, and layer ranges for Nemotron-3-Nano 6-bit
  (25.7 GB, bigger than any of them). Caption: "52 layers split across 3 laptops."
- **0:35 to 1:30. The agent.** Submit "Modernize INTCALC.cbl to Python and prove it with tests." Steps stream in from
  Spacetime: plan, read file, write Python, run tests (fail), fix, tests pass. Each step shows the Macs that served it
  and tokens/s. A full job takes ~2 to 4 minutes, so show it time-lapsed with an on-screen "4x" label.
- **1:30 to 1:45. Private.** Unplug the router's internet cable on camera and run one more step.
- **1:45 to 2:00. Roadmap.** Office PCs join by QR, Windows machines through llama.cpp, and the same agent handles
  other legacy code.

On-screen numbers to expect: 15 to 20 t/s decode (6-bit, 3 Macs), 5 to 20 s to first token per step, 8 to 12 steps per job.

## Top 5 risks

| Risk | Mitigation |
|---|---|
| 1. Nemotron tool calls come back malformed through EXO | Run `tool_test.py` in hour 1. If below 9/10: have the agent ask for a JSON block in plain text and parse it in our worker (no dependence on EXO's tool parser), or switch to Nemotron-3.5-Lightning-30B-A3B-4bit. |
| 2. Macs don't find each other (EXO uses IPv6 multicast discovery) | Travel router only; allow the local-network prompt on every Mac; same EXO build everywhere; USB-C Ethernet if Wi-Fi fails; unicast patch from exo-02 as a last resort. |
| 3. 6-bit doesn't fit with apps open, or disk is short (whole model on every Mac) | Quit apps, check `/instance/previews` before placing; fall back to 5-bit on 3 Macs (still "bigger than any one Mac's GPU budget"); free ~50 GB disk per Mac tonight. |
| 4. Agent too slow or flaky for a live demo | Small legacy sample, thinking off, `max_tokens` caps, gateway retry on 503/`device_unreachable`, never re-run a logged tool; backup video by 1am. |
| 5. Scope creep across 3 tracks | Only the 3 pieces above; stretch list starts after the backup video; feature freeze at 10am. |
