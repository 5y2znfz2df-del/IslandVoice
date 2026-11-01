param(
    [ValidateSet('en', 'zh', 'both')]
    [string]$Language = 'en',
    [string]$InstallDir = (Join-Path $env:APPDATA 'WinIsland\IslandVoice'),
    [string]$VoskVersion = '0.3.45',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$runtimeDir = Join-Path $InstallDir 'vosk'
$modelsDir = Join-Path $InstallDir 'models'
$tempDir = Join-Path $env:TEMP 'islandvoice-vosk'
New-Item -ItemType Directory -Force $runtimeDir, $modelsDir, $tempDir | Out-Null

function Download-File {
    param([string]$Url, [string]$OutFile)
    if ((Test-Path $OutFile) -and -not $Force) {
        if ((Get-Item $OutFile).Length -gt 0) {
            Write-Host "  exists: $OutFile"
            return
        }
        Remove-Item $OutFile -Force
    }
    Write-Host "  download: $Url"
    Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing -TimeoutSec 600
}

function Install-Runtime {
    $zip = Join-Path $tempDir "vosk-win64-$VoskVersion.zip"
    $extract = Join-Path $tempDir 'runtime'
    if (Test-Path $extract) { Remove-Item $extract -Recurse -Force }
    $downloaded = $false
    try {
        Download-File "https://github.com/alphacep/vosk-api/releases/download/v$VoskVersion/vosk-win64-$VoskVersion.zip" $zip
        $downloaded = $true
    } catch {
        Write-Warning "GitHub runtime download failed: $($_.Exception.Message)"
    }

    if (-not $downloaded) {
        if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
            throw "Cannot download the runtime and Python is not installed. Install Vosk manually (see docs/INSTALL.md)."
        }
        Write-Host "  trying PyPI wheel..."
        $wheelDir = Join-Path $tempDir 'wheel'
        New-Item -ItemType Directory -Force $wheelDir | Out-Null
        python -m pip download "vosk==$VoskVersion" --platform win_amd64 --only-binary=:all: --no-deps -d $wheelDir
        $wheel = Get-ChildItem $wheelDir -Filter 'vosk-*.whl' | Select-Object -First 1
        if (-not $wheel) { throw "PyPI download failed: no vosk wheel found" }
        $zip = Join-Path $tempDir 'vosk-wheel.zip'
        Copy-Item $wheel.FullName $zip -Force
    }

    Expand-Archive -Path $zip -DestinationPath $extract -Force
    $lib = Get-ChildItem $extract -Recurse -Filter 'libvosk.dll' | Select-Object -First 1
    if (-not $lib) { throw "libvosk.dll not found in the archive" }
    Get-ChildItem $lib.DirectoryName -Filter '*.dll' | ForEach-Object {
        Copy-Item $_.FullName (Join-Path $runtimeDir $_.Name) -Force
    }
    Write-Host "  runtime -> $runtimeDir"
}

function Install-Model {
    param([string]$Lang)
    $spec = @{
        en = @{
            Name = 'vosk-model-small-en-us-0.15'
            Url  = 'https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip'
            Alt  = 'https://hf-mirror.com/ambind/vosk-model-small-en-us-0.15/resolve/main/vosk-model-small-en-us-0.15_c_.zip'
        }
        zh = @{
            Name = 'vosk-model-small-cn-0.22'
            Url  = 'https://alphacephei.com/vosk/models/vosk-model-small-cn-0.22.zip'
            Alt  = ''
        }
    }[$Lang]
    $target = Join-Path $modelsDir $spec.Name
    if ((Test-Path $target) -and -not $Force) {
        Write-Host "  model exists: $target"
        return
    }
    $zip = Join-Path $tempDir "$($spec.Name).zip"
    $extract = Join-Path $tempDir "model-$Lang"
    if (Test-Path $extract) { Remove-Item $extract -Recurse -Force }
    try {
        Download-File $spec.Url $zip
    } catch {
        if (-not $spec.Alt) { throw }
        Write-Warning "Primary model source failed, trying mirror: $($_.Exception.Message)"
        Download-File $spec.Alt $zip
    }
    Expand-Archive -Path $zip -DestinationPath $extract -Force
    $final = Get-ChildItem $extract -Recurse -Filter 'final.mdl' | Where-Object { $_.Directory.Name -eq 'am' } | Select-Object -First 1
    if (-not $final) { throw "Invalid model archive: am\final.mdl not found" }
    $modelRoot = $final.Directory.Parent.FullName
    if (Test-Path $target) { Remove-Item $target -Recurse -Force }
    Move-Item $modelRoot $target
    Write-Host "  model -> $target"
}

Write-Host "IslandVoice Vosk installer"
Write-Host "  install dir: $InstallDir"
Write-Host "[1/2] runtime"
Install-Runtime
Write-Host "[2/2] models"
$langs = switch ($Language) {
    'both' { @('en', 'zh') }
    default { @($Language) }
}
foreach ($lang in $langs) { Install-Model $lang }

Write-Host ""
Write-Host "Done. Next steps:"
Write-Host "  1) run scripts/verify-vosk.ps1 -Language $Language"
Write-Host "  2) install target/island-voice-*.zip in WinIsland"
Write-Host "  3) fill the DeepSeek API key in the IslandVoice settings page"
