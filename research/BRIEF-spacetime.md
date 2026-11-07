# Brief: worker "spacetime"

Your name: `spacetime`. Notes folder: `research/spacetime/`. Check-ins: `research/checkins/spacetime-NN.md`.

Goal: make Spacetime (formerly SpacetimeDB, by Clockwork Labs; https://spacetimedb.com, docs version 2.0.0) the backend
of the team's project, so the project qualifies for the Spacetime track, and design how it ties to the compute pool
and a Nemotron agent.

## Track rules (HackUTD 2026 sponsor tracks, read Nov 7)

- **Spacetime main track** ($2,048 / $1,024 / $512 per team): "Build any app or game you want. To qualify, submit a
  public GitHub repo, and the majority of your backend must be built on Spacetime." Access code at the Spacetime booth.
- **Spacetime ScrabbleBot track** (tournament 10:45 to 11:45pm Saturday; $1,024 / $512 / $256): write a bot for
  ScrabbleBot, a Scrabble-style auction game built on Spacetime (bid on tiles, spell words). Public repo.
  https://scrabblebot.spacetimedb.com/
- **NVIDIA track** (the team is also entering it): use Nemotron "beyond a chatbot": multi-step agent workflows that
  plan and execute, tool integration with external APIs/services, real-world applicability.
- **8090 track** (probably also entering): "Build agents and tools that accelerate legacy system modernization."
- Teams can enter up to 3 sponsor tracks.

## The project so far

3 MacBooks (M5 24 GB, M3 16 GB, M5 24 GB) pooled into one private AI server with EXO 1.0 (exposes an OpenAI-compatible
API on port 52415 of each Mac). The pooled model is now NVIDIA Nemotron (EXO has built-in cards: Nemotron-3-Nano-30B-A3B
4/5/6/8-bit = 17.8/21.7/25.7/33.6 GB; Nemotron-3.5-Lightning-30B-A3B 4-bit 17.8 GB). A Nemotron agent runs on the pool.
Read research/PRD.md for the latest.

## Sections (one per run)

1. **What Spacetime is, precisely.** Data model (tables, reducers, procedures, views, HTTP handlers, schedule tables,
   subscriptions), module languages (TypeScript/Rust/C#/C++), client SDKs (which languages; is there Python?), running
   locally (`spacetime start`, standalone, offline?) vs Maincloud, the MCP endpoint, auth/identity. Critical checks:
   can a procedure's `ctx.http.fetch` reach a LAN/localhost address like `http://192.168.x.x:52415` (any SSRF or
   private-IP block, on standalone vs Maincloud)? Max HTTP timeout (docs say 180 s) and whether streaming responses
   work. Install on macOS and Windows. Gotchas from GitHub issues/Discord/Reddit, 2.0 changes.
2. **Architecture: Spacetime as the pool's control plane and the agent's backend.** Propose tables and reducers/
   procedures for: devices joining (join codes; a QR code encodes the join URL), heartbeats and per-device stats,
   layer assignments, jobs/requests and their status, the agent's plan/steps/tool calls as live rows, a reactive
   dashboard that subscribes. Where does a long LLM call happen (procedure vs a node-side client that subscribes to a
   jobs table and writes results back)? Make sure "majority of backend on Spacetime" is clearly true.
3. **ScrabbleBot.** Rules, API, how a bot connects, and whether a quick bot (optionally Nemotron-on-the-pool for word
   choice, or plain heuristics) is feasible before 10:45pm tonight, with a skeleton plan. Keep it short.
4. **Build plan.** Exact module skeleton (file layout, table definitions, reducer signatures in TypeScript), the client
   setup for the dashboard (React + TS SDK), the node-side worker, and the order to build in for ~18 hours.
