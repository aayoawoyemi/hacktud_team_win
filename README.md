# hacktud_team_win

Hackathon project repo. Each person and each coding agent works on its own task, in its own folder and branch,
and merges small pieces into `main` many times an hour. Five git commands do all of the git work.

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
    `spacetime publish -s local -p crates/control <db-name> -y`. Always `-s local`: the default server is maincloud.
  - After changing `crates/control`, regenerate the committed client bindings:
    `spacetime generate --lang rust --out-dir crates/contracts/src/control --module-path crates/control -y`.
- Model files (`models/`, `*.gguf`) and `target/` are git-ignored.
