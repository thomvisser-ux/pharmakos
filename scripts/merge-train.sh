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
# this checkout) and <pr> is its pull request number. The train first fast-forwards this
# checkout's main to origin/main and stops if it cannot. Then, for every pair, in order:
#
#   1. wait for the PR's checks (the three-OS matrix, the DCO check, the cross-OS guard)
#      and stop unless every one of them passed;
#   2. with --local-verify only: check the tree that will merge if main has moved (below);
#   3. remove the lane's worktree (it must be clean and fully pushed), so the branch can be
#      deleted after the merge;
#   4. rebase-merge the PR and delete its branch, then fast-forward this checkout's main;
#   5. without --local-verify: rebase the NEXT lane's worktree onto the new main and push it
#      with --force-with-lease, so its checks re-run on the tree that will actually merge.
#
# A rebase conflict always stops the train: resolve it by hand, push (the matrix then runs
# on the resolved branch), and restart from that lane.
#
# --local-verify (decisions-log item 115 (7), taken by the owner on 2026-09-26 and recorded
# in item 116). A lane whose base main has moved past, by anything but prose, is checked on
# this machine instead of by a second three-OS matrix: its worktree is rebased onto
# origin/main, `cargo xtask ci --locked --require-tools --check` runs there on the lane's own
# target (TRAIN_BUILD_ROOT/<lane>, default D:/build), and the worktree is put back on its
# pushed head (also on any exit, by a trap). The lane's green checks on that head still gate
# the merge, which is pinned to that head (--match-head-commit); the train stops if
# origin/main moved by anything but prose during the check, and after the merge it compares
# main's tree with the tree it checked. That local run is one Windows `cargo xtask ci` leg
# with the headless client check: it does not reproduce the Linux and macOS legs, the
# watch-check run or the Linux vista render. So once the train has merged anything on a local
# check, it watches main's own push run of the train's last commit and stops loudly unless
# it is green. That run is the three-OS check of the whole combination; ci.yml gives each
# commit on main a concurrency group of its own, so the push runs of the train's earlier
# commits run to the end beside it rather than being cancelled.
#
# A lane falls back to rebase, push and a full matrix when its own changes touch output that
# only the three legs produce, each on its own OS, or that no local run executes: the
# determinism code (crates/sim/), the walled float crates (crates/mesher/,
# crates/client-gdext/), godot/, the workflows (.github/), the hashed or rendered goldens
# (tests/golden/determinism|scenarios|pathing|mapgen|mesher|vista/), and what the zip ships
# or is built by, which only the package and clean-launch jobs exercise (library/, rules/,
# packaging/, LICENSES/, REUSE.toml, xtask/src/package.rs and zip.rs, and the zips'
# manifests in tests/golden/package/; decisions-log items 119 (1) and 121 (5)), with the
# root Cargo.toml and .cargo/ for the release profiles the package builds with. It also
# falls back when GitHub reports the PR as BEHIND (the ruleset requiring up-to-date
# branches). A base that moved only by prose (docs/, .claude/, AGENTS.md, CLAUDE.md, a
# top-level *.md) needs no check. A branch with merge commits stops the train: a rebase
# would drop what they carry.
#
# The script is the owner's session merging under decisions-log items 85 and 115 (7); it
# only does what a green PR has already earned, and it halts on anything else so a person
# looks. Run it from the repository root or anywhere inside it. Git Bash on Windows is
# enough: bash, git, gh (signed in), awk, grep, tr, and cargo on PATH for --local-verify.

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
git fetch -q origin main || fail "cannot fetch origin/main"
git merge -q --ff-only origin/main || fail "cannot fast-forward this checkout's main to origin/main"
[ "$(git rev-parse main)" = "$(git rev-parse origin/main)" ] \
  || fail "this checkout's main is not origin/main (unpushed commits, or another upstream)"
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
  ! grep -qvE '^(pass|skipping)$' <<<"$states"
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
  # The push runs the pre-push hook (`cargo xtask fmt clippy`, decisions-log item 132) in the
  # lane's warm target, as the local verify below does, never a cold <worktree>/target.
  CARGO_TARGET_DIR="$build_root/$lane" git -C "$wt" push --force-with-lease
}

# The paths two commits differ by, one per line: NUL-separated from git so no name is
# quoted, and --no-renames so both sides of a rename are listed.
paths_between() { git diff --no-renames --name-only -z "$1" "$2" | tr '\0' '\n'; }

