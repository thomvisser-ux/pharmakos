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
#      and stop unless every one of them passed on the head they were read for;
#   2. with --local-verify only: check the tree that will merge if main has moved (below);
#   3. remove the lane's worktree (it must be clean and fully pushed), so the branch can be
#      deleted after the merge;
#   4. merge the PR by a merge commit (`gh pr merge --merge`), pinned to the head whose
#      checks passed (--match-head-commit), delete its branch, and fast-forward this
#      checkout's main;
#   5. without --local-verify: bring main into the NEXT lane's worktree by a signed merge
#      and push it, so its checks re-run on the tree that will actually merge.
#
# The merge-only rule (AGENTS.md section 6, decisions-log item 145 (3)). A lane takes main
# only by a signed merge, never by a rebase, and a pushed branch is never force-pushed: the
# desktop app's Auto-fix needs a pushed history that is never rewritten, and the DCO walk
# checks merge commits as it checks every other. So the train never rebases, never
# squashes, never passes --admin or --auto, and never pushes with --force or
# --force-with-lease. Bringing main into a lane is, in its worktree,
#
#     git fetch origin main
#     git merge --no-ff --no-commit FETCH_HEAD
#     git commit -s -m "Merge main (<sha>) into <branch>"
#
# with the hooks running (CARGO_TARGET_DIR is the lane's own target, below). When
# TRAIN_CO_AUTHOR is set (for example "Name <address>"), a Co-Authored-By trailer with it is
# written above the sign-off, as AGENTS.md section 8 places it; the sign-off is git's own
# identity for the worktree. A merge conflict always stops the train: the merge is aborted,
# and the conflict is resolved by hand in a signed merge commit, pushed (the matrix then runs
# on the resolved branch), and the train restarted from that lane.
#
# --local-verify (decisions-log item 115 (7), taken by the owner on 2026-09-26 and recorded
# in item 116). A lane whose base main has moved past, by anything but prose, is checked on
# this machine instead of by a second three-OS matrix: main is merged into its worktree
# without a commit (`git merge --no-ff --no-commit`, so nothing is signed or pushed),
# `cargo xtask ci --locked --require-tools --check` runs on that merged tree on the lane's
# own target (TRAIN_BUILD_ROOT/<lane>, default D:/build), and the worktree is put back on its
# pushed head (also on any exit, by a trap) — but only while HEAD is still that head and
# MERGE_HEAD is still main, and only after what the reset would discard is saved to the
# lane's log; otherwise the train stops and leaves the worktree as it is. A branch that already carries merge commits of
# main is checked the same way. The lane's green checks on that head still gate the merge,
# which is pinned to that head (--match-head-commit), so GitHub's merge commit joins the same
# two parents the train checked; the train stops if origin/main moved by anything but prose
# during the check, and after the merge it compares main's tree with the tree it checked.
# That local run is one Windows `cargo xtask ci` leg with the headless client check: it does
# not reproduce the Linux and macOS legs, the watch-check run or the Linux vista render. So
# once the train has merged anything on a local check, it watches main's own push run of the
# train's last commit and stops loudly unless it is green. That run is the three-OS check of
# the whole combination; ci.yml gives each commit on main a concurrency group of its own, so
# the push runs of the train's earlier commits run to the end beside it rather than being
# cancelled.
#
# A lane falls back to a signed merge of main, a push and a full matrix when its own changes
# touch output that only the three legs produce, each on its own OS, or that no local run
# executes: the determinism code (crates/sim/), the walled float crates (crates/mesher/,
# crates/client-gdext/), godot/, the workflows (.github/), the hashed or rendered goldens
# (tests/golden/determinism|scenarios|pathing|mapgen|mesher|vista/), and what the zip ships
# or is built by, which only the package and clean-launch jobs exercise (library/, rules/,
# packaging/, LICENSES/, REUSE.toml, xtask/src/package.rs and zip.rs, and the zips'
# manifests in tests/golden/package/; decisions-log items 119 (1) and 121 (5)), with the
# root Cargo.toml and .cargo/ for the release profiles the package builds with. It also
# falls back when GitHub reports the PR as BEHIND (the ruleset requiring up-to-date
# branches). A base that moved only by prose (docs/, .claude/, AGENTS.md, CLAUDE.md, a
# top-level *.md) needs no check.
#
# The train runs only while each lane it names is idle: no lane session, fix pass or
# Auto-fix fix is working in that lane's worktree or pushing its branch (the main session's
# process rule). The train merges, resets and removes in those worktrees; its guards (a clean
# worktree, HEAD at the checked head, MERGE_HEAD at main) catch a collision, but they do not
# make one safe.
#
# Every exit before "done" prints a line beginning "== merge-train: STOP".
#
# The script is the owner's session merging under decisions-log items 85 and 115 (7); it
# only does what a green PR has already earned, and it halts on anything else so a person
# looks. Run it from the repository root or anywhere inside it. Git Bash on Windows is
# enough: bash, git, gh (signed in), awk, grep, tr, and cargo on PATH for --local-verify
# (and for the hooks the signed merges and their pushes run).

