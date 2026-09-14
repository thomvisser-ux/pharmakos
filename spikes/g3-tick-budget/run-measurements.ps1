# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The measurement procedure of docs/spikes/G3prime-segment-budget.md section 3,
# driven from OUTSIDE the process, because three of the plan's hygiene items
# cannot be honoured from inside a dependency-free benchmark:
#
#   * the core pin (plan section 3 step 1) is set on the PROCESS here, with
#     `Process.ProcessorAffinity`, not only on the worker thread from inside;
#   * the CPU's clock is read before and after every run
#     (`Get-CimInstance Win32_Processor`, plus the `% Processor Performance`
#     counter for the pinned core, which is the one with signal on this box --
#     `CurrentClockSpeed` reports the nominal 3792 MHz whatever the core is
#     doing);
#   * peak WORKING SET is polled from outside while the process runs, the way
#     the G2 spike did it (G2-P3-pathing.md section 9.9 item 1): the harness
#     reports peak live heap from its allocator wrapper, because true RSS needs
#     `GetProcessMemoryInfo` and therefore a `windows-sys` dependency the spike
#     may not take. `PeakWorkingSet64` reads 0 after exit, so it has to be
#     sampled live.
#
# Run from this directory, after:
#
#     $env:CARGO_TARGET_DIR = 'D:/build/g3-tickbudget'
#     cargo build --release           --locked --bins
#     cargo build --profile release-unchecked --locked --bins
#
# Everything lands in results/: the JSON the plan's section 9 quotes, the
# per-tick hash streams, one console log per run, and results/hygiene.json with
# the clocks, the affinity and the working set of every run. All of it is
# git-ignored (spikes/README.md, "Build outputs are git-ignored"): read it, put
# the summary in the plan's Results section, and let it be deleted.
#
# Nothing else heavy should be running. Windows Defender real-time scanning is
# NOT excluded for this directory, and the write-up says so rather than
# claiming otherwise.

$ErrorActionPreference = 'Stop'
Set-Location -Path $PSScriptRoot

$BinDir = $env:G3_BIN_DIR
if (-not $BinDir) { $BinDir = 'D:/build/g3-tickbudget' }
$B = Join-Path $BinDir 'release/tickbench.exe'
$U = Join-Path $BinDir 'release-unchecked/tickbench.exe'
$C = Join-Path $BinDir 'release/costmodel.exe'

New-Item -ItemType Directory -Force -Path 'results' | Out-Null

# Core 2, as G2 used. 1 shl 2 = 0x4.
$PinCore = 2
$PinMask = [IntPtr](1 -shl $PinCore)
$PerfCounter = "\Processor Information(0,$PinCore)\% Processor Performance"

$Hygiene = New-Object System.Collections.ArrayList

function Get-ClockSample {
    $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
    $pct = $null
    try {
        $pct = (Get-Counter $PerfCounter -ErrorAction Stop).CounterSamples[0].CookedValue
    } catch { $pct = $null }
    $eff = $null
    $pctRounded = $null
    if ($null -ne $pct) {
        $pctRounded = [math]::Round($pct, 1)
        $eff = [math]::Round($cpu.MaxClockSpeed * $pct / 100.0, 0)
    }
    return [pscustomobject]@{
        max_clock_mhz      = $cpu.MaxClockSpeed
        current_clock_mhz  = $cpu.CurrentClockSpeed
        core_perf_percent  = $pctRounded
        core_effective_mhz = $eff
        load_percent       = $cpu.LoadPercentage
    }
}

