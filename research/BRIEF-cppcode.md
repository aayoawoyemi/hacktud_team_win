# Brief: worker "cppcode" (llama.cpp pass B: bottom-up from the source)

Your name: `cppcode`. Notes folder: `research/cppcode/`. Check-ins: `research/checkins/cppcode-NN.md`.
Partner worker: `cppneed` does pass A (top-down from the product). You do separate but congruent passes: at the
start of each section, read the partner's latest notes in `research/cppneed/` (read only) and reconcile.
Disagreements go in your check-in under "Disagreements with cppneed".

## Context

The team (HackUTD 2026, submission 12pm Sun) is deciding whether to replace EXO with llama.cpp RPC as the pooling
engine, with our own coordinator on top. Read first: `research/PRD.md` and `research/EXO-HOW-IT-WORKS.md` (what EXO
does, so you can compare mechanism by mechanism). Facts: 3 MacBooks (M5 24 GB, M3 16 GB, M5 24 GB) on our own router,
model Nemotron-3-Nano-30B-A3B GGUF (hybrid: 23 Mamba-2 + 23 MoE + 6 attention layers, 52 total), agent with tool
calls on top.
llama.cpp source: `C:\Users\aayoa\Desktop\hackathon\llama-src` (commit e9a2a44, Nov 7 2026). Do not modify it.
Cite file:line for every claim about the code.

## Your question

"How does llama.cpp's multi-machine path actually work in the code, what parts are irrelevant to us, and where could
we improve on it (or on EXO) for our setup?"

## Sections (one per run)

1. **How RPC works, mechanically.** `ggml/src/ggml-rpc/ggml-rpc.cpp` and `tools/rpc/rpc-server.cpp`: the protocol
   (commands, message format), what crosses the network at load (weights? `-c` cache by hash?) and per token (how many
   round trips per token per remote device, payload sizes), how the scheduler (`ggml-backend` sched) splits a graph
   across local Metal + remote RPC devices, how `--tensor-split` maps layers to devices, and what `tools/connect` is.
   Compare per-token round trips with EXO's ~4 sequential waits.
2. **Nemotron-H over RPC.** Find the nemotron_h / Mamba-2 / SSM implementation (src/models, llama-arch, llama-model,
   memory/recurrent cache): where recurrent state lives and whether it is placed on the device that owns the layer
   when split across RPC devices; any ops not supported by the RPC backend; GitHub issues on RPC + Mamba/hybrid/
   Nemotron (gh search). Also tool calling for Nemotron in llama-server (chat template, parser, `--jinja`) and the
   reasoning/thinking toggle. Verdict: will a 3-way split of Nemotron-3-Nano work, with confidence level.
3. **Strip and improve.** What in this path is irrelevant to us (list), and concrete improvements we could make
   within the hackathon, ranked by gain/effort, each with the file to touch or the flag to use. Candidates: fewer
   round trips per token, overlapping network with compute, speed-aware split, avoiding weight resend on restart,
   a join/leave without full reload, a security wrapper (rpc-server has no auth). Mark which improvements also beat
   EXO, so the team can honestly say "we improved on X".
