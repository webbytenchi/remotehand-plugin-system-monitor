$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$sourceExe = Join-Path $repo "target\release\remotehand-system-monitor.exe"
$stageDir = Join-Path $repo "bin\windows-x86_64"
$stageExe = Join-Path $stageDir "remotehand-system-monitor.exe"
$packageDir = Join-Path $repo "dist\remotehand-system-monitor-windows-x86_64"
$packageBinDir = Join-Path $packageDir "bin\windows-x86_64"
$packageUiDir = Join-Path $packageDir "ui"

Write-Host "Building RemoteHand System Monitor (release)..."
Push-Location $repo
try {
    cargo build --release
} finally {
    Pop-Location
}

if (-not (Test-Path $sourceExe)) {
    throw "Build completed but executable was not found: $sourceExe"
}

New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
Copy-Item -Force $sourceExe $stageExe

if (Test-Path $packageDir) {
    Remove-Item -Recurse -Force $packageDir
}
New-Item -ItemType Directory -Force -Path $packageBinDir | Out-Null
New-Item -ItemType Directory -Force -Path $packageUiDir | Out-Null

Copy-Item -Force (Join-Path $repo "plugin.json") (Join-Path $packageDir "plugin.json")
Copy-Item -Force (Join-Path $repo "ui\index.html") (Join-Path $packageUiDir "index.html")
Copy-Item -Force $sourceExe (Join-Path $packageBinDir "remotehand-system-monitor.exe")

$packageBytes = (Get-ChildItem -File -Recurse $packageDir | Measure-Object -Property Length -Sum).Sum
$packageMiB = [Math]::Round($packageBytes / 1MB, 2)

Write-Host ""
Write-Host "Plugin runtime staged successfully:"
Write-Host "  $stageExe"
Write-Host ""
Write-Host "Clean installable plugin package created:"
Write-Host "  $packageDir"
Write-Host "  Size: $packageMiB MiB"