function Warm-Core {
    # Machine hygiene, and it is not optional on this box.
    #
    # Windows ramps the clock PER CORE, and `tickbench` calibrates
    # `synthetic_work` in the first ~120 ms of the process -- that is, on a core
    # that has just been idle at ~1.2 GHz. A cold calibration reports too many
    # picoseconds per iteration, so G2's measured milliseconds get converted into
    # too FEW iterations, and the charged pathing half then runs faster than G2
    # measured.
    #
    # So spin the pinned core to full boost before every launch. Plan section 3
    # step 1 -- "Machine hygiene, before any number is taken" -- is exactly this,
    # and doing it from outside leaves the measured binary untouched.
    #
    # WHAT IT DOES AND DOES NOT FIX, measured rather than assumed. The
    # calibration is largely self-warming already: it doubles the iteration count
    # until one ramp lasts 20 ms and takes the median of five, so the later ramps
    # run on a boosted core anyway. Cold and pre-warmed both report 1664-1665
    # ps/iter on this machine. What the pre-warm removes is the clock ramp inside
    # the first seconds of the 2 000-tick warm-up, not a bias in the constant.
    # The residual is real and is reported rather than smoothed away: the charged
    # loop executes within about +/-3% of what `iters x ps_per_iter` predicts, so
    # the pathing phase's `real_ms` -- the measured phase mean minus the charged
    # figure -- is a small difference of two large numbers and can come out
    # slightly negative. The plan's section 9 says so out loud.
    $w = Start-Process -FilePath 'powershell.exe' -PassThru -WindowStyle Hidden -ArgumentList @(
        '-NoProfile', '-Command',
        '$s=[System.Diagnostics.Stopwatch]::StartNew(); $z=0; while($s.ElapsedMilliseconds -lt 6000){ $z = ($z + 1) % 1000003 }')
    try { $w.ProcessorAffinity = $PinMask } catch { }
    $w.WaitForExit()
}

