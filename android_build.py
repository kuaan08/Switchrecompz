#!/usr/bin/env python3
"""Build a local game as an arm64-v8a NativeActivity APK using Android SDK/NDK.

No games, keys or toolchains are downloaded by this exporter. The small pinned
Khronos header is downloaded once when it is not already cached.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import urllib.request
import zipfile
from xml.sax.saxutils import quoteattr

import switchrecomp as swr

HERE = Path(__file__).resolve().parent
GL_HEADER_URL = ('https://raw.githubusercontent.com/KhronosGroup/OpenGL-Registry/'
                 '6af574a14089ccfee87efe230ebcdd8742859813/api/GL/glcorearb.h')
GL_HEADER_SHA256 = '65fae555a8b3b5709099837001e0c14228b3e45651e2d0fa2ad22649833921e1'


def say(text):
    print(text, flush=True)


def run(args, **kwargs):
    result = subprocess.run([str(a) for a in args], capture_output=True, text=True,
                            errors='replace', **kwargs)
    if result.returncode:
        raise ValueError('Android tool failed: %s\n%s' %
                         (Path(str(args[0])).name, (result.stderr or result.stdout)[-6000:]))
    return result.stdout


def latest(root):
    dirs = [p for p in root.iterdir() if p.is_dir()] if root.is_dir() else []
    return max(dirs, key=lambda p: tuple(int(n) for n in re.findall(r'\d+', p.name)), default=None)


def toolchains(sdk_arg=None, ndk_arg=None):
    sdk = sdk_arg or os.environ.get('ANDROID_SDK_ROOT') or os.environ.get('ANDROID_HOME')
    if not sdk and os.name == 'nt':
        sdk = str(Path(os.environ.get('LOCALAPPDATA', '')) / 'Android/Sdk')
    if not sdk:
        raise ValueError('Choose an Android SDK folder or set ANDROID_HOME.')
    sdk = Path(sdk).resolve()
    ndk = Path(ndk_arg or os.environ.get('ANDROID_NDK_HOME') or latest(sdk / 'ndk') or '')
    host = 'windows-x86_64' if os.name == 'nt' else 'darwin-x86_64' if sys.platform == 'darwin' else 'linux-x86_64'
    llvm = ndk / 'toolchains/llvm/prebuilt' / host
    suffix = '.exe' if os.name == 'nt' else ''
    cc, cxx = llvm / ('bin/clang' + suffix), llvm / ('bin/clang++' + suffix)
    bt = latest(sdk / 'build-tools')
    platform_dir = latest(sdk / 'platforms')
    if not cc.is_file() or not cxx.is_file():
        raise ValueError('Android NDK not found. Install ndk;27.2.12479018 or choose its folder.')
    if not bt or not platform_dir or not (platform_dir / 'android.jar').is_file():
        raise ValueError('Install Android SDK build-tools;35.0.0 and platforms;android-35.')
    tools = {n: bt / (n + suffix) for n in ('aapt2', 'zipalign')}
    tools['apksigner'] = bt / 'lib/apksigner.jar'
    java = shutil.which('java')
    if not java:
        raise ValueError('Install a JDK and put java/keytool on PATH to sign Android APKs.')
    tools['java'] = Path(java)
    if not all(p.is_file() for p in tools.values()):
        raise ValueError('The selected Android SDK build-tools are incomplete.')
    return cc, cxx, llvm / 'sysroot', platform_dir / 'android.jar', tools


def graphics_headers():
    include = HERE / '.ci-cache/graphics-headers/include'
    dest = include / 'GL/glcorearb.h'
    if not dest.is_file() or hashlib.sha256(dest.read_bytes()).hexdigest() != GL_HEADER_SHA256:
        say('Fetching pinned Khronos graphics header...')
        req = urllib.request.Request(GL_HEADER_URL, headers={'User-Agent': 'SwitchRecompz'})
        with urllib.request.urlopen(req, timeout=60) as response:
            data = response.read(2 * 1024 * 1024)
        if hashlib.sha256(data).hexdigest() != GL_HEADER_SHA256:
            raise ValueError('Khronos header checksum mismatch.')
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
    return include


def runtime(cxx, cc, flags, out, jobs):
    rt = HERE / 'runtime'
    names = swr.RUNTIME_SRC + ['gpu_gl', 'gl_load', 'shader', 'astc', 'glq', 'glq_gen',
                              'overlay', 'backend_android', 'host1x', 'lsfg_stub']
    sources = [rt / (n + '.c') for n in names]
    sources += [rt / 'bcn/bcn.cpp', rt / 'bcn/bcdec.cpp']
    include = graphics_headers()
    objdir = out.parent / 'android-runtime-objects'
    objdir.mkdir(exist_ok=True)
    includes = ['-I', rt, '-I', rt / 'bcn', '-I', include]
    # Recompile runtime objects when any runtime/header or compiler input changes.
    signature = hashlib.sha256(str([str(cc), *flags]).encode())
    for p in sorted(rt.rglob('*')):
        if p.suffix in ('.c', '.h', '.cpp'):
            signature.update(p.read_bytes())
    sig = signature.hexdigest()[:16]

    def one(src):
        obj = objdir / (src.name + '.' + sig + '.o')
        if not obj.exists():
            tmp = obj.with_suffix('.tmp')
            compiler = cxx if src.suffix == '.cpp' else cc
            run([compiler, *flags, *includes, '-DSWR_NO_H264', '-O2', '-fPIC',
                 '-fno-strict-aliasing', '-c', src, '-o', tmp])
            tmp.replace(obj)
        return obj

    say('Compiling Android runtime...')
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        objs = list(pool.map(one, sources))
    rsp = objdir / 'objects.rsp'
    rsp.write_text('\n'.join('"%s"' % p.as_posix() for p in objs), encoding='utf-8')
    run([cxx, *flags, '-shared', '-static-libstdc++', '@' + str(rsp),
         '-Wl,-z,defs', '-Wl,-z,max-page-size=16384', '-Wl,-soname,libswr_runtime.so',
         '-landroid', '-llog', '-lEGL', '-lGLESv3', '-laaudio', '-ldl', '-lm', '-o', out])


def asset_files(data):
    result = []
    for directory in ('exefs', 'romfs'):
        source = data / directory
        if not source.is_dir():
            continue
        for p in sorted(source.rglob('*')):
            if p.is_symlink():
                raise ValueError('APK export does not accept asset symlinks.')
            if p.is_file():
                name = p.relative_to(data).as_posix()
                if len(name.encode('utf-8')) > 900 or any(ch in name for ch in '\r\n\\'):
                    raise ValueError('Asset name cannot be represented in the Android manifest.')
                result.append((name, p))
    return result


def make_manifest(package, label):
    if not re.fullmatch(r'[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+', package):
        raise ValueError('Invalid Android package identifier.')
    return ('<manifest xmlns:android="http://schemas.android.com/apk/res/android" package=%s '
            'android:versionCode="1" android:versionName="0.1-experimental">'
            '<uses-sdk android:minSdkVersion="26" android:targetSdkVersion="35"/>'
            '<uses-feature android:glEsVersion="0x00030002" android:required="true"/>'
            '<application android:label=%s android:hasCode="false" android:extractNativeLibs="true" '
            'android:allowBackup="false">'
            '<activity android:name="android.app.NativeActivity" android:exported="true" '
            'android:screenOrientation="sensorLandscape" android:configChanges="orientation|screenSize|keyboardHidden">'
            '<meta-data android:name="android.app.lib_name" android:value="swr_runtime"/>'
            '<intent-filter><action android:name="android.intent.action.MAIN"/>'
            '<category android:name="android.intent.category.LAUNCHER"/></intent-filter>'
            '</activity></application></manifest>') % (quoteattr(package), quoteattr(label))


def package_apk(base, module, rt, jar, tools, package, label, keystore=None):
    work = base.parent / (base.name + '_android')
    work.mkdir(exist_ok=True)
    manifest = work / 'AndroidManifest.xml'
    manifest.write_text(make_manifest(package, label), encoding='utf-8')
    unsigned, aligned = work / 'unsigned.apk', work / 'aligned.apk'
    run([tools['aapt2'], 'link', '-I', jar, '--manifest', manifest, '-o', unsigned])
    assets = asset_files(Path(str(base) + '_data'))
    if len(assets)>60000 or sum(path.stat().st_size for _,path in assets)+module.stat().st_size+rt.stat().st_size>2*1024**3-1024**2:
        raise ValueError('This APK exporter supports packages below 2 GiB and 60000 assets; split/external assets are not implemented.')
    manifest_text = ''.join(name + '\n' for name, _ in assets)
    if len(manifest_text.encode('utf-8')) > 8 * 1024 * 1024:
        raise ValueError('Too many assets for this Android exporter.')
    digest = hashlib.sha256(manifest_text.encode('utf-8'))
    with zipfile.ZipFile(unsigned, 'a', allowZip64=False) as apk:
        for src, name in [(module, 'lib/arm64-v8a/libgame_code.so'),
                          (rt, 'lib/arm64-v8a/libswr_runtime.so')]:
            apk.write(src, name, compress_type=zipfile.ZIP_DEFLATED)
        for name, path in assets:
            with path.open('rb') as source:
                for block in iter(lambda: source.read(1024 * 1024), b''):
                    digest.update(block)
            apk.write(path, 'assets/game/' + name, compress_type=zipfile.ZIP_STORED)
        apk.writestr('assets/switchrecomp/name.txt', 'game\n')
        apk.writestr('assets/switchrecomp/assets.txt', manifest_text)
        apk.writestr('assets/switchrecomp/build-id.txt', digest.hexdigest())
        apk.writestr('assets/switchrecomp/game.ini', '[Display]\nfullscreen=1\n[Frame generation]\nlsfg=0\n')
        for license_file in (HERE/'LICENSE.txt', HERE/'NOTICE.txt', HERE/'runtime/bcn/LICENSE_texture2ddecoder.txt'):
            apk.write(license_file,'assets/switchrecomp/licenses/'+license_file.name)
        apk.writestr('assets/switchrecomp/BUILD_INFO.txt', 'SwitchRecomp by MZ Labs\nFork additions maintained by katakishi0\nExperimental arm64 APK export\n')
    run([tools['zipalign'], '-P', '16', '-f', '4', unsigned, aligned])
    data_home=Path(os.environ.get('LOCALAPPDATA',str(Path.home()/'AppData/Local'))) if os.name=='nt' else Path(os.environ.get('XDG_DATA_HOME',str(Path.home()/'.local/share')))
    key = Path(keystore or os.environ.get('SWR_ANDROID_KEYSTORE') or data_home/'SwitchRecomp/android/debug.keystore')
    if not key.is_file():
        if keystore:
            raise ValueError('The selected keystore does not exist.')
        key.parent.mkdir(parents=True, exist_ok=True)
        kt = shutil.which('keytool')
        if not kt:
            raise ValueError('Install a JDK with keytool to sign the debug APK.')
        run([kt, '-genkeypair', '-noprompt', '-keystore', key, '-storepass', 'android',
             '-keypass', 'android', '-alias', 'androiddebugkey', '-keyalg', 'RSA', '-keysize', '2048',
             '-validity', '10000', '-dname', 'CN=Android Debug,O=Android,C=US'])
    output = base.with_suffix('.apk')
    run([tools['java'], '-jar', tools['apksigner'], 'sign', '--ks', key, '--ks-pass', 'pass:android',
         '--out', output, aligned])
    run([tools['java'], '-jar', tools['apksigner'], 'verify', '--verbose', output])
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input')
    parser.add_argument('-o', default='Game')
    parser.add_argument('--outdir', default='games')
    parser.add_argument('--sdk')
    parser.add_argument('--ndk')
    parser.add_argument('--package')
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('-O', choices=['0', '1', '2', '3'], default='2')
    for opt in ('hactool', 'keys', 'title-keys', 'ticket', 'program-nca'):
        parser.add_argument('--' + opt)
    args = parser.parse_args()
    if args.jobs < 1 or args.jobs > 256:
        parser.error('--jobs must be between 1 and 256')
    cc, cxx, sysroot, jar, tools = toolchains(args.sdk, args.ndk)
    title = ''.join(ch for ch in args.o if ch.isalnum() or ch in '-_ ').strip() or 'Game'
    base = Path(args.outdir).resolve() / title
    # Keep the existing import/loader/code generation path as the source of truth.
    command = [sys.executable, str(HERE / 'switchrecomp.py'), args.input,
               '--no-build', '--target', 'linux', '--outdir', str(base.parent), '-o', title]
    for opt in ('hactool', 'keys', 'title-keys', 'ticket', 'program-nca'):
        value = getattr(args, opt.replace('-', '_'))
        if value:
            command.extend(['--' + opt, value])
    subprocess.run(command, check=True)
    build = Path(str(base) + '_build')
    files = [str(build / 'module.c')] + [str(p) for p in sorted(build.glob('code_*.c'))]
    flags = ['--target=aarch64-linux-android26', '--sysroot=' + str(sysroot)]
    say('Compiling generated C for Android arm64-v8a...')
    objs = swr.compile_all(files, str(cc), args.O, str(build), False, args.jobs, say, flags)
    module = build / 'libgame_code.so'
    rsp = build / 'android-module.rsp'
    rsp.write_text('\n'.join('"%s"' % Path(p).as_posix() for p in objs), encoding='utf-8')
    run([cc, *flags, '-shared', '@' + str(rsp), '-Wl,-z,defs', '-Wl,-z,max-page-size=16384',
         '-Wl,-soname,libgame_code.so', '-lm', '-o', module])
    rt = build / 'libswr_runtime.so'
    runtime(cxx, cc, flags, rt, args.jobs)
    identifier = args.package or 'org.switchrecomp.game.g' + hashlib.sha256(title.encode()).hexdigest()[:16]
    output = package_apk(base, module, rt, jar, tools, identifier, title)
    say('Done: %s' % output)
    say('Experimental Android APK: install on an arm64 device with GLES 3.2. Game compatibility needs device testing.')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as exc:
        raise SystemExit('Android export failed: %s' % exc)
