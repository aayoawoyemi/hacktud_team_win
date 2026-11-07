# 02: The literature on inference across ordinary, mixed devices

Worker: principles. Written Sat Nov 7, 2026. Abstracts read via the arXiv API; prima.cpp and Parallax PDFs read in
full text search; distributed-llama numbers from its GitHub discussion. (measured) = number from the source;
(guess) = mine.

Focus per feedback: measured numbers on mixed consumer devices over slow links, how layers are assigned to devices of
different speeds, and serving many concurrent users across mixed nodes.

## 1. Summary table

| System (date, venue) | Split method | Hardware tested | Network | Headline measured result |
|---|---|---|---|---|
| prima.cpp (arXiv 2504.08791, Apr 2025; ICLR 2026) | Pipelined-ring (layer windows in a ring) + disk offload, Halda layer solver | Mac M1, Linux laptop (i9+3070), desktop (i9+2080Ti), 2 Android/HarmonyOS phones, Intel Mac Air | Home Wi-Fi router, 320-610 Mbps, 3-7 ms per link | Llama 3 70B Q4K at 674 ms/token (1.5 t/s) on 37 GiB total memory; 8B at 15 ms/token vs exo 263 and dllama 1150 |
| distributed-llama "dllama" (GitHub, v0.11.1 Jan 2025) | Tensor parallel, CPU only, 2^n nodes | 1/2/4 × Mac mini M4 Pro 24 GB | Thunderbolt 5 (TCP) and 10 GbE | Llama 3.3 70B Q40: 3.31 t/s (2 nodes, TB5), 6.51 t/s (4 nodes, TB5), 6.04 t/s (4 nodes, 10 GbE) |
| TPI-LLM (arXiv 2410.00531, Oct 2024) | Tensor parallel, star all-reduce, sliding-window weight loading from disk | Low-resource edge devices (emulated + real) | Edge links | ">80% less TTFT and token latency" vs HF Accelerate; finds link latency, not bandwidth, is the bottleneck |
| EdgeShard (arXiv 2405.14371, May 2024) | Pipeline, dynamic programming over device + layer choice | Jetson-class edge devices + server | Edge LAN | Up to 50% lower latency, 2x throughput vs baselines (Llama 2) |
| LinguaLinked (arXiv 2312.00388, Dec 2023) | Pipeline, linear-programming assignment, runtime load balancer | Android phones, high- to low-end | Wi-Fi | Load balancer moves segments off slow phones at runtime (numbers in paper; not extracted) |
| Galaxy (arXiv 2405.17245, May 2024) | Hybrid tensor + sequence parallel, tile-level compute/comm overlap | Heterogeneous edge devices (smart home) | Bandwidth-limited edge | Planning by device capacity; overlap hides sync cost (numbers not extracted) |
| Petals (arXiv 2209.01188, Sep 2022) | Pipeline over internet volunteer servers; client picks a chain of servers | Consumer GPUs worldwide | Internet | BLOOM-176B at ~1 step/s |
| HexGen (arXiv 2311.11514, Nov 2023; ICML 2024) | Asymmetric TP + PP per replica, scheduler for heterogeneous GPUs | Mixed datacenter GPUs | Mixed | Up to 2.3x lower latency deadlines or 4x higher request rate at the same budget |
| HexGen-2 (arXiv 2502.07903, Feb 2025) | Prefill/decode disaggregation on mixed GPUs | Mixed GPUs, OPT-30B, Llama 2 70B | Mixed | Up to 2.0x throughput vs prior heterogeneous serving |
| Helix (arXiv 2406.01566, Jun 2024; ASPLOS 2025) | Max-flow + MILP for joint layer placement and per-request routing | 24 to 42 heterogeneous GPU nodes | Mixed bandwidth | Up to 3.3x throughput; prefill latency -66%, decode -24% |
| Parallax (arXiv 2509.26182, Sep 2025; GradientHQ/parallax, 1,421 stars, last push Jul 2026) | Several pipeline replicas; phase 1 places layers (DP + water-filling), phase 2 picks a chain of stages per request | 5× RTX 5090 + 2× RTX 4090 across data centers; code also runs Macs via MLX | Public internet, ~10 ms | Up to 3.6x (avg 1.58x) throughput and 3.2x (avg 1.66x) lower latency vs HexGen, Qwen3-32B |
| DBRX on Mac Studios (arXiv 2506.23635, Jun 2025) | Expert parallel across 2-4 M2 Ultra | Mac Studio M2 Ultra | Not extracted | Expert compute time ≈ communication time; latency matters more than bandwidth; macOS memory-management overhead had to be removed |

