import io
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

import package_import as imp


def container(files, magic=b'PFS0'):
    names, table, payload = b'', b'', b''
    for name, data in files:
        record = struct.pack('<QQII', len(payload), len(data), len(names), 0)
        table += record + (bytes(40) if magic == b'HFS0' else b'')
        names += name.encode() + b'\0'
        payload += data
    return magic + struct.pack('<III', len(files), len(names), 0) + table + names + payload


class ContainerTests(unittest.TestCase):
    def test_nsp_ranges_and_streaming_contents(self):
        raw = container([('a.nca', b'AAA'), ('b.nca', b'BBBB')])
        stream = io.BytesIO(raw)
        entries = imp.package_entries(stream, '.nsp', len(raw))
        self.assertEqual([(name, imp.read_at(stream, offset, size)) for name, offset, size in entries],
                         [('a.nca', b'AAA'), ('b.nca', b'BBBB')])

    def test_untrusted_filenames(self):
        for name in ['../escape.nca', '/absolute.nca', 'C:evil.nca', 'a\\evil.nca',
                     'CON.nca', 'nul.nca', 'trailing.nca.', '..', 'a\n.nca']:
            with self.subTest(name=name), self.assertRaises(ValueError):
                raw = container([(name, b'data')])
                imp.package_entries(io.BytesIO(raw), '.nsp', len(raw))

    def test_case_insensitive_duplicate(self):
        raw = container([('A.nca', b'a'), ('a.nca', b'b')])
        with self.assertRaisesRegex(ValueError, 'Duplicate'):
            imp.package_entries(io.BytesIO(raw), '.nsp', len(raw))

    def test_truncated_payload_and_table(self):
        raw = container([('a.nca', b'12345')])
        for bad in [raw[:-1], raw[:12], b'PFS0' + struct.pack('<III', 0xFFFFFFFF, 0, 0)]:
            with self.subTest(length=len(bad)), self.assertRaises(ValueError):
                imp.package_entries(io.BytesIO(bad), '.nsp', len(bad))

    def test_overlapping_entries(self):
        raw = bytearray(container([('a.nca', b'123'), ('b.nca', b'456')]))
        struct.pack_into('<Q', raw, 16 + 24, 0)
        with self.assertRaisesRegex(ValueError, 'Overlapping'):
            imp.package_entries(io.BytesIO(raw), '.nsp', len(raw))

    def test_xci_secure_partition(self):
        secure = container([('a.nca', b'payload')], b'HFS0')
        root = container([('secure', secure)], b'HFS0')
        raw = bytearray(0x200)
        raw[0x100:0x104] = b'HEAD'
        struct.pack_into('<Q', raw, 0x130, 0x200)
        raw += root
        stream = io.BytesIO(raw)
        name, offset, size = imp.package_entries(stream, '.xci', len(raw))[0]
        self.assertEqual((name, imp.read_at(stream, offset, size)), ('a.nca', b'payload'))
        struct.pack_into('<Q', raw, 0x130, len(raw) + 1)
        with self.assertRaises(ValueError):
            imp.package_entries(io.BytesIO(raw), '.xci', len(raw))


class ImportTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.tool = self.root / 'hactool'
        self.tool.write_bytes(b'test only')
        self.keys = self.root / 'prod.keys'
        self.keys.write_text('header_key = ' + '0' * 64)
        self.source = self.root / 'game.nsp'
        self.source.write_bytes(container([('program.nca', b'test encrypted content')]))
        self.args = dict(hactool=str(self.tool), keys=str(self.keys), say=lambda _: None)

    def tool_result(self, tool, keys, archive, options, title_key=None):
        if '-i' in options:
            return 'Content Type: Program\nRights ID: ' + '0' * 32
        exefs = Path(next(o.split('=', 1)[1] for o in options if o.startswith('--exefsdir=')))
        romfs = Path(next(o.split('=', 1)[1] for o in options if o.startswith('--romfsdir=')))
        exefs.mkdir(); romfs.mkdir()
        (exefs / 'main').write_bytes(b'NSO0test')
        (romfs / 'asset').write_bytes(b'test')
        return ''

    def test_import_and_cleanup_without_changing_original(self):
        original = self.source.read_bytes()
        with patch.object(imp, 'run_hactool', side_effect=self.tool_result):
            with imp.import_game(self.source, **self.args) as game:
                self.assertEqual((Path(game) / 'exefs/main').read_bytes(), b'NSO0test')
                self.assertEqual((Path(game) / 'romfs/asset').read_bytes(), b'test')
            self.assertFalse(Path(game).exists())
        self.assertEqual(self.source.read_bytes(), original)

    def test_cleanup_after_consumer_failure(self):
        with patch.object(imp, 'run_hactool', side_effect=self.tool_result):
            with self.assertRaises(RuntimeError):
                with imp.import_game(self.source, **self.args) as game:
                    raise RuntimeError('consumer failure')
            self.assertFalse(Path(game).exists())

    def test_multi_program_requires_selection(self):
        self.source.write_bytes(container([('a.nca', b'a'), ('b.nca', b'b')]))
        with patch.object(imp, 'run_hactool', side_effect=self.tool_result):
            with self.assertRaisesRegex(ValueError, 'found 2'):
                with imp.import_game(self.source, **self.args):
                    self.fail('ambiguous package accepted')
            with imp.import_game(self.source, program_nca='b.nca', **self.args) as game:
                self.assertTrue((Path(game) / 'exefs/main').is_file())

    def test_bundled_extractor_is_used_without_configuration(self):
        home = self.root / 'app'
        tool = home / 'tools/hactool' / ('hactool.exe' if os.name == 'nt' else 'hactool')
        tool.parent.mkdir(parents=True)
        tool.write_bytes(b'test only')
        def run(selected, *args, **kwargs):
            self.assertTrue(Path(selected).samefile(tool))
            return self.tool_result(selected, *args, **kwargs)
        with patch.object(imp, '__file__', str(home / 'package_import.py')):
            with patch.dict(os.environ, {'SWR_HACTOOL': '', 'SWR_TITLE_KEYS': ''}):
                with patch.object(imp, 'run_hactool', side_effect=run):
                    with imp.import_game(self.source, keys=str(self.keys), say=lambda _: None) as game:
                        self.assertTrue((Path(game) / 'exefs/main').is_file())

    def test_missing_title_key_stops_import(self):
        with patch.object(imp, 'run_hactool', return_value='Content Type: Program\nRights ID: ' + '1' * 32):
            with self.assertRaisesRegex(ValueError, 'requires its decrypted title key'):
                with imp.import_game(self.source, **self.args):
                    self.fail('missing title key accepted')

    def test_missing_tools_and_compressed_packages(self):
        args = dict(self.args, hactool=str(self.root / 'missing'))
        with self.assertRaisesRegex(ValueError, 'requires hactool'):
            with imp.import_game(self.source, **args):
                self.fail('missing tool accepted')
        with self.assertRaisesRegex(ValueError, 'Compressed/split'):
            with imp.import_game(self.root / 'game.nsz', **self.args):
                self.fail('unsupported format accepted')

    def test_tool_error_does_not_leak_output_or_keys(self):
        def fail(args, **kw):
            self.assertIsInstance(args, list)
            self.assertNotIn('shell', kw)
            kw['stdout'].write(b'SECRET KEY CONTENT')
            return type('Result', (), {'returncode': 1})()
        with patch.object(imp.subprocess, 'run', side_effect=fail):
            with self.assertRaises(ValueError) as error:
                imp.run_hactool(self.tool, self.keys, self.source, ['-i'])
        self.assertNotIn('SECRET', str(error.exception))

    def test_decrypted_title_key_uses_contentkey_option(self):
        secret = 'a' * 32
        def success(args, **kw):
            self.assertIn('--contentkey=' + secret, args)
            self.assertNotIn('--titlekey=' + secret, args)
            return type('Result', (), {'returncode': 0})()
        with patch.object(imp.subprocess, 'run', side_effect=success):
            imp.run_hactool(self.tool, self.keys, self.source, ['-i'], secret)

    def test_keyset_normalizes_windows_encodings(self):
        for encoding in ['utf-8-sig', 'utf-16']:
            self.keys.write_text('header_key = ' + '0' * 64, encoding=encoding)
            normalized = imp.normalize_keyset(self.keys, self.root / 'normalized.keys')
            self.assertTrue(normalized.read_bytes().startswith(b'header_key = '))
        self.keys.write_text('master_key_00 = ' + '0' * 32)
        with self.assertRaisesRegex(ValueError, 'header_key'):
            imp.normalize_keyset(self.keys, self.root / 'bad.keys')

    def test_redundant_malformed_derivation_sources(self):
        original = ('header_key = ' + '0' * 64 + '\nmaster_key_06 = ' + '1' * 32
                    + '\nmaster_kek_source_06 = ' + '2' * 34
                    + '\nmariko_master_kek_source_06 = ' + '3' * 34)
        self.keys.write_text(original)
        messages = []
        normalized = imp.normalize_keyset(self.keys, self.root / 'normalized.keys', messages.append)
        self.assertNotIn('kek_source_06', normalized.read_text())
        self.assertIn('master_key_06', normalized.read_text())
        self.assertEqual(self.keys.read_text(), original)
        self.assertIn('Ignoring 2', messages[0])
        self.keys.write_text(original.replace('master_key_06', 'master_key_07'))
        with self.assertRaisesRegex(ValueError, 'Invalid master_kek_source_06'):
            imp.normalize_keyset(self.keys, self.root / 'rejected.keys')
        message = imp.hactool_failure('Key (' + 'a' * 34 + ') must be 32 hex digits!', 1, True)
        self.assertIn('invalid length', message)
        self.assertNotIn('a' * 34, message)

    def test_error_diagnostics_never_expose_output_keys(self):
        for raw in ['Invalid NCA header! ' + 'abcd' * 32,
                    'Failed to read PFS0 header! ' + '1234' * 32,
                    'SECRET KEY VALUE ' + 'ffff' * 32]:
            result = imp.hactool_failure(raw, 1, True)
            self.assertNotIn('abcd' * 32, result)
            self.assertNotIn('1234' * 32, result)
            self.assertNotIn('ffff' * 32, result)
            self.assertIn('exit 0x00000001', result)
        self.assertIn('crashed', imp.hactool_failure('', 0xc0000005, True))

    def test_probe_uses_header_only_but_extraction_does_not(self):
        def success(args, **kwargs):
            self.assertIn('--suppresskeys', args)
            if '-i' in args:
                self.assertEqual(kwargs['env']['SWR_HACTOOL_HEADER_ONLY'], '1')
            else:
                self.assertNotIn('SWR_HACTOOL_HEADER_ONLY', kwargs['env'])
            return type('Result', (), {'returncode': 0})()
        with patch.object(imp.subprocess, 'run', side_effect=success):
            imp.run_hactool(self.tool, self.keys, self.source, ['-i'])
            imp.run_hactool(self.tool, self.keys, self.source, ['--exefsdir=example'])

    def test_title_key_parser(self):
        path = self.root / 'title.keys'
        path.write_text('# local only\n' + 'a' * 32 + ' = ' + 'b' * 32)
        self.assertEqual(imp.read_title_keys(path), {'a' * 32: 'b' * 32})
        path.write_text('malformed entry')
        with self.assertRaises(ValueError):
            imp.read_title_keys(path)


if __name__ == '__main__':
    unittest.main()