# Prose: nothing the suite reads (decisions-log item 115 (4)'s allow-list).
PROSE='^(docs/|\.claude/|AGENTS\.md$|CLAUDE\.md$|[^/]+\.md$)'
# True when a path list (one per line) names anything but prose; an empty list names nothing.
has_non_prose() { [ -n "$1" ] && grep -qvE "$PROSE" <<<"$1"; }
# Output only the three legs produce, each on its own OS, or that no local run executes.
# Its paths after the golden areas are what ships in the zip, or what builds it, which only
# CI's package and clean-launch jobs exercise, because the local check runs `cargo xtask ci`
# and never `cargo xtask package`. The root Cargo.toml and .cargo/ are on it for their
# profiles: the package builds the client with [profile.release-client] and gamectl with
# --release, and no local run builds either profile. CHANGELOG.md is not on it: the zip's
# manifest pins its path and not its bytes, and PROSE above calls it prose.
MATRIX_ONLY='^(crates/sim/|crates/mesher/|crates/client-gdext/|godot/|\.github/|tests/golden/(determinism|scenarios|pathing|mapgen|mesher|vista)/|library/|rules/|packaging/|LICENSES/|REUSE\.toml$|xtask/src/(package|zip)\.rs$|tests/golden/package/|Cargo\.toml$|\.cargo/)'

# What the train checked for the current lane (set by verify_lane).
checked_head="" checked_main="" checked_tree=""
merged_on_local_check=""

# Rebase the lane onto main, push it, wait until GitHub's PR head is the pushed commit, and
# wait for the full matrix on it.
rematrix() {
  local lane="$1" pr="$2" wt="$parent/pharmakos-$1" pushed tries=0
  rebase_lane "$lane"
  pushed="$(git -C "$wt" rev-parse HEAD)"
  until [ "$(gh pr view "$pr" --json headRefOid --jq .headRefOid)" = "$pushed" ]; do
    tries=$((tries + 1))
    [ "$tries" -le 60 ] || fail "#$pr's head did not become $pushed within five minutes"
    sleep 5
  done
  wait_for_checks "$pr"
  checked_head="$pushed"
  checked_main="$(git rev-parse main)"
  checked_tree=""
}

