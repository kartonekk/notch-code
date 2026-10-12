# Sets up the build environment for Notch Code (MSVC x64 + Ninja + Vulkan SDK + libclang).
# Usage:  . .\scripts\dev-env.ps1   (dot-source it, then run npm run tauri build / cargo build)

# Pick up PATH changes from installers without restarting the shell
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User') +
  ";${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer;$env:USERPROFILE\.cargo\bin"

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Visual Studio Build Tools with C++ tools not found (winget install Microsoft.VisualStudio.2022.BuildTools).' }
$before = @{}
Get-ChildItem env: | ForEach-Object { $before[$_.Name] = $_.Value }
& "$vs\Common7\Tools\Launch-VsDevShell.ps1" -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null

# The dev shell round-trips the environment through cmd.exe, which mangles
# non-ASCII characters (e.g. "Kołodziej" in the user folder). Repair it.
$bad = [char]0xFFFD
foreach ($name in @($before.Keys)) {
  $now = [Environment]::GetEnvironmentVariable($name, 'Process')
  if ($name -ne 'Path' -and $now -and $now.Contains($bad)) {
    [Environment]::SetEnvironmentVariable($name, $before[$name], 'Process')
  }
}
$paths = @($env:Path -split ';' | Where-Object { $_ -and -not $_.Contains($bad) })
foreach ($p in ($before['Path'] -split ';')) { if ($p -and $paths -notcontains $p) { $paths += $p } }
$env:Path = $paths -join ';'

# whisper.cpp's Vulkan shader generator nests deep inside the build tree and
# overflows the 260-char path limit, so build with Ninja in a short target dir.
$env:CMAKE_GENERATOR = 'Ninja'
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = (Split-Path -Qualifier $PSScriptRoot) + '\tkb' }

if (-not $env:VULKAN_SDK) { $env:VULKAN_SDK = [Environment]::GetEnvironmentVariable('VULKAN_SDK', 'Machine') }
if (-not $env:VULKAN_SDK) { throw 'Vulkan SDK not found (winget install KhronosGroup.VulkanSDK).' }

if (-not $env:LIBCLANG_PATH) { $env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin' }

Write-Host "Environment ready: MSVC ($vs), Ninja, Vulkan SDK ($env:VULKAN_SDK)"
