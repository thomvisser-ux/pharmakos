#!/usr/bin/env sh
# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Step 9 of the G4 measurement procedure: feature isolation.
#
# Toy equivalent of the CI dependency check that graduates into
# `cargo xtask ci` at harness part 1. It asserts three things:
#
#   1. `cargo build --release` with no features produces no `fork` symbol.
#      Symbols are read from the rlib, not the exe: an MSVC link puts symbols
#      in the PDB, so grepping the .exe proves nothing either way. The build
#      uses `-C symbol-mangling-version=v0`, whose names embed the module path
#      (`..14g4_determinism4fork4fork`), so a grep cannot be fooled by a
#      coincidental substring.
#   2. The same build *with* `--features research` does contain it — otherwise
#      check 1 is vacuous and would pass on a typo.
#   3. Nothing resolves the `research` feature in the default graph, and no
#      crate on the deny-list resolves it in either graph. The deny-list here
#      stands in for plan-core, verifier, operator and gateway. This check has
#      its own positive control (3a) for the same reason check 2 exists.
#
# Exits non-zero on the first failure.

set -eu

cd "$(dirname "$0")"

# Stand-ins for the four crates that must never reach `research`.
DENY_LIST="imbl rkyv postcard serde xxhash-rust"

DEFAULT_DIR=target/isolate-default
RESEARCH_DIR=target/isolate-research

echo "== 1. default release build, v0 symbol mangling"
RUSTFLAGS="-C symbol-mangling-version=v0" cargo build --release --target-dir "$DEFAULT_DIR" >/dev/null
echo "== 2. research release build, v0 symbol mangling"
RUSTFLAGS="-C symbol-mangling-version=v0" cargo build --release --features research --target-dir "$RESEARCH_DIR" >/dev/null

rlib_of() {
    ls "$1"/release/deps/libg4_determinism-*.rlib 2>/dev/null | head -1
}

# Count symbol names mentioning the `fork` module. `dumpbin /symbols` is the
# authority on Windows; `nm` elsewhere. A plain binary grep is the fallback and
# is sufficient because v0 names are plain ASCII in the object file.
fork_symbols() {
    rlib="$1"
    if command -v dumpbin >/dev/null 2>&1; then
        # dumpbin is a native Windows tool: hand it a Windows path and keep the
        # shell from rewriting the /symbols switch into a path.
        win="$rlib"
        command -v cygpath >/dev/null 2>&1 && win="$(cygpath -w "$rlib")"
        MSYS2_ARG_CONV_EXCL='*' dumpbin /symbols "$win" 2>/dev/null \
            | grep -c '14g4_determinism4fork' || true
    elif command -v nm >/dev/null 2>&1; then
        nm "$rlib" 2>/dev/null | grep -c '14g4_determinism4fork' || true
    else
        # v0 mangled names are plain ASCII inside the object files, so a binary
        # grep is a sound fallback when no symbol dumper is on PATH.
        grep -ac '14g4_determinism4fork' "$rlib" || true
    fi
}

DEF_RLIB="$(rlib_of "$DEFAULT_DIR")"
RES_RLIB="$(rlib_of "$RESEARCH_DIR")"
[ -n "$DEF_RLIB" ] || { echo "FAIL: no default rlib built"; exit 1; }
[ -n "$RES_RLIB" ] || { echo "FAIL: no research rlib built"; exit 1; }

DEF_N="$(fork_symbols "$DEF_RLIB")"
RES_N="$(fork_symbols "$RES_RLIB")"
echo "   default  rlib: $DEF_N fork symbols"
echo "   research rlib: $RES_N fork symbols"

if [ "$DEF_N" -ne 0 ]; then
    echo "FAIL: the default build contains fork symbols"
    exit 1
fi
if [ "$RES_N" -eq 0 ]; then
    echo "FAIL: the research build contains no fork symbols — this check is vacuous"
    exit 1
fi

# The default build must not even produce the forkcheck binary.
if [ -e "$DEFAULT_DIR/release/forkcheck.exe" ] || [ -e "$DEFAULT_DIR/release/forkcheck" ]; then
    echo "FAIL: the default build produced the forkcheck binary"
    exit 1
fi
echo "   default build produced no forkcheck binary"

echo "== 3. deny-list: no crate below may resolve the research feature"
#
# `cargo tree -e features` prints dependency feature EDGES only — it never prints
# the root package's own enabled features — so grepping its output for
# "research" produces byte-identical output whether or not the feature is on, and
# the check cannot fail. `-f '{p} [{f}]'` prints the RESOLVED feature set of
# every package, root included:
#
#     g4-determinism v0.1.0 (…) [default]            <- default build
#     g4-determinism v0.1.0 (…) [default,research]   <- --features research
#
# so the grep below has something real to match. Checks 1 and 2 are guarded
# against vacuity by construction (2 is the positive control for 1); 3 gets the
# same treatment in 3a.
TREE_DEFAULT="$(cargo tree -f '{p} [{f}]' 2>/dev/null)"
TREE_RESEARCH="$(cargo tree -f '{p} [{f}]' --features research 2>/dev/null)"

# The resolved feature list of every line mentioning $1 as a package name.
features_of() {
    printf '%s\n' "$2" | sed -n "s/.*${1} v[0-9][^]]*\[\(.*\)\]\$/\1/p"
}

# POSIX `case` rather than a word-boundary regex: `research` must be a whole
# element of the comma-separated list, not a substring of `research-extra`.
lists_research() {
    while IFS= read -r line; do
        case ",$line," in
            *,research,*) return 0 ;;
            *) ;;
        esac
    done
    return 1
}

STATUS=0

# 3a. Positive control. If the research build does NOT show the feature on the
#     root line, the extraction is broken and 3b below proves nothing.
if features_of "g4-determinism" "$TREE_RESEARCH" | lists_research; then
    echo "   positive control: --features research resolves \"research\" on the root"
else
    echo "FAIL: --features research does not resolve \"research\" — check 3 is vacuous"
    exit 1
fi

# 3b. The root package must not resolve it in the default graph.
if features_of "g4-determinism" "$TREE_DEFAULT" | lists_research; then
    echo "FAIL: the default build resolves feature \"research\""
    STATUS=1
else
    echo "   ok: g4-determinism (root, default graph)"
fi

# 3c. Nor may any crate on the deny-list, in either graph. These five stand in
#     for plan-core, verifier, operator and gateway. None of them currently
#     *has* a feature named research, so this loop is a guard against a future
#     dependency that does, not evidence about today's graph — which is why 3a
#     and 3b carry the weight.
for pkg in $DENY_LIST; do
    if features_of "$pkg" "$TREE_RESEARCH" | lists_research; then
        echo "FAIL: $pkg resolves feature \"research\""
        STATUS=1
    else
        echo "   ok: $pkg"
    fi
done
[ "$STATUS" -eq 0 ] || exit "$STATUS"

echo "PASS: feature isolation holds"
