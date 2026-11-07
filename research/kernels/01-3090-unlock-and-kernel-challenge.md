# 01: The GeForce tensor-core unlock ("Cracking the 3090") and which kernel challenge Aayo means

Researched Sat Nov 7 2026. Fetch date for all links: 2026-11-07.

## Part A: The unlock

### What the limit is

- On GeForce RTX 20/30/40 cards, tensor-core matrix math with **FP16, BF16 or TF32 inputs and FP32 accumulation**
  issues at **half rate**. The same die sold as a workstation card (GA102 = RTX 3090 and RTX A6000; AD102 = RTX 4090
  and RTX 6000 Ada) runs it at full rate. INT8 is not improved by the unlock (TechTimes, Oct 28 2026). FP16 inputs
  with FP16 accumulate were already full rate on GeForce (my reading of NVIDIA's Ampere/Ada whitepaper spec tables;
  not re-checked today).
- This is long-known product segmentation (NVIDIA's whitepapers list the halved "FP16 Tensor TFLOPS with FP32
  accumulate" for GeForce). What is new in Oct 2026 is a public way to turn it off.
- Mechanism in hardware: two "speed selector" registers per GPU (from `docs/How Ampere was unlocked.md` in
  Zagitta/gpu-unlock):

| Register | Address | GA102 default | Governs |
|---|---|---|---|
| `FEAT_OVR_SM_SPD` (SS0) | `0x0082381c` | `0x00000001` | Eight 4-bit fields: DP, FFMA, FMLA16, FMLA32, IMLA0-3 |
| `FEAT_OVR_SM_SPD_1` (SS1) | `0x00823820` | `0x00000001` | One field, IMLA4 |
| `FEAT_OVR_PLM` (write-protect mask for both) | `0x00823804` | `0xffffff8f` | Only privilege level 3 (heavy-secure) may write |

  Each 4-bit field: bit 3 = override enable, bits 2:0 = rate (00 full, 01 half, 10 quarter and 11 eighth are
  unconfirmed). The unlock writes `0x88888888` to SS0 and `0x8` to SS1 (override on, full rate).

### How the exploit works (firmware boot chain, not a VBIOS flash, not a driver patch)

Source: https://github.com/Zagitta/gpu-unlock/blob/main/docs/How%20Ampere%20was%20unlocked.md

1. On Ampere and later, the driver boots the GPU's GSP (a RISC-V management core) by handing a signed "Booter"
   firmware to the SEC2 security processor, which verifies and decrypts GSP-RM and starts it.
2. NVIDIA had published both debug and production Booter blobs. The debug blob was encrypted with a known public
   test key (key index 37), so researchers decrypted it, disassembled it, and found a stack buffer overflow. Production
   Booter is "remarkably similar", so the same bug exists there.
3. A ROP chain (the community "V67 payload") turns the overflow into one 4-byte write while SEC2 runs in heavy-secure
   mode (L3). The chain is too small for two writes, so it writes `0xFFFFFFFF` to `FEAT_OVR_PLM`, which makes SS0/SS1
   writable from the host. The host then writes the full-rate values.
4. It runs before the stock NVIDIA driver loads. It does not persist across power loss; it has to run every boot.
   The same Booter flaw was first used for `cmpunlocker` (CMP 170HX mining cards, memory unlock), which the doc
   credits as the prior work.

### Who and when

| Project | Author | Covers | Form |
|---|---|---|---|
| gpu-unlock (repo created 2026-10-18, last push 2026-10-29, 17.8k stars on Nov 7) | Zagitta (GitHub) | RTX 30: GA102/103/104/106 | Linux kernel module, DKMS, GPL-2.0, v1.0.0 |
| gpu-unlock-ada | JustAResearcher | RTX 40 (4090 primary; 4090 D, 4070 Ti Super, 4060 Ti 16GB experimental) | Linux module |
| turing-unlock | nlzy | RTX 2080 Ti, mainly 22GB modded cards (Turing has no BF16, so FP16 only: +36% to +64% matmul) | Linux module |
| nv-uefi-unlocker | physicsdolphin | RTX 20/30/40 desktop and laptop | Boot-from-USB UEFI app, works for Windows; rebuilds firmware from NVIDIA's public 610.43.03 driver package |

Sources: TechTimes Oct 28 2026 (https://www.techtimes.com/articles/329570/20261028/...); Tom's Hardware Windows/USB
article (~Nov 1 2026, https://www.tomshardware.com/pc-components/gpus/open-source-usb-tool-unlocks-nvidias-half-speed-gpu-ai-limiter-on-windows-...);
GitHub API for repo dates. Not supported: RTX 50 (Blackwell), GA100, Hopper, pre-Turing.

### Measured gains (all community or developer runs, none independent lab)

| Card | Test | Stock | Unlocked | Gain | Source |
|---|---|---|---|---|---|
| RTX 3090 (Linux) | BF16 matmul | 77.5 TFLOPS | 129.6 TFLOPS | +67% | community report via Notebookcheck/Mezha, Oct 24 |
| RTX 3090 Ti, 450 W | BF16 matmul | 87.5 | 157.4 | +80% | project data (TechTimes, tech-insider) |
| RTX 3090 at 225 W cap | BF16 | | | +25% | TechTimes (power limit eats the gain) |
| RTX 3060 12GB | FP16 matmul 4096 | 25.37 | 45.79 | 1.80x | owner, Tom's HW table |
| RTX 3070 Laptop | FP16 matmul 8192 | 35.43 | 56.04 | 1.58x | owner, Tom's HW |
| RTX 4060 Laptop | FP16 matmul | 29.89 | 49.33 | 1.65x | developer, Tom's HW |
| RTX 4080 | BF16 matmul 4096 | 100.37 | 192.18 | 1.91x | owner, Tom's HW |
| RTX 4090 | FP16 matmul 4096 | 118.42 | 171.52 | 1.45x (power-limited) | owner, Tom's HW |

Real workloads (from tech-insider's compiled table, Oct 25):

- ComfyUI BF16 image generation, RTX 3090 Ti: +47% to +59% (Notebookcheck, GSMGoTech; the r/StableDiffusion Oct 23
  "up to 59%" lead matches this).
- Masked cuDNN SDPA attention, RTX 3090 at 280 W: about +29% (GitHub compatibility report).
- LLM prompt prefill: +14% to +36% (GSMGoTech); one user +16% on long context (Notebookcheck).
- RTX 40 real workloads: +12% to +34% FP16, up to +20% BF16, far below the 2x peak (TechTimes).
- MiniMax H3 video/audio generation: +60% in BF16 mode, +31% in INT8 mode (TechTimes; INT8 gain presumably from
  BF16 parts of the pipeline, unverified).
- **LLM token generation (decode): practically unchanged.** Decode reads all weights once per token, so it is bound
  by memory bandwidth (3090: 936 GB/s), not tensor math. Quantized GGUF decode (Q4/Q8 dequant + mostly INT/FP16
  paths) also does not hit the capped instruction path much.
- Gaming: no verified effect; games and DLSS do not use FP16-in/FP32-accumulate in the capped mode.

### Risks and legality

- Voids warranty (all three Linux projects say so; turing-unlock README: "may damage your hardware"). Power and
  thermal limits stay active; only issue rate changes. TechTimes found no documented hardware failures as of Oct 28.
- Needs Secure Boot off (or self-signed EFI trusted). Breaks anti-cheat that requires Secure Boot. Laptop runtime
  power management can power down the dGPU and silently undo the unlock. One OEM RTX 3080 Ti 20GB unlocked with no
  BF16 gain.
- Legality: not assessed by any source I read. It exploits a vulnerability in NVIDIA-signed firmware; DMCA 1201
  anti-circumvention and EULA questions are open. Unverified: no lawsuit or takedown reported as of Nov 7.
- NVIDIA response: **none on record** in any source reviewed (tech-insider: "Nvidia has not commented in any source
  I reviewed"). Expect a fix via a new signed Booter / revoking the vulnerable one in a future driver; the
  nv-uefi-unlocker pins the 610.43.03 firmware, which suggests the authors expect this. Guess, not confirmed.

### What this means for our pool

- Demo hardware is Apple Silicon + a Dell with no discrete GPU. **The unlock does not apply to any demo machine.**
- For real customers with older GeForce cards, it only speeds prefill (long prompts, document ingestion) and BF16
  image gen; it does not speed token generation, which is what a chat user feels. Shipping a firmware exploit to a
  regulated office (clinic, law firm) is a non-starter for trust and warranty reasons.
- Useful as a talking point only in the honest form: "decode is bandwidth-bound; even a 1.8x tensor unlock leaves
  token speed flat, which is why we pool memory bandwidth across machines." That framing is supported by the data above.

## Part B: Which kernel challenge Aayo means

Candidates, ranked by how well they fit "a company-run kernel challenge to write the fastest kernels":

1. **GPU MODE kernel leaderboard competitions, company-sponsored** (most likely). GPU MODE (community, gpumode.com)
   runs the leaderboard; companies fund rounds: AMD Developer Challenge 2025 "Inference Sprint" ($100K, Apr 2025),
   AMD's second competition (Nov 2025, teams reached 1.5x AMD's internal baseline), **NVIDIA NVFP4 Blackwell
   competition** (with NVIDIA, Sesterce, Dell; announced Nov 3 2025), "Linear Algebra Kernels for the Age of Research"
   (Jun 12 2026, winners Aug 17 2026), and the AMD-sponsored "E2E Model Speedrun" virtual hackathon on MI355X (2026).
   The NVIDIA blog "Topping the GPU MODE Kernel Leaderboard with NVIDIA cuda.compute" (Jan 20 2026) is the lead
   Aayo was given. Also notable: GPU MODE post "Anatomy of a Reward Hack" (Mar 9 2026) from the NVFP4 competition.
   Source: https://www.gpumode.com/news
2. **NVIDIA SOL-ExecBench** (NVIDIA Research): submit CUDA/Triton/CUTLASS/CuTe DSL/cuTile/PyTorch kernels, run on real
   B200s, ranked by "SOL Score" (fraction of the analytic roofline, computed with SOLAR). Public global leaderboard.
   https://research.nvidia.com/benchmarks/sol-execbench, https://github.com/NVIDIA/SOL-ExecBench. Fits "company-run"
   best literally.
3. **MLSys 2026 FlashInfer AI Kernel Generation Contest, NVIDIA track**: write LLM-op kernels for B200 by hand or with
   AI agents. https://mlsys26.flashinfer.ai/. Related paper: "Self-Evolving Kernels Catching and Surpassing Experts and
   Agents", arXiv 2611.04998 (early Nov 2026), which reports against the SOL-ExecBench/FlashInfer-Bench B200 leaderboard.

Not a competition: KernelBench (Stanford benchmark for AI-written kernels).

Best guess: if Aayo remembers "a company" and "fastest kernels" with prize money, it is the GPU MODE x AMD $100K
challenge or the GPU MODE x NVIDIA NVFP4 Blackwell competition. If he remembers a leaderboard scored against
theoretical peak, it is SOL-ExecBench.

All three target datacenter GPUs (B200, MI300/MI355X). Techniques transfer as ideas (fusion, quantized kernels,
roofline thinking), not as code we can run on Macs.
