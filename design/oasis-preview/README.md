<!-- CONCEPT: Downloadable Oasis frontend design handoff, isolated from the live dashboard.
WHY: Joseph requested that the team can see his accepted design via issue #11.
DEPENDS ON: A modern browser with OKLCH and JavaScript support; no build or server.
AFFECTS: Design review only. Existing dashboard and backend contracts are untouched. -->
# Oasis — accepted frontend design

## View it

1. Open [oasis.html](oasis.html) on this branch.
2. Use GitHub's **Download raw file** button. If Raw opens source text, save it as `oasis.html`.
3. Open the downloaded HTML in a browser. It includes its CSS and JavaScript; no server, npm install or internet is needed.

GitHub's source view does **not** run the dashboard. This is not a hosted preview URL.
The repository is private, so viewers need team-repository access.

## Accepted by Joseph

- Multiplexer layout: node directory left, connected-node topology centered, inspector right, event log below.
- Graphite palette with SF Mono; all explicit colors use OKLCH.
- Model/context/fit controls open in the right inspector (press **M**).
- Green dot plus connected label; connection and model runtime are distinct.
- Model runtime labels: not loaded, loading, serving, unknown. Initial fixtures are connected with model not loaded.
- Selection traces node, per-helper route, planned layer segment and inspector. Selecting main highlights all helper routes.
- No decorative animation, invented live throughput, generic KPI card grid or copied LUMA identity.

## Try the interactions

Select nodes or their layer segments. Press M for model/context estimates, J for the staged joining preview and ? for keyboard help. Commands such as `help`, `nodes`, `inspect n02`, `model qwen14` and `context 8192` update only local fixtures.

## Integration boundary

**This snapshot is illustrative, not connected to a live cluster.** No SpacetimeDB subscription, real discovery, model load or inference is wired. Runtime speed/latency remain blank. The capacity and layer allocation data are examples, not hardware measurements or a production SplitPlan.

For the live dashboard owner (Agent-P1 / PR #32): use this as a visual/interaction reference, not a replacement patch. Connection, readiness and model runtime need mapping to the current contracts. Honor the current orchestrator decision: failed bring-up/server exit shows STOPPED with the reason in the event log, never FAILED on screen. That live-state flow is not implemented by this snapshot.

## Evidence and source

Exported from Joseph's isolated frontend source commit `3b1de3a` in `pool-dashboard-design`. Nine pure fixture tests pass (capacity, model-state independence, route selection). JavaScript syntax, DOM IDs, accepted visual defaults, OKLCH-only colors and self-contained packaging checks pass. Browser visual/end-to-end QA remains unverified in the agent runner.

No existing application files, shared contracts or root workspace configuration are changed. No merge is requested by sharing this artifact.

Coordination and design decisions: [agent log #11](https://github.com/aayoawoyemi/hacktud_team_win/issues/11).
