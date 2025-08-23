Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-WSLPath([string]$winPath) {
    if (-not $winPath) { throw 'winPath is required' }
    $drive = $winPath.Substring(0,1).ToLower()
    $rest = $winPath.Substring(2).Replace('\', '/')
    return "/mnt/$drive$rest"
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..') | Select-Object -ExpandProperty Path
Push-Location $repoRoot
try {
    cargo test --workspace -- --nocapture
    $wslRoot = Get-WSLPath $repoRoot
    wsl -e bash -lc "set -euo pipefail; cd '$wslRoot'; cargo test --workspace -- --nocapture"
}
finally {
    Pop-Location
}


