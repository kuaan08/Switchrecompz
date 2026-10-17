from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

import android_build


class AndroidExportTests(unittest.TestCase):
    def test_exporter_runs_with_isolated_python(self):
        # Windows' embedded Python uses a ._pth file: script siblings are not
        # necessarily imported unless the exporter adds its own directory.
        exporter = Path(android_build.__file__).resolve()
        result = subprocess.run([sys.executable, '-I', str(exporter), '--help'],
                                capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('--sdk', result.stdout)

    def test_manifest_escapes_label_and_needs_no_storage_permission(self):
        root = ET.fromstring(android_build.make_manifest('org.switchrecomp.test', 'Game & "test"'))
        ns = '{http://schemas.android.com/apk/res/android}'
        self.assertEqual(root.find('application').get(ns + 'label'), 'Game & "test"')
        self.assertFalse(root.findall('uses-permission'))
        self.assertEqual(root.find('application').get(ns + 'hasCode'), 'false')

    def test_invalid_package_identifiers_are_rejected(self):
        for package in ('x', '../test', 'org.test" android:debuggable="true', 'org.1game'):
            with self.subTest(package=package), self.assertRaises(ValueError):
                android_build.make_manifest(package, 'Game')

    def test_assets_are_limited_to_game_inputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'exefs').mkdir()
            (root / 'romfs/nested').mkdir(parents=True)
            (root / 'exefs/main').write_bytes(b'synthetic')
            (root / 'romfs/nested/data').write_bytes(b'data')
            (root / 'prod.keys').write_bytes(b'not-for-apk')
            names = [name for name, _ in android_build.asset_files(root)]
            self.assertEqual(names, ['exefs/main', 'romfs/nested/data'])

    def test_asset_symlinks_are_not_packaged(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'exefs').mkdir()
            (root / 'private').write_bytes(b'outside')
            try:
                (root / 'exefs/link').symlink_to(root / 'private')
            except (OSError, NotImplementedError):
                self.skipTest('Symlinks unavailable')
            with self.assertRaises(ValueError):
                android_build.asset_files(root)
