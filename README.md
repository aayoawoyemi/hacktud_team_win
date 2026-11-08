# hacktud_team_win

Hackathon project repo. Each person and each coding agent works on its own task, in its own folder and branch,
and merges small pieces into `main` many times an hour. Five git commands do all of the git work.

## Results (measured, Sun Nov 8 2026)

<!-- CONCEPT: the numbers Devpost and the pitch quote. WHY: Aries 13:47Z; every row links its raw bench files.
DEPENDS ON: crates/bench (`bench`), test-runs/bench/. AFFECTS: Devpost, video. Add a row per new bench run. -->

Three MacBook Airs on a gigabit switch (M3 16 GB as main, two M5 24 GB as helpers), llama.cpp b8665 over RPC,
SpacetimeDB as the control plane, no internet. Every number is from `bench`: thinking off, ctx 4096, one warm-up +
3 measured runs per prompt (short 128, long 512, edit 512 tokens), medians.

| Run | Model | Machines | Flags | Gen tok/s (median) | TTFT | Raw files |
|---|---|---|---|---|---|---|
| **B3** | **Gemma 4 26B-A4B Q8_0 (25.0 GiB)** | **3 Airs** | FA, q8 KV, `--no-mmap` | **12.5** (12.1-12.6), flat for 5 min | **0.6 s** (short/long) | `test-runs/bench/p4-26b-final/` |
| B2-fa | Gemma 4 12B Q4_K_M (7.1 GB) | 3 Airs | FA, q8 KV | 11.2 (10.8-11.9), flat for 5.5 min | 0.6-0.7 s | `test-runs/bench/p4-20261108-0150-existing-fa-q8/` |
| B1 | Gemma 4 12B Q4_K_M | 1 M5 Air | none | ~16 cold, **8-9 sustained** (throttles after ~1.5 min) | 0.3-0.9 s | `test-runs/bench/night-20261108-0408/` |
| B1s | Gemma 4 12B Q4_K_M | 1 M5 Air | `--spec-type ngram-simple` | rewrite task: **1.6-1.7x** (9 → 14.5, 7.2 → 12.4); new text: no gain | | `test-runs/bench/night-20261108-0329/`, `-0408/` |

What they show:
- **The 26B runs on hardware where no single machine can hold it:** 25.0 GiB of weights; the biggest Air's GPU
  budget is 17.8 GiB (1.4x). Placement on the B3 run: M3 5718 MiB, each M5 9941 MiB, all 31 layers on GPU.
- **Sustained, the pool beats one laptop:** three fanless Airs hold ~11-12.5 tok/s for minutes; one Air alone
  drops from ~16 to 8-9 tok/s as it heats up. Cold, one Air is faster on the 12B. (Caveat: the pool runs used
  flash attention + q8 KV; B1 didn't.)
- **Speculative decoding (n-gram) speeds up rewrite tasks 1.6-1.7x** (79% draft acceptance), nothing on new text.

Known limits:
- **Main must load big models with `--no-mmap`.** With mmap, the 26B mapped 25.6 GB on the 16 GB M3 and generation
  stalled in Metal (`IOGPUCommandDescriptor::prepare … 0xE00002BE`); with `--no-mmap` main holds only its 5718 MiB share.
  Put `extra_args = ["--no-mmap"]` in coordinator.toml when the model is bigger than main's RAM.
- Cold 26B load: ~5 min (weights stream to the helpers); `rpc-server -c` caches them for the next load.
- macOS Local Network privacy: start pool programs from Terminal, not from inside another app.
- Fanless Airs need `caffeinate -dims` and power: a sleeping helper takes the pool down until it's back.

## Setup (once per laptop)

Needs git, Git Bash on Windows, and the GitHub CLI (`gh auth login`).

```
git clone https://github.com/aayoawoyemi/hacktud_team_win.git
cd hacktud_team_win
bash scripts/setup
```

## Commands

| Command | What it does |
|---|---|
| `git task "Add login page"` | Creates a GitHub issue assigned to you, a branch, and a separate folder for it. Prints the command to start your agent there. |
| `git task 12` | Same, for existing issue #12 |
| `git sync` | Puts your work on top of the latest `main`, lists what teammates shipped, and warns if another branch touches your files |
| `git ship "what you did"` | Commits everything, syncs, opens a pull request and merges it into `main` |
| `git board` | Open tasks and owners, branches in progress, recent merges, file overlap warnings |
| `git done` | Task finished: closes the issue, deletes the folder and branch. `git done 12` does it for #12 from anywhere. |

## The loop

1. Hand out work as issues: `git task "..."`, or create them on GitHub and assign people.
2. Each task gets its own folder next to the repo (`hacktud_team_win-wt/12-add-login-page`). Open your agent there.
   Run three tasks and you can have three agents working at once without touching each other's files.
3. `git ship` whenever the app still runs. Every 30 to 60 minutes is the target. Small merges rarely conflict.
4. `git sync` often. It keeps you on current code.

## ? Why it works this way

- One folder per task: two agents cannot overwrite each other's files mid-edit.
- `AGENTS.md`: the rules every agent reads first (Claude Code reads it through `CLAUDE.md`). Everyone's agents follow
  the same conventions.
- `CONTRACTS.md`: shared types, routes, env vars and folder ownership. Changing it first means agents build against the
  same interface. A study of 33,596 agent pull requests (Xu et al.) found pull requests from two different agents
  conflicted 41.7% of the time, against 19.8% for two from the same agent. 26.8% of conflict regions were one agent
  editing a file the other deleted, which is why `AGENTS.md` says to add rather than rename or delete.
- Overlap warnings: `git sync` pushes work in progress, so everyone's sync can see which other branch edits the same files
  before the merge.
- `rerere` (set by setup): git remembers how a conflict was fixed and reuses the fix.
- `.env` files are copied into each new task folder automatically and are never committed.

## Build (Rust workspace)

The Rust code is one Cargo workspace: `Cargo.toml` at the root, one crate per role under `crates/`
(contracts, control, agent, coordinator, planner, relay, bench, netcheck). `rust-toolchain.toml` pins Rust 1.99.1
plus the `wasm32-unknown-unknown` target.

- Windows: Rust (https://rustup.rs) with the MSVC build tools. macOS: rustup and the Xcode command line tools.
- First run: rustup downloads Rust 1.99.1 and the wasm32 target automatically on the first `cargo` command here.
- No `protoc` needed: the contracts crate compiles its `.proto` file with `protox`, a pure-Rust compiler.
- Build and test everything from the repo root: `cargo build` and `cargo test`.
- Control plane (D-13): `crates/control` is a SpacetimeDB module. Only the main machine needs the `spacetime` CLI
  (it is also the server), pinned to 2.14.1:
  - Windows: `iwr https://windows.spacetimedb.com -useb | iex`; macOS: `curl -sSf https://install.spacetimedb.com | sh`
  - then `spacetime version install 2.14.1 --use` (the installer takes the newest release).
  - `spacetime start` (listens on 0.0.0.0:3000), then
    `spacetime publish -s local -p crates/control pool -y` (database name `pool`). Always `-s local`: the default server is maincloud.
  - After changing `crates/control`, regenerate the committed client bindings:
    `spacetime generate --lang rust --out-dir crates/contracts/src/control --module-path crates/control -y`.
- Model files (`models/`, `*.gguf`) and `target/` are git-ignored.
