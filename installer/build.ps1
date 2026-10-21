# Build both rapcoding binaries for the installer (console + GUI).
# Run from anywhere; locates the repo root via $PSScriptRoot.
# Exits non-zero on any build failure so the installer reports it loudly.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot   # repo root (installer/ is one level below)

Push-Location $root
try {
    Write-Host '[rapcoding] building console (headless)…'
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo build --release failed' }
    Copy-Item -Force `
        (Join-Path $root 'target\release\rapcoding.exe') `
        (Join-Path $root 'target\release\rapcoding-console.exe')

    Write-Host '[rapcoding] building GUI…'
    cargo build --release --features gui
    if ($LASTEXITCODE -ne 0) { throw 'cargo build --release --features gui failed' }
    Copy-Item -Force `
        (Join-Path $root 'target\release\rapcoding.exe') `
        (Join-Path $root 'target\release\rapcoding-gui.exe')
} finally {
    Pop-Location
}

Write-Host '[rapcoding] done: rapcoding-console.exe + rapcoding-gui.exe'
