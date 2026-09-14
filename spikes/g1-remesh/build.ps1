# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds both halves of the spike and stages the cdylib where Godot can see it.
#
#   * the gdext cdylib, with the default `gdext` feature;
#   * `meshbench` with --no-default-features, so the honest CPU number is taken
#     by a binary that does not link gdext at all.
#
# THE TWO BUILDS MUST NOT SHARE A TARGET DIRECTORY. `--no-default-features`
# rebuilds the *lib* target too, and the lib target is the cdylib: a plain
#
#     cargo build --release ; cargo build --release --no-default-features --bin meshbench
#
# leaves `release/g1_remesh.dll` overwritten by a build with no gdext in it, and
# Godot then fails with
#
#     Can't resolve symbol gdext_rust_init, error 127
#
# which looks like a toolchain/ABI problem and is not one. The bench build
# therefore gets its own target directory, and the staging step asserts the
# entry point is present before declaring success.

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

Push-Location $here
try {
    cargo build --release
    if (-not $?) { throw "cdylib build failed" }

    $target = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
    $bin = Join-Path $here "godot/bin"
    New-Item -ItemType Directory -Force $bin | Out-Null

    $dll = Join-Path $target "release/g1_remesh.dll"
    $so = Join-Path $target "release/libg1_remesh.so"
    if (Test-Path $dll) {
        # `dumpbin` only exists inside a Visual Studio developer shell, and this
        # script has to work in a plain one. Fall back to a byte scan of the
        # image for the export name, which is enough to catch the one mistake
        # this check exists for: staging a --no-default-features build, whose
        # entry point Godot then reports as "Can't resolve symbol
        # gdext_rust_init, error 127".
        $syms = $null
        if (Get-Command dumpbin -ErrorAction SilentlyContinue) {
            $syms = (& dumpbin /exports $dll) -join "`n"
        }
        else {
            $raw = [System.IO.File]::ReadAllBytes($dll)
            $syms = [System.Text.Encoding]::ASCII.GetString($raw)
        }
        if (-not ($syms -match "gdext_rust_init")) {
            throw "staged cdylib has no gdext_rust_init - was it built --no-default-features?"
        }
        Copy-Item $dll (Join-Path $bin "g1_remesh.dll") -Force
    }
    if (Test-Path $so) { Copy-Item $so (Join-Path $bin "libg1_remesh.so") -Force }

    # Separate target dir: see the header.
    cargo build --release --no-default-features --bin meshbench --target-dir (Join-Path $here "target/bench")
    if (-not $?) { throw "meshbench build failed" }

    Write-Host "staged extension into $bin"
    Write-Host "meshbench at $(Join-Path $here 'target/bench/release/meshbench.exe')"
}
finally { Pop-Location }
