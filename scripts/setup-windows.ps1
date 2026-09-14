# Pharmakos — Windows developer toolchain setup (run in an elevated PowerShell).
# Idempotent: winget skips packages that are already installed; the VS step uses "modify",
# which only adds what is missing.
# Needs about 8 GB free on C: for the tools (MSVC C++ workload ~6 GB, rustup ~1.5 GB,
# Godot ~0.2 GB) plus a Rust target/ directory that grows to several GB once the
# workspace builds.
#
# Verified 2026-09-13: Godot 4.7 stable shipped 2026-06-18 (winget has 4.7.2);
# godot-rust gdext supports the 4.7 API level. Package ids checked with winget show.

$ErrorActionPreference = 'Continue'
$failures = @()

function Step([string]$Name, [scriptblock]$Body) {
  Write-Host "`n== $Name" -ForegroundColor Cyan
  try { & $Body } catch { Write-Warning "$Name failed: $_"; $script:failures += $Name }
}

function Install-Pkg([string]$Id) {
  & winget install --id $Id --exact --silent --accept-package-agreements --accept-source-agreements
  # -1978335189 = "already installed", -1978335135 = "no applicable upgrade"
  if ($LASTEXITCODE -notin @(0, -1978335189, -1978335135)) { throw "winget exit code $LASTEXITCODE" }
}

# 1. MSVC linker + Windows SDK for the x86_64-pc-windows-msvc Rust target.
#    Build Tools 2022 is installed without the C++ workload, so this is a MODIFY of the
#    existing instance, not an install (winget's install path exits 1 with
#    "already installed" and adds nothing).
Step 'MSVC C++ workload (Desktop development with C++)' {
  $vsInstaller = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vs_installer.exe'
  $vswhere     = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
  if (-not (Test-Path $vsInstaller)) {
    Write-Host 'No VS installer found; installing Build Tools 2022 with the C++ workload via winget'
    & winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --silent --accept-package-agreements --accept-source-agreements `
      --override '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
    if ($LASTEXITCODE -ne 0) { throw "winget exit code $LASTEXITCODE" }
    return
  }
  $installPath = & $vswhere -products Microsoft.VisualStudio.Product.BuildTools -property installationPath
  if (-not $installPath) { $installPath = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools' }
  Write-Host "Modifying $installPath"
  $p = Start-Process -FilePath $vsInstaller -Wait -PassThru -ArgumentList @(
    'modify', '--installPath', "`"$installPath`"",
    '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended',
    '--quiet', '--norestart')
  # 0 = ok, 3010 = ok but reboot required, 1641 = reboot initiated
  if ($p.ExitCode -notin @(0, 3010, 1641)) { throw "vs_installer modify exit code $($p.ExitCode) (log: $env:TEMP\dd_installer_*.log)" }
  if ($p.ExitCode -in @(3010, 1641)) { Write-Warning 'Windows wants a reboot before the C++ tools are usable.' }
}

# 2. Rust (stable, MSVC). rust-toolchain.toml in the repo pins the exact channel.
Step 'rustup' { Install-Pkg 'Rustlang.Rustup' }

# 3. Protobuf compiler and Buf (schema lint + breaking checks in cargo xtask ci).
Step 'protoc' { Install-Pkg 'Google.Protobuf' }
Step 'buf'    { Install-Pkg 'bufbuild.buf' }

# 4. Godot 4.7 (standard build; the Mono build is not needed for gdext).
Step 'Godot 4.7' { Install-Pkg 'GodotEngine.GodotEngine' }

# 5. Cargo helpers used by cargo xtask ci. cargo is not on this shell's PATH until it is reopened.
Step 'cargo-deny + cargo-nextest' {
  $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
  if (-not (Test-Path $cargo)) { throw 'cargo.exe not found yet; open a new shell and run: cargo install cargo-deny cargo-nextest --locked' }
  & $cargo install cargo-deny cargo-nextest --locked
  if ($LASTEXITCODE -ne 0) { throw "cargo install exit code $LASTEXITCODE" }
}

Write-Host ''
if ($failures.Count) {
  Write-Warning ("Steps that failed: " + ($failures -join ', ') + ". Fix and re-run; completed steps are skipped.")
  exit 1
}
Write-Host 'Done. Open a NEW shell, then verify:  rustc -V; cargo -V; protoc --version; buf --version; godot --version' -ForegroundColor Green
