## Check-in principles 06: Recommendation

Full one-page plan: `research/principles/06-recommendation.md`

**Found (most important first):**
- Three pieces only:
  - Pool: EXO plus Nemotron-3-Nano-30B-A3B 6-bit (25.7 GB) on all 3 Macs with `min_nodes: 3`, falling back to 5-bit (21.7 GB). Expect about 15 to 20 t/s.
  - Spacetime backend: a TypeScript worker on the coordinator Mac bridges to EXO, because Spacetime procedures cannot reach LAN addresses (spacetime-01).
  - One Nemotron agent that modernizes a COBOL program into Python and proves it with tests.
- Build order from 4:15pm:
  - Pool test and model pick by 6:15pm, with agent and Spacetime skeletons in parallel.
  - Full integration by 11:15pm.
  - Backup video of a good run by 1am.
  - Overnight soak test, feature freeze at 10am, video 10 to 11am, submit 11:45am.
- Demo (2 min): the problem, 3 Macs holding a model none of them can, the agent's steps streaming live (time-lapsed and labelled, since a job takes 2 to 4 min), then the internet cable pulled on camera.
- Top risks: Nemotron tool calls through EXO, EXO discovery, memory/disk, agent speed, scope creep. Each has a fallback in the table.

**Surprises / things that change the plan:**
- A full agent job will take about 2 to 4 minutes on the pool (8 to 12 steps at 15 to 20 t/s). The video must time-lapse it with an honest label, or the job must be tiny.
- Don't depend on EXO's tool parser. A plain-text JSON tool protocol parsed by our worker removes risk 1 entirely.

**PRD suggestions:**
- Replace Must have with the 3 pieces. Move routing, QR join, Rebalance, speed-of-light meter and the contribution slider to Stretch, gated on "backup video recorded".
- Add the build timetable and the 10am feature freeze.
- Add the demo script and the risk table from 06.

**Questions for Aayo:**
- None. I am done unless asked.

**Next section:** none (brief complete).
