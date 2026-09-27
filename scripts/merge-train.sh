#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Merge a wave's pull requests in order, and stop at the first thing that is not green.
#
#     scripts/merge-train.sh t8:10 t9:11 t7:12
#     scripts/merge-train.sh --local-verify t18b:39 t20a:40
#
# Each argument is <lane>:<pr>: the lane's worktree is ../pharmakos-<lane> (a sibling of
# this checkout) and <pr> is its pull request number. For every pair, in order:
#
#   1. wait for the PR's checks (the three-OS matrix, the DCO check, the cross-OS guard)
#      and stop unless every one of them passed;
#   2. with --local-verify only: if main has moved past the PR's branch point with anything
#      but prose, check the tree that will actually merge (see below);
#   3. remove the lane's worktree (it must be clean and fully pushed), so the branch can be
#      deleted after the merge;
#   4. rebase-merge the PR and delete its branch, then fast-forward this checkout's main;
#   5. without --local-verify: rebase the NEXT lane's worktree onto the new main and push it
#      with --force-with-lease, so its checks re-run on the tree that will actually merge.
#      A rebase conflict aborts the rebase and stops the train.
#
# --local-verify (decisions-log item 116, the owner's answer to item 115 (7)). A lane whose
# base has moved is checked on this machine instead of by a second three-OS matrix: its
# worktree is rebased onto main locally, the full `cargo xtask ci` runs there on the lane's
# own target (TRAIN_BUILD_ROOT/<lane>, default D:/build), and the worktree is put back on its
# pushed head. The PR's green checks on that head still gate the merge, and GitHub rebases it
# onto the same main when it merges, so the tree the suite checked is the tree that lands;
# main's own push run is the three-OS check of the combination. The lane falls back to the
# old way (rebase, push, wait for the matrix) when its rebase conflicts, or when it touches
# the determinism code or a hash chain (crates/sim/, tests/golden/determinism/,
# tests/golden/scenarios/), whose cross-OS agreement only the matrix shows. A base that moved
# only by prose (docs/, .claude/, AGENTS.md, CLAUDE.md, a top-level *.md) needs no check.
#
# The script is the owner's session merging under decisions-log item 85; it only does what a
# green PR has already earned, and it halts on anything else so a person looks. Run it from
# the repository root or anywhere inside it. Git Bash on Windows is enough: bash, git, gh
# (signed in), awk, grep, and cargo on PATH for --local-verify.

set -euo pipefail

fail() { printf '\n== merge-train: STOP — %s\n' "$*" >&2; exit 1; }

local_verify=0
if [ "${1:-}" = "--local-verify" ]; then
  local_verify=1
  shift
fi
build_root="${TRAIN_BUILD_ROOT:-D:/build}"

repo_root="$(git rev-parse --show-toplevel)" || fail "not inside a git repository"
cd "$repo_root"
parent="$(dirname "$repo_root")"

[ "$#" -ge 1 ] || fail "usage: scripts/merge-train.sh [--local-verify] <lane>:<pr> [<lane>:<pr> ...]"
for pair in "$@"; do
  case "$pair" in
    *:*) ;;
    *) fail "argument '$pair' is not <lane>:<pr>" ;;
  esac
done

[ "$(git branch --show-current)" = "main" ] || fail "this checkout is not on main"
[ -z "$(git status --porcelain)" ] || fail "this checkout is not clean"
# Start from origin's main, so a check against main is a check against what the PR merges onto.
git pull -q --ff-only || fail "cannot fast-forward this checkout's main to origin/main"
if [ "$local_verify" = 1 ]; then
  command -v cargo >/dev/null 2>&1 || fail "--local-verify needs cargo on PATH"
fi

# Every check on the PR must have passed. `gh pr checks` prints one tab-separated line per
# check: name, state, duration, url. States seen in this repository: pass, fail, pending,
# skipping (a job whose `if:` was false, which is not a failure).
checks_green() {
  local pr="$1" states
  states="$(gh pr checks "$pr" 2>/dev/null | awk -F'\t' 'NF >= 2 { print $2 }')" || return 1
  [ -n "$states" ] || return 1
  ! printf '%s\n' "$states" | grep -qvE '^(pass|skipping)$'
}

wait_for_checks() {
  local pr="$1" tries=0
  printf '== #%s: waiting for checks\n' "$pr"
  # Right after a push GitHub lists no checks for a few seconds, and `--watch` returns at
  # once with nothing to watch; an empty list is "not yet", not "not green". Wait for the
  # first check to appear (up to ten minutes) before watching.
  until [ -n "$(gh pr checks "$pr" 2>/dev/null | awk -F'\t' 'NF >= 2')" ]; do
    tries=$((tries + 1))
    [ "$tries" -le 40 ] || fail "no checks appeared on #$pr within ten minutes"
    sleep 15
  done
  # --watch returns 0 when every check passed and non-zero otherwise; the verdict is taken
  # from a fresh listing rather than from that exit code.
  gh pr checks "$pr" --watch --interval 30 >/dev/null 2>&1 || true
  gh pr checks "$pr" 2>/dev/null | awk -F'\t' 'NF >= 2 { printf "   %-40s %s\n", $1, $2 }' || true
  checks_green "$pr" || fail "checks on #$pr are not all green"
}

