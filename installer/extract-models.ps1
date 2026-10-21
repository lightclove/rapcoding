# Extract downloaded Vosk model zips from a source directory into the models
# dir. Used by the installer after the built-in download page fetches the
# archives into %TEMP%\rapcoding-models.
#
# Idempotent: skips a model whose directory already exists and is non-empty.
param(
    [Parameter(Mandatory=$true)][string] $SrcDir,
    [Parameter(Mandatory=$true)][string] $Dest
)

$ErrorActionPreference = 'Stop'

$zips = Get-ChildItem -Path $SrcDir -Filter '*.zip' -File -ErrorAction SilentlyContinue
if (-not $zips) {
    Write-Host '[rapcoding] no model archives to extract.'
    exit 0
}

New-Item -ItemType Directory -Force -Path $Dest | Out-Null

foreach ($zip in $zips) {
    $modelDir = Join-Path $Dest $zip.BaseName
    $alreadyPresent = (Test-Path $modelDir) -and `
        (Get-ChildItem -Path $modelDir -Recurse -File -ErrorAction SilentlyContinue | Select-Object -First 1)
    if ($alreadyPresent) {
        Write-Host "[rapcoding] model '$($zip.BaseName)' already present, skipping"
        Remove-Item -Path $zip.FullName -Force
        continue
    }
    Write-Host "[rapcoding] extracting $($zip.Name) ..."
    Expand-Archive -Path $zip.FullName -DestinationPath $Dest -Force
    Remove-Item -Path $zip.FullName -Force
    if (-not (Test-Path $modelDir)) {
        throw "extraction did not produce $modelDir"
    }
}

Write-Host '[rapcoding] models ready.'
