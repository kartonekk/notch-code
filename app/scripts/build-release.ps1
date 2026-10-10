# Builds the standalone transkrypt.exe and copies it to <repo>\release.
# Usage:  powershell -ExecutionPolicy Bypass -File .\app\scripts\build-release.ps1

. "$PSScriptRoot\dev-env.ps1"

$app = Split-Path -Parent $PSScriptRoot
$out = Join-Path (Split-Path -Parent $app) 'release'

Push-Location $app
try {
  # --no-bundle: just the exe, no installer. Everything else (the speech model)
  # is downloaded by the app itself on first launch, shown as progress in the notch.
  npm run tauri build -- --no-bundle
  if ($LASTEXITCODE -ne 0) { throw "tauri build failed ($LASTEXITCODE)" }
} finally {
  Pop-Location
}

New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item "$env:CARGO_TARGET_DIR\release\transkrypt.exe" (Join-Path $out 'Transkrypt.exe') -Force
Get-ChildItem $out | ForEach-Object { '{0}  {1:N1} MB' -f $_.FullName, ($_.Length / 1MB) }
