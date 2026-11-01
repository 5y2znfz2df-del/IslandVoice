param(
    [ValidateSet('en', 'zh')]
    [string]$Language = 'en'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) {
    $fallback = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
    if (Test-Path $fallback) { $cargo = $fallback }
}
if (-not $cargo) {
    throw "cargo not found. Add the Rust bin directory to PATH or set CARGO_HOME."
}

$installDir = Join-Path $env:APPDATA 'WinIsland\IslandVoice'
$lib = Join-Path $installDir 'vosk\libvosk.dll'
if ($Language -eq 'en') {
    $model = Join-Path $installDir 'models\vosk-model-small-en-us-0.15'
    $phrase = 'hey siri'
} else {
    $model = Join-Path $installDir 'models\vosk-model-small-cn-0.22'
    $phrase = [string]::Concat([char]0x4F60, [char]0x597D, [char]0x5C0F, [char]0x5C9B)
}
if (-not (Test-Path $lib)) { throw "Missing $lib; run scripts/fetch-vosk.ps1 first." }
if (-not (Test-Path $model)) { throw "Missing model $model; run scripts/fetch-vosk.ps1 -Language $Language first." }

$wav = Join-Path $env:TEMP 'islandvoice-test.wav'
Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
try {
    $voice = if ($Language -eq 'en') { 'Microsoft Zira Desktop' } else { 'Microsoft Huihui Desktop' }
    try { $synth.SelectVoice($voice) } catch { }
    $format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
        16000,
        [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
        [System.Speech.AudioFormat.AudioChannel]::Mono
    )
    $synth.SetOutputToWaveFile($wav, $format)
    $synth.Speak($phrase)
} finally {
    $synth.Dispose()
}

Write-Host "WAV: $wav"
Write-Host "Vosk: $lib"
Write-Host "Model: $model"
Write-Host ""
& $cargo run --example vosk-check -- $lib $model $wav
if ($LASTEXITCODE -ne 0) {
    throw "Vosk self-test failed with exit code $LASTEXITCODE"
}
