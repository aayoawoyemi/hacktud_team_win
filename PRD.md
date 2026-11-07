> **Agents: before doing anything, read the pinned agent log, issue #11: https://github.com/aayoawoyemi/hacktud_team_win/issues/11. Introduce yourself there, post every status change there (started, PR, blocked, needs-decision, done), and work only in your own worktree and branch. Aries (Aayo's orchestrator agent) keeps this PRD and the decision log.**

# PRD: [Name TBD], one AI model pooled across ordinary machines

A pool of ordinary machines (one Windows Dell laptop plus the team's other laptops) that runs one AI language model too big for any of them alone, with no cloud. llama.cpp does the inference. The team's Rust code finds the machines, decides how to split the model, starts llama.cpp on every machine, and reports what the pool is doing.

- Version: v2, detailed skeleton. Written Sat Nov 7, 2026, 3:55pm CT for the WP-0 kickoff. Replaces the v1 outline sent at 3:30pm.
- Event: HackUTD 2026 Zero Day, UT Dallas. Final submission due Sun Nov 8, 12:00pm CT.
- Status: draft. Anything marked OPEN or PROPOSED needs a team decision before anyone builds it.
- How to use it: each work package (WP) in §7 is self-contained for its owner and their coding agent. Read §1 to §3 first (about 10 minutes), then your WP.

---

## 0. Assignment board

| WP | Name | Owner | Status | Needed by (PROPOSED) |
|---|---|---|---|---|
| WP-0 | Kickoff: owners and blocking decisions | whole team | not started | 4:30pm |
| WP-1 | Relay bring-up: the initial llama.cpp relay infrastructure, by hand | | not started | Checkpoint A, 6:30pm |
| WP-2 | Contracts crate: shared Rust types and llama.cpp output parsers | | not started | first version 5:30pm |
| WP-3 | Node agent: hardware probe, rpc-server supervision, heartbeat | | not started | Checkpoint B, 9:30pm |
| WP-4 | Coordinator: node registry, launch, one endpoint | | not started | Checkpoint B |
| WP-5 | Capacity normalization (the "RAM normalization" item) and split planner | | not started | fit calculator 7:30pm, rest Checkpoint B |
| WP-6 | Relay layer between main and helpers: telemetry, optional proxy | | not started | Checkpoint C, 3:00am |
| WP-7 | Status and metrics API (backend only) | | not started | Checkpoint C |
| WP-8 | Benchmark and demo runner | | not started | basic at Checkpoint B, full at C |
| WP-9 | Failure detection and recovery | | not started | Checkpoint C |
| WP-10 | Network and connectivity, plus the netcheck tool | | not started | reachability answer 5:00pm |
| WP-11 | Models and storage | | not started | first download started 4:30pm |
| WP-12 | Build, packaging, cross-platform setup | | not started | workspace builds everywhere 5:30pm |
| WP-13 | Demo scenarios and runbook (no code) | | not started | Checkpoint C |
| WP-14 | Devpost, video, write-up (no code) | | not started | registration 11:00pm, submit 11:00am |
| WP-S1 | Stretch: speculative decoding on the pool | | not started | only after Checkpoint C |
| WP-S2 | Stretch: distributed-llama comparison | | not started | only after Checkpoint C |

Status values: not started, in progress, blocked (say on what), done.

---

## 1. How to read this document

### 1.1 Labels
- **DECIDED**: the team made this call. Change it only by team agreement and log the change in §3.3.
- **OPEN**: no decision yet. Options are listed. Don't build as if one were chosen.
- **PROPOSED**: a draft offered so work can start in parallel (contract shapes, folder names, times). Confirm or change it at kickoff.
- **VERIFY**: a fact to check on the team's machines or the pinned llama.cpp build before anyone relies on it.
- **ROADMAP**: outside hackathon scope. Listed so there is an answer when someone asks.

### 1.2 Requirement words
- **MUST**: the WP isn't done without it.
- **SHOULD**: expected. Skipping it needs a reason written in `docs/findings.md`.
- **MAY**: optional.

### 1.3 How this maps to the repo workflow
The repo already has a workflow (README.md, AGENTS.md):
- Each WP becomes one GitHub issue: `git task "WP-3 Node agent"`. Paste the WP section into the issue body.
- Each WP owns the folders listed under "Owns" in its section (layout PROPOSED in §5.7). Agents stay inside them, per AGENTS.md.
- Contract shapes in §6 are PROPOSED. Once confirmed at kickoff, the WP-2 owner adds them to CONTRACTS.md and ships that change alone (`git ship "contract: ..."`) before other WPs build against them. AGENTS.md already requires this.
- The "Project" section of AGENTS.md (idea, stack, run, test) is empty. Filling it is a WP-0 output.

### 1.4 Changing a contract
A §6 contract changes only when the owner of the producing WP and the owners of every consuming WP (§6.14) agree. Add fields; don't rename or remove them (AGENTS.md rule).

---

## 2. What this is

### 2.1 Summary
The model is split by layers. Each machine holds a block of the model's layers in its memory. For every token, a small state vector passes through each machine's block in turn, then main picks the token. The weights never move after loading; only that state moves. This lets the pool run a model larger than any single machine's memory. It does not make one answer faster than a single machine that could hold the model (§4.4).

### 2.2 The demo
- Part 1 (DECIDED): a fast model running on the pool. Whether Part 1 must also be too big for any one machine is OPEN (O-09).
- Part 2 (DECIDED): a model about 3x the size of any one machine. Slower, but it works. The message: imagine this scaled up to every PC in an office.
- Both models are downloaded and loaded before the demo (DECIDED). Nothing downloads live.

### 2.3 Non-goals (DECIDED)
- Routing whole requests to separate machines. Routing mode is ROADMAP.
- Cloud calls of any kind during the demo.
- Personal-data features (redaction, classification).
- A frontend: no web app or GUI work package. How pool status is shown is OPEN (O-10).
- Security features (root of trust, join codes, tamper-proof log, trust levels, encryption): ROADMAP.
- Ray: ROADMAP.
- Writing GPU kernels or modifying llama.cpp source.

---

## 3. Decisions

### 3.1 Decided

| ID | Decision | Source |
|---|---|---|
| D-01 | The demo shows pooling: one model split by layers across machines. It does not show routing. | Aayo, Sat afternoon |
| D-02 | Local only. No cloud calls in the demo. Cloud can be an optional add-on later. | Aayo, Sat afternoon |
| D-03 | No personal-data features in the demo. | Aayo, Sat afternoon |
| D-04 | Inference engine: llama.cpp, using its RPC backend to split one model across machines. | Aayo and team, 3:16pm and kickoff request |
| D-05 | All code the team writes is Rust. llama.cpp is used as shipped (C++, third party). | Aayo, kickoff request |
| D-06 | No frontend. Work is divided into backend work packages. | Aayo, kickoff request |
| D-07 | Two-part demo: Part 1 fast on the pool; Part 2 about 3x any one machine, slower. Models pre-downloaded and pre-loaded. | Aayo, 3:16pm |
| D-08 | At least 3 machines serve one model behind one endpoint. | Aayo and Eva, Sat afternoon |
| D-09 | Security features are ROADMAP. | Aayo, Sat afternoon |
| D-10 | Ray is ROADMAP. | Aayo, Sat afternoon |
| D-11 | Fixed by the event: Devpost team registration by 11:00pm Sat; submission (repo plus a demo video of 2 minutes or less) by 12:00pm Sun. | Devpost page |

### 3.2 Open decisions

| ID | Question | Options | Blocks | Decide by (PROPOSED) |
|---|---|---|---|---|
| O-01 | How does Rust drive llama.cpp? | A: run the official llama.cpp binaries (`rpc-server`, `llama-server`) as supervised child processes. B: link llama.cpp into Rust through a bindings crate (for example `llama-cpp-2`), building llama.cpp from source on each machine. C: reimplement llama.cpp's RPC protocol in Rust. Notes: B adds a C++ build per machine; C tracks an internal protocol that changes between builds (this build reports RPC v3.6.1). | WP-3, WP-4, WP-6, WP-12 | kickoff |
| O-02 | One binary with subcommands, or one binary per role? | one CLI (`<name> agent`, `<name> coordinator`, `<name> bench`, ...); separate binaries per crate | WP-3, WP-4, WP-8, WP-10, WP-12 | kickoff |
| O-03 | Control-plane transport between agents and coordinator | HTTP with JSON; gRPC (needs `protoc` on every machine); raw TCP with length-prefixed JSON | WP-2, WP-3, WP-4, WP-7 | kickoff |
| O-04 | How agents and the coordinator find each other | static list of addresses in config; coordinator address given to each agent, agents register; mDNS (campus Wi-Fi may block multicast); UDP broadcast | WP-3, WP-4 | kickoff |
| O-05 | Network for build and demo | CometNet; phone hotspot; travel router; wired switch; Tailscale (setup needs Tailscale's online service, which conflicts with D-02 during the demo) | WP-1, WP-10 | after the reachability test, about 5:00pm |
| O-06 | Which machine is main (holds model files, runs llama-server and the coordinator)? | any machine. Criteria: free disk, memory, stays plugged in, owner awake | WP-1, WP-11 | kickoff |
| O-07 | Split strategy | llama.cpp default (proportional to each device's free memory); a computed `--tensor-split` from one of the WP-5 strategies | WP-4, WP-5 | Checkpoint B, by measurement |
| O-08 | Relay layer | 6A: llama.cpp's own relay, telemetry from its debug logs; 6B: a Rust TCP proxy in the data path; both | WP-3, WP-6, WP-7 | Checkpoint A |
| O-09 | Models for Part 1 and Part 2: file, quantization. Must Part 1 exceed any one machine? | candidates after hardware intake; sizing math and download times in §4.2 and §4.4 | WP-5, WP-11, WP-13 | 5:00pm for downloads, final at Checkpoint B |
| O-10 | How pool status is shown during the demo | nothing beyond the chat output; a CLI printing status; a terminal UI; anything reading the WP-7 status API | WP-7, WP-13 | Checkpoint B |
| O-11 | Failure policy when a machine drops | automatic re-plan and restart; manual restart from the runbook; fallback to a smaller model | WP-9, WP-13 | Checkpoint B |
| O-12 | Number of llama-server slots (`-np`) | 1; more than 1 (more concurrent requests, more KV-cache memory) | WP-4, WP-5 | Checkpoint B |
| O-13 | Which measurements to run | single machine vs pool; largest model alone vs pool; split across 2 vs 3 machines; llama.cpp vs distributed-llama | WP-8, WP-14 | Checkpoint B |
| O-14 | Which llama.cpp release every machine pins | build 8665 (already on the Dell); the newest release | WP-1, WP-12 | kickoff |
| O-15 | Speculative decoding | none; prompt-based n-gram drafting (`--spec-type`); a small draft model on main (`-md`) | WP-S1 | after Checkpoint C |
| O-16 | Node identity | hostname; generated ID saved on disk; set in config | WP-2, WP-3 | kickoff |
| O-17 | Ports for the team's services | agent API port; coordinator API port; proxy ports (if 6B). Known defaults: rpc-server 50052, llama-server 8080 | WP-3, WP-4, WP-6, WP-10 | kickoff |
| O-18 | Project name | | WP-12 (crate and binary names), WP-14 | before Devpost registration |
| O-19 | Owner for each WP | | everything | kickoff |
| O-20 | Client endpoint | clients call llama-server directly; a Rust proxy in front (request log, token timing) | WP-4, WP-8 | Checkpoint B |
| O-21 | Where the relay proxy runs (only if O-08 includes 6B) | on each helper; on main, one listener per helper | WP-6 | with O-08 |
| O-22 | How each machine's speed is measured | memory-bandwidth microbenchmark in Rust; `llama-bench` on a small model; both | WP-3, WP-5 | Checkpoint B |
| O-23 | Do main's own devices hold layers? | main contributes its devices; main only coordinates and keeps embeddings and output | WP-4, WP-5 | Checkpoint A |
| O-24 | Config mechanism and logging | TOML file per role; `.env` per machine (the repo copies `.env` into task folders and never commits it); CLI flags. Logs as text or JSON lines, and where | WP-2, WP-3, WP-4, WP-12 | kickoff |
| O-25 | Context length per demo part | set per model in config; drives KV-cache memory (§4.4) | WP-5, WP-11 | with O-09 |
| O-26 | Heartbeat interval, node timeout, launch-ready timeout | numbers in config | WP-3, WP-4, WP-9 | kickoff (any values; tune later) |

Kickoff must settle: O-01, O-02, O-03, O-04, O-06, O-14, O-16, O-17, O-19, O-24, O-26. Each blocks parallel work.

### 3.3 Decision log

| ID | Decision | Who | When |
|---|---|---|---|
| O-06 | Main machine is a Mac: the one with the most memory and free disk, kept plugged in, not used for coding | Aayo | Sat 5:30pm |
| O-01 | Option A: Rust supervises the official llama.cpp binaries as child processes | Aayo | Sat 5:30pm |
| O-03 | Control-plane transport is gRPC with tonic; `.proto` files are the contract; `protox` compiles them so no machine needs `protoc` | Aayo | Sat 5:30pm |
| O-14 | Pin llama.cpp build 8665 on every machine (matches the Dell) | Aayo | Sat 5:30pm |
| O-19 | Owners: WP-11 + WP-1 setup on the main Mac = model person; WP-5 (R5.1 to R5.6, R5.13 to R5.15) = Rust coder; WP-12 + WP-2 = Aayo's agent; WP-1 multi-laptop test and WP-10 = open | Aayo | Sat 5:30pm |

---

## 4. Known facts and constraints

### 4.1 Machines

| Machine | Owner | OS | CPU | RAM | Devices (from llama.cpp) | llama.cpp | Rust | Notes |
|---|---|---|---|---|---|---|---|---|
| Dell laptop | Aayo | Windows | Intel i7-11800H, 8 cores, 16 threads | 15.7 GiB | Vulkan0: Intel UHD Graphics, shares system RAM (8051 MiB total, 7393 MiB free). Vulkan1: NVIDIA RTX 3050 Ti Laptop GPU (3965 MiB, 3370 MiB free) | build 8665 (commit b8635075f), installed with winget. Backends: CPU, Vulkan, RPC. No CUDA backend. rpc-server reports RPC protocol v3.6.1 | cargo and rustc 1.93.1, stable-x86_64-pc-windows-msvc | no WSL distro, no Docker, no Tailscale |
| teammate | | | | | | | | fill in Appendix B |
| teammate | | | | | | | | fill in Appendix B |
| teammate | | | | | | | | fill in Appendix B |

Observed on the Dell at 3:50pm: started without `-d`, rpc-server exposes the two Vulkan devices and not the CPU. VERIFY whether `-d` can add the CPU device next to them, and whether it matters, since Vulkan0 already uses system RAM.

### 4.2 Network (measured on the Dell, 3:20pm)
- CometNet, 5 GHz, 802.11ax, 75% signal, link rate 155 Mbps receive and 206 Mbps transmit.
- Round trip to 1.1.1.1: 3 to 12 ms, average 6 ms.
- Hugging Face download: 8.1 MB/s over 15 seconds.
- Device-to-device reachability on CometNet: UNKNOWN. WP-10 tests it first.

Download times at 8.1 MB/s (expect slower at night):

| File size | Time |
|---|---|
| 5 GB | about 10 min |
| 10 GB | about 21 min |
| 18.6 GB | about 38 min |
| 30 GB | about 62 min |
| 42 GB | about 86 min |
| 48 GB | about 99 min |

### 4.3 llama.cpp facts this plan relies on
Sources: [README] llama.cpp `tools/rpc/README.md`, fetched Sat; [help] `--help` output of build 8665 on the Dell; [obs] observed on the Dell.

rpc-server (runs on helpers):
- Exposes a machine's compute devices to another machine over TCP. By default it exposes all accelerator devices; with no accelerator it exposes one CPU device. [README] [obs]
- Flags: `-H/--host` (default 127.0.0.1, so it only accepts other machines when given a reachable address), `-p/--port` (default 50052), `-d/--device` (comma-separated devices), `-t/--threads` (CPU threads, default 8 on the Dell), `-c/--cache` (local tensor cache). [help]
- The cache keeps large tensors on the helper so later loads skip the network. Default location `$HOME/.cache/llama.cpp/rpc`, changed with `LLAMA_CACHE`. [README]
- Prints a banner at start: RPC protocol version, endpoint, cache, devices with total and free MiB (sample in WP-2). [obs]
- `GGML_RPC_DEBUG`: 1 = connections, handshake, buffer allocations, graph computes, cache evictions; 2 = every RPC message, tensor transfers, cache hits and misses; 3 = per-command timings, graph node dumps, transport byte counts. Client apps drop these records unless verbosity is raised; the README shows `-lv 5` with llama-cli. VERIFY on llama-server. [README]
- Transport is TCP. RDMA is negotiated automatically when both sides support it (Linux RoCEv2 NICs; Apple silicon Macs with Thunderbolt 5 on macOS 26.2+). `GGML_RPC_NO_RDMA=1` forces TCP. [README]
- Security: the README calls the RPC backend proof-of-concept, "fragile and insecure", and says never to run the RPC server on an open network or in a sensitive environment. [README]

llama-server (runs on main):
- `--rpc host:port,host:port` adds remote devices. [help]
- By default, weights and KV cache are split across all devices, local and remote, in proportion to each device's available memory. `--tensor-split` sets custom proportions. [README]
- `-ngl/--n-gpu-layers N`: maximum layers stored on GPU-type devices; README examples use `-ngl 99`. VERIFY the accepted values on build 8665 (the help line is cut off in our capture) and the behavior with models over 99 layers. [help] [README]
- `-sm/--split-mode {none,layer,row}`, `-ts/--tensor-split N0,N1,...`, `-mg/--main-gpu`, `-dev/--device`, `--list-devices`. [help]
- `-c/--ctx-size N` (0 = from the model), `-np/--parallel N` (default -1 = auto), `--host`, `--port` (default 8080), `--metrics` (Prometheus endpoint, off by default), `--slots/--no-slots` (slots endpoint, on by default), `-fa/--flash-attn on|off|auto` (default auto), `--mlock`, `--mmap/--no-mmap`. [help]
- Speculative decoding: `-md/--model-draft`, `--draft-max` (default 16), `--draft-min`, `--draft-p-min` (default 0.75), `-devd/--device-draft`, `-ngld`, `-cd`, `--spec-type` (none, ngram-cache, ngram-simple, ngram-map-k, ngram-map-k4v, ngram-mod), `--spec-ngram-size-n`, `--spec-ngram-size-m`, `--spec-ngram-min-hits`. [help]
- Endpoints: `/health`, `/v1/chat/completions` (OpenAI-style, streaming), `/v1/models`, `/metrics` (with `--metrics`), `/slots`. VERIFY on build 8665: `/health` status codes while loading, metric names, and whether the final stream chunk carries usage and timings.
- Watch out: `-c` means cache on rpc-server and context size on llama-server.

Model files and versions:
- Only main needs the model file. llama-server sends each helper its share of the weights at load time; with `-c`, helpers cache it. [README]
- Every machine should run the same llama.cpp release. The RPC handshake carries a protocol version (v3.6.1 here). VERIFY what a mismatch looks like (WP-1).

### 4.4 Model math (for WP-5 and O-09)
- Weights per layer: the sum of the sizes of the GGUF tensors named `blk.<i>.*`. Other tensors (`token_embd.weight`, `output_norm.weight`, `output.weight`) belong to no layer. VERIFY where llama.cpp places them on build 8665.
- KV-cache bytes = context length × layers × KV heads × (key length + value length) × bytes per element (2 for f16). Example, Llama 3.1 8B (32 layers, 8 KV heads, head size 128): 131,072 bytes per token, so 1 GiB at 8,192 tokens of context. Models with per-layer KV head counts, sliding-window or hybrid attention don't follow this formula; check each candidate.
- Usable memory per device = free memory − reserve (OS and other apps) − compute buffers (llama.cpp prints their sizes at load) − KV share. Unified-memory devices (Intel UHD here, Apple GPUs) use system RAM, so never count the same RAM twice. VERIFY on each Mac how much unified memory the GPU may use by default and the `iogpu.wired_limit_mb` setting.
- Time per generated token with a layer split ≈ sum over devices of (weight bytes read on that device ÷ its memory bandwidth) + one network hop per machine boundary + fixed overhead. So:
  - Splitting a model that fits on one machine makes each answer slower, not faster.
  - Fewer machines means fewer hops.
  - More layers on faster devices lowers the sum: for one stream, fastest-first beats proportional.
  - Mixture-of-experts models read only the active experts per token, so use active bytes, not file size.
- Sizing Part 2: "about 3x any one machine" (D-07) is measured against the usable memory of the largest single machine, using the WP-5 fit calculator. Example only: if the largest machine has 10 GB usable, Part 2 is about 30 GB, about 62 minutes to download at 8.1 MB/s.

### 4.5 Related projects (for positioning and the write-up)

| Project | Splits one model across machines | Windows | macOS | Notes |
|---|---|---|---|---|
| llama.cpp RPC | yes, by layers | yes | yes | the engine (D-04) |
| distributed-llama | yes; all machines work on every token (tensor parallelism) | yes, built from source | yes | runs on 1, 2, 4, ... machines only; q40 models in its own format; WP-S2 |
| prima.cpp | yes, by layers, with a scheduler for mixed devices | no ("future update") | yes | research system built on llama.cpp; reports up to 80% speedup from speculative decoding |
| Exo | yes | no (Windows listed as "longer term") | yes, main target | about 48K GitHub stars; the closest product |
| Ray (+ vLLM) | Ray schedules jobs; splitting needs an engine such as vLLM | multi-machine clusters supported only on Linux | same | ROADMAP (D-10). ShadowRay (CVE-2023-48022): attackers took over Ray clusters through its unauthenticated job API |

### 4.6 Fixed event times (CT)
- Sat 7:30–9:30pm: dinner, ECSW Atrium
- Sat 11:00pm: Devpost team registration deadline
- Sat 11:30pm: midnight snack
- Sun 9:00–10:30am: breakfast
- Sun 11:45am: reminder. 12:00pm: submissions due (repo plus video of 2 minutes or less)

---

## 5. System overview

### 5.1 Roles
- **Main machine** (one, O-06): holds the model files; runs llama-server (the endpoint) and the coordinator. Its own devices may hold layers (O-23).
- **Helper machines** (two or more, D-08): run rpc-server, which exposes their devices to main.
- **Node agent** (WP-3): runs on every machine, main included.
- **Client**: whatever sends prompts. During the demo and measurements, the WP-8 runner.

### 5.2 Planes
- **Control plane** (team's Rust code): agents and coordinator. Registration, heartbeats, commands. Transport per O-03.
- **Data plane** (llama.cpp): llama-server to each rpc-server over TCP, in llama.cpp's own protocol. Optionally through the team's relay proxy (O-08, option 6B).
- **Client plane**: OpenAI-style HTTP to llama-server, or to a Rust proxy in front of it (O-20).

### 5.3 Diagram

```
                      client (WP-8 runner)
                             | client plane: HTTP, OpenAI-style
                             v
MAIN .................................................................
   llama-server  <----- starts / stops -----  coordinator (WP-4, WP-7, WP-9)
       |   (own devices hold layers? O-23)          ^
       |                                            | control plane (O-03)
       | data plane: llama.cpp RPC over TCP         |
       | (optional relay proxy, O-08 / O-21)        |
       +----------------------+                     |
       |                      |                     |
HELPER A ...............  HELPER B ...............  |
   rpc-server                rpc-server             |
       ^ supervised by           ^ supervised by    |
   agent (WP-3) -------------- agent (WP-3) --------+
(main runs an agent too)
```

### 5.4 Path of one token
1. Main computes the embedding, plus any layers on its own devices (O-23).
2. The token's state (a vector of a few KB to tens of KB) goes to helper A, which runs its layers.
3. The state goes to helper B, then back to main for the output layer and the choice of token.
4. The next token starts again at step 1.

VERIFY in WP-1 and WP-6: in llama.cpp RPC every connection runs between main and one helper (a star). Copies between two helpers probably pass through main. Measure it.

### 5.5 Startup sequence (target behavior with the Rust tooling)
1. Agents start on every machine, probe hardware (WP-3) and register with the coordinator (O-04).
2. The coordinator takes the configured model (O-09) and asks the planner (WP-5) for a SplitPlan over READY nodes.
3. If the plan doesn't fit, it stops with `E_MODEL_DOES_NOT_FIT`.
4. It sends StartRpcServer to each helper in the plan and waits until all report healthy.
5. It builds the LaunchSpec and starts llama-server on main.
6. It polls `/health` until ready or the timeout (O-26).
7. It reads llama-server's load log, records where layers actually landed and compares that with the plan (`placement_mismatch` if they differ).
8. SERVING: status comes from heartbeats, llama-server metrics and relay telemetry (WP-7).

The WP-1 manual runbook does the same by hand. It stays the fallback for every later WP.

### 5.6 States (PROPOSED)
- Node: UNKNOWN → REGISTERED → READY (rpc-server healthy, or not needed) → ACTIVE (holds layers in the running plan) → LOST (heartbeat timeout). A LOST node that returns goes back to REGISTERED.
- Cluster: EMPTY → PLANNED → STARTING_HELPERS → LAUNCHING → SERVING → DEGRADED (a node lost, policy O-11) → RECOVERING → SERVING or FAILED. STOPPED on request from any state.

### 5.7 Workspace layout and folder ownership (PROPOSED; names change with O-02 and O-18)

```
hacktud_team_win/
  Cargo.toml              workspace                                  WP-12
  rust-toolchain.toml     pinned toolchain (version OPEN)            WP-12
  crates/
    contracts/            shared types, llama.cpp output parsers     WP-2
    agent/                node agent                                 WP-3
    coordinator/          registry, launcher, endpoint               WP-4
      src/status/         status and metrics module                  WP-7
      src/recovery/       failure detection and recovery module      WP-9
    planner/              GGUF reader, capacity normalization        WP-5
    relay/                telemetry parsing; proxy only if 6B        WP-6
    bench/                benchmark and demo runner                  WP-8
    netcheck/             round-trip time and throughput tool        WP-10
  prompts/                demo and benchmark prompts                 WP-8, WP-13
  docs/
    runbook-manual.md     manual bring-up and demo fallback          WP-1
    findings.md           measurements and surprises (all append)    WP-1 creates
    network.md            chosen network, firewall steps             WP-10
    models.md             model files, sizes, fit table              WP-11
    demo-runbook.md       demo scripts, checklists, fallbacks        WP-13
    devpost.md            write-up draft                             WP-14
  fixtures/
    llama/                captured llama.cpp outputs                 WP-1
    json/                 contract examples                          WP-2
  config/                 example config per role (mechanism O-24)   WP-12
  models/                 local only, never committed                WP-11
```

### 5.8 Single-machine dev mode
Rust WPs can start before the network works:
- Run two or three rpc-servers on one laptop on 127.0.0.1 with different ports (50052, 50053, ...) and point llama-server at them with a small model.
- Good for control-plane work (agent, coordinator, planner, status). Not valid for speed numbers.
- VERIFY in WP-1: several rpc-servers on one machine sharing the same devices behave correctly.
- A mock agent and a mock coordinator built from the WP-2 fixtures let WP-3 and WP-4 develop against each other before both exist.

---

## 6. Contracts (PROPOSED wire shapes)

### 6.0 Conventions (PROPOSED)
- JSON with snake_case field names. Every message has `schema_version` (integer, starting at 1).
- Sizes are integer bytes, except fields that mirror llama.cpp output, which stay in MiB and say so in the name (`free_mib`).
- Wall-clock times are RFC 3339 UTC strings. Durations are integers with a unit suffix (`_ms`, `_s`).
- Readers ignore unknown fields. Fields are added, never renamed or removed.
- `null` means unknown or not measured yet.
- Shapes are transport-independent. Routes or services come after O-03.
- Rust types live in `crates/contracts` (WP-2) with serde derives.
- All values in the examples below are examples, not targets or decisions. Model numbers use Llama 3.1 8B Q4_K_M only to show the shape.

### 6.1 NodeCapabilities (agent to coordinator, on registration and whenever it changes)

```json
{
  "schema_version": 1,
  "node_id": "example-dell",
  "hostname": "EXAMPLE-DELL",
  "agent_version": "0.1.0",
  "captured_at": "2026-11-07T21:50:00Z",
  "os": { "family": "windows", "version": null, "arch": "x86_64" },
  "cpu": { "model": "11th Gen Intel(R) Core(TM) i7-11800H @ 2.30GHz", "physical_cores": 8, "logical_cores": 16 },
  "memory": { "total_bytes": 16857000000, "available_bytes": 9800000000 },
  "llama_cpp": {
    "bin_dir": "C:\\path\\to\\llama.cpp",
    "build": 8665,
    "commit": "b8635075f",
    "rpc_protocol": "3.6.1",
    "backends": ["CPU", "Vulkan", "RPC"]
  },
  "devices": [
    { "name": "Vulkan0", "description": "Intel(R) UHD Graphics", "backend": "Vulkan", "total_mib": 8051, "free_mib": 7393, "unified_memory": true },
    { "name": "Vulkan1", "description": "NVIDIA GeForce RTX 3050 Ti Laptop GPU", "backend": "Vulkan", "total_mib": 3965, "free_mib": 3370, "unified_memory": false }
  ],
  "rpc_server_devices": ["Vulkan0", "Vulkan1"],
  "disk": { "model_dir": null, "model_dir_free_bytes": null, "cache_dir": null, "cache_dir_free_bytes": null },
  "network": { "addresses": ["192.0.2.10"], "interface": "Wi-Fi" },
  "speed": { "mem_bandwidth_gbps": null, "llama_bench_tg_tps": null, "measured_with": null },
  "power": { "on_ac": true, "battery_pct": null }
}
```

| Field | Notes |
|---|---|
| `node_id` | scheme per O-16 |
| `devices` | one entry per line of `llama-server --list-devices`; MiB as printed |
| `unified_memory` | true when the device uses system RAM: the Vulkan init log prints `uma: 1` for Intel integrated graphics; Apple GPUs always share memory |
| `rpc_server_devices` | what rpc-server actually exposes, from its start banner |
| `llama_cpp.build`, `rpc_protocol` | the coordinator flags any mismatch across nodes (`version_mismatch`) |
| `speed` | filled by the speed probe chosen in O-22; null until measured |
| `power` | SHOULD; null where the OS doesn't report it |

### 6.2 Heartbeat (agent to coordinator, every `heartbeat_interval_ms`)

```json
{
  "schema_version": 1,
  "node_id": "example-dell",
  "seq": 42,
  "sent_at": "2026-11-07T22:10:05Z",
  "rpc_server": {
    "state": "running",
    "pid": 12345,
    "bind": "192.0.2.10",
    "port": 50052,
    "started_at": "2026-11-07T22:01:00Z",
    "restarts": 0,
    "last_exit_code": null,
    "port_accepting": true
  },
  "load": { "cpu_util_pct": 63.5, "mem_used_bytes": 11200000000, "device_free_mib": null },
  "relay": { "source": "debug_log", "bytes_in": 1048576, "bytes_out": 2097152, "graph_computes": 120, "last_activity_at": "2026-11-07T22:10:04Z" },
  "power": { "on_ac": true, "battery_pct": 88 },
  "last_error": null
}
```

| Field | Notes |
|---|---|
| `rpc_server.state` | one of `not_started`, `starting`, `running`, `exited`, `failed` |
| `rpc_server.port_accepting` | the agent's own TCP connect check |
| `load.device_free_mib` | only if a live source exists (llama.cpp prints free memory only at start); else null |
| `relay` | filled by WP-6: `source` is `debug_log` (6A), `proxy` (6B) or null |
| `last_error` | `{ "code": "...", "message": "..." }` with a §6.11 code, or null |

### 6.3 Command and CommandResult (coordinator to agent)

```json
{
  "schema_version": 1,
  "command_id": "cmd-0007",
  "kind": "start_rpc_server",
  "args": {
    "bind": "192.0.2.10",
    "port": 50052,
    "devices": null,
    "threads": null,
    "cache": true,
    "debug_level": null,
    "env": {}
  }
}
```

```json
{
  "schema_version": 1,
  "command_id": "cmd-0007",
  "ok": true,
  "error": null,
  "data": {
    "pid": 12345,
    "banner": { "rpc_protocol": "3.6.1", "endpoint": "192.0.2.10:50052", "local_cache": "n/a", "devices": ["Vulkan0", "Vulkan1"] }
  }
}
```

| `kind` | `args` | `data` on success | Rule |
|---|---|---|---|
| `start_rpc_server` | as above; null means rpc-server's default | pid and parsed banner | idempotent: already running with the same args returns ok |
| `stop_rpc_server` | none | exit code | idempotent |
| `probe` | none | NodeCapabilities | |
| `speed_probe` | method (O-22) | the `speed` block | |
| `netcheck` | target address, duration_s | NetcheckResult (§6.9) | |

### 6.4 ModelInfo (planner output, one per model file)

```json
{
  "schema_version": 1,
  "model_id": "example-8b-q4_k_m",
  "path": "models/example-8b-q4_k_m.gguf",
  "file_bytes": 4920000000,
  "split_files": 1,
  "gguf_version": 3,
  "architecture": "llama",
  "name": "example",
  "file_type": 15,
  "n_layers": 32,
  "n_embd": 4096,
  "n_head": 32,
  "n_head_kv": 8,
  "key_length": 128,
  "value_length": 128,
  "n_ctx_train": 131072,
  "expert_count": null,
  "expert_used_count": null,
  "layer_bytes": [131000000, 131000000, 131000000, 131000000],
  "non_layer_bytes": { "token_embd.weight": 295000000, "output.weight": 431000000, "output_norm.weight": 16384 },
  "kv_bytes_per_token_f16": 131072,
  "warnings": []
}
```

| Field | Notes |
|---|---|
| `model_id` | scheme OPEN (file name or content hash); decide with O-16 |
| `layer_bytes` | one entry per layer (cut to 4 entries in this example) |
| `file_type` | `general.file_type`; 15 is Q4_K_M |
| `kv_bytes_per_token_f16` | §4.4 formula; null plus a warning when the formula doesn't apply |

### 6.5 SplitPlan (planner output)

```json
{
  "schema_version": 1,
  "plan_id": "plan-0003",
  "created_at": "2026-11-07T22:00:00Z",
  "model_id": "example-8b-q4_k_m",
  "strategy": "llama_default",
  "ctx_size": 8192,
  "parallel": 1,
  "device_order": ["Vulkan1", "RPC0", "RPC1"],
  "tensor_split": null,
  "assignments": [
    { "node_id": "example-dell", "device": "Vulkan1", "llama_device": "Vulkan1", "layers": [0, 5], "weight_bytes": 786000000, "kv_bytes": 201326592, "compute_buffer_bytes_est": null, "usable_bytes": 3000000000, "headroom_bytes": null },
    { "node_id": "example-mac-a", "device": "Metal", "llama_device": "RPC0", "layers": [6, 18], "weight_bytes": 1703000000, "kv_bytes": 436207616, "compute_buffer_bytes_est": null, "usable_bytes": 9000000000, "headroom_bytes": null },
    { "node_id": "example-mac-b", "device": "Metal", "llama_device": "RPC1", "layers": [19, 31], "weight_bytes": 1703000000, "kv_bytes": 436207616, "compute_buffer_bytes_est": null, "usable_bytes": 9000000000, "headroom_bytes": null }
  ],
  "main_cpu_holds": ["token_embd.weight"],
  "fits": true,
  "est_ms_per_token": null,
  "warnings": ["device order not yet verified against llama-server output"]
}
```

| Field | Notes |
|---|---|
| `strategy` | `llama_default`, `proportional_usable`, `fastest_first`, `min_machines` (WP-5); choice is O-07 |
| `device_order`, `llama_device` | VERIFY in WP-1 how llama-server names and orders RPC devices; `-ts` follows that order |
| `tensor_split` | null for `llama_default`, else proportions in `device_order` |
| `layers` | inclusive range [first, last] |
| `kv_bytes` | KV bytes per token per layer (from ModelInfo) × layers × `ctx_size` × `parallel`. VERIFY how `-np` shares the context |
| `fits` | false means the coordinator refuses to launch |

### 6.6 LaunchSpec (coordinator to its llama-server launcher)

```json
{
  "schema_version": 1,
  "plan_id": "plan-0003",
  "llama_server_path": "C:\\path\\to\\llama-server.exe",
  "model_path": "models/example-8b-q4_k_m.gguf",
  "rpc": ["192.0.2.11:50052", "192.0.2.12:50052"],
  "n_gpu_layers": "99",
  "split_mode": "layer",
  "tensor_split": null,
  "devices": null,
  "ctx_size": 8192,
  "parallel": null,
  "host": "127.0.0.1",
  "port": 8080,
  "metrics": true,
  "flash_attn": null,
  "speculative": null,
  "extra_args": [],
  "env": {}
}
```

| Field | llama-server flag | Notes |
|---|---|---|
| `rpc` | `--rpc a,b` | helper addresses, or proxy addresses under 6B |
| `n_gpu_layers` | `-ngl` | string because accepted values are VERIFY |
| `split_mode` | `-sm` | |
| `tensor_split` | `-ts` | from SplitPlan |
| `devices` | `-dev` | per O-23 |
| `ctx_size` | `-c` | per O-25 |
| `parallel` | `-np` | per O-12 |
| `host`, `port` | `--host`, `--port` | per O-20 |
| `metrics` | `--metrics` | SHOULD be true for WP-7 |
| `flash_attn` | `-fa` | |
| `speculative` | `--spec-type`, `-md`, `--draft-max`, `--draft-min`, `--draft-p-min`, `-devd` | WP-S1 only |

### 6.7 ClusterStatus (WP-7 output)

```json
{
  "schema_version": 1,
  "generated_at": "2026-11-07T22:15:00Z",
  "cluster_state": "SERVING",
  "endpoint": "http://127.0.0.1:8080/v1",
  "model": { "model_id": "example-8b-q4_k_m", "name": "example", "file_bytes": 4920000000 },
  "plan_id": "plan-0003",
  "strategy": "llama_default",
  "nodes": [
    {
      "node_id": "example-mac-a",
      "role": "helper",
      "state": "ACTIVE",
      "last_heartbeat_age_ms": 900,
      "llama_cpp_build": 8665,
      "devices": [
        { "llama_device": "RPC0", "planned_layers": [6, 18], "actual_layers": null, "planned_bytes": 2139207616, "actual_buffer_mib": null }
      ],
      "cpu_util_pct": 41.0,
      "relay": { "bytes_in": 1048576, "bytes_out": 2097152, "last_activity_at": "2026-11-07T22:14:59Z" },
      "rtt_ms_to_main": null
    }
  ],
  "pool": {
    "usable_bytes_total": 21000000000,
    "largest_single_machine_usable_bytes": 9000000000,
    "model_bytes": 4920000000,
    "model_to_largest_machine_ratio": 0.55
  },
  "llama_server": { "pid": 23456, "ready": true, "metrics": null, "slots": null },
  "last_request": { "ttft_ms": null, "gen_tokens_per_s": null, "completion_tokens": null, "finished_at": null },
  "events_tail": []
}
```

`model_to_largest_machine_ratio` is the number behind D-07: Part 2 should read about 3. This example model (0.55) would fit on one machine.

### 6.8 BenchResult (WP-8 output, one line per request)

```json
{
  "schema_version": 1,
  "run_id": "bench-2026-11-07T23-10",
  "scenario": "pool_part1",
  "repeat_index": 1,
  "warmup": false,
  "started_at": "2026-11-07T23:10:00Z",
  "model_id": "example-8b-q4_k_m",
  "plan_id": "plan-0003",
  "nodes": ["example-dell", "example-mac-a", "example-mac-b"],
  "llama_cpp_build": 8665,
  "ctx_size": 8192,
  "prompt_id": "p1",
  "max_tokens": 256,
  "prompt_tokens": null,
  "completion_tokens": null,
  "ttft_ms": null,
  "gen_tokens_per_s": null,
  "prompt_tokens_per_s": null,
  "total_ms": null,
  "server_timings": null,
  "error": null,
  "notes": ""
}
```

Scenario IDs (PROPOSED; O-13 picks which run): `single_fit`, `split_2`, `split_3`, `largest_single`, `pool_part1`, `pool_part2`, `dllama_compare`.

### 6.9 NetcheckResult (WP-10 output)

```json
{
  "schema_version": 1,
  "from_node": "example-dell",
  "to_node": "example-mac-a",
  "to_addr": "192.0.2.11:50060",
  "measured_at": "2026-11-07T22:30:00Z",
  "rtt_ms": { "min": null, "p50": null, "p95": null, "max": null, "samples": 100 },
  "throughput_mbps": { "send": null, "recv": null, "duration_s": 10 },
  "tcp_nodelay": true,
  "during_generation": false,
  "error": null
}
```

### 6.10 Event (coordinator event log)

```json
{ "schema_version": 1, "seq": 118, "at": "2026-11-07T22:40:12Z", "kind": "node_lost", "node_id": "example-mac-b", "plan_id": "plan-0003", "message": "no heartbeat for 6000 ms", "data": {} }
```

Kinds: `node_registered`, `node_capabilities_changed`, `node_lost`, `node_recovered`, `version_mismatch`, `rpc_server_started`, `rpc_server_exited`, `plan_created`, `plan_rejected`, `launch_started`, `llama_server_ready`, `llama_server_exited`, `placement_mismatch`, `request_failed`, `recovery_started`, `recovery_succeeded`, `recovery_failed`, `netcheck_result`, `bench_completed`.

### 6.11 Error codes

| Code | Meaning |
|---|---|
| `E_CONFIG` | config missing or invalid |
| `E_TRANSPORT` | control-plane message failed |
| `E_NODE_UNREACHABLE` | agent unreachable |
| `E_RPC_SERVER_START` | rpc-server failed to start or exited at once |
| `E_PORT_IN_USE` | the port is already taken (leftover process?) |
| `E_RPC_UNREACHABLE` | main can't reach a helper's rpc-server port |
| `E_VERSION_MISMATCH` | llama.cpp build or RPC protocol differs between nodes |
| `E_MODEL_NOT_FOUND` | model file missing on main |
| `E_GGUF_PARSE` | GGUF reader failed |
| `E_MODEL_DOES_NOT_FIT` | no plan fits the ready nodes |
| `E_LAUNCH_FAILED` | llama-server failed to start |
| `E_NOT_READY_TIMEOUT` | llama-server not ready before the timeout |
| `E_PLACEMENT_MISMATCH` | actual placement differs from the plan |
| `E_REQUEST_FAILED` | client request failed or stream cut |
| `E_INTERNAL` | anything else |

### 6.12 Config keys (mechanism per O-24; values marked with an O-number are decided there)

| Role | Key | Meaning | Default or status |
|---|---|---|---|
| agent | `node_id` | node identity | O-16 |
| agent | `coordinator_addr` | where to register | O-04 |
| agent | `listen_addr` | agent API, if O-03 needs one | O-17 |
| agent | `llama_bin_dir` | folder with the pinned llama.cpp binaries | per machine |
| agent | `rpc_bind` | address rpc-server binds to | O-05, O-08 (127.0.0.1 behind a proxy) |
| agent | `rpc_port` | rpc-server port | 50052 (llama.cpp default) |
| agent | `rpc_devices` | `-d` value | null = rpc-server default |
| agent | `rpc_threads` | `-t` value | null = rpc-server default |
| agent | `rpc_cache` | pass `-c` | the README recommends it for faster loads |
| agent | `rpc_debug_level` | `GGML_RPC_DEBUG` | O-08 |
| agent | `llama_cache_dir` | `LLAMA_CACHE` | null = llama.cpp default |
| agent | `heartbeat_interval_ms` | | O-26 |
| agent | `log_dir` | | O-24 |
| coordinator | `listen_addr` | coordinator API | O-17 |
| coordinator | `helpers` | static helper list | only if O-04 = static |
| coordinator | `models` | list of `{ model_id, path, part, ctx_size }`; part is `test`, `part1` or `part2` | O-09, O-25 |
| coordinator | `split_strategy` | | O-07 |
| coordinator | `reserve_bytes` | per device kind (CPU RAM, discrete GPU, unified GPU) | set by measurement |
| coordinator | `main_devices` | which of main's devices hold layers | O-23 |
| coordinator | `llama_server_path` | | per machine |
| coordinator | `llama_server_host` | | O-20 |
| coordinator | `llama_server_port` | | 8080 (llama.cpp default) |
| coordinator | `metrics` | pass `--metrics` | needed by WP-7 |
| coordinator | `parallel` | `-np` | O-12 |
| coordinator | `node_timeout_ms` | | O-26 |
| coordinator | `ready_timeout_s` | | O-26 |
| coordinator | `recovery_policy` | | O-11 |
| coordinator | `speculative` | WP-S1 settings | O-15 |
| coordinator | `log_dir` | | O-24 |
| bench | `endpoint`, `prompts_file`, `scenarios`, `repeats`, `warmups`, `max_tokens`, `output_dir` | | O-13, O-20 |
| netcheck | `listen_port`, `targets`, `rtt_samples`, `throughput_duration_s` | | O-17 |

### 6.13 Environment variables

| Name | What it is | Set by |
|---|---|---|
| `LLAMA_CACHE` | rpc-server tensor cache folder | agent, from `llama_cache_dir` |
| `GGML_RPC_DEBUG` | RPC debug log level, 0 to 3 | agent, from `rpc_debug_level` (O-08) |
| `GGML_RPC_NO_RDMA` | forces TCP | only if RDMA ever gets negotiated |
| team variables | config path, overrides | O-24 |

### 6.14 Contract ownership

| Contract | Producer | Consumers |
|---|---|---|
| NodeCapabilities | WP-3 | WP-4, WP-5, WP-7 |
| Heartbeat (relay block from WP-6) | WP-3 | WP-4, WP-7, WP-9 |
| Command, CommandResult | WP-4 | WP-3 |
| ModelInfo | WP-5 | WP-4, WP-7, WP-11 |
| SplitPlan | WP-5 | WP-4, WP-7, WP-9 |
| LaunchSpec | WP-4 | WP-4, WP-9 |
| ClusterStatus | WP-7 | WP-8, WP-13, the O-10 display |
| BenchResult | WP-8 | WP-13, WP-14 |
| NetcheckResult | WP-10 | WP-5, WP-7 |
| Event | WP-4, WP-9 | WP-7 |
| Config format | WP-12, WP-2 | everyone |
| Parsers for llama.cpp output | WP-2 (fixtures from WP-1) | WP-3, WP-4, WP-6 |

### 6.15 CONTRACTS.md handoff (after kickoff)
The WP-2 owner fills CONTRACTS.md and ships it alone before the other WPs build against it:
- **Who owns what**: the §5.7 folder table with owners from the board.
- **Data types**: the confirmed §6 shapes as Rust type names, with the JSON examples. The current template shows TypeScript; replace it.
- **API routes**: once O-03 is decided (HTTP routes or gRPC services).
- **Env vars**: §6.13.

---

## 7. Work packages

Every WP has the same parts: Purpose, Issue title, Owns, Depends on, Blocks, Requirements, Open inside this WP, Acceptance, Target, Risks. Requirements that depend on an OPEN decision name it.

### WP-0 Kickoff
- **Purpose**: assign owners and settle the decisions that block parallel work.
- **Issue title**: none (whole team, in person).
- **Time box**: 25 minutes, PROPOSED 4:05 to 4:30pm.
- **Agenda**:
  1. Read §2, §3 and the board (5 min).
  2. Fill Appendix B for every machine, in parallel (10 min): chip, RAM, GPU, OS, `llama-server --version`, `--list-devices`, free disk.
  3. Decide O-01, O-02, O-03, O-04, O-06, O-14, O-16, O-17, O-19, O-24, O-26 (10 min). Log them in §3.3.
  4. Create one issue per WP with `git task`. Owners start.
- **Start right away, before the meeting ends**: WP-11 starts the small test model download on the likely main machine.
- **Outputs**: owners on the board; decisions logged; the AGENTS.md "Project" section filled (idea, stack, how to run, how to test); issues created.
- **Acceptance**: every WP from WP-1 to WP-14 has an owner or is explicitly parked; every kickoff decision is logged.

### WP-1 Relay bring-up (the initial llama.cpp relay infrastructure, by hand)
- **Purpose**: prove llama.cpp's relay works on the team's machines and network before any automation. Its runbook is the demo fallback for every later WP.
- **Issue title**: `WP-1 Relay bring-up (manual llama.cpp RPC across machines)`
- **Owns**: `docs/runbook-manual.md`, `docs/findings.md` (creates it), `fixtures/llama/`.
- **Depends on**: O-06, O-14; O-05 (or test on CometNet first); a small model file on main (WP-11); the WP-10 reachability answer.
- **Blocks**: WP-2 parsers (fixtures), WP-5 calibration, WP-9 (failure behavior), WP-13, Checkpoint A.
- **Requirements**:
  - R1.1 MUST install the pinned llama.cpp release (O-14) on every machine and record `llama-server --version` and `llama-server --list-devices` for each, in Appendix B and `fixtures/llama/`.
  - R1.2 MUST let rpc-server accept inbound connections through each OS firewall (Appendix A) and record the exact steps.
  - R1.3 MUST start rpc-server on helper A bound to a reachable address and confirm main can reach the port (Appendix A).
  - R1.4 MUST run the small model on main with `--rpc` pointing at helper A and get one answer from `/v1/chat/completions`.
  - R1.5 MUST repeat with helper B so at least 3 machines serve the model (D-08). This is Checkpoint A.
  - R1.6 MUST save the full stdout and stderr of llama-server's load and of each rpc-server's start in `fixtures/llama/`.
  - R1.7 MUST record: device names and order as llama-server reports them; per-device buffer sizes; per-device layer ranges if printed; first-load time; second-load time with `-c` on helpers.
  - R1.8 SHOULD record failure behavior:
    - llama-server started while a helper is down
    - a helper killed mid-answer
    - mismatched llama.cpp versions, if a second build is at hand
  - R1.9 SHOULD try a non-default `--tensor-split` and record how the proportions map to devices (order, rounding to whole layers).
  - R1.10 SHOULD capture `GGML_RPC_DEBUG=1` output from a helper and llama-server output with `-lv 5`, and save samples for WP-6.
  - R1.11 SHOULD record whether traffic between helpers passes through main (§5.4).
  - R1.12 MAY try single-machine dev mode (§5.8).
  - R1.13 MAY test whether rpc-server `-d` accepts the CPU next to GPU devices (§4.1).
- **Open inside this WP**: none beyond the listed O-items.
- **Acceptance**: Checkpoint A, one answer from a model split across 3 or more machines through one endpoint, reproduced by a second teammate using only `docs/runbook-manual.md`.
- **Target**: two machines by about 5:30pm; Checkpoint A at 6:30pm (PROPOSED).
- **Risks**: R-01, R-02, R-06, R-13.

### WP-2 Contracts crate
- **Purpose**: one set of Rust types for every §6 contract so WP-3 to WP-10 can be built in parallel, plus parsers for llama.cpp's text output.
- **Issue title**: `WP-2 Contracts crate (shared types + llama.cpp output parsers)`
- **Owns**: `crates/contracts/`, `fixtures/json/`, edits to CONTRACTS.md (shipped as `contract: ...`).
- **Depends on**: O-03 (wire format details), O-16, O-24; WP-1 fixtures for the load-log parser (the other parsers can start from the samples below).
- **Blocks**: WP-3, WP-4, WP-5, WP-6, WP-7, WP-8, WP-10.
- **Requirements**:
  - R2.1 MUST define serde types for §6.1 to §6.12, each with `schema_version`, ignoring unknown fields on read.
  - R2.2 MUST include a JSON fixture per type and round-trip tests.
  - R2.3 MUST define config structs (§6.12). Undecided values are `Option` with no default.
  - R2.4 MUST provide parsers that skip the `load_backend:` and `ggml_vulkan:` noise lines:
    - `llama-server --list-devices` output to devices
    - `--version` output to build and commit
    - the rpc-server start banner to protocol version, endpoint, cache and devices
    - llama-server's load log to per-device buffers and layer counts (format from WP-1 fixtures)
  - R2.5 SHOULD provide the §6.11 error-code enum and the §6.10 event-kind enum.
  - R2.6 MUST put the confirmed contracts in CONTRACTS.md and ship that change alone, first (AGENTS.md).
- **Parser samples** (captured on the Dell, 3:50pm):

```
Starting RPC server v3.6.1
  endpoint       : 127.0.0.1:50999
  local cache    : n/a
Devices:
  Vulkan0: Intel(R) UHD Graphics (8051 MiB, 7393 MiB free)
  Vulkan1: NVIDIA GeForce RTX 3050 Ti Laptop GPU (3965 MiB, 3370 MiB free)
```

```
Available devices:
  Vulkan0: Intel(R) UHD Graphics (8051 MiB, 7393 MiB free)
  Vulkan1: NVIDIA GeForce RTX 3050 Ti Laptop GPU (3965 MiB, 3370 MiB free)
```

```
version: 8665 (b8635075f)
built with Clang 19.1.5 for Windows x86_64
```

- **Open inside this WP**: crate names follow O-02 and O-18.
- **Acceptance**: `cargo test` for the crate passes on Windows and macOS; the parsers pass on fixtures from the Dell and at least one Mac.
- **Target**: first version (types and fixtures) at 5:30pm; the load-log parser after WP-1 fixtures land.
- **Candidate crates** (not decisions): serde, serde_json, thiserror.

### WP-3 Node agent
- **Purpose**: make each machine's compute available to the pool and report its state.
- **Issue title**: `WP-3 Node agent (probe, rpc-server supervision, heartbeat)`
- **Owns**: `crates/agent/`.
- **Depends on**: WP-2; O-01, O-02, O-03, O-04, O-16, O-17, O-22, O-24, O-26; WP-1 findings.
- **Blocks**: WP-4 end to end, WP-6 option 6A (runs inside the agent), WP-7, WP-9.
- **Requirements**:
  - Probe
    - R3.1 MUST report NodeCapabilities (§6.1): OS, architecture, CPU model and cores, total and available RAM, llama.cpp build, commit and backends (from `--version`), devices (from `--list-devices`), what rpc-server exposes (from its banner), free disk at the cache folder, network addresses.
    - R3.2 MUST offer a command that prints the probe for a human (name per O-02).
    - R3.3 SHOULD run the speed probe chosen in O-22 and fill `speed`.
  - Serve the pool (written for O-01 = A; rewrite if O-01 is B or C)
    - R3.4 MUST start rpc-server with the StartRpcServer args (bind, port, devices, threads, cache, debug level, env) and write its stdout and stderr to `log_dir`.
    - R3.5 MUST report rpc-server health: process alive and port accepting TCP.
    - R3.6 MUST stop rpc-server on StopRpcServer and when the agent exits, leaving no orphan process:
      - Windows: a plain kill doesn't cover an agent crash. Use a Job Object with kill-on-close (candidate crates: `win32job`, `windows`).
      - macOS: kill-on-drop, plus a check at startup for a leftover process on the port.
    - R3.7 MUST make Start and Stop idempotent.
    - R3.8 SHOULD restart a crashed rpc-server with backoff, unless policy O-11 says otherwise.
    - R3.9 MUST detect a port already in use at start and report `E_PORT_IN_USE`.
  - Register and report
    - R3.10 MUST register NodeCapabilities at start and whenever they change.
    - R3.11 MUST send a Heartbeat every `heartbeat_interval_ms` (O-26), including the relay block once WP-6 provides it.
    - R3.12 MUST run coordinator commands (§6.3) and reply with CommandResult.
  - Run
    - R3.13 MUST build and run on Windows x86_64 (MSVC) and macOS arm64, plus macOS x86_64 if a teammate has an Intel Mac.
    - R3.14 MUST read its config (O-24) and log to `log_dir`.
    - R3.15 SHOULD report AC power and battery level where the OS allows.
- **Open inside this WP**: how to get live device memory (likely null; see §6.2).
- **Acceptance**: on the Dell and one Mac:
  - the agent registers with the coordinator, or with a mock built from WP-2 fixtures
  - Start and Stop work, and repeating them changes nothing
  - killing rpc-server by hand shows up within two heartbeats
  - after the agent exits, `tasklist` or `ps` shows no llama.cpp process
- **Target**: Checkpoint B.
- **Candidate crates** (not decisions): tokio, serde, clap, tracing, sysinfo, and per O-03 reqwest and axum, or tonic.
- **Risks**: R-06, R-07, R-08.

### WP-4 Coordinator
- **Purpose**: the main machine's control process. Keeps the node registry, turns a plan into running llama.cpp processes, and exposes one endpoint.
- **Issue title**: `WP-4 Coordinator (registry, launch, endpoint)`
- **Owns**: `crates/coordinator/`, except `src/status/` (WP-7) and `src/recovery/` (WP-9).
- **Depends on**: WP-2; WP-3 (or a mock); WP-5; O-01, O-02, O-03, O-04, O-06, O-12, O-17, O-20, O-23, O-24, O-26.
- **Blocks**: Checkpoint B, WP-7, WP-8 (endpoint), WP-9.
- **Requirements**:
  - Registry
    - R4.1 MUST keep a record per node: capabilities, last heartbeat, state (§5.6).
    - R4.2 MUST mark a node LOST after `node_timeout_ms` without a heartbeat and emit `node_lost`.
    - R4.3 MUST flag llama.cpp build or RPC protocol mismatches across nodes (`version_mismatch`) and refuse to launch on a mismatch unless config overrides it.
  - Plan
    - R4.4 MUST ask WP-5 for a SplitPlan for the configured model over READY nodes, and MUST refuse to launch when `fits` is false (`E_MODEL_DOES_NOT_FIT`, with the reason).
  - Prepare helpers
    - R4.5 MUST send StartRpcServer to each helper in the plan and wait until all report healthy (timeout per O-26).
  - Launch
    - R4.6 MUST build llama-server's arguments from the LaunchSpec (§6.6 mapping) and start it.
    - R4.7 MUST poll `/health` until ready or `ready_timeout_s`. VERIFY its responses while loading.
    - R4.8 MUST capture llama-server's load log, record actual placement per device with the WP-2 parser, and emit `placement_mismatch` when it differs from the plan by more than a tolerance (value OPEN, set by WP-5 calibration).
    - R4.9 MUST stop llama-server cleanly, with the same orphan rules as R3.6.
  - Endpoint
    - R4.10 MUST give clients one OpenAI-style streaming chat endpoint: llama-server's own, or a Rust proxy in front (O-20).
    - R4.11 If O-20 picks a proxy: MUST forward requests and streams unchanged; SHOULD record time to first token and token timing per request for WP-7 and WP-8.
    - R4.12 SHOULD start llama-server with `--metrics` for WP-7.
  - Control
    - R4.13 MUST offer commands to: start the pool for model X (plan, helpers, launch); stop the pool; show the plan; print status as JSON; switch models (stop, plan, launch).
- **Acceptance**:
  - with agents on 3 or more machines, one command takes model X from cold to SERVING and answers a prompt
  - stop leaves no llama.cpp process on any machine
  - a model that doesn't fit is refused with `E_MODEL_DOES_NOT_FIT` and a readable reason
- **Target**: Checkpoint B.
- **Risks**: R-05, R-12.

### WP-5 Capacity normalization (the "RAM normalization" item) and split planner
- **Purpose**: turn each machine's memory and speed into a split for a given model. Answers: does it fit, where does each layer go, how fast should it be.
- **Issue title**: `WP-5 Capacity normalization + split planner (GGUF reader, fit calculator)`
- **Owns**: `crates/planner/`.
- **Depends on**: WP-2; WP-1 fixtures for calibration; WP-10 round-trip times for the speed estimate; O-07, O-12, O-22, O-23, O-25.
- **Blocks**: WP-4 (plans), WP-11 (fit numbers for choosing models), WP-13 (the "3x" claim).
- **Requirements**:
  - GGUF reader
    - R5.1 MUST parse the GGUF header, metadata and tensor table without reading tensor data (Appendix C).
    - R5.2 MUST output ModelInfo (§6.4): per-layer bytes, non-layer bytes, KV bytes per token.
    - R5.3 MUST compute tensor sizes from the ggml type table or from gaps between sorted offsets (Appendix C). The sum MUST match the file size up to alignment padding.
    - R5.4 MUST handle split GGUF files (`*-00001-of-0000N.gguf`) if a chosen model comes split. VERIFY the metadata key names.
    - R5.5 SHOULD warn when the KV formula doesn't apply (per-layer KV head counts, sliding-window or hybrid attention).
  - Usable memory
    - R5.6 MUST compute usable bytes per device: free memory (device list or rpc-server banner) − reserve (config) − KV share − compute-buffer estimate (calibrated from WP-1 logs). MUST NOT count system RAM twice for unified-memory devices.
  - Strategies (O-07 picks one; build in this order as time allows)
    - R5.7 S0 `llama_default`: no `-ts`. Predict llama.cpp's proportional placement and record it.
    - R5.8 S1 `proportional_usable`: `-ts` proportional to usable bytes.
    - R5.9 S2 `fastest_first`: fill devices in order of measured speed (O-22), each up to its usable cap.
    - R5.10 S3 `min_machines`: the smallest set of machines that fits, then S1 or S2 inside it.
  - Output
    - R5.11 MUST output a SplitPlan (§6.5) with `tensor_split` in llama-server's device order (from WP-1), expected layer ranges, per-device bytes, `fits`, headroom and warnings.
    - R5.12 SHOULD estimate milliseconds per token with the §4.4 formula, using measured speed (O-22) and round-trip times (WP-10). For mixture-of-experts models, use active bytes.
  - Fit calculator
    - R5.13 MUST answer, for a model and a context length: does it fit on each machine alone, does it fit on the pool, and the ratio of model bytes to the largest single machine's usable bytes (the "3x" number for D-07).
    - R5.14 MUST answer the reverse: the largest model (bytes) that fits on each machine alone and on the pool for a given context. This feeds O-09 and WP-11.
    - R5.15 SHOULD print both as a table for humans (command name per O-02).
  - Calibration
    - R5.16 MUST compare predicted per-device bytes with llama.cpp's reported buffers (WP-1 fixtures), record the error in `docs/findings.md`, and tune the reserve and compute-buffer estimates.
- **Open inside this WP**: GGUF parsing approach (hand-written reader or an existing crate, for example the GGUF module in `candle-core`); reserve values; how to estimate compute buffers; whether memory-mapping the model file on main (`--mmap`, the default) needs counting against main's RAM. VERIFY.
- **Acceptance**:
  - predicted per-device bytes match llama.cpp's reported buffers within a tolerance the WP-4 and WP-5 owners agree on and record
  - every plan with `fits: true` loads on the real pool without running out of memory
  - the strategy comparison numbers needed for O-07 are recorded
- **Target**: O-09 can't wait for code. By 5:00pm the WP-11 owner sizes models by hand with §4.4 and Appendix B. The fit calculator in code at 7:30pm confirms it. Everything else by Checkpoint B.
- **Risks**: R-04.

### WP-6 Relay layer between main and helpers ("the relay between this and that")
- **Purpose**: understand, measure and, optionally, carry the traffic between llama-server and each rpc-server.
- **Issue title**: `WP-6 Relay layer (telemetry; optional Rust TCP proxy)`
- **Owns**: `crates/relay/`.
- **Depends on**: O-08, O-17, O-21; WP-1 debug-log samples; WP-3 (6A runs inside or next to the agent).
- **Blocks**: the relay fields in Heartbeat and ClusterStatus; the ROADMAP encrypted relay.
- **Requirements, option 6A (telemetry from llama.cpp's own logs)**:
  - R6.1 MUST run each helper's rpc-server with `GGML_RPC_DEBUG` at the chosen level (1 to 3) and parse its output into per-helper counters: connections, graph computes, and at level 3 bytes and timings per command.
  - R6.2 MUST hand the counters to the agent for the Heartbeat `relay` block (`source: debug_log`).
  - R6.3 MUST measure whether the chosen debug level slows generation (WP-8 with and without) and record it.
  - R6.4 SHOULD derive bytes per token per helper.
- **Requirements, option 6B (Rust TCP proxy in the data path)**:
  - R6.5 MUST forward bytes unchanged in both directions. It must work without parsing anything.
  - R6.6 MUST set TCP_NODELAY on both sockets (RPC messages are small; Nagle's algorithm adds delay).
  - R6.7 MUST handle long-lived and concurrent connections, and pass closes and errors through in both directions.
  - R6.8 MUST count bytes per direction with timestamps and hand the counters to the agent (`source: proxy`).
  - R6.9 With the proxy on the helper (O-21), rpc-server SHOULD bind to 127.0.0.1 so the proxy is the only port facing the network.
  - R6.10 MUST measure the latency it adds per token (WP-8 with and without the proxy) and record it.
  - R6.11 MAY decode llama.cpp's RPC message framing to count commands. VERIFY the wire format in the ggml-rpc source at the pinned build; it changes between versions.
  - R6.12 Note for ROADMAP: a proxy pair (one on main, one on each helper) is where authentication and encryption would go (§13).
- **Both options**: R6.13 SHOULD record bytes per token per helper and latency per hop for the write-up.
- **Acceptance**: while a prompt runs, the relay counters in ClusterStatus move for every helper; tokens/s with telemetry on stays within a recorded margin of telemetry off.
- **Target**: Checkpoint C.
- **Candidate crates** (not decisions): tokio (`io::copy_bidirectional`), bytes.
- **Risks**: R-10, R-13.

### WP-7 Status and metrics API (backend only)
- **Purpose**: one machine-readable view of the whole pool for narration, debugging and whatever O-10 picks to display. No UI in this WP (D-06).
- **Issue title**: `WP-7 Status + metrics API (backend only)`
- **Owns**: `crates/coordinator/src/status/`.
- **Depends on**: WP-2, WP-3, WP-4, WP-5; WP-6 for the relay fields; O-03 (how it's served); O-10 (who reads it).
- **Requirements**:
  - R7.1 MUST produce ClusterStatus (§6.7), refreshed at least on every heartbeat.
  - R7.2 MUST include, per node: planned and actual layers and bytes, state, heartbeat age, CPU use, relay counters.
  - R7.3 MUST compute pool totals from WP-5: total usable bytes, largest single machine's usable bytes, model bytes, and their ratio (the "3x" number).
  - R7.4 SHOULD read llama-server's `/metrics` (with `--metrics`) and `/slots`. VERIFY the metric names at the pinned build.
  - R7.5 MUST keep a rolling event log (§6.10) and the last request's time to first token and tokens/s (from WP-8, or from the O-20 proxy).
  - R7.6 MUST offer a CLI command that prints ClusterStatus as JSON.
  - R7.7 MAY stream status changes for a future display.
- **Acceptance**: during a WP-8 run, the status shows every node ACTIVE, the plan, moving relay counters, and tokens/s within a recorded margin of WP-8's own measurement.
- **Target**: Checkpoint C.

### WP-8 Benchmark and demo runner
- **Purpose**: send prompts to the endpoint, measure them, and drive the demo.
- **Issue title**: `WP-8 Benchmark + demo runner`
- **Owns**: `crates/bench/`; `prompts/` (shared with WP-13).
- **Depends on**: WP-2; any working endpoint (the WP-1 manual setup is enough to start); O-13, O-20.
- **Requirements**:
  - R8.1 MUST send streaming chat requests to the OpenAI-style endpoint and measure per request: time to first token, generation tokens/s, prompt tokens/s when available, total time.
  - R8.2 MUST count tokens from server-reported usage when present (VERIFY whether streamed responses include usage at the pinned build); otherwise count stream chunks and mark the count as estimated.
  - R8.3 MUST read prompts from a file in `prompts/` with an id, max tokens, and use (bench or demo).
  - R8.4 MUST support warm-up runs and N repeats, and write BenchResult as JSON lines and CSV to `output_dir`.
  - R8.5 SHOULD save llama-server's `timings` object when a response includes it.
  - R8.6 MUST print a markdown summary table (scenario, model, machines, time to first token, tokens/s, size ratio) for WP-14 and `docs/findings.md`.
  - R8.7 MUST implement the scenarios O-13 selects (ids in §6.8):
    - `single_fit`: a model on the single best machine alone
    - `split_2`, `split_3`: the same model split across 2 and 3 machines
    - `largest_single`: the largest model one machine can hold alone (from WP-5)
    - `pool_part1`, `pool_part2`: the demo models on the pool
    - `dllama_compare`: WP-S2
  - R8.8 SHOULD have a demo mode that streams the answer to the terminal with a live tokens/s readout. It becomes the demo display only if O-10 picks it.
- **Acceptance**: repeated runs of one prompt on one plan stay within a recorded spread; every result records plan_id, nodes, model and llama.cpp build.
- **Target**: R8.1, R8.3 and R8.4 at Checkpoint B; the rest at Checkpoint C.
- **Candidate crates** (not decisions): reqwest (streaming), eventsource-stream or hand-written SSE parsing, serde_json, csv, clap.

### WP-9 Failure detection and recovery
- **Purpose**: notice when the pool breaks and get back to SERVING in a known way.
- **Issue title**: `WP-9 Failure detection + recovery`
- **Owns**: `crates/coordinator/src/recovery/`.
- **Depends on**: WP-3, WP-4; WP-1 failure findings (R1.8); O-11, O-26.
- **Requirements**:
  - R9.1 MUST detect, and emit an event for each:
    - a helper LOST (heartbeat timeout)
    - an unhealthy rpc-server (agent report)
    - llama-server exited or not ready
    - request errors (HTTP errors, cut streams)
  - R9.2 MUST apply the O-11 policy:
    - automatic: re-plan with the remaining READY nodes. If it fits, restart helpers as needed, then llama-server. If not, switch to the configured fallback model or go to FAILED.
    - manual: print the exact runbook steps for the operator.
  - R9.3 MUST keep the restart order: helpers healthy first, then the plan, then llama-server.
  - R9.4 MUST measure and record, per demo model, cold-start time (nothing running to SERVING) and recovery time (node loss to SERVING).
  - R9.5 SHOULD keep helper caches warm (`-c`) so a restart doesn't resend every weight, and record second-load times.
  - R9.6 MUST leave no orphan llama.cpp process after a recovery.
- **Acceptance**: kill a helper mid-answer. The event appears within the node timeout, the pool reaches the O-11 end state, and the times are in `docs/findings.md`.
- **Target**: Checkpoint C.
- **Risks**: R-05.

### WP-10 Network and connectivity, plus the netcheck tool
- **Purpose**: machines that reach each other with low, steady latency on the network O-05 picks.
- **Issue title**: `WP-10 Network + connectivity (+ netcheck tool)`
- **Owns**: `crates/netcheck/`, `docs/network.md`.
- **Depends on**: two machines with any TCP listener (llama.cpp's rpc-server works); O-17.
- **Blocks**: R1.3 in WP-1, O-05, every multi-machine step.
- **Requirements**:
  - R10.1 MUST test device-to-device TCP on CometNet first (a helper listens on a port, main connects) and report the result by 5:00pm. If it's blocked, test the other O-05 options.
  - R10.2 MUST document in `docs/network.md`: the chosen network; how each machine finds and reports its address; the firewall steps per OS (Appendix A); the port plan (O-17).
  - R10.3 MUST provide a netcheck tool in Rust with a listener mode and a client mode. It measures round-trip time (many small messages, TCP_NODELAY) and throughput (bulk transfer) and prints NetcheckResult (§6.9).
  - R10.4 SHOULD measure round-trip time while a model is generating, to see the effect of load.
  - R10.5 SHOULD record per-link results in `docs/findings.md` and pass them to the WP-5 speed estimate.
  - R10.6 MUST document how to show "no internet" on the chosen network (D-02), and VERIFY the method keeps the local network working. For example, a phone hotspot with mobile data off may stop routing between devices, while a travel router with its WAN cable unplugged usually keeps its LAN.
  - R10.7 SHOULD bind rpc-server only to the chosen private network's address, since llama.cpp's README says never to expose it on an open network (R-09).
- **Acceptance**: every main-to-helper link has a recorded round-trip time and throughput on the chosen network; a teammate can add a new machine using `docs/network.md` alone.
- **Target**: reachability answer at 5:00pm; netcheck tool by Checkpoint B.
- **Candidate crates** (not decisions): tokio, clap, serde_json, or only `std::net`.
- **Risks**: R-01, R-06, R-09.

### WP-11 Models and storage
- **Purpose**: the right model files on main, early, checked, with their sizes known.
- **Issue title**: `WP-11 Models + storage`
- **Owns**: `docs/models.md`, `models/` (local only, never committed).
- **Depends on**: O-06 (main), O-09, O-25; Appendix B; WP-5 fit numbers once they exist.
- **Requirements**:
  - R11.1 MUST make sure `models/` and `*.gguf` are git-ignored before anything downloads into the repo folder.
  - R11.2 MUST download a small test model (any small GGUF instruct model) to main right after kickoff, for WP-1.
  - R11.3 MUST propose Part 1 and Part 2 candidates for O-09 by 5:00pm, sized by hand with §4.4 and Appendix B, with download times from §4.2.
  - R11.4 MUST download each chosen model once, to main only, and check the file: size, plus SHA-256 when the source publishes it.
  - R11.5 MUST check free disk on main (every model file) and on each helper (the `-c` cache can hold up to that helper's share of each model).
  - R11.6 SHOULD keep a second copy on another machine or a USB drive, against a disk or download failure.
  - R11.7 MUST record per model in `docs/models.md`: file name, source URL, size, quantization, layer count, KV bytes per token (from WP-5), and which machines it fits on alone.
  - R11.8 MUST warm the helper caches with one full load of each demo model before the demo (WP-13 preflight).
- **Acceptance**: both demo models are on main, checked, recorded, and have loaded on the pool at least once.
- **Target**: test model downloading by 4:30pm; O-09 candidates at 5:00pm; the Part 2 download starts as soon as O-09 is decided (a 42 GB file takes about 86 minutes at 8.1 MB/s).
- **Risks**: R-03, R-14.

### WP-12 Build, packaging, cross-platform setup
- **Purpose**: everyone can build and run the Rust workspace on their own machine, and the pinned llama.cpp sits in a known place.
- **Issue title**: `WP-12 Build + packaging (workspace, toolchain, per-OS setup)`
- **Owns**: root `Cargo.toml`, `rust-toolchain.toml`, `config/`, the build and run sections of README.md.
- **Depends on**: O-02, O-14, O-18, O-24.
- **Requirements**:
  - R12.1 MUST create the cargo workspace (§5.7) with empty crates that build, so other WPs start from it.
  - R12.2 MUST pin the Rust toolchain in `rust-toolchain.toml` (version OPEN; the Dell has 1.93.1).
  - R12.3 MUST confirm `cargo build` works on every teammate's machine: Windows needs the MSVC build tools (VERIFY on the Dell with a real build); macOS needs the Xcode command line tools.
  - R12.4 Each machine builds natively. Cross-compiling for macOS from Windows is impractical and isn't planned.
  - R12.5 SHOULD avoid system dependencies: no `protoc` unless O-03 picks gRPC; no OpenSSL.
  - R12.6 MUST document where the llama.cpp binaries live on each machine and how to install the pinned release (O-14): Windows through winget or the release zip; macOS through the release zip or Homebrew. Homebrew installs the newest build, which may not match O-14.
  - R12.7 MUST provide one start command per role (agent, coordinator, bench, netcheck) plus the probe command, documented in README.md.
  - R12.8 SHOULD provide example config files per role in `config/` (mechanism per O-24).
  - R12.9 MAY provide a script that downloads the pinned llama.cpp release for the current OS.
  - R12.10 SHOULD fill the stack, run and test lines of the AGENTS.md "Project" section, with WP-0.
- **Acceptance**: a teammate who has never built the repo follows README.md and runs the probe command in under 15 minutes.
- **Target**: workspace builds on every machine by 5:30pm.
- **Risks**: R-07.

### WP-13 Demo scenarios and runbook (no code)
- **Purpose**: the demo runs the same way every time, with a plan for each failure.
- **Issue title**: `WP-13 Demo runbook`
- **Owns**: `docs/demo-runbook.md`; demo prompts in `prompts/` (with WP-8).
- **Depends on**: WP-1, WP-8, WP-11; O-09, O-10, O-11.
- **Requirements**:
  - R13.1 MUST script Part 1 and Part 2: the prompt, the spoken lines, expected timings (from WP-8), and an output length cap so Part 2 finishes in the time available.
  - R13.2 MUST write preflight checklists for T-60, T-15 and T-2 minutes: machines plugged in, sleep and notifications off, versions match, network up, helper caches warm, both models load-tested, status command works.
  - R13.3 MUST write a fallback ladder:
    - a helper drops (per O-11)
    - Part 2 fails to load or runs too slowly
    - the network fails
    - everything fails: play the recorded video (WP-14)
  - R13.4 MUST pick 3 to 5 numbers to say out loud (from WP-8 and WP-7), such as the size ratio and tokens/s.
  - R13.5 SHOULD rehearse end to end. Checkpoint C needs 3 clean runs in a row.
- **Acceptance**: someone who didn't build the system can run the demo from `docs/demo-runbook.md`.
- **Target**: Checkpoint C.

### WP-14 Devpost, video, write-up (no code)
- **Purpose**: everything the event requires, on time.
- **Issue title**: `WP-14 Devpost + video + write-up`
- **Owns**: `docs/devpost.md`, the Devpost project page, the video.
- **Depends on**: O-18 (name); WP-8 (numbers); WP-13 (demo script).
- **Requirements**:
  - R14.1 MUST register the team on Devpost before 11:00pm Saturday (D-11).
  - R14.2 MUST record a demo video of 2 minutes or less (D-11). It doubles as the last-resort fallback in WP-13.
  - R14.3 MUST submit the repo link and video by 12:00pm Sunday; the board targets 11:00am.
  - R14.4 MUST write the Devpost text: what it is (first line), how it works, the measurements table (WP-8), related work stated honestly (§4.5), the roadmap (§13).
  - R14.5 SHOULD make the repo README say what the project is and how to run it.
- **Acceptance**: Devpost shows the project with its video and repo before 11:00am Sunday.
- **Target**: registration by 11:00pm; video 8 to 10am; submission by 11:00am.

### WP-S1 Stretch: speculative decoding on the pool
- **Starts**: only after Checkpoint C.
- **Purpose**: fewer trips through the machines per generated token. Each trip pays a network hop per machine. When the model checks several drafted tokens in one pass, the hops are paid once for several tokens. prima.cpp reports up to 80% faster on a home cluster.
- **Owns**: the `speculative` config section; results in `docs/findings.md`.
- **Requirements**:
  - RS1.1 MUST measure Part 1 and Part 2 tokens/s with and without, using the WP-8 runner and the same prompts.
  - RS1.2 SHOULD try prompt-based n-gram drafting first (`--spec-type` options), which needs no extra model.
  - RS1.3 MAY try a small draft model on main (`-md`, kept on main's devices with `-devd`). VERIFY whether the draft model must share the main model's vocabulary.
  - RS1.4 MUST stay out of the demo unless the team decides O-15 after seeing the numbers.

### WP-S2 Stretch: distributed-llama comparison
- **Starts**: only after Checkpoint C.
- **Purpose**: compare layer splitting (llama.cpp) with every-machine-on-every-token splitting (distributed-llama) on the same hardware, for the write-up and the paper.
- **Owns**: results in `docs/findings.md`.
- **Requirements**:
  - RS2.1 MUST use 2 or 4 machines (distributed-llama runs on powers of two only).
  - RS2.2 MUST use a model distributed-llama ships in its q40 format (its launcher lists Qwen 3 8B, 14B and 30B A3B, among others) and the closest GGUF quantization for llama.cpp. q40 and Q4_K_M differ, so the write-up must say the comparison isn't exact.
  - RS2.3 Needs Python 3 and a C++ compiler on each machine. distributed-llama is C++; running it doesn't conflict with D-05 because the team isn't writing it.
  - RS2.4 MUST record tokens/s for both on the same machines and network.

---

## 8. Timeline and checkpoints (PROPOSED)

| Time (CT) | What | WPs |
|---|---|---|
| 4:05–4:30pm | Kickoff. Test-model download starts. | WP-0, WP-11 |
| 4:30–5:30pm | Reachability answer by 5:00pm. Two-machine bring-up. Workspace skeleton. First contract types. Part 1 and Part 2 candidates by 5:00pm, then the team decides O-09 and the Part 2 download starts. | WP-10, WP-1, WP-12, WP-2, WP-11 |
| 5:30–6:30pm | Three-machine bring-up. Agent, coordinator and planner start against the contracts in single-machine dev mode (§5.8). Basic runner against the manual endpoint. | WP-1, WP-3, WP-4, WP-5, WP-8 |
| **6:30pm** | **Checkpoint A** | |
| 6:30–9:30pm | Agent v1, coordinator v1, GGUF reader and fit calculator (code by 7:30pm), first split strategies, netcheck. Downloads finish. Dinner in shifts, 7:30–9:30pm. | WP-3, WP-4, WP-5, WP-10, WP-11 |
| **9:30pm** | **Checkpoint B** | |
| 9:30–11:00pm | Load and measure Part 2. Decide O-07, O-10, O-11, O-12, O-13, O-20. Register on Devpost before 11:00pm. | WP-4, WP-8, WP-14 |
| 11:00pm–3:00am | Relay telemetry, status API, recovery, remaining strategies and scenarios, demo runbook, rehearsals. | WP-5 to WP-9, WP-13 |
| **3:00am** | **Checkpoint C**, then feature freeze. Stretch WPs only if C passed and the team agrees. | |
| 3:00–8:00am | Sleep in shifts. Fixes only. Write-up draft. | WP-14 |
| 8:00–10:00am | Record the video. Breakfast 9:00–10:30am. | WP-14 |
| 10:00–11:00am | Submit. | WP-14 |
| 11:00am–12:00pm | Buffer. Deadline 12:00pm. | |

Checkpoint definitions:
- **A (6:30pm)**: one answer from a model split across 3 or more machines through one endpoint, reproduced by a second teammate from `docs/runbook-manual.md` alone.
- **B (9:30pm)**: the Part 1 model running on 3 or more machines, launched by the team's Rust tooling (agent, coordinator, planner) and measured with the WP-8 runner. If the tooling isn't ready, the demo still runs from the WP-1 runbook while tooling work continues.
- **C (3:00am)**: the full demo (Part 1 and Part 2) runs 3 times in a row from cold, following `docs/demo-runbook.md`.

---

## 9. Dependencies

```
WP-0 kickoff ---------------------------------------------> everything
WP-10 reachability --+
WP-11 test model ----+--> WP-1 manual bring-up --> Checkpoint A
                          |
                          +--> fixtures --> WP-2 parsers, WP-5 calibration,
                                            WP-6 log samples, WP-9 failure behavior
WP-12 workspace --> WP-2 contracts --> WP-3 agent ----+
                                   --> WP-4 coordinator +--> Checkpoint B (with WP-8 basic,
                                   --> WP-5 planner ----+     WP-11 Part 1 model)
                                   --> WP-8 runner, WP-10 netcheck
WP-4 --> WP-7 status, WP-9 recovery
WP-6 --> relay fields in WP-3 heartbeat and WP-7 status
WP-8 --> numbers for WP-13 and WP-14
WP-7 + WP-9 + WP-13 + Part 2 model --> Checkpoint C
WP-14 registration: independent, before 11:00pm
```

Critical path: WP-0 → WP-10 reachability → WP-1 → Checkpoint A, and in parallel WP-12 → WP-2 → WP-3, WP-4, WP-5 → Checkpoint B. Downloads (WP-11) run alongside both.

---

## 10. Risks

Likelihood and impact are estimates: H high, M medium, L low.

| ID | Risk | Likelihood | Impact | Mitigation | WPs |
|---|---|---|---|---|---|
| R-01 | CometNet blocks device-to-device traffic | M | H | test first; fall back to hotspot, travel router or switch (O-05) | WP-10, WP-1 |
| R-02 | llama.cpp versions differ across machines and the RPC handshake fails | M | H | pin one release (O-14); the agent reports build and protocol; the coordinator refuses mismatches | WP-1, WP-3, WP-4, WP-12 |
| R-03 | Downloads slow down at night | M | M | start early; download once on main; copy by USB or LAN | WP-11 |
| R-04 | Usable memory overestimated (Mac GPU memory limit, OS use, compute buffers) | M | M | plan from the free memory llama.cpp reports; reserves; calibrate against load logs | WP-5 |
| R-05 | llama-server aborts when a helper drops mid-generation | H | H | recovery policy (O-11); runbook fallback; warm caches for fast restarts | WP-9, WP-13 |
| R-06 | Firewall or macOS permission prompts block rpc-server or the agent | H | M | written per-OS steps (Appendix A); test on every machine early | WP-10, WP-1 |
| R-07 | Rust build problems on a teammate's machine | M | M | build tools checked at kickoff; workspace builds everywhere by 5:30pm | WP-12 |
| R-08 | Laptops sleep, throttle or run out of battery mid-run | M | H | plugged in; sleep off (Appendix A); preflight checklist | WP-13, WP-3 |
| R-09 | rpc-server exposed on a shared network (the README calls it insecure) | M | M | private network (O-05); bind to that network's address only | WP-10 |
| R-10 | The relay proxy adds latency | L | M | TCP_NODELAY; measure with and without | WP-6 |
| R-11 | Part 2 too slow to watch | M | M | short prompt; output cap; scripted narration while it streams | WP-13, WP-8 |
| R-12 | Rust tooling not ready by Checkpoint B | M | H | the WP-1 runbook is the demo fallback | all |
| R-13 | llama.cpp behaves differently between Windows and macOS builds | L | H | test a mixed Windows and Mac pool in WP-1, early | WP-1 |
| R-14 | Not enough disk on main for models, or on helpers for caches | M | M | check free disk at kickoff (Appendix B) | WP-11 |
| R-15 | Single-machine dev mode gives misleading numbers (shared devices) | M | L | use it for control-plane work only, never for speed numbers | all |

---

## 11. Test plan

| Level | Tests | WPs |
|---|---|---|
| Unit | contract round-trips; parsers on captured outputs from the Dell and a Mac; GGUF reader on 2 or more real files (sum of tensor sizes matches the file size); planner math against a hand calculation written in `docs/findings.md`; netcheck on loopback | WP-2, WP-5, WP-10 |
| Single-machine integration (§5.8) | agent, coordinator and 2 to 3 local rpc-servers with a small model: start, stop, status, kill rpc-server, restart | WP-3, WP-4, WP-7, WP-9 |
| Multi-machine | 2 machines, then 3 or more; a mixed Windows and macOS pool as early as possible | WP-1, WP-4 |
| Failure | kill a helper mid-generation; stop an agent; disconnect a helper's network; restart the coordinator; start with a helper missing; mismatched versions if available | WP-9, WP-1 |
| Performance | the O-13 scenarios; telemetry on and off; proxy on and off if 6B | WP-8, WP-6 |
| Soak | 30 minutes of continuous prompts on the Part 1 plan: errors, memory growth, tokens/s drift | WP-8, WP-9 |
| Cold start | nothing running to SERVING, for each demo model, timed | WP-9 |
| Offline | the full demo with the internet disconnected (R10.6) | WP-10, WP-13 |

---

## 12. Demo acceptance criteria

| # | Criterion | Source | Status |
|---|---|---|---|
| 1 | One model split across 3 or more machines behind one endpoint | D-08 | DECIDED |
| 2 | Part 1 runs on the pool | D-07 | DECIDED |
| 3 | Part 1 is larger than any single machine's usable memory | O-09 | OPEN |
| 4 | Part 2 is about 3x the largest single machine's usable memory (ClusterStatus ratio near 3) | D-07 | DECIDED; measured by WP-5 |
| 5 | Part 1 speed target | O-09, O-13 | OPEN (v1 suggested about 10 tokens/s; not decided) |
| 6 | Part 2 finishes its scripted answer within the demo slot | WP-13 | OPEN (slot length) |
| 7 | No cloud calls during the demo, shown with the internet disconnected | D-02, R10.6 | DECIDED; method OPEN |
| 8 | Both models downloaded and loaded before the demo | D-07 | DECIDED |
| 9 | Part 1 cold start within a time limit | WP-9 | OPEN (v1 suggested 10 minutes) |
| 10 | Pool status visible during the demo | O-10 | OPEN |
| 11 | Video of 2 minutes or less recorded and submitted | D-11 | DECIDED |
| 12 | A teammate can run the demo from the runbook | WP-13 | PROPOSED |

---

## 13. Roadmap (not in hackathon scope)

| Item | What it is | Question it answers |
|---|---|---|
| Root of trust | A central server issues one-time join codes and a key per machine. Machines run only jobs the server signs. | "What stops a random laptop from joining or sending jobs?" ShadowRay is the failure this prevents. |
| Tamper-proof log | Each log entry carries the hash of the previous one, so editing any entry breaks the chain. Optional: post the latest hash on a public blockchain (the crypto lane). | "How do you prove what ran where?" |
| Encrypted relay | The WP-6 proxy pair adds authentication and encryption between main and helpers. | "llama.cpp's RPC traffic is unencrypted. Isn't that a problem?" |
| Trust levels per machine | Sensitive work runs only on dedicated machines; employees' daily PCs get low-risk work. | "What if a laptop with malware joins?" |
| Routing mode | Each machine runs a whole small model, and requests go to free machines. | "Can it serve many users at once?" |
| Multi-site pools | Sites linked over Tailscale or WireGuard. Whole jobs go to whichever site is free, with a scheduler such as Ray, because splitting one model across the internet is too slow. | "What about branches in different cities?" |
| Shared pools across owners | Credit unions, franchises. Smart contracts track who contributed what. | "Can organizations share machines?" |
| Speed work | Speculative decoding (WP-S1), tuned kernels, re-splitting by measured speed while serving. | "Can it get faster?" |
| Optional cloud add-on | Cloud models for work that's allowed to leave the building. | "What about models too big for the pool?" |
| Overnight batch | Wake idle office PCs at night (Wake-on-LAN) for batch jobs. | Idea only, not decided. |

---

## 14. Glossary

- **Token**: a piece of text, roughly a word. Models read and write tokens.
- **Layer**: one step of the model's stack. Every token passes through every layer, in order.
- **Layer split (pipeline)**: each machine holds a block of layers, and the token's state passes from machine to machine. What this project does.
- **Tensor parallelism**: every machine works on every layer at once and syncs many times per token. Needs very fast links. Not used here.
- **Hidden state**: the vector for the current token that passes between layers, and between machines.
- **KV cache**: memory holding earlier tokens' keys and values so the model doesn't recompute them. Grows with context length.
- **Context length**: how many tokens the model can see at once, prompt plus answer.
- **GGUF**: the model file format llama.cpp uses.
- **Quantization**: storing weights with fewer bits to shrink the file. Q4_K_M averages about 5 bits per weight.
- **Mixture of experts (MoE)**: a model whose layers hold many expert blocks; each token uses only a few, so it reads less memory per token.
- **Main machine, helper machine, agent, coordinator**: see §5.1.
- **rpc-server**: the llama.cpp program that exposes a machine's devices to another machine.
- **llama-server**: the llama.cpp program that loads the model, serves the HTTP endpoint and drives the helpers.
- **Time to first token (TTFT)**: the delay between sending a prompt and receiving the first token of the answer.
- **Generation tokens/s**: how fast the answer is written after the first token.
- **Prompt processing**: reading the prompt before answering.
- **Unified memory**: a GPU that uses the same RAM as the CPU (Apple silicon, Intel integrated graphics).
- **Vulkan, Metal, CUDA**: GPU programming interfaces. The Dell's llama.cpp build uses Vulkan; Macs use Metal.
- **RPC**: remote procedure call. Here, llama.cpp's protocol between llama-server and rpc-server.

---

## Appendices (not written yet)

- A: command reference per OS, including firewall and keep-awake steps
- B: machine intake form (fields: owner, make and model, OS, CPU, RAM, GPU memory, `llama-server --version`, `--list-devices`, rpc-server banner, free disk, Rust installed, IP on the chosen network, can stay awake for 20 hours, firewall prompt handled)
- C: GGUF reading notes (header layout, metadata keys, tensor sizes)
- D: sources

Sections that refer to these appendices have nothing to point to until they're written.

