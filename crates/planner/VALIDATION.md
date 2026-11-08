<!-- CONCEPT: Evidence and limits of the WP-5 workspace handoff.
WHY: R5.3/R5.13–15 and D-07 require reproducible numbers, not unrun-test claims.
DEPENDS ON: Rust 1.99.1 (O-14b), Cargo, WP-1 calibration fixtures, WP-2 D-13 schema.
AFFECTS: Aries review, WP-11 model selection, future docs/findings.md calibration. -->
# Validation record

**Follow-up after #18:** See [FOLLOWUP.md](FOLLOWUP.md). The current branch is
rebased onto 3d6f7ce and now consumes shared contracts. The full-Cargo passes
below describe the earlier draft. Current isolated placement checks pass, but
the expanded workspace needs uncached dependencies before a full build can run.

2026-11-07, Joseph's Mac. Original draft: standalone repo a0587d3.
Team parent: main 8006aaa19e449d7a1cdd73e1bca563c5c833a7a6.

| Check | Actual result |
|---|---|
| GitHub connector authentication | Authenticated as josephibka; repository reports push permission |
| Shell git connectivity | `git ls-remote` fails: cannot resolve github.com |
| `git sync` | Unavailable Git alias; exact main snapshot obtained through connector instead |
| Snapshot integrity | All 83 file blob hashes and complete tree hash match GitHub; reconstructed signed parent commit hash matches exactly |
| Rust tooling | Stable Rust/Cargo 1.99.1 matches current PRD O-14b. Use `cargo +stable` because this base still has the older 1.93.1 toolchain file; WP-12 owns the pin update. gh remains absent |
| Agent installer attempt | Failed DNS for sh.rustup.rs; user subsequently installed Rust independently |
| `cargo +stable fmt -p planner` and `--check` | PASS with Rust 1.99.1 |
| `cargo +stable build --workspace --frozen` | PASS in normal Terminal and independently in the agent runner |
| `cargo +stable test --workspace --frozen` | PASS: 17 integration tests, 0 failures; includes serde serialization. Other skeleton crates have no tests yet |
| `cargo +stable clippy -p planner --all-targets --frozen -- -D warnings` | PASS in normal Terminal and independently in the agent runner |
| Built CLI smoke checks | PASS: inspect JSON and file accounting, exact-fit human table and ratio, invalid-command exit 1 |
| Dependency cache and lockfile | RESOLVED: normal Terminal downloaded all 11 dependencies; Cargo regenerated root Cargo.lock. Agent runner now builds/tests offline |
| Offline source backup | cargo vendor saved a second copy under ~/.cache/wp5-validation/vendor-20261107-182523 |
| Real GGUF download | curl fails: cannot resolve huggingface.co |
| Temporary std-only test harness | 17 passed using rustc 1.99.1; serde derives removed from copied sources and JSON assertion replaced with direct schema_version assertion. This does not validate serde or the CLI |
| Temporary std-only Clippy harness | Library and test copies pass clippy-driver with -D warnings after fixing range patterns and nested conditions |
| macOS linker | Default MacOSX27.0 SDK fails with installed linker (unknown arm64e.x1 architecture). Using existing MacOSX15.4 SDK via per-command SDKROOT works for the full workspace build/tests and Clippy |

The agent installed no tools; Joseph installed Rust. The full validation log is
`~/.cache/wp5-validation/terminal-20261107-182523.log`. Root Cargo.lock is the
Cargo-generated resolution, included in this PR so offline/locked checks reproduce.
The temporary harness entries above are historical; full Cargo checks supersede them.

## Hand calculation for the supplied synthetic fixture

Two layers, embedding width 256, four attention heads and two KV heads.
Head dimension = 256 / 4 = 64. F16 KV per token =
2 layers × 2 KV heads × (64 key + 64 value) × 2 bytes = **1024 bytes**.
Two tensor spans occupy 32 bytes each, including alignment: **64 weight bytes**.

At context 1 and parallel 1, a machine with 1088 free bytes and no overhead
has 1088 − 1024 = **64 bytes** for weights: exact fit, ratio **1.0**.
At 1087 bytes, it misses by one byte. Two independent 600-byte pools sum to
1200 bytes; after one total-model KV allocation, aggregate weight budget is
**176 bytes**. That establishes aggregate capacity only: it does not prove
that indivisible layers, KV and main-node tensors can be placed successfully.

## Reproducing the completed Cargo checks

From repository root, use `cargo +stable` to select installed 1.99.1 (O-14b)
while WP-12 updates the old root toolchain pin. On this Mac, prefix build/test commands with
`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk` to avoid the
current linker/SDK mismatch. This is a local workaround, not a team SDK pin.

Dependencies are cached; these commands require no registry networking:

```sh
cargo +stable fmt -p planner --check
cargo +stable build --workspace --frozen
cargo +stable test --workspace --frozen
cargo +stable clippy -p planner --all-targets --frozen -- -D warnings
```

## Outstanding integration and real-model validation

Inspect two real GGUFs and reconcile metadata + tensor span totals with file sizes.
Run fit with measured device free memory, Metal budgets, reserves and buffer estimates.
WP-1 load-log calibration and its agreed tolerance must eventually be recorded in
the shared docs/findings.md by coordination with that file's editors. This handoff
does not assert WP-5's full acceptance criteria are met.
