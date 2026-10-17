# Copies what the single-file build carries into -Out: the recompiler scripts,
# the runtime sources and builds, the bundled compilers and the license files.
#   Windows: Python, TCC and GCC (w64devkit)
#   Linux:   TCC; Python and GCC come from the system
# Build the app from it with: $env:SWR_PAYLOAD_DIR = <Out>; cargo build --release
param([Parameter(Mandatory)][string]$Out)

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)
if ($IsWindows) {
    $tools = 'python', 'tcc', 'w64devkit'
    $runtimes = 'bin/runtime_win64_win32.exe', 'bin/runtime_win64_headless.exe'
    $fetch = 'ci/toolchains.ps1'
} else {
    $tools = , 'tcc'
    $runtimes = 'bin/runtime_linux_x11', 'bin/runtime_linux_headless'
    $fetch = 'ci/toolchains.sh'
}
$tools += , 'tools'
$hactool = if ($IsWindows) { 'tools/hactool/hactool.exe' } else { 'tools/hactool/hactool' }
if (-not (Test-Path $hactool)) { throw 'Build the bundled hactool before staging the app' }
foreach ($dir in $tools) {
    if (-not (Test-Path $dir)) { throw "$dir/ is missing: run $fetch first" }
}
foreach ($rt in $runtimes) {
    if (-not (Test-Path $rt)) { throw "$rt is missing: build it with switchrecomp.py --build-runtime" }
}
if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item 'switchrecomp.py', 'android_build.py', 'android_tools.py', 'update_check.py', 'package_import.py', 'nxloader.py', 'codegen.py', 'a64.py', 'LICENSE.txt', 'NOTICE.txt', 'README.md', 'PACKAGE_IMPORT.md', 'CODE_REVIEW.md', 'PLATFORM_SUPPORT.md', 'ANDROID.md' $Out
foreach ($dir in @('runtime', 'bin') + $tools) {
    Copy-Item $dir $Out -Recurse
}
# Android's native runtime needs the Khronos GL header. Fetch its pinned,
# SHA-256-verified version at release-build time, not on the user's first export.
if ($IsWindows) {
    & python -c 'import android_build; android_build.graphics_headers()'
    if ($LASTEXITCODE) { throw 'Could not stage the Android graphics headers' }
    $headerRoot = '.ci-cache/graphics-headers/include'
    if (-not (Test-Path "$headerRoot/GL/glcorearb.h" -PathType Leaf)) {
        throw 'Verified Android graphics header is missing'
    }
    $dest = Join-Path $Out '.ci-cache/graphics-headers'
    New-Item -ItemType Directory -Force $dest | Out-Null
    Copy-Item $headerRoot $dest -Recurse
}
Get-ChildItem $Out -Recurse -Directory -Filter __pycache__ | Remove-Item -Recurse -Force
$size = (Get-ChildItem $Out -Recurse -File | Measure-Object Length -Sum).Sum / 1MB
Write-Host ("Staged {0:N0} MB in {1}" -f $size, (Resolve-Path $Out))
