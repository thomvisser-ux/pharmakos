# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The whole in-engine measurement set, in one deterministic order, so a re-run
# after a code change reproduces every row of the plan's §10 table.
#
#   * headline runs: 3 reps x path A and 3 reps x path B at 3,600 frames
#     (plan §3 step 3: one minute at 60 fps) with the chosen K/B;
#   * exploratory sweeps: K and B at 900 frames, which is all a sweep needs;
#   * stress: 40 explosions/s and radius 8, at 900 frames (plan §3 step 6);
#   * two vista screenshots, one per path, for the geometry check and the A-vs-B
#     pixel diff.
#
# vsync is off and delta smoothing is off (project.godot), so `ext_dt_us` — the
# extension's own monotonic delta — is the frame-time series. `--shot=0` means
# "no screenshot": a screenshot costs a frame and must not land inside a
# measured series.

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$out = Join-Path $here "results"
New-Item -ItemType Directory -Force $out | Out-Null

# PowerShell variable names are CASE-INSENSITIVE: a `foreach ($k in ...)` loop
# and a `$K` constant are the same variable, and the loop silently leaves the
# constant holding its last iteration value. That cost one whole measurement
# set — the B sweep ran at K = unbounded instead of K = 4 and the stress runs at
# B = 1 MiB instead of 512 KiB, and nothing said so except the CSV header the
# analysis prints. Hence the distinct names, and hence `Run` echoing the
# effective K/B it was actually given.
$CHOSEN_K = 4
$CHOSEN_B = 524288
$LONG = 3600
$SWEEP = 900

function Run($tag, $argv) {
    Write-Host "== $tag ==  $($argv -join ' ')"
    $a = @("--path", "$here/godot", "--rendering-driver", "vulkan", "--resolution", "1920x1080", "--") + $argv + @("--out=$out", "--tag=$tag")
    & godot_console @a 2>&1 | Select-String -Pattern "^\[g1\]|ERROR|SCRIPT" | ForEach-Object { $_.Line }
}

# -- headline: 3 reps of each path at 3,600 frames --------------------------
foreach ($rep in 1..3) {
    Run "final_a_rep$rep" @("--path=a", "--frames=$LONG", "--eps=20", "--radius=4", "--k=$CHOSEN_K", "--b=$CHOSEN_B", "--shot=0")
    Run "final_b_rep$rep" @("--path=b", "--frames=$LONG", "--eps=20", "--radius=4", "--k=$CHOSEN_K", "--b=$CHOSEN_B", "--shot=0")
}

# -- K sweep, both paths, 900 frames (B unbounded, so K is what binds) ------
foreach ($sweepK in 1, 2, 4, 8, 0) {
    Run "k_a_$sweepK" @("--path=a", "--frames=$SWEEP", "--eps=20", "--radius=4", "--k=$sweepK", "--b=0", "--shot=0")
    Run "k_b_$sweepK" @("--path=b", "--frames=$SWEEP", "--eps=20", "--radius=4", "--k=$sweepK", "--b=0", "--shot=0")
}

# -- B sweep on path B at the chosen K, 900 frames --------------------------
foreach ($sweepB in 131072, 262144, 524288, 1048576) {
    Run "b_b$sweepB" @("--path=b", "--frames=$SWEEP", "--eps=20", "--radius=4", "--k=$CHOSEN_K", "--b=$sweepB", "--shot=0")
}

# -- stress beyond the gate --------------------------------------------------
Run "stress_eps40" @("--path=b", "--frames=$SWEEP", "--eps=40", "--radius=4", "--k=$CHOSEN_K", "--b=$CHOSEN_B", "--shot=0")
Run "stress_r8" @("--path=b", "--frames=$SWEEP", "--eps=20", "--radius=8", "--k=$CHOSEN_K", "--b=$CHOSEN_B", "--shot=0")

# -- vistas: identical run on both paths, screenshot at frame 880 -----------
# The screenshot costs one ~65 ms frame (image read-back + PNG encode), which is
# why no measured series is allowed to contain one.
Run "vistaA" @("--path=a", "--frames=$SWEEP", "--eps=20", "--radius=4", "--k=$CHOSEN_K", "--b=$CHOSEN_B", "--shot=880")
Run "vistaB" @("--path=b", "--frames=$SWEEP", "--eps=20", "--radius=4", "--k=$CHOSEN_K", "--b=$CHOSEN_B", "--shot=880")

Write-Host "`nall runs complete -> $out"