set -Eeuo pipefail

fail() { printf '\n== merge-train: STOP — %s\n' "$*" >&2; exit 1; }
# A command that fails outside a checked context (set -e's exit) still prints a STOP line;
# -E carries the trap into functions and command substitutions.
trap 'printf "\n== merge-train: STOP — a command failed at line %s (exit %s); look before restarting\n" "$LINENO" "$?" >&2' ERR

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
  # A lane name is lower-case letters, digits and hyphens, and a PR number is digits, so
  # neither can carry a path, an option or a shell metacharacter into the commands below.
  grep -qE '^[a-z0-9-]+:[0-9]+$' <<<"$pair" || fail "argument '$pair' is not <lane>:<pr> (lane [a-z0-9-]+, pr [0-9]+)"
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

# Why a `git merge` in worktree <wt> failed: a conflict when the index holds unmerged paths,
# and otherwise a merge that failed for some other reason (a dirty file in the way, a
# missing ref), which is not a conflict and is not called one.
merge_failure() {
  if [ -n "$(git -C "$1" ls-files -u 2>/dev/null)" ]; then
    printf 'the merge conflicts'
  else
    printf 'the merge failed (not a conflict)'
  fi
}

# The lane last merged into main, as "<lane> #<pr>", for the next merge commit's subject.
last_merged=""

# Bring main into a lane by a signed merge and push it (the merge-only rule, header): no
# rebase, no force. On return, merged_main_into is 1 when a merge commit was made and
# pushed, and 0 when the lane already held main.
merged_main_into=0
merge_main_into_lane() {
  local lane="$1" wt="$parent/pharmakos-$1" branch fetched short msg why
  local -a trailer=()
  merged_main_into=0
  [ -d "$wt" ] || fail "worktree $wt does not exist"
  [ -z "$(git -C "$wt" status --porcelain)" ] || fail "worktree $wt is not clean"
  branch="$(git -C "$wt" branch --show-current)"
  [ -n "$branch" ] || fail "worktree $wt is not on a branch"
  git -C "$wt" fetch -q origin "$branch" || fail "cannot fetch origin/$branch in $wt"
  [ "$(git -C "$wt" rev-parse HEAD)" = "$(git -C "$wt" rev-parse "origin/$branch")" ] \
    || fail "worktree $wt is not at origin/$branch (unpushed commits, or behind it); look, then restart from $lane"
  # Fetched last, so FETCH_HEAD is main.
  git -C "$wt" fetch -q origin main || fail "cannot fetch origin/main in $wt"
  fetched="$(git -C "$wt" rev-parse FETCH_HEAD)"
  [ "$fetched" = "$(git rev-parse main)" ] \
    || fail "origin/main moved past this checkout's main while bringing it into $lane; restart from $lane"
  if git -C "$wt" merge-base --is-ancestor "$fetched" HEAD; then
    printf '== %s: already holds main; nothing to merge\n' "$lane"
    return 0
  fi
  short="$(git rev-parse --short "$fetched")"
  printf '== %s: merging main (%s) into %s\n' "$lane" "$short" "$branch"
  if ! git -C "$wt" merge -q --no-ff --no-commit FETCH_HEAD; then
    why="$(merge_failure "$wt")"
    git -C "$wt" merge --abort || fail "merging main into $lane: $why, and the merge could not be aborted; look in $wt"
    fail "merging main into $lane: $why; resolve it by hand in a signed merge commit, push, and restart the train from $lane"
  fi
  msg="Merge main ($short${last_merged:+, $last_merged}) into $branch"
  if [ -n "${TRAIN_CO_AUTHOR:-}" ]; then
    trailer=(-m "Co-Authored-By: $TRAIN_CO_AUTHOR")
  fi
  # The commit and the push run the versioned hooks (pre-commit's rustfmt and clippy, then
  # pre-push's `cargo xtask fmt clippy`, decisions-log item 132) in the lane's warm target,
  # as the local verify below does, never a cold <worktree>/target. -s writes the sign-off
  # after the trailer, so it stays the last line (AGENTS.md section 8).
  if ! CARGO_TARGET_DIR="$build_root/$lane" git -C "$wt" commit -q -s -m "$msg" "${trailer[@]}"; then
    git -C "$wt" merge --abort \
      || fail "the signed merge commit in $lane was not made (a hook refused it?), and the merge could not be aborted; look in $wt"
    fail "the signed merge commit in $lane was not made (a hook refused it?); look, then restart from $lane"
  fi
  [ "$(git -C "$wt" rev-list --parents -n 1 HEAD | awk '{ print NF }')" = 3 ] \
    || fail "the commit made in $lane is not a two-parent merge commit; look before pushing"
  # A plain push: the merge commit only adds to the pushed history, so it is a fast-forward
  # of the branch, and a push that is not one is refused, never forced.
  CARGO_TARGET_DIR="$build_root/$lane" git -C "$wt" push -q origin "HEAD:$branch" \
    || fail "the push of $lane's merge of main was refused; look (never force it), then restart from $lane"
  merged_main_into=1
}

