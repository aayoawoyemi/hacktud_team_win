<!-- CONCEPT: Apply the actionable Joseph/WP-5 requests from agent log #11.
WHY: WP-4 needs placement plans; the earlier draft only sized aggregate capacity.
DEPENDS ON: main 3d6f7ce (#18), contracts §6.4/6.5, llama.cpp b8665.
AFFECTS: PR #20 review and the remaining real-model acceptance work. -->
# WP-5 follow-up to issue #11

The original GGUF reader and fit calculator remain available. The follow-up adds
`planner::plan::build_plan(&ModelInspection, &PlanRequest) -> Result<contracts::SplitPlan>`
and `planner::shared::model_info(&ModelInspection) -> Result<contracts::ModelInfo>`.
These consume the existing shared schema; they do not change it.

| Request from #11 | Result |
|---|---|
| Aries: rebase on current main and resolve only Cargo.lock | Rebased onto 3d6f7ce. Main's lockfile retained; only planner's dependencies added to its package entry. No dependency versions changed. |
| Aries/WP-2: use published shared shapes | Checked ModelInfo adapter and shared SplitPlan/LayerAssignment output. Unknown required ModelInfo fields produce an error rather than zero. |
| Agent P2: implement split planning for WP-4 | `llama_default`, `proportional_usable`, `fastest_first`; exact byte budgets, contiguous ranges, KV share, headroom and `fits`. |
| Research: integer layer shares, RPC first and main last | Explicit launch order; integer `tensor_split` sums to `n_layers+1`, counting output. Confirmed against actual b8665 source. |
| Aries/P2: measured Metal cap, main participates, configurable reserve | Uses `min(free_bytes, device_limit_bytes)` minus supplied reserve and compute buffer. Unified main's CPU tensors consume the same pool. No fixed RAM fraction or increased GPU limits. |
| Aries: real GGUF inspection | Still pending network-enabled download. Script prepared on Joseph's Mac. No synthetic file is claimed as a real model. |
| PRD R5.16: measured buffer calibration | Needs WP-1 load logs and an agreed tolerance; no measured error or successful pool load is claimed. |
| Aries: review and merge ownership | PR stays draft; Aries reviews and merges after the outstanding evidence is available. |

## Library and development CLI

```sh
cargo +stable run -p planner -- plan MODEL.gguf crates/planner/plan.example.json
cargo +stable run -p planner -- inspect-shared MODEL.gguf
```

`plan.example.json` is illustrative. Replace **all memory budgets and reserves**
with measured values. `devices` must match the exact launcher order: RPC devices
in `--rpc` order, then the main GPU. `free_bytes` for `llama_default` must be the
device-reported free bytes used by llama.cpp. Unified RAM appears only once per
physical pool, and `device_limit_bytes` is required. A discrete main GPU also
requires `main_cpu_available_bytes` after CPU reserves.

For `fastest_first`, every device needs a positive finite
`measured_bytes_per_second` in the same measured bandwidth metric. It minimizes
serial weight-read time subject to capacity and contiguous ranges; it does not
invent speed numbers or re-sort the launcher's device list. S1 instead chooses
the feasible partition closest to usable-capacity layer shares. Both use dynamic
programming so a greedy choice cannot strand a later large layer. A device with
zero assigned slots keeps a zero `tensor_split` share and has no assignment row.
S3 `min_machines`, RTT-based latency estimates and the shared `pool` CLI hook
remain later work under the PRD's staged strategy requirements.

`layers` is an inclusive slot range. Slots 0 through `n_layers-1` are transformer
layers; slot `n_layers` is output norm/head. Therefore an output-only main has
`layers: [n_layers,n_layers]`. Its output bytes are included but its KV is zero.
Non-output non-layer tensors are conservatively held on main CPU. Tied output
embeddings are charged a second copy on the output GPU. Total-model KV is rounded
up per layer and multiplied by context and parallel slots; unknown architectures
require an explicit measured KV override. This remains conservative pending the
WP-1 context/cache/buffer measurements.

The launcher must use **layer split, full GPU offload** (`-ngl` at least
`n_layers+1`) and offloaded KV. `fits: false` means refuse to launch. A true
estimate still needs real load validation; mmap/page-cache and backend fallback
can change observed memory. `est_ms_per_token` stays null until suitable measured
active-byte/RTT data exists, especially for MoE.

Pinned source checked:
[b8665 llama-model.cpp](https://github.com/ggml-org/llama.cpp/blob/b8665/src/llama-model.cpp),
`load_tensors`: float cumulative splits, `upper_bound`, input on CPU, output slot.

## Verification for this follow-up

- Planner formatting, patch whitespace and workspace manifest metadata: pass.
- **Isolated check only:** 27 tests pass (17 reader/capacity + 10 placement/adapter
  tests) with cached serde and schema-derived stand-ins for the three shared
  messages. The independent placement oracle checks 12,150 small model/budget/
  strategy combinations. Strict Clippy passes in that same temporary workspace.
- **Full current workspace:** blocked before compilation because #18's `prost`
  and other new dependencies are not cached and the agent cannot resolve
  `index.crates.io`. The isolated check does **not** validate actual prost
  generation, SDK linking or the contracts crate's custom JSON adapters.
- Real GGUF files and WP-1 calibration are still pending. The previous 17-test
  full-Cargo result in VALIDATION.md predates #18 and this follow-up.

On Joseph's Mac run `Downloads/Validate WP5 Suggestions.command` in normal
Terminal to fetch the current workspace dependencies and two official Qwen GGUFs
into `~/.cache/wp5-validation/models/`. It installs no tools and does not commit,
push or merge. Then run the full build/tests/Clippy and inspect both files. The
agent could not open Terminal through LaunchServices; that external fetch step
has not been run by this session.