rebase_lane() {
  local lane="$1" wt="$parent/pharmakos-$1"
  [ -d "$wt" ] || fail "worktree $wt does not exist"
  [ -z "$(git -C "$wt" status --porcelain)" ] || fail "worktree $wt is not clean"
  printf '== %s: rebasing onto main\n' "$lane"
  if ! git -C "$wt" rebase main; then
    git -C "$wt" rebase --abort || true
    fail "rebase of $lane onto main conflicts; resolve it by hand, push, and restart the train from $lane"
  fi
  git -C "$wt" push --force-with-lease
}

# Paths that are prose: nothing the suite reads (decisions-log item 115 (4)'s allow-list).
PROSE='^(docs/|\.claude/|AGENTS\.md$|CLAUDE\.md$|[^/]+\.md$)'
# Paths whose cross-OS agreement only the three-OS matrix shows.
CHAINS='^(crates/sim/|tests/golden/determinism/|tests/golden/scenarios/)'

# With --local-verify: check the tree that will merge when main has moved past the lane's
# branch point with anything but prose. Returns after the worktree is back on its pushed head.
verify_lane() {
  local lane="$1" pr="$2" branch="$3" wt="$parent/pharmakos-$1" base moved own log code
  [ -d "$wt" ] || fail "worktree $wt does not exist"
  [ -z "$(git -C "$wt" status --porcelain)" ] || fail "worktree $wt is not clean"
  git -C "$wt" fetch -q origin "$branch"
  [ "$(git -C "$wt" rev-parse HEAD)" = "$(git -C "$wt" rev-parse "origin/$branch")" ] \
    || fail "worktree $wt is not at origin/$branch; push or reset it, then restart from $lane"
  base="$(git merge-base main "origin/$branch")"
  moved="$(git diff --no-renames --name-only "$base" main)"
  if [ -z "$moved" ] || ! printf '%s\n' "$moved" | grep -qvE "$PROSE"; then
    printf '== %s: main moved only by prose since its branch point; its checks stand\n' "$lane"
    return 0
  fi
  own="$(git diff --no-renames --name-only "$base" "origin/$branch")"
  if printf '%s\n' "$own" | grep -qE "$CHAINS"; then
    printf '== %s: touches the determinism code or a hash chain; re-running the matrix\n' "$lane"
    rebase_lane "$lane"
    wait_for_checks "$pr"
    return 0
  fi
  printf '== %s: rebasing onto main locally to check the tree that will merge\n' "$lane"
  if ! git -C "$wt" rebase -q main; then
    git -C "$wt" rebase --abort || true
    printf '== %s: the local rebase conflicts; re-running the matrix instead\n' "$lane"
    rebase_lane "$lane"
    wait_for_checks "$pr"
    return 0
  fi
  log="${TMPDIR:-/tmp}/merge-train-$lane-ci.log"
  printf '== %s: cargo xtask ci on %s (log %s)\n' "$lane" "$(git -C "$wt" log --oneline -1)" "$log"
  code=0
  ( cd "$wt" && CARGO_TARGET_DIR="$build_root/$lane" cargo xtask ci ) > "$log" 2>&1 || code=$?
  grep -A 20 '== summary' "$log" || true
  git -C "$wt" reset -q --hard "origin/$branch"
  [ "$code" = 0 ] || fail "cargo xtask ci failed on $lane rebased onto main (exit $code); see $log"
  printf '== %s: green on main; merging on its checks\n' "$lane"
}

i=0
total="$#"
for pair in "$@"; do
  i=$((i + 1))
  lane="${pair%%:*}"
  pr="${pair##*:}"
  wt="$parent/pharmakos-$lane"
  branch="$(gh pr view "$pr" --json headRefName --jq .headRefName)" || fail "cannot read PR #$pr"
  state="$(gh pr view "$pr" --json state --jq .state)"
  [ "$state" = "OPEN" ] || fail "PR #$pr is $state, not OPEN"

  wait_for_checks "$pr"
  if [ "$local_verify" = 1 ]; then
    verify_lane "$lane" "$pr" "$branch"
  fi

  if [ -d "$wt" ]; then
    [ -z "$(git -C "$wt" status --porcelain)" ] || fail "worktree $wt is not clean; commit or stash, then restart from $lane"
    unpushed="$(git -C "$wt" log --oneline "origin/$branch..HEAD" 2>/dev/null || true)"
    [ -z "$unpushed" ] || fail "worktree $wt has commits not on origin/$branch; push them, then restart from $lane"
    git worktree remove "$wt"
  fi

  printf '== %s: merging #%s (%s)\n' "$lane" "$pr" "$branch"
  gh pr merge "$pr" --rebase --delete-branch
  git pull --ff-only
  git branch -D "$branch" >/dev/null 2>&1 || true
  printf '== %s: merged; main is %s\n' "$lane" "$(git log --oneline -1)"

  if [ "$local_verify" = 0 ] && [ "$i" -lt "$total" ]; then
    next="${@:$((i + 1)):1}"
    rebase_lane "${next%%:*}"
  fi
done

git fetch --prune >/dev/null 2>&1 || true
printf '\n== merge-train: done; main is %s\n' "$(git log --oneline -1)"
