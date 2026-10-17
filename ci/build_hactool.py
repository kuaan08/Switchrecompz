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


def patch_header_probe(source):
    """Keep header discovery independent from title-key/section decryption."""
    path = source / 'nca.c'
    text = path.read_text()
    if 'SWR_HACTOOL_HEADER_ONLY' in text:
        return
    anchor = 'void nca_process(nca_ctx_t *ctx) {'
    if text.count(anchor) != 1:
        raise SystemExit('Pinned hactool NCA probe patch anchor changed.')
    text = text.replace(anchor, 'static const char *nca_get_content_type(nca_ctx_t *ctx);\n\n' + anchor)
    anchor = '    /* Enforce content type for extraction if required. */'
    patch = r"""    /* Switchrecompz: inspect only the decrypted header before a title key is selected. */
    if (getenv("SWR_HACTOOL_HEADER_ONLY") != NULL) {
        printf("SWR Header-only: 1\n");
        printf("Content Type: %s\n", nca_get_content_type(ctx));
        memdump(stdout, "Rights ID: ", ctx->header.rights_id, 0x10);
        return;
    }

"""
    if text.count(anchor) != 1:
        raise SystemExit('Pinned hactool section patch anchor changed.')
    text = text.replace(anchor, patch + anchor)
    text = text.replace('fprintf(stderr, "Invalid NCA header! Are keys correct?\\n");\n        return;',
                        'fprintf(stderr, "Invalid NCA header! Are keys correct?\\n");\n        exit(EXIT_FAILURE);')
    path.write_text(text)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', required=True)
    args = parser.parse_args()
    source = Path(args.source).resolve()
    revision = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    if revision != COMMIT:
        raise SystemExit('hactool source revision does not match the pinned commit.')
    patch_header_probe(source)
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
    with tempfile.TemporaryDirectory(prefix='hactool-header-smoke-') as tmp:
        path = Path(tmp) / 'synthetic.nca'
        header = bytearray(0xC00)
        header[0x200:0x204] = b'NCA3'
        header[0x230:0x240] = bytes.fromhex('11' * 16)
        # A section that must not be parsed during header-only discovery.
        struct.pack_into('<II', header, 0x240, 6, 8)
        path.write_bytes(header)
        probe = subprocess.run([str(executable), '--suppresskeys', '-i', str(path)],
                               env=dict(env, SWR_HACTOOL_HEADER_ONLY='1'), capture_output=True, check=True)
        if b'SWR Header-only: 1' not in probe.stdout or b'Content Type: Program' not in probe.stdout:
            raise SystemExit('Native header-only NCA probe smoke test failed.')
        path.write_bytes(bytes(0xC00))
        invalid = subprocess.run([str(executable), '--suppresskeys', '-i', str(path)],
                                 env=dict(env, SWR_HACTOOL_HEADER_ONLY='1'), capture_output=True)
        if invalid.returncode == 0 or b'Invalid NCA header' not in invalid.stderr:
            raise SystemExit('Invalid NCA header must return failure.')
    target = ROOT / 'tools' / 'hactool'
    target.mkdir(parents=True, exist_ok=True)
    shutil.copy2(executable, target / executable.name)
    shutil.copy2(source / 'LICENSE', target / 'LICENSE-hactool.txt')
    shutil.copy2(source / 'mbedtls/LICENSE', target / 'LICENSE-mbedtls.txt')
    shutil.copy2(source / 'mbedtls/apache-2.0.txt', target / 'apache-2.0.txt')
    (target / 'SOURCE.txt').write_text('hactool by SciresM (ISC)\n'
        'https://github.com/SciresM/hactool\nCommit: ' + COMMIT + '\n'
        'Includes the mbed TLS sources vendored at that commit; see LICENSE-mbedtls.txt.\n'
        'Switchrecompz header-only inspection patch applied; see ci/build_hactool.py in the fork.\n')
    print('Bundled hactool built and smoke-tested: ' + str(target))


if __name__ == '__main__':
    main()
