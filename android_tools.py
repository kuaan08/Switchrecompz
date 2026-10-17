#!/usr/bin/env python3
"""Install pinned Android build tools in the user's private SwitchRecomp cache.

Only used on Windows with the explicit --auto-tools option. Never run tools
from an archive before checking the complete SHA-256 of the download.
"""
import hashlib
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import urllib.request
import zipfile

BASE = Path(os.environ.get('LOCALAPPDATA') or Path.home() / 'AppData/Local') / 'SwitchRecomp/android/toolchains'
SDK = BASE / 'sdk'
JDK = BASE / 'jdk17'

# Immutable vendor archives and independently published SHA-256 fingerprints.
# Changing any URL must also require reviewing the corresponding digest.
PACKAGES = {
    'ndk': (
        'https://dl.google.com/android/repository/android-ndk-r27c-windows.zip',
        '27e49f11e0cee5800983d8af8f4acd5bf09987aa6f790d4439dda9f3643d2494',
        'android-ndk-r27c', SDK / 'ndk/27.2.12479018',
        'toolchains/llvm/prebuilt/windows-x86_64/bin/clang.exe'),
    'platform': (
        'https://dl.google.com/android/repository/platform-35_r02.zip',
        '0988cacad01b38a18a47bac14a0695f246bc76c1b06c0eeb8eb0dc825ab0c8e0',
        'android-35', SDK / 'platforms/android-35', 'android.jar'),
    'build-tools': (
        'https://dl.google.com/android/repository/build-tools_r35_windows.zip',
        '5753c679a1b90bcf6fbc9945a2ce39dfb9e74f1df0831a1e1866ae5b594326f0',
        'android-15', SDK / 'build-tools/35.0.0', 'aapt2.exe'),
    'jdk': (
        'https://github.com/adoptium/temurin17-binaries/releases/download/'
        'jdk-17.0.20.1+1/OpenJDK17U-jdk_x64_windows_hotspot_17.0.20.1_1.zip',
        'e53a79c3c3d86865bd7e787903884331068e71321714ffd44f145785affc7cb0',
        'jdk-17.0.20.1+1', JDK, 'bin/java.exe'),
}


def _download(url, sha256, dest):
    if not url.startswith(('https://dl.google.com/android/repository/',
                           'https://github.com/adoptium/temurin17-binaries/releases/download/')):
        raise ValueError('Unexpected Android tool download source.')
    dest.parent.mkdir(parents=True, exist_ok=True)
    h = hashlib.sha256()
    count = 0
    req = urllib.request.Request(url, headers={'User-Agent': 'SwitchRecompz-Android-Setup'})
    try:
        with urllib.request.urlopen(req, timeout=90) as resp, dest.open('wb') as out:
            for chunk in iter(lambda: resp.read(1024 * 1024), b''):
                count += len(chunk)
                if count > 2 * 1024 ** 3:
                    raise ValueError('Android tool archive exceeds size limit.')
                h.update(chunk)
                out.write(chunk)
                if count // (64 * 1024 ** 2) != (count - len(chunk)) // (64 * 1024 ** 2):
                    print('  received %d MiB...' % (count // 1024 ** 2), flush=True)
        if h.hexdigest() != sha256:
            raise ValueError('Android tool archive failed SHA-256 verification.')
    except BaseException:
        dest.unlink(missing_ok=True)
        raise


def _extract_safe(archive, dest, root_name):
    """Extract only beneath one expected top-level directory; reject zip slips."""
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    count = total = 0
    try:
        with zipfile.ZipFile(archive) as z:
            for info in z.infolist():
                count += 1
                total += info.file_size
                if count > 200000 or total > 12 * 1024 ** 3:
                    raise ValueError('Android tool archive exceeds extraction limits.')
                name = info.filename
                parts = PurePosixPath(name).parts
                if (not parts or parts[0] != root_name or
                    any(p in ('', '.', '..') or ':' in p for p in parts) or
                    '\\' in name or name.startswith('/')):
                    raise ValueError('Unsafe or unexpected Android tool archive path.')
                if len(parts) == 1:
                    continue
                mode = info.external_attr >> 16
                if stat.S_ISLNK(mode):
                    raise ValueError('Android tool archive contains an unsupported symbolic link.')
                target = dest.joinpath(*parts[1:])
                if info.is_dir():
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with z.open(info) as src, target.open('wb') as output:
                        shutil.copyfileobj(src, output, 1024 * 1024)
    except BaseException:
        shutil.rmtree(dest, ignore_errors=True)
        raise


def _install(component):
    url, digest, root_name, destination, required = PACKAGES[component]
    if (destination / required).is_file():
        return destination
    if os.name != 'nt':
        raise ValueError('Automatic Android tool setup currently supports Windows x64.')
    temp = destination.with_name(destination.name + '.partial')
    download = BASE / 'downloads' / (component + '.zip')
    print('Installing %s (verified official archive; this may take several minutes)...' % component,
          flush=True)
    try:
        _download(url, digest, download)
        _extract_safe(download, temp, root_name)
        if not (temp / required).is_file():
            raise ValueError('The %s archive did not contain %s.' % (component, required))
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.exists():
            shutil.rmtree(destination)
        temp.replace(destination)
    finally:
        download.unlink(missing_ok=True)
        if temp.exists():
            shutil.rmtree(temp, ignore_errors=True)
    print('Installed %s in %s' % (component, destination), flush=True)
    return destination


def ensure_sdk():
    """Install SDK platform/build-tools and NDK only when their files are absent."""
    for component in ('platform', 'build-tools', 'ndk'):
        _install(component)
    return SDK


def ensure_jdk():
    home = _install('jdk')
    if not (home / 'bin/keytool.exe').is_file():
        raise ValueError('Downloaded JDK is missing keytool.exe.')
    return home
