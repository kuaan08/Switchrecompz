"""Install a pinned Linux SDK/NDK for CI without relying on sdkmanager in PATH."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import os
from pathlib import Path
import shutil
import stat
import tempfile
import urllib.request
import zipfile

PACKAGES = [
    ('android-ndk-r27c-linux.zip', '59c2f6dc96743b5daf5d1626684640b20a6bd2b1d85b13156b90333741bad5cc', 'ndk'),
    ('platform-35_r02.zip', '0988cacad01b38a18a47bac14a0695f246bc76c1b06c0eeb8eb0dc825ab0c8e0', 'platforms'),
    ('build-tools_r35_linux.zip', 'bd3a4966912eb8b30ed0d00b0cda6b6543b949d5ffe00bea54c04c81e1561d88', 'build-tools'),
]


def install(root, entry):
    name, expected, directory = entry
    stamp = root / (name + '.sha256')
    if stamp.is_file() and stamp.read_text() == expected:
        return
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / name
        with urllib.request.urlopen('https://dl.google.com/android/repository/' + name, timeout=90) as response, path.open('wb') as f:
            shutil.copyfileobj(response, f, 1024 * 1024)
        h = hashlib.sha256()
        with path.open('rb') as f:
            for block in iter(lambda: f.read(1024 * 1024), b''):
                h.update(block)
        if h.hexdigest() != expected:
            raise ValueError('Android tool archive checksum mismatch: ' + name)
        dest = root / directory
        dest.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(path) as z:
            for info in z.infolist():
                target = dest / info.filename
                if not target.resolve().is_relative_to(dest.resolve()):
                    raise ValueError('Invalid SDK archive path')
                mode = info.external_attr >> 16
                if stat.S_ISLNK(mode):
                    link = z.read(info).decode()
                    if not (target.parent / link).resolve().is_relative_to(dest.resolve()):
                        raise ValueError('Invalid SDK archive link')
                    target.parent.mkdir(parents=True, exist_ok=True)
                    if target.exists() or target.is_symlink():
                        target.unlink()
                    target.symlink_to(link)
                else:
                    z.extract(info, dest)
                    if target.is_file() and mode & 0o111:
                        target.chmod(target.stat().st_mode | 0o111)
    stamp.write_text(expected)
    print('Installed ' + name, flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    root = args.out.resolve()
    root.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=3) as pool:
        list(pool.map(lambda entry: install(root, entry), PACKAGES))
    values = {'ANDROID_HOME': str(root), 'ANDROID_NDK_HOME': str(root / 'ndk/android-ndk-r27c')}
    if os.environ.get('GITHUB_ENV'):
        with open(os.environ['GITHUB_ENV'], 'a') as f:
            for key, value in values.items():
                f.write(key + '=' + value + '\n')
    print('Pinned Android SDK ready.', flush=True)


if __name__ == '__main__':
    main()
