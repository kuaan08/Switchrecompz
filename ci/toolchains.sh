#!/usr/bin/env bash
# The Linux counterpart of toolchains.ps1: downloads pinned sources (checked
# against SHA-256) into .ci-cache/ and builds what the Linux release carries:
#   tcc/                 Tiny C Compiler 0.9.27, quick builds
#   runtime/oh264/lin/   OpenH264 2.4.1 decoder, game videos
# Python and GCC come from the system. Needs curl, gcc, make, and nasm for
# OpenH264's assembly (without it the decoder is built in plain C, slower).
set -euo pipefail
cd "$(dirname "$0")/.."

TCC_URL=https://download.savannah.gnu.org/releases/tinycc/tcc-0.9.27.tar.bz2
TCC_SHA=de23af78fca90ce32dff2dd45b3432b2334740bb9bb7b05bf60fdbfc396ceb9c
OH_URL=https://github.com/cisco/openh264/archive/refs/tags/v2.4.1.tar.gz
OH_SHA=8ffbe944e74043d0d3fb53d4a2a14c94de71f58dbea6a06d0dc92369542958ea

fetch() { # url sha256 file
    local file=.ci-cache/$3
    mkdir -p .ci-cache
    if [ ! -f "$file" ]; then
        echo "Downloading $1"
        curl -fsSL -o "$file.part" "$1"
        mv "$file.part" "$file"
    fi
    if ! echo "$2  $file" | sha256sum -c --quiet -; then
        rm -f "$file"
        echo "$file does not match its SHA-256" >&2
        exit 1
    fi
}
fetch "$TCC_URL" "$TCC_SHA" tcc-0.9.27.tar.bz2
fetch "$OH_URL" "$OH_SHA" openh264-2.4.1.tar.gz

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
jobs=$(nproc)

# tcc: the bundle keeps include/ and libtcc1.a next to the binary (switchrecomp.py passes -B).
# The search paths cover Debian/Ubuntu, Fedora and Arch (SteamOS) layouts. bcheck.c, the bounds
# checker, no longer builds against glibc 2.34+ and games never use it.
tar -xjf .ci-cache/tcc-0.9.27.tar.bz2 -C "$work"
(
    cd "$work/tcc-0.9.27"
    ./configure --prefix="$work/tcc-install" \
        --sysincludepaths='{B}/include:/usr/local/include:/usr/include/x86_64-linux-gnu:/usr/include' \
        --libpaths='{B}:/usr/lib/x86_64-linux-gnu:/usr/lib64:/usr/lib:/lib/x86_64-linux-gnu:/lib64:/lib' \
        --crtprefix='/usr/lib/x86_64-linux-gnu:/usr/lib64:/usr/lib' >/dev/null
    make -j"$jobs" BCHECK_O= >/dev/null
    make install BCHECK_O= >/dev/null
)
rm -rf tcc
mkdir -p tcc
cp "$work/tcc-install/bin/tcc" tcc/
cp -r "$work/tcc-install/lib/tcc/include" tcc/
cp "$work/tcc-install/lib/tcc/libtcc1.a" tcc/
cp ci/licenses/tcc-COPYING.txt tcc/COPYING.txt

# OpenH264: only the two decoder libraries the runtime links
tar -xzf .ci-cache/openh264-2.4.1.tar.gz -C "$work"
asm=Yes
command -v nasm >/dev/null || { asm=No; echo "nasm not found: OpenH264 is built without its assembly"; }
make -C "$work/openh264-2.4.1" -j"$jobs" OS=linux ARCH=x86_64 USE_ASM=$asm libdecoder.a libcommon.a >/dev/null
mkdir -p runtime/oh264/lin
cp "$work/openh264-2.4.1/libdecoder.a" "$work/openh264-2.4.1/libcommon.a" runtime/oh264/lin/

python3 --version
tcc/tcc -v
gcc --version | head -n 1
