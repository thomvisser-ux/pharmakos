#!/bin/sh
# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Pharmakos pre-commit hook: rustfmt and clippy on the crates you actually touched.
#
# This is the fast guard, not the real one. `cargo xtask ci` is the real one, and CI runs
# that; the hook exists so the obvious two failures — unformatted code and a clippy warning
# — never reach a pull request. It takes seconds, not minutes.
#
# INSTALL (from the repository root):
#
#     ln -s ../../scripts/pre-commit.sh .git/hooks/pre-commit    # macOS, Linux, Git Bash
#     cp scripts/pre-commit.sh .git/hooks/pre-commit             # Windows without symlinks
#     chmod +x .git/hooks/pre-commit
#
# It is a shell script and Git for Windows ships the shell that runs it, so one file works
# on all three operating systems.
#
# WHY NOT .cargo-husky: cargo-husky installs hooks as a side effect of `cargo test`, which
# means a dev-dependency, a surprise write into .git/hooks, and nothing at all until the
# first test run. xtask is deliberately dependency-free and the install is one line, so the
# explicit script wins. If hook management ever needs to be automatic, add
# `scripts/githooks/pre-commit` (a two-line wrapper around this file) and point
# `git config core.hooksPath scripts/githooks` at it — that keeps the hooks in review.
#
# ESCAPE HATCH: `PHARMAKOS_SKIP_HOOKS=1 git commit ...`, or `git commit --no-verify`. Both
# are for genuine emergencies; CI will still have its say.

set -eu

if [ "${PHARMAKOS_SKIP_HOOKS:-0}" = "1" ]; then
    echo "pre-commit: skipped (PHARMAKOS_SKIP_HOOKS=1)"
    exit 0
fi

root=$(git rev-parse --show-toplevel)
cd "$root"

# Staged files, added/copied/modified/renamed only — a deleted file has nothing to lint.
staged=$(git diff --cached --name-only --diff-filter=ACMR)
if [ -z "$staged" ]; then
    exit 0
fi

# ---------------------------------------------------------------------------------------
# 1. Contract files — owner approval, not a hook decision
# ---------------------------------------------------------------------------------------
# AGENTS.md section 5: the .proto files, the lint configuration, the determinism code, the
# CI definition and the licence files are contract files. The hook cannot approve them and
# does not block them; it makes sure nobody edits one by accident.

contract=$(printf '%s\n' "$staged" | grep -E \
    '^(proto/|clippy\.toml$|deny\.toml$|rust-toolchain\.toml$|\.cargo/|\.github/workflows/|xtask/src/|Cargo\.toml$|LICENSES?/|REUSE\.toml$|tests/golden/|AGENTS\.md$|CLAUDE\.md$|docs/design/)' \
    || true)
if [ -n "$contract" ]; then
    echo "pre-commit: this commit touches contract files —"
    printf '  %s\n' $contract
    echo "  These need owner approval (AGENTS.md section 5). Say in the PR what changed and why."
    echo
fi

# A golden file or a hash chain that moved needs an explanation, every time.
goldens=$(printf '%s\n' "$staged" | grep -E '^tests/golden/' || true)
if [ -n "$goldens" ]; then
    echo "pre-commit: golden files moved. The PR must say WHY they moved."
    echo "  Never re-bless a golden to turn a red build green (AGENTS.md section 4)."
    echo
fi

# ---------------------------------------------------------------------------------------
# 2. rustfmt on the staged Rust files
# ---------------------------------------------------------------------------------------

rust_files=$(printf '%s\n' "$staged" | grep -E '\.rs$' || true)

if [ -n "$rust_files" ]; then
    if ! command -v rustfmt >/dev/null 2>&1; then
        echo "pre-commit: rustfmt is not installed (rustup component add rustfmt)" >&2
        exit 1
    fi
    echo "pre-commit: rustfmt --check"
    # Checks the file in the working tree, which is what you are about to commit in the
    # normal case. A partially staged file (`git add -p`) is checked whole; that is the
    # one place the hook is stricter than the commit.
    # shellcheck disable=SC2086
    if ! rustfmt --edition 2024 --check --quiet $rust_files; then
        echo >&2
        echo "pre-commit: formatting differs. Run \`cargo fmt --all\` (or \`cargo xtask ci --fix\`)," >&2
        echo "            stage the result, and commit again." >&2
        exit 1
    fi
fi

# ---------------------------------------------------------------------------------------
# 3. clippy on the crates those files belong to
# ---------------------------------------------------------------------------------------
# The lint set lives in exactly one place — xtask — so the hook shells out to
# `cargo xtask clippy -p <crate>` rather than repeating the deny flags and drifting from
# what CI enforces.

packages=""
for file in $staged; do
    case "$file" in
        *.rs | */Cargo.toml | Cargo.toml) ;;
        *) continue ;;
    esac

    # Walk up to the nearest Cargo.toml and read its package name.
    dir=$(dirname "$file")
    manifest=""
    while [ "$dir" != "." ] && [ "$dir" != "/" ]; do
        if [ -f "$dir/Cargo.toml" ]; then
            manifest="$dir/Cargo.toml"
            break
        fi
        dir=$(dirname "$dir")
    done
    [ -n "$manifest" ] || continue

    name=$(awk '
        /^\[package\]/      { in_package = 1; next }
        /^\[/               { in_package = 0 }
        in_package && /^[[:space:]]*name[[:space:]]*=/ {
            sub(/^[^=]*=[[:space:]]*/, "")
            gsub(/["'"'"']/, "")
            print
            exit
        }' "$manifest")
    [ -n "$name" ] || continue

    case " $packages " in
        *" $name "*) ;;
        *) packages="$packages $name" ;;
    esac
done

if [ -n "$packages" ]; then
    if ! command -v cargo >/dev/null 2>&1; then
        echo "pre-commit: cargo is not installed; skipping clippy" >&2
        exit 0
    fi
    args=""
    for name in $packages; do
        args="$args -p $name"
    done
    echo "pre-commit: cargo xtask clippy$args"
    # shellcheck disable=SC2086
    if ! cargo xtask clippy $args; then
        echo >&2
        echo "pre-commit: clippy failed. Fix the lint — never silence a determinism lint" >&2
        echo "            to get past this hook (AGENTS.md section 4)." >&2
        exit 1
    fi
fi

# ---------------------------------------------------------------------------------------
# 4. Reminders the hook cannot check for you
# ---------------------------------------------------------------------------------------
# The DCO sign-off lives in the commit message, which a pre-commit hook has not been given
# yet. `git config format.signoff true` adds it to every commit in this repository.

if [ "$(git config --get format.signoff || echo false)" != "true" ]; then
    echo "pre-commit: reminder — commits need a DCO sign-off (\`git commit -s\`)."
    echo "            \`git config format.signoff true\` makes that automatic here."
fi

echo "pre-commit: ok — run \`cargo xtask ci\` before you open the PR."
exit 0
