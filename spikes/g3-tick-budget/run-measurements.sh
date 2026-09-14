#!/bin/sh
# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The ten runs behind the plan's Results section, in the order they were taken,
# so the numbers can be reproduced rather than believed. Run from this
# directory, after
#
#     cargo build --release --locked --bins
#     cargo build --profile release-unchecked --locked --bins
#
# Plan §3 step 1: nothing else heavy should be running, and the process is
# pinned to a core (`--pin-core 2`). This is the POSIX twin of
# `run-measurements.ps1`, which is what the plan's §9 numbers were taken with:
# on Windows the PowerShell driver additionally pins the *process* from outside,
# reads the CPU's clock before and after every run, and polls peak working set
# while the process is alive -- three hygiene items a dependency-free benchmark
# cannot do for itself. This script does the runs and nothing else, so on a
# machine where those three matter, use the PowerShell driver.
#
# (`ci/spike-g3.yml` writes into out/ instead; that is the runner's directory and
# is uploaded as the workflow artefact. Local runs land in results/.) Every run writes its JSON into results/ and its
# console output into results/; both are git-ignored, so read them, put the
# summary into docs/spikes/G3prime-segment-budget.md §9, and let them be
# deleted (spikes/README.md, "Build outputs are git-ignored").
#
# Why each run exists:
#   1  the gate configuration itself, three repetitions, median of the p99s
#   2  the same in incremental hash mode — the hashing decision (plan §8 item 3)
#   3  the same with the per-tick repath cap at 16 — the lever, and the
#      configuration the spike RECOMMENDS, so it gets three repetitions too
#   4  the same with the destruction stream off — the sim without G2's load, and
#      the only configuration in which the sim's own cost is visible
#   5  the same in the release-unchecked profile — the cost of overflow checks,
#      DILUTED: at 20 edits/s ~98% of the tick is a calibrated busy loop whose
#      duration is pinned to G2's milliseconds, so this pair cannot show what
#      the checks cost the sim's own integer code
#   6  destruction off, release-unchecked — the pair that CAN show it: compared
#      against run 4, this is ~2/3 real sim work
#   7-9 the repath burst frequency swept over {32, 128, off} against run 1's 64.
#      That constant is a PLACEHOLDER nobody has measured, and at the unbounded
#      cap the G3'-a verdict is a direct readout of it, so the plan's §9
#      publishes the p99 at every rate next to the verdict
#   10 the sweep, the cost-model fits and the power-budget arithmetic
set -e
cd "$(dirname "$0")"
BIN_DIR="${G3_BIN_DIR:-D:/build/g3-tickbudget}"
B="$BIN_DIR/release/tickbench.exe"
U="$BIN_DIR/release-unchecked/tickbench.exe"
C="$BIN_DIR/release/costmodel.exe"
mkdir -p results

GATE="--seats 4 --units 300 --beacons 40 --ticks 9600 --warmup 2000"

echo "== 1 gate, full hash, repeat 3 =="
"$B" $GATE --edits-per-s 20 --repeat 3 --hash full --pin-core 2 \
     --json results/tick.json --hashes results/hashes.txt > results/tick.log 2>&1

echo "== 2 gate, incremental hash =="
"$B" $GATE --edits-per-s 20 --repeat 1 --hash incremental --pin-core 2 \
     --json results/tick-incremental.json --hashes results/hashes-incremental.txt \
     > results/tick-incremental.log 2>&1

echo "== 3 gate, repath cap 16, repeat 3 (the recommended operating point) =="
"$B" $GATE --edits-per-s 20 --repath-cap 16 --repeat 3 --hash full --pin-core 2 \
     --json results/tick-cap16.json --hashes results/hashes-cap16.txt > results/tick-cap16.log 2>&1

echo "== 4 gate, no destruction =="
"$B" $GATE --edits-per-s 0 --repeat 3 --hash full --pin-core 2 \
     --json results/tick-noedits.json > results/tick-noedits.log 2>&1

echo "== 5 gate, overflow-checks OFF (diluted by the synthetic charge) =="
"$U" $GATE --edits-per-s 20 --repeat 1 --hash full --pin-core 2 \
     --json results/tick-unchecked.json --hashes results/hashes-unchecked.txt \
     > results/tick-unchecked.log 2>&1

echo "== 6 no destruction, overflow-checks OFF (the pair that can see it) =="
"$U" $GATE --edits-per-s 0 --repeat 3 --hash full --pin-core 2 \
     --json results/tick-noedits-unchecked.json > results/tick-noedits-unchecked.log 2>&1

echo "== 7 burst 1-in-32 =="
"$B" $GATE --edits-per-s 20 --burst-one-in 32 --repeat 1 --hash full --pin-core 2 \
     --json results/tick-burst32.json > results/tick-burst32.log 2>&1

echo "== 8 burst 1-in-128 =="
"$B" $GATE --edits-per-s 20 --burst-one-in 128 --repeat 1 --hash full --pin-core 2 \
     --json results/tick-burst128.json > results/tick-burst128.log 2>&1

echo "== 9 burst off =="
"$B" $GATE --edits-per-s 20 --burst-one-in off --repeat 1 --hash full --pin-core 2 \
     --json results/tick-burstoff.json > results/tick-burstoff.log 2>&1

echo "== 10 costmodel =="
"$C" --seats 4 --ticks-per-point 2400 --warmup 2000 --repeat 3 --pin-core 2 \
     --json results/cost.json > results/cost.log 2>&1

echo "ALL DONE"
