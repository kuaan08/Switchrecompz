"""Exercise the compile/data boundary with a synthetic imported NSO, not game assets."""
from contextlib import contextmanager
from pathlib import Path
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import switchrecomp as swr


def make_game(root):
    exefs = root / 'exefs'
    exefs.mkdir(parents=True)
    # main immediately exits via svc ExitProcess. No copyrighted game code.
    header = bytearray(0x100)
    header[:4] = b'NSO0'
    segments = [struct.pack('<IIII', 0xD40000E1, 0, 0, 0), bytes(16), bytes(16)]
    file_offset = 0x100
    for index, data in enumerate(segments):
        struct.pack_into('<III', header, 0x10 + index * 0x10, file_offset, index * 0x1000, len(data))
        struct.pack_into('<I', header, 0x60 + index * 4, len(data))
        file_offset += len(data)
    (exefs / 'main').write_bytes(header + b''.join(segments))
    (root / 'romfs').mkdir()
    (root / 'romfs' / 'asset.txt').write_text('synthetic test asset')
    return root


class PipelineTests(unittest.TestCase):
    def run_imported(self, compiler=None):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            package = tmp / 'original.nsp'
            package.write_bytes(b'package placeholder: importer is replaced by a fixture')
            original = package.read_bytes()
            output = tmp / 'out'
            @contextmanager
            def imported(*args, **kwargs):
                with tempfile.TemporaryDirectory(dir=tmp) as extracted:
                    yield str(make_game(Path(extracted)))
            argv = ['switchrecomp.py', str(package), '-o', 'hello', '--outdir', str(output), '--backend', 'headless', '--jobs', '2']
            if compiler:
                argv += ['--cc', str(compiler)]
            else:
                argv += ['--no-build']
            with patch.object(swr, 'import_game', imported), patch.object(sys, 'argv', argv):
                swr.main()
            self.assertEqual(package.read_bytes(), original)
            self.assertTrue((output / 'hello_data/exefs/main').is_file())
            self.assertEqual((output / 'hello_data/romfs/asset.txt').read_text(), 'synthetic test asset')
            self.assertTrue((output / 'hello_build/module.c').is_file())
            if compiler:
                exe = output / ('hello.exe' if os.name == 'nt' else 'hello')
                result = subprocess.run([str(exe)], cwd=output, capture_output=True, timeout=60)
                self.assertEqual(result.returncode, 0)
                self.assertIn('The game exited (code 0)', (output / 'hello.log').read_text(errors='replace'))

    def test_imported_folder_reaches_data_copy_and_translation(self):
        self.run_imported()

    def test_imported_folder_compiles_and_runs(self):
        root = Path(swr.HERE)
        compilers = [root / 'tcc/tcc.exe', root / 'w64devkit/bin/gcc.exe'] if os.name == 'nt' else [shutil.which('gcc')]
        available = [Path(cc) for cc in compilers if cc and Path(cc).is_file()]
        if not available:
            self.skipTest('No native compiler installed')
        for compiler in available:
            with self.subTest(compiler=compiler.name):
                self.run_imported(compiler)

    def test_missing_exefs_reports_a_clear_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError, 'no extracted ExeFS'):
                swr.copy_data([], None, None, str(Path(tmp) / 'not-a-folder.nsp'), str(Path(tmp) / 'data'), lambda _: None)
