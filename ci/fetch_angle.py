#!/usr/bin/env python3
"""Stage pinned ANGLE DLLs from an official Chromium snapshot, with notices."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
URL = 'https://storage.googleapis.com/chromium-browser-snapshots/Win_x64/1400034/chrome-win.zip'
ARCHIVE_SHA256 = '3c67b5ca7ca5bce53b9b9526173b1ff62f127cafcd16f9b2f5d53bf5d78341ac'
FILES = {
    'libEGL.dll': '85684a319fdbb967cf023b81ac30ec13c80ca35fa7b66e4b10b7eafe10dedc9a',
    'libGLESv2.dll': 'd11a13a8debe6d0c6f8873956bb516b4eb1c264fc80fe97b8902cf68bc211245',
    'vulkan-1.dll': '0499f6f32d660f198a9cb4723b80e1dd8cbd889a3fed5d8b8d7952bc049398fe',
    'vk_swiftshader.dll': '9374556828f0be51d85d0022bfb93034466acbc2f54e6ba4f1e2f18a9dbd4893',
    'vk_swiftshader_icd.json': '32d83ff113fef532a9f97e0d2831f8656628ab1c99e9060f0332b1532839afd9',
}


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def require_exports(path, required):
    data=path.read_bytes()
    pe=struct.unpack_from('<I',data,0x3c)[0]
    count=struct.unpack_from('<H',data,pe+6)[0]
    opt=struct.unpack_from('<H',data,pe+20)[0]
    def offset(rva):
        for index in range(count):
            at=pe+24+opt+index*40
            size,va,raw_size,raw=struct.unpack_from('<IIII',data,at+8)
            if va<=rva<va+max(size,raw_size):
                return raw+rva-va
        raise ValueError('Invalid PE export address')
    export_rva=struct.unpack_from('<I',data,pe+24+112)[0]
    if not export_rva:
        raise ValueError(path.name+' has no API exports; not a standalone ANGLE library')
    directory=offset(export_rva)
    names_count=struct.unpack_from('<I',data,directory+24)[0]
    names=offset(struct.unpack_from('<I',data,directory+32)[0])
    exported=set()
    for index in range(names_count):
        at=offset(struct.unpack_from('<I',data,names+index*4)[0])
        exported.add(data[at:data.index(0,at)].decode('ascii'))
    if not required.issubset(exported):
        raise ValueError('Missing graphics API exports: '+', '.join(sorted(required-exported)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path)
    args = parser.parse_args()
    cache = ROOT / '.ci-cache/angle-1400034'
    cache.mkdir(parents=True, exist_ok=True)
    if not all((cache / n).is_file() and digest(cache / n) == sha for n, sha in FILES.items()):
        with tempfile.TemporaryDirectory() as tmp:
            archive = args.archive or Path(tmp) / 'angle.zip'
            if not args.archive:
                print('Downloading pinned Chromium ANGLE snapshot...', flush=True)
                with urllib.request.urlopen(URL, timeout=90) as response, archive.open('wb') as f:
                    shutil.copyfileobj(response, f, 1024 * 1024)
            if digest(archive) != ARCHIVE_SHA256:
                raise SystemExit('ANGLE archive checksum mismatch')
            with zipfile.ZipFile(archive) as z:
                for name, sha in FILES.items():
                    data = z.read('chrome-win/' + name)
                    if hashlib.sha256(data).hexdigest() != sha:
                        raise SystemExit('ANGLE component checksum mismatch: ' + name)
                    (cache / name).write_bytes(data)
    out = ROOT / 'tools/angle'
    require_exports(cache/'libEGL.dll',{'eglGetProcAddress','eglInitialize','eglChooseConfig'})
    require_exports(cache/'libGLESv2.dll',{'glGetString','glCreateShader'})
    require_exports(cache/'vk_swiftshader.dll',{'vkGetInstanceProcAddr'})
    out.mkdir(parents=True, exist_ok=True)
    for name in FILES:
        shutil.copyfile(cache / name, out / name)
    for license_file in (ROOT / 'ci/licenses').glob('ANGLE-*-LICENSE.txt'):
        shutil.copyfile(license_file, out / license_file.name)
    (out / 'PROVENANCE.json').write_text(json.dumps({
        'source': URL, 'sha256': ARCHIVE_SHA256,
        'angle_commit': '711b2c32641521c9e60a903b85c7367755c6448a',
        'swiftshader_commit': '118f577b4188528566a1f23d88c4f0041372b83a',
        'files': FILES,
        'notice': 'ANGLE and its dependencies retain their original licenses. WARP is supplied by Windows.'
    }, indent=2), encoding='utf-8')
    print('Pinned Vulkan/DirectX ANGLE libraries staged.', flush=True)


if __name__ == '__main__':
    main()
