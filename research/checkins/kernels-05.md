## Check-in kernels 05: Applying it (final)

**Found (most important first, max 8 bullets, each with a source):**
- **Pitch claim 1, MoE.** The 5-bit model holds 21.7 GB but reads only 2.38 GB per token (3.46B of 31.8B parameters), so the pool runs a model no single Mac holds, at usable speed (kernels-04 config math).
- **Pitch claim 2, context.** The KV cache grows 6 KB per token, plus a fixed 49 MB of Mamba state. A 32k-token legacy file costs 0.2 GB, versus 3.1 GB on a standard 30B MoE. This is the agent-modernizes-legacy-code story (kernels-04).
- **Pitch claim 3, network.** The ceiling is 55 tok/s for 5-bit on 3 Macs. Each token makes about 4 sequential network trips of 5.4 KB, so delay per trip matters, not bandwidth. Make the claim only with measured Ethernet vs Wi-Fi numbers (EXO `pipeline.py`; kernels-04).
- **No kernel-speedup claims.** All kernels are MLX's own.
- **Speed-of-light meter:** a cheap stretch item, about 30 lines (measured tok/s ÷ a ceiling computed once per placement). Skip it if the core demo isn't done by Sunday 8am. A prediction-vs-measurement slide gives most of the value with no code.

**Surprises / things that change the plan:** none.

**PRD suggestions:** Add the 6-row day-1 measurement table from `research/kernels/05-applying-it.md` under "First hour". It replaces the old `llama-bench` 8B line, which is now stale after the switch to Nemotron.

**Questions for Aayo:** none.

**Next section:** done unless asked.
