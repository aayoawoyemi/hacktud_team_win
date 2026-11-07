# Brief: worker "cppneed" (llama.cpp pass A: top-down from the product)

Your name: `cppneed`. Notes folder: `research/cppneed/`. Check-ins: `research/checkins/cppneed-NN.md`.
Partner worker: `cppcode` does pass B (bottom-up from the llama.cpp source). You do separate but congruent passes:
at the start of each section, read the partner's latest notes in `research/cppcode/` (read only) and reconcile.
Disagreements go in your check-in under "Disagreements with cppcode".

## Context

The team (HackUTD 2026, submission 12pm Sun) is deciding whether to replace EXO with llama.cpp RPC as the pooling
engine, with our own coordinator on top. Read first: `research/PRD.md`, `research/EXO-HOW-IT-WORKS.md` (what EXO does,
so you know what we would have to replace), and `research/principles/03-tools.md`, `04-*.md`, `05-*.md`.
Facts: 3 MacBooks (M5 24 GB, M3 16 GB, M5 24 GB), our own router (venue Wi-Fi isolates devices), model
Nemotron-3-Nano-30B-A3B (hybrid Mamba + MoE + 6 attention layers; GGUF by unsloth: Q4_0 18.2 GB, UD-Q4_K_XL 22.8,
Q4_K_M 24.6, Q5_K_M 26.1). Spacetime is the backend (device registry, jobs, agent steps; procedures cannot reach LAN
IPs, so a TypeScript worker on each Mac does the HTTP calls). A Nemotron agent with tool calls (8090 track: legacy
code modernization) runs on top. Windows/old Dell PCs joining is the customer story.
llama.cpp source: `C:\Users\aayoa\Desktop\hackathon\llama-src` (commit e9a2a44, Nov 7 2026). Do not modify it.

## Your question

"Starting from what the product needs, what is the smallest set of llama.cpp pieces we use, what do we drop, and
what must we build ourselves?"

## Sections (one per run)

1. **Requirements to components.** List every product requirement (pool a model too big for one Mac, routing for
   several users, tool calls + thinking toggle for the agent, join/leave, layer placement by speed, dashboard data,
   offline, Windows node for the roadmap). For each: the llama.cpp component that covers it (rpc-server,
   llama-server flags, `--rpc`, `--tensor-split`, `-c` cache, `--jinja`, slots/parallel, `/metrics`, `/props`,
   `tools/connect` if relevant), the gap, and who builds the gap (coordinator / Spacetime / agent). One table.
   Mark everything we should NOT use (and why) in a "Drop" list.
2. **The coordinator we would write.** Its exact responsibilities and interface: inputs from Spacetime (device rows,
   measured speed, join events), how it computes `--tensor-split` (speed-aware), how it starts/restarts llama-server
   with the `--rpc` list, health checks, what happens on join and leave (restart cost: weights resent unless cached),
   and the HTTP contract the agent and dashboard use. Estimate lines of code and hours. Compare with what EXO gives
   for free (from EXO-HOW-IT-WORKS.md): what we lose, what we gain.
3. **Decision + bring-up.** A clear recommendation (llama.cpp or EXO or both) with the deciding test, and copy-paste
   bring-up commands for 3 Macs (release binaries or cmake build with RPC enabled; exact flags verified against the
   source's argument parsing, not memory).
