#!/usr/bin/env python3
"""Build a pinned official hactool source checkout and stage its licenses."""
import argparse
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile

COMMIT = '1d64a83450e025622f3468c28fc4164dad2c5ef6'
ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', required=True)
    args = parser.parse_args()
    source = Path(args.source).resolve()
    revision = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    if revision != COMMIT:
        raise SystemExit('hactool source revision does not match the pinned commit.')
    windows = os.name == 'nt'
    if windows:
        bindir = ROOT / 'w64devkit' / 'bin'
        compiler = (bindir / 'gcc.exe').as_posix()
        archiver = (bindir / 'ar.exe').as_posix()
        make = str(bindir / 'make.exe')
        env = dict(os.environ, PATH=str(bindir) + os.pathsep + os.environ.get('PATH', ''))
    else:
        compiler, archiver, make = 'gcc', 'ar', 'make'
        env = os.environ.copy()
    # mbedtls/library includes this same config. Static linking on Windows avoids
    # requiring GCC DLLs next to the bundled extractor.
    config = ('CC = %s\nAR = %s\nCFLAGS = -O2 -std=gnu11 -fPIC\n'
              'LDFLAGS = %s-lmbedtls -lmbedx509 -lmbedcrypto%s\n') % (
                  compiler, archiver, '-static ' if windows else '', ' -lws2_32' if windows else '')
    (source / 'config.mk').write_text(config)
    command = [make, '-j%d' % min(os.cpu_count() or 2, 8)]
    if windows:
        command += ['EXEEXT=.exe', 'WINDOWS=1']
    subprocess.run(command, cwd=source, env=env, check=True)
    executable = source / ('hactool.exe' if windows else 'hactool')
    # Verify that the native executable actually launches and extracts a harmless
    # PFS0 file, rather than only checking that a compiler produced some bytes.
    with tempfile.TemporaryDirectory(prefix='hactool-smoke-') as tmp:
        tmp = Path(tmp)
        filename, data = b'hello.txt\0', b'Switchrecompz extractor smoke test'
        archive = (b'PFS0' + struct.pack('<III', 1, len(filename), 0)
                   + struct.pack('<QQII', 0, len(data), 0, 0) + filename + data)
        (tmp / 'test.nsp').write_bytes(archive)
        subprocess.run([str(executable), '-t', 'pfs0', '--outdir=' + str(tmp / 'out'),
                        str(tmp / 'test.nsp')], env=env, check=True)
        if (tmp / 'out/hello.txt').read_bytes() != data:
            raise SystemExit('Bundled hactool extraction smoke test failed.')
    target = ROOT / 'tools' / 'hactool'
    target.mkdir(parents=True, exist_ok=True)
    shutil.copy2(executable, target / executable.name)
    shutil.copy2(source / 'LICENSE', target / 'LICENSE-hactool.txt')
    shutil.copy2(source / 'mbedtls/LICENSE', target / 'LICENSE-mbedtls.txt')
    (target / 'SOURCE.txt').write_text('hactool by SciresM (ISC)\n'
        'https://github.com/SciresM/hactool\nCommit: ' + COMMIT + '\n'
        'Includes the mbed TLS sources vendored at that commit; see LICENSE-mbedtls.txt.\n')
    print('Bundled hactool built and smoke-tested: ' + str(target))


if __name__ == '__main__':
    main()
