<!-- CONCEPT:26B failure/recovery evidence. WHY: avoid treating health as generation acceptance. DEPENDS ON:b8665 and main ef738ae. AFFECTS:B3/B5 interpretation. -->
# 26B on three Macs: mmap failure and no-mmap recovery

Main M3 Air .113/16GB, helpers M5 Air .174/.204/24GB each. Verified model 26859861728 bytes, SHA256 5f7cbd0f4564e84342fc34321a09acb54b1a3da9215124e5bf444baa6dda152c. Runtime coordinator source ef738ae.

The initial26B mmap load reached health200/SERVING, but no26B answer had been verified. B3's first warmup stalled. All1496 main-thread stack samples waited in common_sampler_sample → llama_context::synchronize → ggml_backend_sched_synchronize → ggml_metal_synchronize → MTLCommandBuffer waitUntilCompleted. Kernel repeatedly reported IOGPUCommandDescriptor::prepare memory-preparation failure0xE00002BE. BothRPCconnections existed; P1's helper was idle and waiting onmain. /metrics timedout5s. OriginalB3 was aborted before a completedwarmup: no benchmark speed is derived from it.

A first no-mmap attempt mistakenly omitted .174 after stale readiness/start/stop state. Its2-node load later failed in RPC buffer set_tensor; it is not3-Mac acceptance. This was stopped and both managed helpers' heartbeats were confirmed not_started before retry.

The correct3-node retry retained FAon/q8KV and added only --no-mmap. Model buffers then were CPU748MiB, MTL05718.50MiB, RPC.1749940.98MiB, RPC.2049940.98MiB;31/31layersGPU. Earlier MTL0_Mapped25600MiB was a virtual mapping, not additive residentRAM.

A bounded chat check max_tokens8, temperature0, enable_thinking=false returned assistant **4**, stop,27prompt+2completiontokens. Prompt907.131ms, generation139.721ms. This tinyrequest is smoke evidence, not a headline speed benchmark. FullB3 was then restarted, using fresh timestamped result files; failed initial headers are excluded.

Working flags:
`--rpc 192.168.8.174:50052,192.168.8.204:50052 -ngl99 -c4096 --host0.0.0.0 --port8080 -fa on --metrics -ctk q8_0 -ctv q8_0 --no-mmap`.

B5 must use the identical flags plus `--spec-type ngram-simple`. No GPU budget/sysctl/privacy-database changes were used. Logs support the memory-preparation diagnosis and observed recovery; this is not an upstream engine bisect.