# Wait until GitHub's head for <pr> is <sha>.
wait_for_head() {
  local pr="$1" sha="$2" tries=0
  until [ "$(gh pr view "$pr" --json headRefOid --jq .headRefOid)" = "$sha" ]; do
    tries=$((tries + 1))
    [ "$tries" -le 60 ] || fail "#$pr's head did not become $sha within five minutes"
    sleep 5
  done
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

# Bring main into the lane by a signed merge, push it, wait until GitHub's PR head is the
# pushed commit, and wait for the full matrix on it.
rematrix() {
  local lane="$1" pr="$2" wt="$parent/pharmakos-$1" pushed
  merge_main_into_lane "$lane"
  pushed="$(git -C "$wt" rev-parse HEAD)"
  wait_for_head "$pr" "$pushed"
  wait_for_checks "$pr"
  # Read once after the checks: a head that is no longer the pushed one moved while they ran.
  [ "$(gh pr view "$pr" --json headRefOid --jq .headRefOid)" = "$pushed" ] \
    || fail "#$pr's head moved while its checks ran; restart from $lane"
  checked_head="$pushed"
  checked_main="$(git rev-parse main)"
  checked_tree=""
}

# Put the worktree back on the lane's pushed head after a local check, and only if nothing
# but the train's own uncommitted merge is in it: HEAD must still be checked_head and, once
# the merge has started, MERGE_HEAD must be main's sha. Otherwise someone else is working
# there (the header: the train runs only while the lane is idle), and nothing is discarded.
# What the reset would discard (the diff against the index and the untracked list) is saved
# to <save> first. Returns non-zero, with the reason on stderr, when it refuses or fails.
restore_checked_worktree() {
  local wt="$1" save="$2" started="$3" merge_head
  [ "$(git -C "$wt" rev-parse HEAD 2>/dev/null)" = "$checked_head" ] || {
    printf 'worktree %s is no longer at %s; someone else is working there, so it was left as it is\n' "$wt" "$checked_head" >&2
    return 1
  }
  if [ "$started" = 1 ]; then
    merge_head="$(git -C "$wt" rev-parse -q --verify MERGE_HEAD 2>/dev/null || true)"
    [ "$merge_head" = "$checked_main" ] || {
      printf 'worktree %s holds no merge of main %s (MERGE_HEAD %s), so it was left as it is\n' "$wt" "$checked_main" "${merge_head:-absent}" >&2
      return 1
    }
  fi
  {
    printf '== restore of %s to %s: git diff against the index\n' "$wt" "$checked_head"
    git -C "$wt" diff
    printf '== untracked, unignored files\n'
    git -C "$wt" ls-files --others --exclude-standard
  } >> "$save" 2>&1 || { printf 'cannot save what the reset discards to %s\n' "$save" >&2; return 1; }
  if [ "$started" = 1 ]; then
    git -C "$wt" merge --abort || { printf 'git merge --abort failed in %s\n' "$wt" >&2; return 1; }
  fi
  git -C "$wt" reset -q --hard "$checked_head" || { printf 'git reset --hard failed in %s\n' "$wt" >&2; return 1; }
}

# With --local-verify: decide what the lane needs, and check it. On entry, checked_head is
# the head whose checks the main loop waited on; the lane is checked at that head or not at
# all. On return, checked_head is the head the merge is pinned to, checked_main the main it
# was checked against, and checked_tree the tree a local check verified (empty when none ran).
verify_lane() {
  local lane="$1" pr="$2" branch="$3" wt="$parent/pharmakos-$1" base moved own log code dirty
  local local_branch why changed untracked
  git fetch -q origin main "$branch" || fail "cannot fetch origin/main and origin/$branch"
  git merge -q --ff-only origin/main || fail "cannot fast-forward main to origin/main"
  checked_main="$(git rev-parse main)"
  checked_tree=""
  [ "$(git rev-parse "origin/$branch")" = "$checked_head" ] \
    || fail "origin/$branch is not $checked_head, the head whose checks passed; restart from $lane"
  [ "$(gh pr view "$pr" --json headRefOid --jq .headRefOid)" = "$checked_head" ] \
    || fail "#$pr's head is no longer $checked_head, the head whose checks passed; restart from $lane"
  # The merge base is the last main the lane took, by its branch point or by a merge of main,
  # so a branch that carries merge commits of main is judged from the main it last merged.
  base="$(git merge-base main "$checked_head")" || fail "no merge base of main and $checked_head"
  moved="$(paths_between "$base" main)"
  if [ -z "$moved" ]; then
    printf '== %s: up to date with main; its checks stand\n' "$lane"
    return 0
  fi
  if ! has_non_prose "$moved"; then
    printf '== %s: main moved only by prose since its branch point; its checks stand\n' "$lane"
    return 0
  fi
  own="$(paths_between "$base" "$checked_head")"
  if grep -qE "$MATRIX_ONLY" <<<"$own"; then
    printf '== %s: touches output only the three-OS matrix shows; re-running it\n' "$lane"
    rematrix "$lane" "$pr"
    return 0
  fi
  if [ ! -d "$wt" ]; then
    # Never -B: a local branch that exists is used only if it is the checked head, so a
    # worktree add never moves a branch someone left elsewhere.
    local_branch="$(git rev-parse -q --verify "refs/heads/$branch" || true)"
    if [ -n "$local_branch" ]; then
      [ "$local_branch" = "$checked_head" ] \
        || fail "local branch $branch is at $local_branch, not origin/$branch ($checked_head); look, then restart from $lane"
      git worktree add -q "$wt" "$branch" || fail "cannot add worktree $wt on $branch"
    else
      git worktree add -q -b "$branch" "$wt" "$checked_head" || fail "cannot add worktree $wt on a new $branch"
    fi
  fi
  [ -z "$(git -C "$wt" status --porcelain)" ] || fail "worktree $wt is not clean"
  [ "$(git -C "$wt" rev-parse HEAD)" = "$checked_head" ] \
    || fail "worktree $wt is not at origin/$branch; reset it with: git -C $wt reset --hard origin/$branch (never push it), then restart from $lane"
  log="${TMPDIR:-/tmp}/merge-train-$lane-ci.log"
  : > "$log" || fail "cannot write $log"
  # Put the worktree back on its pushed head whatever happens from here, under the same
  # guard as below; on a refusal the trap only says so.
  trap "restore_checked_worktree '$wt' '$log' 1 || printf '\n== merge-train: STOP — %s left as it is; see %s\n' '$wt' '$log' >&2" EXIT
  # The tree that will merge is main merged into the lane's pushed head. It is made without a
  # commit, so nothing is signed or pushed and the branch's history is untouched; the index
  # then holds the merged tree, which `git write-tree` names.
  printf '== %s: merging main into it locally, without a commit, to check the tree that will merge\n' "$lane"
  if ! git -C "$wt" merge -q --no-ff --no-commit main; then
    why="$(merge_failure "$wt")"
    fail "merging main into $lane: $why; resolve it by hand in a signed merge commit, push (its matrix then runs on the resolved branch), and restart from $lane"
  fi
  checked_tree="$(git -C "$wt" write-tree)" || fail "cannot write the merged tree in $wt"
  printf '== %s: cargo xtask ci on %s merged with main %s (tree %s; log %s)\n' "$lane" \
    "$(git rev-parse --short "$checked_head")" "$(git rev-parse --short main)" "$checked_tree" "$log"
  code=0
  ( cd "$wt" && CARGO_TARGET_DIR="$build_root/$lane" PHARMAKOS_REQUIRE_TOOLS=1 \
      cargo xtask ci --locked --require-tools --check ) >> "$log" 2>&1 || code=$?
  grep -A 20 '== summary' "$log" || true
  # A file the run changed against the merged index, or one it left untracked and unignored;
  # each command's exit is checked on its own.
  changed="$(git -C "$wt" diff --name-only)" || fail "cannot list the files the run changed in $wt"
  untracked="$(git -C "$wt" ls-files --others --exclude-standard)" || fail "cannot list the untracked files in $wt"
  dirty="$changed${changed:+${untracked:+$'\n'}}$untracked"
  trap - EXIT
  restore_checked_worktree "$wt" "$log" 1 || fail "the worktree was not put back on $checked_head; see $log"
  [ "$code" = 0 ] || fail "cargo xtask ci failed on $lane merged with main (exit $code); see $log"
  [ -z "$dirty" ] || fail "cargo xtask ci changed files in $lane's merged tree (saved to $log): $dirty"
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
  git check-ref-format --branch "$branch" >/dev/null 2>&1 || fail "#$pr's head branch '$branch' is not a valid branch name"
  state="$(gh pr view "$pr" --json state --jq .state)" || fail "cannot read #$pr's state"
  [ "$state" = "OPEN" ] || fail "PR #$pr is $state, not OPEN"

  # The merge is pinned to the head whose checks passed: it is read before and after the
  # wait and must be the same both times, so a push during the wait stops the train rather
  # than merging a head nobody checked.
  head_before="$(gh pr view "$pr" --json headRefOid --jq .headRefOid)" || fail "cannot read #$pr's head"
  wait_for_checks "$pr"
  checked_head="$(gh pr view "$pr" --json headRefOid --jq .headRefOid)" || fail "cannot read #$pr's head"
  [ "$checked_head" = "$head_before" ] || fail "#$pr's head moved while its checks ran; restart from $lane"
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
  fi
  # A merge commit, never a rebase or a squash, and never --admin or --auto (the header).
  merge_args=(--merge --delete-branch --match-head-commit "$checked_head")

  if [ -d "$wt" ]; then
    [ -z "$(git -C "$wt" status --porcelain)" ] || fail "worktree $wt is not clean; commit or stash, then restart from $lane"
    unpushed="$(git -C "$wt" log --oneline "origin/$branch..HEAD" 2>/dev/null || true)"
    [ -z "$unpushed" ] || fail "worktree $wt has commits not on origin/$branch; push them, then restart from $lane"
    git worktree remove "$wt" || fail "cannot remove worktree $wt; look, then restart from $lane"
  fi
  # --delete-branch deletes the local branch too, which git refuses, half-way through, while
  # any worktree has it checked out; so no worktree may hold it when the merge runs.
  if git worktree list --porcelain | grep -qxF "branch refs/heads/$branch"; then
    fail "$branch is checked out in a worktree (git worktree list); remove that worktree, then restart from $lane"
  fi

  printf '== %s: merging #%s (%s)\n' "$lane" "$pr" "$branch"
  gh pr merge "$pr" "${merge_args[@]}" \
    || fail "gh pr merge #$pr failed; the merge may have landed; check #$pr (and origin/main) before restarting from $lane"
  git pull --ff-only || fail "cannot fast-forward this checkout's main after merging #$pr; look before the next merge"
  git branch -D "$branch" >/dev/null 2>&1 || true
  printf '== %s: merged; main is %s\n' "$lane" "$(git log --oneline -1)"
  last_merged="$lane #$pr"

  if [ -n "$checked_tree" ]; then
    # What landed must be the tree the suite checked, give or take prose that landed beside it.
    if has_non_prose "$(paths_between "$checked_tree" HEAD)"; then
      fail "main's tree after merging #$pr differs from the tree checked for $lane by more than prose; look before the next merge"
    fi
    merged_on_local_check="$merged_on_local_check #$pr"
  fi

  if [ "$local_verify" = 0 ] && [ "$i" -lt "$total" ]; then
    # Bring the new main into the next lane and push it, so its checks, which the next
    # iteration waits for on that head, run on the tree that will actually merge.
    next="${@:$((i + 1)):1}"
    merge_main_into_lane "${next%%:*}"
    if [ "$merged_main_into" = 1 ]; then
      wait_for_head "${next##*:}" "$(git -C "$parent/pharmakos-${next%%:*}" rev-parse HEAD)"
    fi
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