# With --local-verify: decide what the lane needs, and check it. On return, checked_head is
# the head the merge is pinned to, checked_main the main it was checked against, and
# checked_tree the tree a local check verified (empty when none ran).
verify_lane() {
  local lane="$1" pr="$2" branch="$3" wt="$parent/pharmakos-$1" base moved own log code dirty
  git fetch -q origin main "$branch" || fail "cannot fetch origin/main and origin/$branch"
  git merge -q --ff-only origin/main || fail "cannot fast-forward main to origin/main"
  checked_main="$(git rev-parse main)"
  checked_head="$(git rev-parse "origin/$branch")"
  checked_tree=""
  [ "$(gh pr view "$pr" --json headRefOid --jq .headRefOid)" = "$checked_head" ] \
    || fail "#$pr's head is not origin/$branch; restart from $lane"
  base="$(git merge-base main "origin/$branch")"
  [ -z "$(git rev-list --merges "$base..origin/$branch")" ] \
    || fail "#$pr's branch has merge commits, which a rebase would drop; rebase it linearly, push, and restart from $lane"
  moved="$(paths_between "$base" main)"
  if [ -z "$moved" ]; then
    printf '== %s: up to date with main; its checks stand\n' "$lane"
    return 0
  fi
  if ! has_non_prose "$moved"; then
    printf '== %s: main moved only by prose since its branch point; its checks stand\n' "$lane"
    return 0
  fi
  own="$(paths_between "$base" "origin/$branch")"
  if grep -qE "$MATRIX_ONLY" <<<"$own"; then
    printf '== %s: touches output only the three-OS matrix shows; re-running it\n' "$lane"
    rematrix "$lane" "$pr"
    return 0
  fi
  if [ ! -d "$wt" ]; then
    git worktree add -q -B "$branch" "$wt" "origin/$branch"
  fi
  [ -z "$(git -C "$wt" status --porcelain)" ] || fail "worktree $wt is not clean"
  [ "$(git -C "$wt" rev-parse HEAD)" = "$checked_head" ] \
    || fail "worktree $wt is not at origin/$branch; reset it with: git -C $wt reset --hard origin/$branch (never push it), then restart from $lane"
  # Put the worktree back on its pushed head whatever happens from here.
  trap "git -C '$wt' rebase --abort >/dev/null 2>&1 || true; git -C '$wt' reset -q --hard '$checked_head' >/dev/null 2>&1 || true" EXIT
  printf '== %s: rebasing onto main locally to check the tree that will merge\n' "$lane"
  if ! git -C "$wt" rebase -q main; then
    fail "rebase of $lane onto main conflicts; resolve it by hand, push (its matrix then runs on the resolved branch), and restart from $lane"
  fi
  log="${TMPDIR:-/tmp}/merge-train-$lane-ci.log"
  printf '== %s: cargo xtask ci on %s (log %s)\n' "$lane" "$(git -C "$wt" log --oneline -1)" "$log"
  code=0
  ( cd "$wt" && CARGO_TARGET_DIR="$build_root/$lane" PHARMAKOS_REQUIRE_TOOLS=1 \
      cargo xtask ci --locked --require-tools --check ) > "$log" 2>&1 || code=$?
  grep -A 20 '== summary' "$log" || true
  dirty="$(git -C "$wt" status --porcelain)"
  checked_tree="$(git -C "$wt" rev-parse 'HEAD^{tree}')"
  git -C "$wt" reset -q --hard "$checked_head"
  trap - EXIT
  [ "$code" = 0 ] || fail "cargo xtask ci failed on $lane rebased onto main (exit $code); see $log"
  [ -z "$dirty" ] || fail "cargo xtask ci changed files in $lane's rebased tree: $dirty"
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
  merge_args=(--rebase --delete-branch)
  if [ "$local_verify" = 1 ]; then
    verify_lane "$lane" "$pr" "$branch"
    if [ -n "$checked_tree" ] \
       && [ "$(gh pr view "$pr" --json mergeStateStatus --jq .mergeStateStatus)" = "BEHIND" ]; then
      printf '== %s: GitHub requires #%s to be up to date; re-running the matrix\n' "$lane" "$pr"
      rematrix "$lane" "$pr"
    fi
    # Nothing but prose may land on main between the check and the merge.
    git fetch -q origin main || fail "cannot fetch origin/main"
    if has_non_prose "$(paths_between "$checked_main" origin/main)"; then
      fail "origin/main moved by more than prose while $lane was being checked; restart from $lane"
    fi
    merge_args+=(--match-head-commit "$checked_head")
  fi

  if [ -d "$wt" ]; then
    [ -z "$(git -C "$wt" status --porcelain)" ] || fail "worktree $wt is not clean; commit or stash, then restart from $lane"
    unpushed="$(git -C "$wt" log --oneline "origin/$branch..HEAD" 2>/dev/null || true)"
    [ -z "$unpushed" ] || fail "worktree $wt has commits not on origin/$branch; push them, then restart from $lane"
    git worktree remove "$wt"
  fi

  printf '== %s: merging #%s (%s)\n' "$lane" "$pr" "$branch"
  gh pr merge "$pr" "${merge_args[@]}"
  git pull --ff-only
  git branch -D "$branch" >/dev/null 2>&1 || true
  printf '== %s: merged; main is %s\n' "$lane" "$(git log --oneline -1)"

  if [ -n "$checked_tree" ]; then
    # What landed must be the tree the suite checked, give or take prose that landed beside it.
    if has_non_prose "$(paths_between "$checked_tree" HEAD)"; then
      fail "main's tree after merging #$pr differs from the tree checked for $lane by more than prose; look before the next merge"
    fi
    merged_on_local_check="$merged_on_local_check #$pr"
  fi

  if [ "$local_verify" = 0 ] && [ "$i" -lt "$total" ]; then
    next="${@:$((i + 1)):1}"
    rebase_lane "${next%%:*}"
  fi
done

git fetch --prune >/dev/null 2>&1 || true

if [ -n "$merged_on_local_check" ]; then
  # main's push run of the train's last commit is the three-OS check of the combination.
  sha="$(git rev-parse HEAD)"
  printf '== main: waiting for the push run of %s (merged on a local check:%s)\n' "$sha" "$merged_on_local_check"
  run_id="" tries=0
  until [ -n "$run_id" ]; do
    run_id="$(gh run list --workflow ci.yml --branch main --event push --commit "$sha" --json databaseId --jq '.[0].databaseId // empty' 2>/dev/null || true)"
    [ -n "$run_id" ] && break
    tries=$((tries + 1))
    [ "$tries" -le 40 ] || fail "no push run of ci.yml appeared for $sha within ten minutes; check main by hand"
    sleep 15
  done
  gh run watch "$run_id" --exit-status --interval 60 >/dev/null \
    || fail "main's push run $run_id for $sha is not green after merging$merged_on_local_check on a local check; revert or fix before the next run"
  printf '== main: push run %s is green\n' "$run_id"
fi

printf '\n== merge-train: done; main is %s\n' "$(git log --oneline -1)"
