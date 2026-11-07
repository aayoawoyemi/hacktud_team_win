## Check-in principles 02: The literature

Full notes: `research/principles/02-literature.md`

**Found (most important first, max 8 bullets, each with a source):**
- Splitting layers by memory share (the method EXO used) is badly slow on mixed devices. In prima.cpp's test, EXO gave an M1 9 of 32 layers of Llama 3 8B and ran at 263 ms/token. The best single device alone ran at 15 ms/token. prima.cpp's Halda solver assigns layers by measured speed, drops any device that would get only 1 layer, and cut time per token by up to 31x (prima.cpp, arXiv 2504.08791, ICLR 2026, Table 4 and §4.2).
- prima.cpp on a home Wi-Fi router (320-610 Mbps, 3-7 ms) ran Llama 3 70B Q4K at 674 ms/token (1.5 t/s) on 37 GiB of combined memory across a Mac M1, two Linux PCs and phones. Speed did not depend on bandwidth above 50 Mbps but rose with round-trip time (Fig. 19).
- Tensor parallel over Wi-Fi measured about 150 ms per all-reduce for distributed-llama, so 8B ran at 1,150 ms/token (prima.cpp Table 4). Section 1 assumed 5 ms per all-reduce, 30x too optimistic for real software.
- Tensor parallel on a wire does scale. distributed-llama, CPU only, Llama 3.3 70B Q40 on Mac mini M4 Pro 24 GB: 3.31 t/s on 2 nodes, 6.51 t/s on 4 over TB5 TCP, and 6.04 t/s on 4 over 10 GbE (b4rtaz/distributed-llama discussion #147, Jan 8 2025). Each token moved about 1.4 to 1.6 MB per node pair.
- For many users at once, the serving papers all keep several copies of the model and route each request to the least-loaded one. Helix (ASPLOS 2025) reports up to 3.3x throughput, HexGen (ICML 2024) up to 4x request rate, and HexGen-2 (2025) up to 2x by running prompt reading and generation on different devices.
- Parallax (GradientHQ, arXiv 2509.26182, Sep 2025, 1,421 stars, last push Jul 2026) is open source and runs Macs via MLX LM with continuous batching and paged KV cache. It keeps several layer-split copies of the model and picks a chain of stages per request. It reports up to 3.6x throughput (1.58x average) over HexGen on Qwen3-32B. It is the closest existing design to "office pool serving many users".
- Speculative decoding: a 2026 paper found that every method it tested lowered single-request throughput on four Apple machines (arXiv 2610.31479, Oct 2026). Other 2026 papers say drafting on a separate device helps capacity with many users, not single-request latency (arXiv 2606.25091; DiP-SD 2604.20919, 17.89x in simulation only).
- Expert parallel for DBRX across 2 to 4 M2 Ultra Mac Studios: compute time per expert was about the same as communication time, so latency mattered more than bandwidth (arXiv 2506.23635, Jun 2025).

**Surprises / things that change the plan:**
- Joining more devices can make the pool slower. The scheduler needs a "fast enough to help" test at join time and must be allowed to give a device zero layers. That also makes a good honest demo moment: show the dashboard deciding that an old Mac should serve its own small model instead of slowing the big one.
- Parallax may already be most of our "route + pool on Macs" backend. I had it nowhere on the list before. It should be tested next to EXO and llama.cpp RPC in section 3.
- Drop speculative decoding from the demo ideas. On Apple Silicon it measured slower.

**PRD suggestions:**
- Open question 2: add Parallax (GradientHQ/parallax: MLX on Mac, pipeline split, continuous batching, per-request routing) as a candidate.
- Must have: "Place layers by measured speed (benchmark at join), not by memory; a device can get zero layers." Cite prima.cpp's 263 vs 15 ms/token.
- Open question 3: tensor parallel over Wi-Fi measured about 150 ms per all-reduce (8B at 1.15 s/token), so it is ruled out for Wi-Fi. On 10 GbE/TB5 it is about 6 t/s for 70B on 4 M4 Pro (distributed-llama).
- Later/roadmap: "Prompt reading on the newest chip, generation on the highest-bandwidth chip" (HexGen-2-style split) for long office documents.

**Questions for Aayo:**
1. Still pending and still decisive: the Mac count, chips and RAM. With mixed generations (for example M1 Air plus M4 Pro), speed-aware placement is a visible feature. With identical Macs it matters less.

**Next section:** Section 3, the tools: llama.cpp RPC, MLX distributed (ring/JACCL), distributed-llama, prima.cpp, Parallax (added), vLLM/Ray. For each: how it reaches remote compute, what runs on macOS and Windows, dated user benchmarks, and the install/run steps on a Mac in under 30 minutes. I will prioritise Parallax and llama.cpp RPC (the two with the best route + pool fit) and look at the code paths for layer placement and request routing.
