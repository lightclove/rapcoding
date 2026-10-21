# Download and extract the Vosk speech models for rapcoding.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File download-models.ps1 [-Models <csv>] [-Dest <path>]
#
# -Models is a comma-separated list of keys:
#   ru-light, ru-heavy, en-light, en-heavy
#   aliases: ru (ru-light), en (en-light), both (ru-light,en-light), all (all four)
#
# Exits non-zero on any failure so the installer can report it loudly.
# Idempotent: skips a model whose directory already exists and is non-empty.

param(
    [string] $Models = 'ru-light,en-light',
    [string] $Dest = ''
)

$ErrorActionPreference = 'Stop'

if (-not $Dest) {
    $Dest = Join-Path $env:APPDATA 'rapcoding\models'
}

$modelDefs = @{
    'ru-light' = @{ url = 'https://alphacephei.com/vosk/models/vosk-model-small-ru-0.22.zip'; dir = 'vosk-model-small-ru-0.22' }
    'ru-heavy' = @{ url = 'https://alphacephei.com/vosk/models/vosk-model-ru-0.42.zip';        dir = 'vosk-model-ru-0.42' }
    'en-light' = @{ url = 'https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip'; dir = 'vosk-model-small-en-us-0.15' }
    'en-heavy' = @{ url = 'https://alphacephei.com/vosk/models/vosk-model-en-us-0.22.zip';      dir = 'vosk-model-en-us-0.22' }
}

$aliases = @{
    'ru'   = 'ru-light'
    'en'   = 'en-light'
    'both' = 'ru-light,en-light'
    'all'  = 'ru-light,en-light,ru-heavy,en-heavy'
}

function Write-Log {
    param([string] $Message)
    $line = "[rapcoding] $Message"
    Write-Host $line
    try { Add-Content -Path (Join-Path $Dest 'download.log') -Value $line } catch {}
}

# Parse -Models, expand aliases, validate, dedupe (keep order).
$selected = @()
foreach ($tok in ($Models -split ',')) {
    $key = $tok.Trim()
    if ($key -eq '') { continue }
    $expanded = if ($aliases.ContainsKey($key)) { $aliases[$key] } else { $key }
    foreach ($k in ($expanded -split ',')) {
        $k = $k.Trim()
        if ($k -eq '') { continue }
        if (-not $modelDefs.ContainsKey($k)) {
            throw "unknown model key '$k' (expected ru-light/ru-heavy/en-light/en-heavy)"
        }
        if ($selected -notcontains $k) { $selected += $k }
    }
}

if ($selected.Count -eq 0) {
    Write-Log 'no models selected, nothing to download.'
    exit 0
}

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
