# Best-effort taskbar pin. Windows 10/11 blocks silent pinning, so failures are
# swallowed: the installer must never fail because of this.
param(
    [Parameter(Mandatory = $true)][string]$Target
)
$ErrorActionPreference = 'SilentlyContinue'
try {
    $shell = New-Object -ComObject Shell.Application
    $folder = $shell.Namespace((Split-Path $Target -Parent))
    $item = $folder.ParseName((Split-Path $Target -Leaf))
    $item.InvokeVerb('taskbarpin')
} catch { }
exit 0
