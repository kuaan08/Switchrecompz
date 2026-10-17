#!/usr/bin/env python3
"""Stage pinned ANGLE DLLs from an official Chromium snapshot, with notices."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
URL = 'https://storage.googleapis.com/chromium-browser-snapshots/Win_x64/1721443/chrome-win.zip'
ARCHIVE_SHA256 = '57db60c2c41f5166384543ef3556fc7765e8ffde925c98c4d2b538f93f1550cc'
FILES = {
    'libEGL.dll': '526cfc55ba4fd28d19b0e0654a816b6a02be3f5ce00f4387c993e73cc69fdf48',
    'libGLESv2.dll': 'f64a29c5057e5fd997c7afa4ca28f1afb05ccc9e776983f7b4b1c4bce1ac8777',
    'vulkan-1.dll': 'edd2ff435a8f342c320f96aaf191144761c904f52169bc06cd6308b75c46c9a3',
    'vk_swiftshader.dll': 'd7d124bd6af72f3e7cdf8538be5bd09b4fc76b5b62413936c17232b6d450dada',
    'vk_swiftshader_icd.json': '32d83ff113fef532a9f97e0d2831f8656628ab1c99e9060f0332b1532839afd9',
}


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path)
    args = parser.parse_args()
    cache = ROOT / '.ci-cache/angle-1721443'
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
    out.mkdir(parents=True, exist_ok=True)
    for name in FILES:
        shutil.copyfile(cache / name, out / name)
    for license_file in (ROOT / 'ci/licenses').glob('ANGLE-*-LICENSE.txt'):
        shutil.copyfile(license_file, out / license_file.name)
    (out / 'PROVENANCE.json').write_text(json.dumps({
        'source': URL, 'sha256': ARCHIVE_SHA256,
        'angle_commit': '7865acacbf6380ce2acc54f5afd86a242cd239eb',
        'swiftshader_commit': 'cddb5c91b23ff70a1f607ea31610aa0d7f4b5ad5',
        'files': FILES,
        'notice': 'ANGLE and its dependencies retain their original licenses. WARP is supplied by Windows.'
    }, indent=2), encoding='utf-8')
    print('Pinned Vulkan/DirectX ANGLE libraries staged.', flush=True)


if __name__ == '__main__':
    main()