## 2. How layers get assigned to devices of different speed

This is the most useful result for us, and prima.cpp measures it directly.

- **By memory share (EXO's method) is bad on mixed devices.** prima.cpp, Table 4: Llama 3 8B fits on the best GPU
  alone (D3, 2080 Ti, 11 GiB). exo split it by memory ratio into 9/10/13 layers across a Mac M1 (8 GiB), a 3070 and the
  2080 Ti. The M1 became the bottleneck: **exo 263 ms/token vs 15 ms/token** for llama.cpp on D3 alone (measured).
  In their 9-device test (Table 12) exo got 2,500 ms/token on 8B where the best single device gave 35 ms.
- **prima.cpp's Halda solver** models each device's compute, memory bandwidth, disk speed, OS memory reclamation and
  link latency, then solves for layer windows that minimise time per output token. It also drops devices:
  "Weak devices assigned only one layer are removed, since Halda indicates that excluding them would improve
  speed." For small models it "prefers to allocate all layers to one most powerful device, reducing prima.cpp to
  llama.cpp." Devices that are the only network route can stay as relays with zero layers. Halda cut TPOT up to 31x
  vs the memory-ratio partition (prima.cpp §4.2).
- prima.cpp authors, Appendix A.7: "more is not always better, and aggregated memory need not match the model's
  needs."
- **Workloads can be re-partitioned whenever the request queue is empty**, since no layers need to move between
  devices when each device memory-maps the full GGUF file (prima.cpp §3.3). That is a cheap way to handle devices
  joining or leaving.
- EdgeShard uses dynamic programming, LinguaLinked linear programming plus a runtime load balancer, Helix MILP over a
  max-flow graph, Parallax dynamic programming plus water-filling. All of them weight by measured speed and link cost,
  not by memory alone.
- Rule for us: place layers in proportion to measured ms-per-layer (from a short benchmark at join time), cap by free
  memory, and drop any device that would get only 1 to 2 layers. With only Macs at the demo, the gap is smaller than
  prima.cpp's mixed testbed, but an M1 Air next to an M4 Pro is still a ~3.6x bandwidth gap (section 1 table).

## 3. Measured cost of slow links (confirms section 1)

- **Tensor parallel over Wi-Fi:** prima.cpp measured "~150 ms per all-reduce" for dllama on their Wi-Fi testbed;
  "An 8B model needs 64 all-reduce operations", so dllama 8B was 1,150 ms/token (measured). My section 1 estimate
  (5 ms per all-reduce) was optimistic by 30x for this software.
- **Tensor parallel over wire:** dllama, 4 × Mac mini M4 Pro, CPU only, 70B Q40: 6.51 t/s on TB5 TCP vs 6.04 t/s on
  10 GbE (measured, discussion #147, Jan 8 2025). Per token on 2 nodes the root sent 1,392 kB and received 1,610 kB
  (dllama log: "S 1392 kB R 1610 kB"). Wired TP with a lean CPU implementation does scale (3.31 → 6.51 t/s, 2 → 4
  nodes). This matches my section 1 guess of 7 to 9 t/s for 4 M4 Pro on Ethernet.
- **Pipeline over Wi-Fi:** prima.cpp Fig. 19 (Llama 3 70B): TPOT is "insensitive to bandwidth changes, except under
  extremely low bandwidth (e.g., lower than 50 Mbps)" and rises with RTT. Each hop sends e × 4 bytes (FP32 hidden
  state), as I assumed.
- TPI-LLM (same first author as prima.cpp, Zonghang Li, as far as I can tell) argued in Oct 2024 for tensor
  parallel on edge devices; by Apr 2025 the same group built prima.cpp on pipeline parallel for Wi-Fi and measured TP
  (dllama) as the slowest option there. I read that as the field moving to pipeline for Wi-Fi.

## 4. Serving many users at once on mixed nodes

An office has 5 to 20 people sending requests at the same time. The single-request papers (prima.cpp, TPI-LLM) say
"no queueing" and optimise latency for one user. The serving papers optimise throughput:

- **Helix** (ASPLOS 2025): treats GPUs as nodes and links as edges in a flow graph; the max flow is the cluster's
  throughput limit. One MILP chooses which layers sit on which node and how each request is routed. Up to 3.3x
  throughput over prior systems on 24 to 42 mixed GPU nodes. Takeaway: placement and routing have to be decided
  together; a slow link caps everything behind it.
- **HexGen** (ICML 2024): each replica can use a different TP/PP shape on different GPU types; up to 4x higher
  request rate at the same cost.
- **HexGen-2** (Feb 2025): run prefill on one set of devices and decode on another (disaggregation), choosing
  devices per phase; up to 2x throughput. A 2026 survey of heterogeneous prefill/decode deployments (arXiv
  2606.29708, Jun 2026) says this is now in production: "prefill on cost-efficient ... accelerators, decode on
  bandwidth-strong ones". Relevant to Macs: M5 prefill is ~3x M4 at similar bandwidth (section 1), so in a mixed
  office the newest chip should take prompt reading and the highest-bandwidth chip decode (idea; moving KV caches
  between Macs adds 128 KiB per prompt token for 8B, so 256 MB for a 2,000-token prompt, ~2 s on 1 GbE (calc)).
- **Parallax** (Sep 2025, open source, Macs supported via MLX LM, continuous batching and paged KV cache on Mac):
  keeps several pipeline replicas and, per request, stitches a chain of stages from different replicas to balance
  load. Up to 3.6x throughput vs HexGen on Qwen3-32B over 10 ms links. This is the closest published design to "an
  office pool that serves many users". Section 3 should test whether it runs on our Macs.
- **Petals:** clients choose a chain of servers holding consecutive blocks and re-route around failed servers;
  ~1 step/s for 176B over the internet.
- **Distributed speculative decoding for many users:**
  - "Speculation at a Distance" (arXiv 2606.25091, Jun 2026): drafting on the user's device and verifying on a
    server rarely lowers single-request latency, but lets a saturated server serve (1 + γ·t_d/t_v) times more clients
    at the same per-client speed. "DSD should therefore be evaluated primarily by multi-tenant capacity."
  - DiP-SD (arXiv 2604.20919, Apr 2026): many users draft locally (Qwen3-1.7B), the edge server verifies in batches
    (Qwen3-32B); up to 17.89x throughput over plain decoding and 1.93x over plain decoding with greedy batching
    (numerical results, not a real testbed).
  - Caution, Apple-specific: "Evaluating the Applicability of LLM Inference Optimizations on Apple Silicon" (arXiv
    2610.31479, Oct 2026): on four Apple machines with single-request decode, "all evaluated speculative decoding
    methods reduce throughput on Apple, despite reported 2-6 times gains on server GPUs." prima.cpp's 26 t/s for 32B
    with speculative decoding was on a testbed with NVIDIA GPUs. Do not plan a speculative-decoding speedup on Macs
    without measuring it.

## 5. Mixed OS and backend support in these systems

- prima.cpp: macOS (Metal and CPU), Linux (CUDA, CPU), Android/HarmonyOS (CPU via Termux). Mixed-OS ring tested.
  Windows not in the testbed (to check in section 3). Q4K GGUF on every platform, no conversion to FP16.
- dllama: Linux, macOS, Windows, CPU only (ARM and x86 AVX2). Only 1, 2, 4 ... 2^n nodes; max nodes = KV heads.
- exo (as tested by prima.cpp in 2025): memory-ratio partition, Q4 + FP32. exo 1.0 is MLX-only now (see exo
  worker).
- Parallax: GPU nodes via SGLang/vLLM (Linux), Mac nodes via MLX LM. No Windows CPU backend listed in the README.
- For the customer story (old Windows/Dell machines): dllama runs on Windows CPU but needs 2^n identical-ish nodes;
  llama.cpp RPC runs on Windows. No paper above tested a Windows CPU node in a pool.

## 6. What this means for us

1. Assign layers by measured speed, not by memory, and drop weak devices. prima.cpp shows memory-ratio splitting
   (EXO's old method) made 8B 17x slower than one good device. Our scheduler should run a 10-second benchmark at join
   time and keep a "fast enough to help" threshold.
2. For a single pooled model on Wi-Fi, use pipeline (layer split). Measured TP over Wi-Fi is ~150 ms per all-reduce.
   TP is only an option on a wire (dllama 6 t/s for 70B on 4 M4 Pro over 10 GbE) or TB5 RDMA.
3. For many users, the published answer is replicas plus a scheduler (Helix, HexGen, Parallax): several copies of the
   model (whole or pipelined), with each request sent to the least-loaded chain. That is our "route" mode, and the
   pooled model becomes one more replica.
4. Parallax already does Mac pooling with continuous batching and request routing. It is a candidate base or a
   design to copy; test it in section 3.
5. Skip speculative decoding for the demo; on Apple Silicon it measured slower in Oct 2026.
6. A prefill/decode split by chip type (M5 reads prompts, high-bandwidth chip writes) is a good roadmap item for
   long office documents, not a hackathon item.
