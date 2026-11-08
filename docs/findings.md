<!--
CONCEPT: shared append-only experimental findings; P4's initial main-machine evidence.
WHY: WP-1 real observations must be distinguishable from estimates and team reports.
DEPENDS ON: local commands, #11 assignment, pinned source snapshot.
AFFECTS: WP-2 fixtures, WP-4 launches, WP-5 fit calibration, demo acceptance.
-->
# Findings

## P4 — main-Mac intake, 2026-11-08 02:01 UTC

- Hardware: Apple M3 MacBook Air (Mac15,12), 8 CPU cores (4 performance / 4 efficiency),
  16 GB memory. macOS 27.2, build 26B123. Hardware identifiers deliberately omitted.
- Main volume: df -h reported 460 GiB size, 50 GiB used, 385 GiB available (rounded).
- Runtime search: llama-server, llama-cli, rpc-server, spacetime, rustup and cargo not
  found on PATH; no llama.cpp Homebrew installation found in checked locations.
- No listeners on TCP 3000 or 8080 were returned by lsof at inspection.
- Swift/Metal one-off probe compiled but MTLCreateSystemDefaultDevice returned nil
  in this agent process. This does **not** establish that the machine lacks Metal.
  The actual llama-server --list-devices budget must be captured from a permitted terminal.
- No Gemma GGUF found in accessible searched home folders. Only an unrelated cached
  Parakeet speech-model blob was found; not used for inference.
- git ls-remote to GitHub and curl to github.com / install.spacetimedb.com fail hostname
  resolution. It is not established whether this is runner restriction or host DNS.
  No binary or model installation succeeded.
- GitHub connector works: restored all 185 tracked files at upstream commit
  81813cc5e603dfc82eb158581599a0424bc3eb14, verified every Git blob hash, reconstructed
  the exact signed commit, and verified index tree c3e314b8fdfc742bee48c9f4861cc49d49eb6c02.
  This is a shallow checkout on prest626/p4-main-bringup with origin configured.
- git sync alias unavailable; bash scripts/sync attempted and fails at git fetch due
  to DNS. Connector supplied the pinned current source instead. git fsck connectivity
  and clean checkout checks passed before adding these docs.
- At this snapshot WP-4 slice 1, WP-3 core and WP-10 are present on main.
  WP-5 work remains in PR #20. Do not rely on the stale blank PRD assignment board.
- Version/device/load fixtures, memory reserve calibration, standalone tokens/s,
  two/three-machine inference, helper cache timings, and offline test: **not measured**.
  These cannot be declared complete until the binaries, approved model, and helpers exist.
- Manual instructions are prepared in runbook-manual.md; commands there remain
  unverified on this machine. D-14 excludes mid-answer recovery from demo scope.
