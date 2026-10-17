from io import BytesIO
import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import android_tools


class AndroidToolsTests(unittest.TestCase):
    def test_manifest_uses_fixed_vendor_sha256(self):
        for component, (url, digest, root, target, required) in android_tools.PACKAGES.items():
            with self.subTest(component=component):
                self.assertTrue(url.startswith('https://'))
                self.assertEqual(len(digest), 64)
                self.assertEqual(bytes.fromhex(digest).hex(), digest)
                self.assertTrue(root)
                self.assertTrue(required)

    def test_safe_extract_strips_expected_archive_prefix(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = root / 'test.zip'
            with zipfile.ZipFile(archive, 'w') as z:
                z.writestr('android-35/', b'')
                z.writestr('android-35/android.jar', b'test android platform')
                z.writestr('android-35/other/data', b'test')
            android_tools._extract_safe(archive, root / 'out', 'android-35')
            self.assertEqual((root / 'out/android.jar').read_bytes(), b'test android platform')
            self.assertEqual((root / 'out/other/data').read_bytes(), b'test')

    def test_rejects_archive_path_traversal(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = root / 'test.zip'
            with zipfile.ZipFile(archive, 'w') as z:
                z.writestr('android-35/../../outside', b'bad')
            with self.assertRaises(ValueError):
                android_tools._extract_safe(archive, root / 'out', 'android-35')
            self.assertFalse((root / 'outside').exists())

    def test_download_verifies_digest_and_cleans_invalid_files(self):
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / 'package.zip'
            body = b'only synthetic data'
            url = 'https://dl.google.com/android/repository/test.zip'
            with patch('android_tools.urllib.request.urlopen', return_value=BytesIO(body)):
                android_tools._download(url, hashlib.sha256(body).hexdigest(), out)
            self.assertEqual(out.read_bytes(), body)
            with patch('android_tools.urllib.request.urlopen', return_value=BytesIO(body)):
                with self.assertRaises(ValueError):
                    android_tools._download(url, '0' * 64, out)
            self.assertFalse(out.exists())


if __name__ == '__main__':
    unittest.main()
