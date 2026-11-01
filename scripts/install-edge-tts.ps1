# Install the Edge neural voice backend used by IslandVoice.
# The plugin calls `python -m edge_tts`; if this is missing it falls back to
# the offline Windows SAPI voice automatically.

$ErrorActionPreference = "Stop"
$python = Get-Command python -ErrorAction SilentlyContinue
if (-not $python) {
    Write-Host "Python 3 was not found. Install it from https://www.python.org/downloads/ and tick 'Add python.exe to PATH'."
    exit 1
}

& $python.Source -m pip install --upgrade edge-tts
& $python.Source -m edge_tts --list-voices | Select-String "zh-CN-XiaoxiaoNeural"
Write-Host "Edge TTS is ready: $($python.Source)"
