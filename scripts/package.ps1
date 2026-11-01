param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
cargo run --example pack
if ($LASTEXITCODE -ne 0) { throw "plugin packaging failed" }
Get-ChildItem (Join-Path $root 'target') -Filter 'island-voice-*.zip' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1 |
    ForEach-Object { Write-Host "Plugin package: $($_.FullName)" }
