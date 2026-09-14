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
#   3. No crate on the deny-list resolves the `research` feature. The deny-list
#      here stands in for plan-core, verifier, operator and gateway.
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
TREE="$(cargo tree -e features 2>/dev/null)"
STATUS=0
for pkg in $DENY_LIST; do
    if printf '%s\n' "$TREE" | grep -E "^[^A-Za-z]*${pkg} feature \"research\"" >/dev/null 2>&1; then
        echo "FAIL: $pkg resolves feature \"research\""
        STATUS=1
    else
        echo "   ok: $pkg"
    fi
done
if printf '%s\n' "$TREE" | grep -i research >/dev/null 2>&1; then
    echo "FAIL: the default feature graph mentions research at all"
    STATUS=1
fi
[ "$STATUS" -eq 0 ] || exit "$STATUS"

echo "PASS: feature isolation holds"
