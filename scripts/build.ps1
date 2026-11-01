param(
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not $SkipTests) {
    Write-Host "== cargo test =="
    cargo test
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }
}

Write-Host "== cargo build --release --locked =="
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw "release build failed" }

Write-Host "== package =="
cargo run --example pack
if ($LASTEXITCODE -ne 0) { throw "plugin packaging failed" }

$zip = Get-ChildItem (Join-Path $root 'target') -Filter 'island-voice-*.zip' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $zip) { throw "plugin zip not found" }
Write-Host ""
Write-Host "Plugin package: $($zip.FullName)"
Write-Host "Install it from WinIsland Settings -> Plugins -> Install from file."
