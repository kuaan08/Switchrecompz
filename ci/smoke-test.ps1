# Recompiles a tiny generated homebrew with TCC and with GCC, using only the
# tools in -Root (the repository after toolchains.ps1 / toolchains.sh, or the
# folder an unpacked single-file build produced), and runs both builds on the
# headless runtime. Fails unless each one exits cleanly. On Linux, Python and
# GCC come from the system, like for the release.
param([Parameter(Mandatory)][string]$Root)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path $Root).Path
if ($IsWindows) {
    $python = Join-Path $Root 'python/python.exe'
    $compilers = [ordered]@{ tcc = Join-Path $Root 'tcc/tcc.exe'; gcc = Join-Path $Root 'w64devkit/bin/gcc.exe' }
    $exe = 'hello.exe'
} else {
    $python = 'python3'
    $compilers = [ordered]@{ tcc = Join-Path $Root 'tcc/tcc'; gcc = 'gcc' }
    $exe = 'hello'
}
# Verify the Android entry point after payload unpacking using isolated mode;
# this catches missing sibling imports with the embedded Windows Python ._pth.
$exporter = Join-Path $Root 'android_build.py'
if (-not (Test-Path $exporter -PathType Leaf)) { throw 'Bundled android_build.py is missing' }
& $python -I -X utf8 $exporter --help | Out-Null
if ($LASTEXITCODE) { throw 'Android exporter failed to import with isolated Python' }

$work = Join-Path ([IO.Path]::GetTempPath()) "swr-smoke-$PID"
New-Item -ItemType Directory -Force $work | Out-Null
$nro = Join-Path $work 'hello.nro'
& $python -X utf8 (Join-Path $PSScriptRoot 'make_test_nro.py') $nro
if ($LASTEXITCODE) { throw 'make_test_nro.py failed' }

foreach ($name in $compilers.Keys) {
    $out = Join-Path $work $name
    & $python -X utf8 (Join-Path $Root 'switchrecomp.py') $nro -o hello --outdir $out --backend headless `
        --cc $compilers[$name] --jobs 4
    if ($LASTEXITCODE) { throw "recompiling with $name failed" }
    $p = Start-Process (Join-Path $out $exe) -WorkingDirectory $out -NoNewWindow -PassThru
    if (-not $p.WaitForExit(60000)) { $p.Kill(); throw "the $name build did not exit within 60 s" }
    $log = Get-Content (Join-Path $out 'hello.log') -Raw
    if ($p.ExitCode -ne 0 -or $log -notmatch 'The game exited \(code 0\)') {
        Write-Host $log
        throw "the $name build did not run cleanly (exit code $($p.ExitCode))"
    }
    Write-Host "$name build: recompiled and ran cleanly"
}
Remove-Item -Recurse -Force $work
