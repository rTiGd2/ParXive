Param(
    [switch]$LinuxToo = $true
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-WSLPath([string]$winPath) {
    if (-not $winPath) { throw 'winPath is required' }
    $drive = $winPath.Substring(0,1).ToLower()
    $rest = $winPath.Substring(2).Replace('\', '/')
    return "/mnt/$drive$rest"
}

# Resolve repo root from script location
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..') | Select-Object -ExpandProperty Path
Write-Host "Repo: $repoRoot"

Push-Location $repoRoot
try {
    cargo fmt --all
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

    if ($LinuxToo) {
        $wslRoot = Get-WSLPath $repoRoot
        Write-Host "Running in WSL at $wslRoot"
        wsl -e bash -lc "set -euo pipefail; cd '$wslRoot'; cargo fmt --all; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace"
    }
}
finally {
    Pop-Location
}


