# Brief: worker "exo"

Your name: `exo`. Notes folder: `research/exo/`. Check-ins: `research/checkins/exo-NN.md`.

Goal: understand EXO (https://github.com/exo-explore/exo) down to the code and patterns, so the team can decide what to
reuse, copy or avoid while building its own solution. Not building from scratch is preferred.

Clone it (shallow is fine) to `C:\Users\aayoa\Desktop\hackathon\exo-src` (outside the team repo). Note the commit hash
and date you studied. EXO has been rewritten at least once; establish which generation the current main branch is and
how it differs from the older, widely discussed version.

## Sections (one per run)

1. **Map the repo.** Languages, top-level layout, entry points, dependencies, how you install and run it on a Mac and
   whether it runs on Windows/Linux. Size (files, lines). Which version/generation. Where the core logic lives.
2. **Cluster formation.** How devices discover each other (mDNS/UDP/manual), how a device joins, identity, topology
   and how it is represented, what network transports it uses (gRPC, TCP, Thunderbolt/RDMA). Exact files and functions.
3. **Splitting the model.** How a model is partitioned across devices (pipeline/layer ring, tensor parallel?), the
   partition algorithm (memory-weighted?), how activations move between devices, which inference engines (MLX,
   tinygrad, others), how heterogeneous devices (Mac + non-Mac) are handled or not.
4. **Serving and UX.** The API (OpenAI-compatible?), request flow end to end, scheduler/placement, model download and
   caching, the dashboard/UI, and the joining experience a user actually sees.
5. **What people say.** Reddit (r/LocalLLaMA, r/MacStudio, etc.), Hacker News, GitHub issues: real benchmarks
   (tokens/sec, hardware, network), what works well, common failures, shortcomings, abandoned features.
6. **Outline and verdict.** A one-page outline of what EXO is, plus: what to reuse directly, what patterns to copy,
   what to avoid, and where the team should start. Include a concrete plan for 2 to 4 MacBooks + 1 Windows laptop.
