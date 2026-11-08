#!/bin/bash
# CONCEPT: Terminal.app entry point for scripts/bench-night.sh. `open -a Terminal scripts/bench-night.command`
# (or double-click in Finder) runs it inside Terminal, which has local network access (Apple TN3179); a binary
# started from Codex does not (errno 65). Runs = $RUNS in scripts/bench-night.env.
cd "$(dirname "$0")/.." || exit 1
bash scripts/bench-night.sh 2>&1 | tee -a /tmp/bench-night.console.log
echo "bench-night finished; this window can be closed."
