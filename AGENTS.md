# AGENTS.md

Rules for every coding agent on this repo (Claude Code, Codex, Cursor, pi, anything). Several people and several
agents work on this repo at the same time, each in its own folder (git worktree) and branch.

## Shared picture (read first, every session)

- Read the pinned **agent log, issue #11** (`gh issue view 11 --comments`): start from the latest "state of the world"
  comment, then everything after it. Then read `PRD.md` on the `notes` branch (`git show origin/notes:PRD.md`).
- Introduce yourself once in #11 (agent name, human, work package WP-n, branch). Post on every status change:
  `[agent] [WP-n] [started | progress | blocked | needs-decision | concept-change | done] summary + links`.
- Comment densely: the why, rejected options, assumptions. Every file and major block starts with
  `CONCEPT:` / `WHY:` (decision ID) / `DEPENDS ON:` / `AFFECTS:`.
- Changed a concept (a contract, a dependency, an assumption)? Post `concept-change` in #11.
- Ship messages and commits include `Why:` and `Affects:`.

## Before you write code

1. Run `git sync`. It puts this branch on top of the latest `main` and lists what teammates shipped.
2. Read `CONTRACTS.md`. It has the shared shapes: API routes, data types, env vars, folder ownership.
3. If you were given an issue number, read it: `gh issue view <number>`.

## While working

- Stay inside the files and folders your task owns (see the issue and the ownership table in `CONTRACTS.md`).
  If you must touch another area, keep the change small and say so in the ship message.
- Never edit files in another worktree folder. Never commit to `main` directly.
- Need a new route, type or env var that others will use? Add it to `CONTRACTS.md` **first**, ship that change on its
  own (`git ship "contract: add X"`), then build against it.
- Don't rename or delete shared files, routes or types. Add new ones instead; other branches depend on the old names.
- Don't reformat files you didn't otherwise change (it creates merge conflicts).
- Run `git sync` again before any large change and whenever you have been working for more than ~30 minutes.

## Shipping

- Ship small and often: `git ship "what changed"` whenever the app still runs. It commits, rebases on `main`,
  opens a pull request and merges it.
- If `git sync` or `git ship` reports a conflict: resolve it keeping the intent of both sides, `git add` the files,
  `git rebase --continue`, then ship again. Ask the human if the two sides really disagree.
- If `git sync` prints a heads-up about another branch changing the same files, tell the human before continuing.
- When the task is finished, tell the human. They run `git done` (or `git done <issue-number>`).

## Project

<!-- Fill in once the stack is picked. Keep it short; agents read this every session. -->

- Idea: pool ordinary laptops (one Windows Dell, several M5 Macs) into one llama.cpp model too big for any one of them, no cloud. Full plan: `PRD.md` on `notes`.
- Stack: Rust (all team code). llama.cpp RPC (official binaries, build 8665) supervised as child processes. gRPC via tonic + protox.
- Run locally:
- Test / check before shipping:
- Deploy:
