# Download and extract the Vosk speech models for rapcoding.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File download-models.ps1 [-Models ru|en|both] [-Dest <path>]
#
# Exits non-zero on any failure so the installer can report it loudly.
# Idempotent: skips a model whose directory already exists and is non-empty.

param(
    [ValidateSet('ru', 'en', 'both')]
    [string] $Models = 'both',
    [string] $Dest = ''
)

$ErrorActionPreference = 'Stop'

if (-not $Dest) {
    $Dest = Join-Path $env:APPDATA 'rapcoding\models'
}

# NOTE: not named `$models` — PowerShell variables are case-insensitive, so it
# would collide with the `$Models` parameter above.
$modelDefs = @{
    ru = @{
        url = 'https://alphacephei.com/vosk/models/vosk-model-small-ru-0.22.zip'
        dir = 'vosk-model-small-ru-0.22'
    }
    en = @{
        url = 'https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip'
        dir = 'vosk-model-small-en-us-0.15'
    }
}

function Write-Log {
    param([string] $Message)
    $line = "[rapcoding] $Message"
    Write-Host $line
    try { Add-Content -Path (Join-Path $Dest 'download.log') -Value $line } catch {}
}

$selected = if ($Models -eq 'both') { @('ru', 'en') } else { @($Models) }

New-Item -ItemType Directory -Force -Path $Dest | Out-Null

foreach ($id in $selected) {
    $m = $modelDefs[$id]
    $zip = Join-Path $Dest ($m.dir + '.zip')
    $modelDir = Join-Path $Dest $m.dir

    $alreadyPresent = (Test-Path $modelDir) -and `
        (Get-ChildItem -Path $modelDir -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 1)
    if ($alreadyPresent) {
        Write-Log "model '$($m.dir)' already present, skipping"
        continue
    }

    Write-Log "downloading $($m.url) ..."
    & curl.exe -L --retry 3 --retry-delay 2 --fail --silent --show-error -o $zip $m.url
    if ($LASTEXITCODE -ne 0) {
        throw "curl failed (exit code $LASTEXITCODE) for $($m.url)"
    }

    Write-Log "extracting $($m.dir) ..."
    Expand-Archive -Path $zip -DestinationPath $Dest -Force
    Remove-Item -Path $zip -Force

    if (-not (Test-Path $modelDir)) {
        throw "extraction did not produce $modelDir"
    }
    if (-not (Get-ChildItem -Path $modelDir -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 1)) {
        throw "model dir $modelDir is empty"
    }
}

Write-Log 'models ready.'
