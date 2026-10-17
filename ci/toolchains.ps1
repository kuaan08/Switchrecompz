# Downloads the tools the release carries (pinned versions, checked against
# SHA-256) into .ci-cache\ and unpacks them into the repository root, laid out
# like the release folder:
#   python\     embeddable Python, runs switchrecomp.py
#   tcc\        Tiny C Compiler, quick builds
#   w64devkit\  MinGW-w64 GCC, fast builds and the runtime itself
# GCC is trimmed of what a C/C++ build never uses (32-bit libraries, Fortran,
# CMake, gdb, vim), which roughly halves its size.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # the progress bar makes Invoke-WebRequest very slow
Set-Location (Split-Path $PSScriptRoot)

$tools = @(
    @{
        Dir    = 'python'
        Url    = 'https://www.python.org/ftp/python/3.14.8/python-3.14.8-embed-amd64.zip'
        Sha256 = 'a93abe456ab01bd96d7a085b3cdb6566b3063f4241360d114142fbdb07f0a310'
    }
    @{
        Dir    = 'tcc'
        Url    = 'https://download.savannah.gnu.org/releases/tinycc/tcc-0.9.27-win64-bin.zip'
        Sha256 = '34a721949a2583fdff725312da092fa0f5f1f284b702e6f811c6954714faabb2'
    }
    @{
        Dir    = 'w64devkit'
        Url    = 'https://github.com/skeeto/w64devkit/releases/download/v2.10.0/w64devkit-x64-2.10.0.7z.exe'
        Sha256 = '18d0a4c71a166f8401ab6305781bec5882b40b5e06ba9807c61cb5f3b3c6325e'
    }
)

New-Item -ItemType Directory -Force .ci-cache | Out-Null
foreach ($t in $tools) {
    $file = Join-Path .ci-cache (Split-Path $t.Url -Leaf)
    if (-not (Test-Path $file)) {
        Write-Host "Downloading $($t.Url)"
        Invoke-WebRequest $t.Url -OutFile $file
    }
    $hash = (Get-FileHash $file -Algorithm SHA256).Hash
    if ($hash -ne $t.Sha256) {
        Remove-Item $file
        throw "$file has SHA-256 $hash, expected $($t.Sha256)"
    }
    if (Test-Path $t.Dir) { Remove-Item -Recurse -Force $t.Dir }
    $t.File = (Resolve-Path $file).Path
}

# python: a flat zip
Expand-Archive $tools[0].File -DestinationPath python

# tcc: a zip with a tcc\ folder inside; it has no license file, so add the LGPL text
Expand-Archive $tools[1].File -DestinationPath .
Copy-Item ci/licenses/tcc-COPYING.txt tcc/COPYING.txt

# w64devkit: a 7-Zip self-extracting archive with a w64devkit\ folder inside
if (Get-Command 7z -ErrorAction SilentlyContinue) {
    7z x -y "-o$PWD" $tools[2].File | Out-Null
    if ($LASTEXITCODE) { throw '7z could not unpack w64devkit' }
} else {
    $p = Start-Process $tools[2].File -ArgumentList "-o`"$PWD`"", '-y' -Wait -PassThru
    if ($p.ExitCode) { throw 'w64devkit could not unpack itself' }
}

$w = 'w64devkit'
$gccLib = (Get-ChildItem "$w/lib/gcc/x86_64-w64-mingw32" -Directory | Select-Object -First 1).FullName
$gccExec = (Get-ChildItem "$w/libexec/gcc/x86_64-w64-mingw32" -Directory | Select-Object -First 1).FullName
$unused = '^(i686-.*|.*gfortran.*|c?cmake\.exe|cpack\.exe|ctest\.exe|cmcldeps\.exe|dcmake\.exe|gdb(server)?\.exe|makensis\.exe|g?vim\.bat|vimtutor\.bat|ctags\.exe)$'
$drop = @("$w/lib32", "$w/share", "$gccLib/32", "$gccExec/f951.exe")
$drop += Get-ChildItem "$w/bin" -File | Where-Object Name -Match $unused | ForEach-Object FullName
foreach ($path in $drop) { if (Test-Path $path) { Remove-Item -Recurse -Force $path } }
# Fortran runtime libraries left in the 64-bit library folders
Get-ChildItem $w -Recurse -File | Where-Object Name -Match '(gfortran|^libcaf_)' | Remove-Item -Force

& python/python.exe --version
& tcc/tcc.exe -v
& w64devkit/bin/gcc.exe --version | Select-Object -First 1
