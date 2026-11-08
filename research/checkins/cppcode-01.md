## Check-in cppcode 01: How RPC works, mechanically

Full notes are in research/cppcode/01-how-rpc-works.md. All references are to llama-src at commit e9a2a44.

**Found (most important first, max 8 bullets, each with a source):**

- **Each token costs 1 blocking round trip per remote Mac.** With 3 Macs (1 head + 2 rpc-servers), that is 2 waits. SET_TENSOR, GRAPH_COMPUTE and GRAPH_RECOMPUTE get no reply and are sent without waiting (`RPC.cpp:577-585`, `:2854-2873`). The only blocking call is GET_TENSOR. It waits for the remote compute to finish first (`RPC.cpp:1943`, `sync_all_backends` `:2236`). Per token: about `2×RTT + t_A + t_B + t_C`, computed one machine after another. EXO has about 4 sequential waits (EXO-HOW-IT-WORKS.md:86).
- **Hidden states always pass through the head (a star layout).** COPY_TENSOR only works inside one server. Copying between two servers falls back to GET to the head, then SET to the next server (`RPC.cpp:933-944`, `BE.cpp:832-853`). Workers never talk to each other in layer mode.
- **Payload per hop during decode is about 10 KB.** It is one F32 row of n_embd values; I assumed n_embd = 2688 from memory and have not checked it yet. A 512-token prefill ubatch is about 5.5 MB per hop. On Wi-Fi, latency is the cost, not bandwidth.
- **Graphs are cached.** The first run sends the serialized sub-graph (about 300 B per tensor). Later runs send GRAPH_RECOMPUTE, which is 12 bytes. Up to 1024 graphs are cached per device (`RPC.cpp:1250-1277`, `:122`).
- **Layer placement:**
  - RPC devices go first in `--rpc` order and the local GPU goes last (`src/llama.cpp:275`).
  - `--tensor-split` (or free memory if it is not set) becomes cumulative fractions. Each device gets a contiguous block of layers chosen by `upper_bound` (`src/llama-model.cpp:1734-1776`).
  - The embedding layer stays on the head CPU. The output layer goes to the last device, which is the head's Metal (`:1784-1794`).
  - The cut uses layer index only. It does not know which layers are Mamba, MoE or attention.
- **What crosses the network at load:**
  - Only the head needs the GGUF. It pushes the weights with SET_TENSOR.
  - With `rpc-server -c`, each weight tensor over 10 MiB is hashed byte by byte with FNV (`RPC.cpp:323`). Then a *synchronous* SET_TENSOR_HASH is sent, with a "TODO: make this async" comment (`RPC.cpp:863`).
  - On a hit, the server reads `<cache>/<hash>` from disk (`RPC.cpp:1838-1876`). Tensors of 10 MiB or less are always resent.
- **One rpc-server serves one client at a time.** It accepts a connection and serves it to the end before accepting the next (`RPC.cpp:2985-2993`). The protocol has no auth or encryption (only a warning in `rpc-server.cpp:208-211`).
- **`tools/connect` is unrelated to pooling.** It downloads a prebuilt `ggml-org/llama-connect` binary at build time (`tools/connect/CMakeLists.txt:6-78`). That binary is a WebRTC proxy that lets a browser reach a local llama-server (v0.0.1, checked Nov 7 2026). Drop it; it also needs internet at build time.

**Surprises / things that change the plan:**

- **Nemotron-H cannot use `--split-mode tensor`.** The architecture is on the not-supported list (`src/llama-arch.cpp:1457-1474`, enforced at `src/llama-model.cpp:398`). Layer split is the only option, which matches the plan anyway.
- **RPC now has its own tensor-parallel path.** It uses COMM_INIT and COMM_ALLREDUCE, with servers opening sockets directly to each other (`RPC.cpp:2277-2560`).
- **Apple RDMA over Thunderbolt is built in** (`transport-apple.cpp`, auto-negotiated in HELLO). This could be a credible roadmap line ("plug in a TB5 cable"), but it does not apply to the Wi-Fi demo.
- **A star costs about as many one-way network legs as EXO's ring.** For 2 remotes, the star has 4 legs and the ring has 3 legs plus the all-gather. "Fewer round trips than EXO" is true by count (2 vs about 4), but the gap is a few ms per token. It has to be measured before we claim it. `GGML_RPC_DEBUG=3` on the client logs per-command ms (`RPC.cpp:681-693`).

**PRD suggestions:**

- In the engine section, describe llama.cpp RPC this way: a star through the head; 1 blocking round trip per remote Mac per token; only the head holds the GGUF; one client per rpc-server; no auth. Nemotron-H can only use layer split.
- Add a risk: restarts on join/leave re-read and re-hash the whole GGUF on the head. That cost needs measuring before we promise a "fast rejoin".

**Disagreements with cppneed:**

None of substance. Two additions to their R2 and R4:
- R2: one rpc-server can serve only one head at a time.
- R4: a restart also pays for the byte-by-byte FNV hashing and one synchronous round trip per tensor over 10 MiB.

**Questions for Aayo:**

1. Can someone run `llama-server --rpc` across 2 Macs tonight with `GGML_RPC_DEBUG=3` and send me the log? Real per-command ms would replace my estimates in section 3.

**Next section:**

Section 2 covers Nemotron-H over RPC:
- where the Mamba recurrent state is allocated per layer and device;
- whether SSM_CONV and SSM_SCAN run on the Metal backend through RPC;
- the real n_embd and layer pattern from the GGUF metadata;
- GitHub issues on RPC with hybrid or Mamba models;
- the tool-call parser and thinking toggle (cppneed covered these already, so I will only verify and not repeat).

I would also add one item: check whether the user inputs for recurrent state (the s_copy indices) add extra SET messages per split per token.
