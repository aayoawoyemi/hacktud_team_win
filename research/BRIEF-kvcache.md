# Brief: worker "kvcache" (low priority, requested by Aayo via aries)

Your name: `kvcache`. Notes folder: `research/kvcache/`. Check-ins: `research/checkins/kvcache-NN.md`.

Context: the team pools 3-4 machines (main Mac + M5 24 GB, M3 16 GB, Windows Dell with Vulkan RTX 3050 Ti) with
llama.cpp RPC (build 8665, Apr 4 2026; layer split, main holds the GGUF, star topology), Rust supervisors, and
SpacetimeDB as the control plane. Test model: Gemma 4B; Qwen2.5-0.5B fallback. Read `git show origin/notes:PRD.md`
§4.3 and §4.4 for the llama.cpp facts and memory math. Not needed for the demo; this is for the write-up and
stretch work.

Question: what are the newest (2025-2026) KV-cache techniques (compression, quantization, offload, eviction, sharing
or moving KV across machines, prefix caching) that could help a llama.cpp RPC pool, and which are actually usable in
llama.cpp build 8665 today (flags such as `--cache-type-k/v`, `-fa`, `--kv-offload`/`-nkvo`, `--cache-reuse`, slot
save/restore, context shift), with the memory saved per token for a 4B model and the expected effect on a pool.

## Sections (one per run)

1. **Usable now in llama.cpp:** exact flags (verify in `C:\Users\aayoa\Desktop\hackathon\llama-src` `common/arg.cpp`,
   noting that source is newer than b8665), what each does to memory and speed, and where the KV cache lives when
   layers are split over RPC (on the device that owns the layer?). A ranked list for our pool.
2. **Research frontier:** papers and serious blogs 2025-2026 (KV quantization like KIVI/KVQuant, eviction like
   H2O/SnapKV, offload, disaggregated prefill/decode, cross-machine KV transfer, Mooncake-style pools), each with the
   measured gain and whether anything is in llama.cpp yet. One paragraph each; what we could honestly say in a write-up.
