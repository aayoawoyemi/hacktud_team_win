#!/usr/bin/env bash
# CONCEPT: build the digest from GitHub data only (no AI) and post it to the agent log issue.
# WHY: Aries can't run on a timer; a mechanical digest keeps the shared picture fresh between Aries summaries.
# DEPENDS ON: env REPO, ISSUE, GH_TOKEN; gh + jq (both preinstalled on ubuntu-latest runners).
# AFFECTS: posts one comment per run on $ISSUE. Skips its own digests when reading statuses.
set -euo pipefail
since=$(date -u -d '2 hours ago' +%Y-%m-%dT%H:%M:%SZ)
now=$(date -u +'%Y-%m-%d %H:%M UTC')

# Latest status per agent: comments start with "[agent]"; digest comments start with "### Agent log digest" so they're skipped.
statuses=$(gh api "repos/$REPO/issues/$ISSUE/comments" --paginate --jq '.[] | select(.body | startswith("[")) | {a: (.body | capture("^\[(?<n>[^\]]+)\]").n), t: .created_at, u: .html_url, b: (.body | split("\n")[0] | .[0:160])}' \
  | jq -s 'group_by(.a) | map(max_by(.t)) | sort_by(.t) | reverse | .[] | "| \(.a) | \(.t[11:16]) UTC | \(.b | gsub("\|"; "/")) | [link](\(.u)) |"' -r)

open_prs=$(gh pr list -R "$REPO" --state open --json number,title,author,headRefName --jq '.[] | "- #\(.number) \(.title) (`\(.headRefName)`, \(.author.login))"')
merged=$(gh pr list -R "$REPO" --state merged --search "merged:>=$since" --json number,title,mergedAt --jq '.[] | "- #\(.number) \(.title) (merged \(.mergedAt[11:16]) UTC)"')
branches=$(gh api "repos/$REPO/branches" --paginate --jq '.[].name' | sed 's/^/`/; s/$/`/' | paste -sd ' ' -)
blocked=$(gh api "repos/$REPO/issues/$ISSUE/comments" --paginate --jq --arg s "$since" '.[] | select(.created_at >= $s) | select(.body | test("\[(status: )?(blocked|needs-decision)\]")) | "- \(.body | split("\n")[0] | .[0:160]) ([link](\(.html_url)))"')

body="### Agent log digest, $now (automatic, no AI)

**Latest status per agent**
| Agent | Time | Latest post | |
|---|---|---|---|
${statuses:-| none yet | | | |}

**Blocked or needs a decision (last 2 h)**
${blocked:-- none}

**Open PRs**
${open_prs:-- none}

**Merged in the last 2 h**
${merged:-- none}

**Branches:** ${branches}

Agents: read the latest Aries \"state of the world\" comment for decisions and the big picture; this digest only lists facts."
gh issue comment "$ISSUE" -R "$REPO" --body "$body"
