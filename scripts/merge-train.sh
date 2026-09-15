#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Merge a wave's pull requests in order, rebasing the next one onto the new main after
# each merge, and stop at the first thing that is not green.
#
#     scripts/merge-train.sh t8:10 t9:11 t7:12
#
# Each argument is <lane>:<pr>: the lane's worktree is ../pharmakos-<lane> (a sibling of
# this checkout) and <pr> is its pull request number. For every pair, in order:
#
#   1. wait for the PR's checks (the three-OS matrix, the DCO check, the cross-OS guard)
#      and stop unless every one of them passed;
#   2. remove the lane's worktree (it must be clean and fully pushed), so the branch can be
#      deleted after the merge;
#   3. rebase-merge the PR and delete its branch, then fast-forward this checkout's main;
#   4. rebase the NEXT lane's worktree onto the new main and push it with
#      --force-with-lease, so its checks re-run on the tree that will actually merge.
#      A rebase conflict aborts the rebase and stops the train.
#
# Nothing here runs cargo: the matrix is the authority on the rebased tree, and the train
# waits for it at the next step. The script is the owner's session merging under
# decisions-log item 85; it only does what a green PR has already earned, and it halts on
# anything else so a person looks. Run it from the repository root or anywhere inside it.
#
# Git Bash on Windows is enough: bash, git, gh (signed in), awk, grep.

set -euo pipefail

fail() { printf '\n== merge-train: STOP — %s\n' "$*" >&2; exit 1; }

repo_root="$(git rev-parse --show-toplevel)" || fail "not inside a git repository"
cd "$repo_root"
parent="$(dirname "$repo_root")"

[ "$#" -ge 1 ] || fail "usage: scripts/merge-train.sh <lane>:<pr> [<lane>:<pr> ...]"
for pair in "$@"; do
  case "$pair" in
    *:*) ;;
    *) fail "argument '$pair' is not <lane>:<pr>" ;;
  esac
done

[ "$(git branch --show-current)" = "main" ] || fail "this checkout is not on main"
[ -z "$(git status --porcelain)" ] || fail "this checkout is not clean"

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
  local pr="$1"
  printf '== #%s: waiting for checks\n' "$pr"
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

  if [ "$i" -lt "$total" ]; then
    next="${@:$((i + 1)):1}"
    rebase_lane "${next%%:*}"
  fi
done

git fetch --prune >/dev/null 2>&1 || true
printf '\n== merge-train: done; main is %s\n' "$(git log --oneline -1)"
