# Builds Notch Code and swaps it in for the copy you use: stops the running
# app, replaces %LOCALAPPDATA%\Programs\Notch Code\NotchCode.exe and starts it.
# The app re-points its "Start with Windows" task at that exe on launch.
# Usage:  powershell -ExecutionPolicy Bypass -File .\app\scripts\deploy.ps1 [-SkipBuild]

param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'

$app = Split-Path -Parent $PSScriptRoot
$built = Join-Path (Split-Path -Parent $app) 'release\NotchCode.exe'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\Notch Code'
$installed = Join-Path $installDir 'NotchCode.exe'

if (-not $SkipBuild) {
  if (-not (Test-Path (Join-Path $app 'node_modules'))) {
    Push-Location $app
    try {
      npm ci
      if ($LASTEXITCODE -ne 0) { throw "npm ci failed ($LASTEXITCODE)" }
    } finally {
      Pop-Location
    }
  }
  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'build-release.ps1')
  if ($LASTEXITCODE -ne 0) { throw "build failed ($LASTEXITCODE)" }
}
if (-not (Test-Path $built)) { throw "$built not found - run without -SkipBuild first" }

# Stop the running copy, wherever it was started from (a running recording is lost).
Get-Process -Name NotchCode, notch-code -ErrorAction SilentlyContinue | ForEach-Object {
  Write-Host "Stopping $($_.Path) (pid $($_.Id))"
  Stop-Process -Id $_.Id -Force
  $_.WaitForExit(10000) | Out-Null
}

New-Item -ItemType Directory -Force $installDir | Out-Null
Copy-Item $built $installed -Force
Start-Process $installed
Write-Host "Started $installed"
