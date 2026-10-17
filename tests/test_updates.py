import unittest
from copy import deepcopy

import update_check as updates


class UpdateTests(unittest.TestCase):
    def setUp(self):
        self.release = {'name': 'Development build', 'html_url': updates.PAGE, 'draft': False,
                        'target_commitish': 'a' * 40,
                        'assets': [{'name': 'SwitchRecomp.exe', 'browser_download_url': updates.DOWNLOAD}]}

    def test_exact_development_commit_comparison(self):
        self.assertTrue(updates.release_info(self.release, 'dev-aaaaaaa')['current'])
        self.assertFalse(updates.release_info(self.release, 'dev-bbbbbbb')['current'])
        self.assertFalse(updates.release_info(self.release, '1.0.0 (local build)')['current'])

    def test_unexpected_or_missing_assets_are_rejected(self):
        for change in [{'draft': True}, {'html_url': 'https://example.com/update'}, {'assets': []},
                       {'target_commitish': 'main'}, {'assets': [{'name': 'SwitchRecomp.exe', 'browser_download_url': 'https://example.com/app.exe'}]}]:
            release = deepcopy(self.release)
            release.update(change)
            with self.assertRaises(ValueError):
                updates.release_info(release, 'dev-aaaaaaa')
