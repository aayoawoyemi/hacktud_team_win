#!/usr/bin/env bash
# Run one section of a research worker as its own Pi session (not a subagent).
#   bash research/run.sh <exo|principles> start
#   bash research/run.sh <exo|principles> next ["FEEDBACK: ..."]
#   bash research/run.sh status
set -uo pipefail
cd "$(dirname "$0")/.."            # worktree root: sessions are keyed to this folder
R=research
mkdir -p "$R/logs" "$R/checkins"
WORKERS=$(for f in "$R"/BRIEF-*.md; do b=${f##*/BRIEF-}; echo "${b%.md}"; done)

if [ "${1:-}" = status ]; then
  for w in $WORKERS; do
    st=$(cat "$R/logs/$w.state" 2>/dev/null || echo "not started")
    last=$(ls -1 "$R/checkins/$w-"*.md 2>/dev/null | tail -1)
    echo "$w: $st  | latest check-in: ${last:-none}"
  done
  exit 0
fi

w=${1:?worker}; mode=${2:?start|next}; fb=${3:-}
[ -f "$R/BRIEF-$w.md" ] || { echo "No brief: $R/BRIEF-$w.md"; exit 1; }
mkdir -p "$R/$w"
sid="hackutd-research-$w"
run=$(ls "$R/logs/$w-run"*.log 2>/dev/null | wc -l | tr -d ' '); run=$((run + 1))
log="$R/logs/$w-run$(printf %02d "$run").log"

if [ "$mode" = start ]; then
  msg="Read research/PROTOCOL.md, then research/BRIEF-$w.md, then research/PRD.md. You are worker \"$w\". Do section 1 only, then stop with your check-in."
else
  msg="${fb:+$fb

}Continue: do your next section only, then stop with your check-in. Re-read research/PRD.md first if it changed."
fi

echo "running section-run $run (started $(date +%H:%M))" > "$R/logs/$w.state"
pi -p --session-id "$sid" --name "HackUTD research: $w" \
   --model claude-sub/claude-opus-5-5 --thinking medium \
   "$msg" > "$log" 2>&1
code=$?
echo "idle: run $run finished $(date +%H:%M) exit=$code, check-in ready" > "$R/logs/$w.state"