function Invoke-Run {
    param(
        [string]   $Label,
        [string]   $Exe,
        [string[]] $Arguments,
        [switch]   $NoSampler
    )
    $log = "results/$Label.log"
    $err = "results/$Label.err"
    $hyg = "results/hygiene-$Label.json"
    # Resume: a run whose hygiene sidecar exists is already done. The sidecar is
    # written per run rather than assembled at the end so that a crash in run N
    # does not cost runs 1..N-1 their clock readings.
    if (Test-Path $hyg) {
        Write-Host ("== {0} == (already done, skipped)" -f $Label)
        [void]$Hygiene.Add((Get-Content $hyg -Raw | ConvertFrom-Json))
        return
    }
    Write-Host ("== {0} ==" -f $Label)

    Warm-Core
    $before = Get-ClockSample
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $Exe -ArgumentList $Arguments -PassThru -NoNewWindow `
                       -RedirectStandardOutput $log -RedirectStandardError $err
    # Pin the PROCESS from outside. Set as early as the API allows; the worker
    # thread also pins itself to the same core from inside (--pin-core), so the
    # two agree and the few milliseconds before this line are inside the 2 000
    # tick warm-up, not inside the measured segment.
    try { $p.ProcessorAffinity = $PinMask } catch { Write-Host "  affinity set FAILED: $_" }
    $observedAffinity = $null
    try { $observedAffinity = '0x{0:X}' -f [int64]$p.ProcessorAffinity } catch { }

    $peak = 0
    $perfSamples = New-Object System.Collections.ArrayList
    if (-not $NoSampler) {
        while (-not $p.HasExited) {
            $p.Refresh()
            try { if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 } } catch { }
            if ($perfSamples.Count -lt 400 -and ($perfSamples.Count * 40) -lt $sw.ElapsedMilliseconds) {
                try {
                    $v = (Get-Counter $PerfCounter -ErrorAction Stop).CounterSamples[0].CookedValue
                    [void]$perfSamples.Add([math]::Round($v, 1))
                } catch { }
            }
            Start-Sleep -Milliseconds 40
        }
    }
    $p.WaitForExit()
    $sw.Stop()
    $after = Get-ClockSample

    $duringMean = $null
    $duringMin  = $null
    $duringMax  = $null
    if ($perfSamples.Count -gt 0) {
        $duringMean = [math]::Round(($perfSamples | Measure-Object -Average).Average, 1)
        $duringMin  = ($perfSamples | Measure-Object -Minimum).Minimum
        $duringMax  = ($perfSamples | Measure-Object -Maximum).Maximum
    }

    $entry = [pscustomobject]@{
        label                     = $Label
        exe                       = $Exe
        args                      = ($Arguments -join ' ')
        exit_code                 = $p.ExitCode
        wall_seconds              = [math]::Round($sw.Elapsed.TotalSeconds, 1)
        affinity_requested        = '0x{0:X}' -f [int64]$PinMask
        affinity_observed         = $observedAffinity
        clock_before              = $before
        clock_after               = $after
        core_perf_percent_during  = [pscustomobject]@{ mean = $duringMean; min = $duringMin; max = $duringMax; n = $perfSamples.Count }
        peak_working_set_bytes    = $peak
        peak_working_set_kib      = [math]::Round($peak / 1024.0, 0)
        memory_sampler            = (-not $NoSampler.IsPresent)
        prewarm_seconds           = 6
    }

    $errText = ''
    if (Test-Path $err) {
        $raw = Get-Content $err -Raw
        if ($null -ne $raw) { $errText = [string]$raw }
    }
    if ($errText.Trim().Length -gt 0) {
        Write-Host $errText
        throw "$Label wrote to stderr"
    }
    $entry | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 $hyg
    [void]$Hygiene.Add($entry)
    Write-Host ("   {0}s, peak WS {1} KiB, core perf {2}% -> {3}%" -f `
        [math]::Round($sw.Elapsed.TotalSeconds,1), [math]::Round($peak/1024.0,0), `
        $before.core_perf_percent, $after.core_perf_percent)
}

$GATE = @('--seats','4','--units','300','--beacons','40','--ticks','9600','--warmup','2000','--pin-core',"$PinCore")

# 1  the gate configuration itself; three repetitions, median of the p99s.
Invoke-Run -Label 'tick' -Exe $B -Arguments ($GATE + @(
    '--edits-per-s','20','--repeat','3','--hash','full',
    '--json','results/tick.json','--hashes','results/hashes.txt'))

# 1b a single gate run WITHOUT the memory sampler, so the sampler's 40 ms poll
#    can be shown to be invisible in the distribution (G2 section 9.9 item 9).
Invoke-Run -Label 'tick-nosampler' -Exe $B -NoSampler -Arguments ($GATE + @(
    '--edits-per-s','20','--repeat','1','--hash','full',
    '--json','results/tick-nosampler.json'))

# 2  the same in incremental hash mode -- the hashing decision (plan section 8 item 3).
Invoke-Run -Label 'tick-incremental' -Exe $B -Arguments ($GATE + @(
    '--edits-per-s','20','--repeat','1','--hash','incremental',
    '--json','results/tick-incremental.json','--hashes','results/hashes-incremental.txt'))

# 3  the per-tick repath cap at 16 -- the lever, and the recommended operating
#    point, so it gets three repetitions too.
Invoke-Run -Label 'tick-cap16' -Exe $B -Arguments ($GATE + @(
    '--edits-per-s','20','--repath-cap','16','--repeat','3','--hash','full',
    '--json','results/tick-cap16.json','--hashes','results/hashes-cap16.txt'))

# 4  destruction off -- the only configuration in which the sim's own cost is visible.
Invoke-Run -Label 'tick-noedits' -Exe $B -Arguments ($GATE + @(
    '--edits-per-s','0','--repeat','3','--hash','full',
    '--json','results/tick-noedits.json'))

# 5  overflow checks off at the gate -- DILUTED by the calibrated pathing charge.
Invoke-Run -Label 'tick-unchecked' -Exe $U -Arguments ($GATE + @(
    '--edits-per-s','20','--repeat','1','--hash','full',
    '--json','results/tick-unchecked.json','--hashes','results/hashes-unchecked.txt'))

# 6  destruction off, overflow checks off -- the pair that CAN see the delta.
Invoke-Run -Label 'tick-noedits-unchecked' -Exe $U -Arguments ($GATE + @(
    '--edits-per-s','0','--repeat','3','--hash','full',
    '--json','results/tick-noedits-unchecked.json'))

# 7-9 the repath burst frequency, the unmeasured PLACEHOLDER constant that the
#     G3'-a verdict at an unbounded cap is a direct readout of.
Invoke-Run -Label 'tick-burst32' -Exe $B -Arguments ($GATE + @(
    '--edits-per-s','20','--burst-one-in','32','--repeat','1','--hash','full',
    '--json','results/tick-burst32.json'))
Invoke-Run -Label 'tick-burst128' -Exe $B -Arguments ($GATE + @(
    '--edits-per-s','20','--burst-one-in','128','--repeat','1','--hash','full',
    '--json','results/tick-burst128.json'))
Invoke-Run -Label 'tick-burstoff' -Exe $B -Arguments ($GATE + @(
    '--edits-per-s','20','--burst-one-in','off','--repeat','1','--hash','full',
    '--json','results/tick-burstoff.json'))

# 10 the sweep, the cost-model fits, the repath-cap sweep, the per-seat reading
#    and the power-budget arithmetic.
Invoke-Run -Label 'cost' -Exe $C -Arguments @(
    '--seats','4','--ticks-per-point','2400','--warmup','2000','--repeat','3',
    '--pin-core',"$PinCore",'--json','results/cost.json')

$Hygiene | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 'results/hygiene.json'
Write-Host 'ALL DONE'
