# Shared helpers for the team git scripts. Sourced, not run.
set -euo pipefail

say()  { printf '\033[36m%s\033[0m\n' "$*"; }
ok()   { printf '\033[32m%s\033[0m\n' "$*"; }
warn() { printf '\033[33m%s\033[0m\n' "$*" >&2; }
die()  { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

need_gh() {
  command -v gh >/dev/null 2>&1 || die "Install the GitHub CLI (https://cli.github.com), then run: gh auth login"
}

# Folder of the main checkout, even when called from a task worktree.
main_root() {
  dirname "$(git rev-parse --path-format=absolute --git-common-dir)"
}

current_branch() { git branch --show-current; }

# "aayo/12-add-login" -> "12"
issue_of_branch() { printf '%s' "$1" | sed -nE 's#^[^/]+/([0-9]+)-.*#\1#p'; }

slugify() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-+//; s/-+$//' | cut -c1-40 | sed -E 's/-+$//'
}

# GitHub login, cached in git config.
me() {
  local u
  u=$(git config --get hack.user || true)
  if [ -z "$u" ]; then
    u=$(gh api user -q .login)
    git config hack.user "$u"
  fi
  printf '%s' "$u"
}

# Warn when another in-flight branch on GitHub touches the same files as this one.
overlap_check() {
  local b mine rb theirs both found=0
  b=$(current_branch)
  [ -n "$b" ] && [ "$b" != main ] || return 0
  mine=$( { git diff --name-only origin/main...HEAD; git diff --name-only HEAD; git ls-files --others --exclude-standard; } 2>/dev/null | grep -v '^$' | sort -u || true)
  [ -n "$mine" ] || return 0
  for rb in $(git for-each-ref --format='%(refname:short)' refs/remotes/origin); do
    case "$rb" in origin|origin/HEAD|origin/main|"origin/$b") continue ;; esac
    theirs=$(git diff --name-only origin/main..."$rb" 2>/dev/null | sort -u || true)
    both=$(comm -12 <(printf '%s\n' "$mine") <(printf '%s\n' "$theirs") | grep -v '^$' || true)
    if [ -n "$both" ]; then
      found=1
      warn "Heads up: ${rb#origin/} is also changing:"
      printf '%s\n' "$both" | sed 's/^/    /' >&2
    fi
  done
  if [ "$found" = 1 ]; then
    warn "Talk to whoever owns that branch, or ship the smaller change first."
  fi
  return 0
}

# Remember which main this worktree last saw (refs/worktree/* is per-worktree).
mark_seen() { git update-ref refs/worktree/hack-last-main "$(git rev-parse origin/main)"; }
