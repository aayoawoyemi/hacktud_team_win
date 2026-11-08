## Check-in cppneed 02: The coordinator we would write

**Found (most important first, max 8 bullets, each with a source):**
- The coordinator fits in the one host `worker.ts` as a module (`engine-llamacpp.ts`, ~430 lines, ~6-7 h including tests on the Macs). It has the same `getState()` shape as the EXO adapter, so `sync_devices` and the `device` table stay unchanged. The other Macs run only `rpc-server -H 0.0.0.0 -p 50052 -c`. Full notes: `research/cppneed/02-coordinator.md`.
- A leave crashes llama-server. If a remote fails mid-run, `RPC_STATUS_ASSERT` calls `GGML_ABORT` (`ggml/src/ggml-rpc/ggml-rpc.cpp:31`). An unreachable IP at launch also aborts (`:653`). The coordinator watches for the child process exiting, probes TCP 50052 before each launch, and never lists a dead IP.
- `--tensor-split` sets shares of the layer count, not of the bytes. Layer `il` goes to the first device whose cumulative share is greater than `il/(n_layer+1)` (`src/llama-model.cpp:1733-1789`). Passing integer layer counts (`k1,k2,khead`, summing to n_layer+1) gives exact boundaries. Nemotron's 23 MoE layers hold most of the bytes, so the plan needs per-layer bytes from the GGUF (a 20-line gguf-py dump at setup).
- Speed-aware placement here means "fewest Macs, fill the fastest first up to its memory cap", not shares in proportion to speed. Decode through the pipeline is serial: token time is the sum of bytes/bandwidth on each device plus 1 RTT per remote. With Q4_0 (18.2 GB), two M5s are enough, so pure speed would leave the M3 idle. For the demo, the M3 joins with the smallest share.
- Warm restart is estimated at ~20-30 s. The largest part is the head hashing the remote share byte by byte with FNV-1a on one thread: ~9-12 GB at ~1 GB/s (guess), about 9-12 s (`ggml-rpc.cpp:313-320, 853`). A cold restart over Wi-Fi is estimated at 2.5-3.5 min. None of these numbers are measured.
- After a re-plan, the cache does not help for layers a worker never held, so those loads are cold. The fix is to build the cache offline from the GGUF:
  - The file name is the `%016llx` FNV-1a-64 hash of the raw tensor bytes, and the content is the raw bytes (`ggml-rpc.cpp:1760-1770, 1838-1856`).
  - With mmap, each tensor is uploaded whole (`llama-model-loader.cpp:1749`), so a hash computed from the file should match.
  - A 40-line C tool would write the cache, and a USB copy would make every plan warm. This needs a check with `GGML_RPC_DEBUG=2` showing `cache: hit`.
- Draining before a restart is a mutex. The agent's LLM calls run in the same process, so a restart waits for the current call to finish. A join never restarts automatically; a dashboard "Rebalance" button calls a reducer, and the worker restarts at the next idle point.
- Compared with EXO, the gains are speed-aware exact placement, only the head storing the GGUF, a Windows node for the roadmap, and no multicast. The losses are ~6 h of our own code plus restart testing, and multi-copy routing (not needed for one agent). Tool calls, thinking and network cost per token come out about equal.

**Surprises / things that change the plan:**
- Fewer Macs is faster, so a 3-Mac split is only worth it when the model needs the third Mac's memory. The pitch line has to fit that: "the slow Mac gets the fewest layers, or none when not needed."
- Any 2 of the 3 Macs can hold Q4_0 (M3 + M5 is ~23 GB of budget). That makes "a Mac leaves and the pool recovers in ~30 s" a demo moment, as long as the cache is pre-built.

**Disagreements with cppcode:**
- Cache location: in 01 I quoted the README (`$HOME/.cache/llama.cpp/rpc`). cppcode reads `~/Library/Caches/llama.cpp/rpc` on macOS from `rpc-server.cpp:48-66`. The source wins, and I corrected this in 02. We should set `LLAMA_CACHE` explicitly anyway.
- Agreed: the star topology costs 1 blocking RTT per remote per token, and rpc-server serves one client. In 01, R2 understated the second point; there can be exactly one llama-server per pool.

**PRD suggestions:**
- Under the llama.cpp option, add: "Coordinator = one module in the host worker.ts (~430 lines). Fixed IP list on our router, TCP probes, plan with integer layer counts filling the fastest Macs first, restart on leave, Rebalance button on join. Warm restart ~20-30 s (est.); pre-build the rpc cache from the GGUF so every plan is warm."
- Spacetime: add `standby` to `device.status`, add `engineStatus` and `lastRestartMs` (pool row or host row), and add a `request_rebalance()` reducer.

**Questions for Aayo:**
- Do we want the "a Mac leaves and the pool recovers" moment in the demo? If yes, I make the cache builder plus a measured restart time part of section 3's bring-up. If no, pre-warming by launching each plan once is enough.
- Is the demo network Wi-Fi only, or can the 3 Macs use Ethernet on our router (USB-C adapters)? Ethernet halves cold loads and cuts the RTT per token.

**Next section:** Section 3, the decision and bring-up. It will contain:
- a recommendation with one deciding test: Nemotron-H Q4_0 over RPC on 3 Macs, temp-0 output identical to a single Mac, plus one tool call
- copy-paste commands for a release binary or a cmake build with `-DGGML_RPC=ON`, with flags verified in `common/arg.cpp` and `tools/rpc/rpc-server.cpp`
- the gguf-py layer dump
- the cache-builder sketch, if wanted

I will read cppcode-02 first.
