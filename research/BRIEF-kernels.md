# Brief: worker "kernels"

Your name: `kernels`. Notes folder: `research/kernels/`. Check-ins: `research/checkins/kernels-NN.md`.

Goal: find out, physically and at the code level, how people are squeezing maximum speed and minimum latency out of
GPUs right now, and turn that into concrete techniques the team can use in its compute pool (demo hardware: Apple
Silicon MacBooks; real customers: older Windows/Dell machines). The team wants to be able to say "we built this on
technique X, and it measurably speeds up our pool" and have that be true. Every claim needs a source and, where we
would use it, a way to measure it ourselves this weekend.

Leads to start from (verify them; do not trust headlines):
- **"Cracking the 3090":** late October 2026 reports of a firmware exploit that removes a half-speed AI (tensor core)
  limit on NVIDIA RTX 20/30/40 GPUs. Reported numbers: RTX 3090 BF16 77.5 to 129.6 TFLOPS on Linux; RTX 3090 Ti up to
  59% higher ComfyUI throughput (r/StableDiffusion, Oct 23 2026); Tom's Hardware / VideoCardz / TechTimes coverage.
- **A company-run kernel challenge to write the fastest kernels.** Aayo does not remember which. Candidates: GPU MODE
  kernel leaderboard (NVIDIA blog on cuda.compute topping it), NVIDIA SOL-ExecBench (B200 leaderboard), KernelBench,
  AMD Developer Challenge, others you find. Identify the most likely one(s), and list them for Aayo in check-in 1.

## Sections (one per run)

1. **The 3090 unlock + identify the kernel challenge.** Exactly what the limit was (which instruction/precision path,
   e.g. FP16/BF16 with FP32 accumulate on GeForce vs workstation cards), how it was found, how the exploit works
   (firmware, driver, what is patched), who did it, the real measured gains and on which workloads, risks/legality,
   and NVIDIA's response. Then: which kernel challenge Aayo most likely means, with 2-3 candidates and why.
2. **The kernel challenge in depth.** Rules, problems, hardware, scoring, winning entries and the techniques they used
   (tiling, tensor cores, async copies/TMA, warp specialization, fusion, quantized kernels, autotuning, AI-written
   kernels). Read actual winning code or write-ups, not just the leaderboard.
3. **What dominates LLM inference speed in practice.** Decode is memory-bandwidth bound: kernel fusion, quantization
   formats (Q4/Q8, MXFP4, etc.) and their kernels, KV cache tricks, speculative decoding, batching, FlashAttention-style
   attention, CUDA graphs/launch overhead. Which of these matter at batch size 1 on a laptop.
4. **Apple Silicon specifically.** Metal/MLX kernel performance, unified memory bandwidth per chip (M1 to M5), custom
   Metal kernels in MLX/llama.cpp, what fraction of peak bandwidth real runtimes hit, and what a hackathon team could
   realistically speed up. Also the network side: overlapping communication with compute, compressing activations.
5. **Applying it to our pool.** A ranked list of techniques we could actually use or showcase this weekend, each with:
   expected gain, effort, how we would measure it live in the demo, and the honest claim we could make. Flag anything
   only applicable to NVIDIA (the Dell customers' machines may have none).
