# Brief: worker "principles"

Your name: `principles`. Notes folder: `research/principles/`. Check-ins: `research/checkins/principles-NN.md`.

Goal: from first principles (not from EXO), figure out how to build a shared AI server out of several ordinary
computers, and exactly what the team must build to make it work across different kinds of devices: Apple Silicon
MacBooks (the demo) and older Windows/Dell machines with no discrete GPU (the real customer). Go deep: papers,
engineering blogs, Reddit and HN threads with real measurements, project docs and code. Not surface level.

## Sections (one per run)

1. **The physics.** What limits LLM inference on one device (memory capacity, memory bandwidth, compute; prefill vs
   decode) and across devices (network bandwidth and latency per token). The ways to share work: request routing,
   pipeline/layer parallel, tensor parallel, expert parallel, speculative decoding across devices, batching. For each:
   what crosses the network per token (bytes), and expected speedup or slowdown. Back-of-envelope numbers for an
   8B and a 70B model on 2 to 4 MacBooks over Wi-Fi, Ethernet and Thunderbolt.
2. **The literature.** Papers and serious write-ups on inference over heterogeneous consumer devices (e.g. Petals,
   prima.cpp, distributed-llama, EdgeShard, LinguaLinked, HexGen, Helix, Galaxy, others you find). For each: method,
   hardware, measured results, and what it means for us.
3. **The tools that exist.** llama.cpp RPC backend, MLX distributed (mx.distributed, ring/MPI/Thunderbolt), distributed-
   llama, prima.cpp, vLLM/Ray, others. How each "calls" a remote GPU/CPU, what runs on Mac and Windows, and real
   benchmark numbers from users (Reddit/GitHub/blogs, dated).
4. **Mixed devices.** How to make Macs (Metal) and Windows/Linux CPU machines work in one pool: backend
   compatibility, quantization formats, weighting work by device speed, what happens when a device is slow or leaves.
   Specifically: can a Windows Dell join a Mac pool with each tool, and how?
5. **The control plane.** Discovery (mDNS, etc.), join flow and auth (join codes, QR code onboarding), health checks,
   placement/scheduling, failure handling, security on an office LAN. Patterns from real systems (Tailscale, k3s,
   Kubernetes join tokens, etc.).
6. **Recommendation.** The architecture and build plan for a ~20-hour hackathon: what to use, what to build, the demo
   moment, and the risks. Numbers for what the demo will realistically show.
