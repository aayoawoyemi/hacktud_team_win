## Check-in principles 01: The physics

Full notes: `research/principles/01-physics.md`

**Found (most important first, max 8 bullets, each with a source):**
- On one device, decode speed is set by memory bandwidth. llama.cpp gets 55 to 78% of spec bandwidth on every M-chip. Llama 3.1 8B Q4_K_M runs at about 17.5 t/s on a base M4, 37 on an M4 Pro and 23 on an M5 (my math from the llama.cpp #4167 table: M4 7B Q4_0 TG 24.1 t/s, M4 Pro 50.7, M5 31.9).
- Splitting a model by layers (pipeline) adds capacity but no speed for one user. Each hop sends only 16 KiB per token (8B) or 32 KiB (70B), so latency matters and bandwidth doesn't. Measured cost: llama.cpp RPC with Llama 3.2 3B on 2 Macs over 2.5 GbE went from 171 to 93 t/s, about 5 ms added per token per extra node (Geerling, beowulf-ai-cluster #17, Dec 10 2025).
- The software costs about as much as the network. llama.cpp RPC on localhost alone ran at 72% of direct speed, and moving from 10 Gb to 50 Gb Ethernet changed nothing (37 to 38 t/s) (r/LocalLLaMA 1q9yd1w, Jan 11 2026).
- Tensor parallel needs 2 all-reduces per layer, so 64 network round trips per token for 8B and 160 for 70B. Over Wi-Fi at about 5 ms each, that adds 320 to 800 ms per token, 5 to 20x slower than one Mac. It only pays off with Thunderbolt 5 RDMA (under 50 μs vs 300 μs over TCP, Geerling Dec 18 2025). With RDMA, EXO on Qwen3 235B went from 19.5 t/s on 1 node to 31.9 t/s on 4. llama.cpp RPC got slower as nodes were added.
- Thunderbolt RDMA needs Thunderbolt 5 on both Macs, macOS 26.2+, and `rdma_ctl enable` run in Recovery OS (Apple TN3205, Mar 2026; MLX PR #2808: "Tb5 only"). Of the MacBooks, only the M4 Pro/Max MacBook Pros and newer Pro/Max models have TB5. Airs and base-chip MacBook Pros fall back to TCP.
- 70B Q4 (42.5 GB) fits on no MacBook under 64 GB. Pooled on 4 base M4 Macs it would run at about 2 t/s and take about 85 s to read a 2,000-token prompt, over any network (my math). On M4 Pro machines: about 4 t/s.
- For an office, long prompts (contracts, client files) make prompt-reading time matter more than generation speed. An 8B model on a base M4 takes about 10 s to read 2,000 tokens. Splitting by layers does not shorten that for a single request.
- The Dell's CPU decode is also limited by memory bandwidth: 8B Q4 runs at about 5 to 10 t/s (bigiron.cc CPU guide, 2026). Per GB of model it holds, it is 2 to 6x slower than a Mac, so in a layer split it slows every token.

**Surprises / things that change the plan:**
- Routing (each machine runs a whole model) beats pooling on speed for any model that fits on one machine. Pooling is only worth it for capacity.
- The best pooled demo is a MoE model, not dense 70B. Qwen3-30B-A3B is about 18.6 GB at Q4, too big for one 16 GB MacBook, and reads only about 2 GB per token, so a 2-Mac split should still reach 20+ t/s (my estimate, not measured). Dense 70B on base MacBooks runs at 2 t/s, which demos badly.
- Venue Wi-Fi is the biggest risk. Campus Wi-Fi often isolates clients from each other and blocks mDNS, so the machines may not see each other at all (not checked at HackUTD). Bring a router or gigabit switch plus USB-C Ethernet adapters.

**PRD suggestions:**
- Open question 1: answer it as "route first, pool as the wow moment". Route any model that fits on one machine. Pool only a model no single laptop can hold, ideally a MoE such as Qwen3-30B-A3B.
- Open question 3: add the expected numbers. 8B: about 17.5 t/s per base-M4 Mac when routed, 11 to 16 t/s split over 2 Macs. 70B: about 2 t/s pooled on base Macs, about 4 t/s on M4 Pro, and about 85 s to read a 2,000-token prompt. Over Wi-Fi, a layer split loses 10 to 30 ms per extra node per token.
- Demo hardware: add "bring our own router/switch plus USB-C Ethernet adapters; do not depend on venue Wi-Fi".
- Must have: add "measure first hour: `llama-bench` (8B Q4_K_M) on every machine plus ping between machines".

**Questions for Aayo:**
1. What exactly are the MacBooks (chip such as M4 / M4 Pro, and RAM)? That decides whether 70B or TB5 RDMA tensor parallel is possible at all.
2. Can the team bring a router or switch and Ethernet adapters? If not, I will plan for Wi-Fi only (layer split only, no tensor parallel).

**Next section:** Section 2, the literature: Petals, prima.cpp, distributed-llama, EdgeShard, LinguaLinked, HexGen, Helix, Galaxy, plus papers that combine speculative decoding with layer splitting. One change: I will focus on papers with measured numbers on mixed consumer devices over Wi-Fi or slow links, and on how they assign layers to devices of different speeds, because that is the open question for the Dell.
