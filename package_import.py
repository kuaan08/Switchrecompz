# SwitchRecomp by MZ Labs; package import additions licensed under Apache-2.0.
"""Bounded, streaming NSP/XCI import. No keys or games are bundled; releases include a locally built extractor."""
from contextlib import contextmanager
from pathlib import Path
import os
import re
import shutil
import struct
import subprocess
import tempfile

PACKAGE_EXTENSIONS = {'.nsp', '.xci', '.nca'}
MAX_TABLE = 16 * 1024 * 1024
MAX_FILES = 4096


def read_at(stream, offset, size):
    stream.seek(offset)
    data = stream.read(size)
    if len(data) != size:
        raise ValueError('Truncated container.')
    return data


def partition(stream, start, length, magic):
    """Validate every entry, including names we will not extract."""
    header = read_at(stream, start, 16)
    if header[:4] != magic:
        raise ValueError('Invalid %s container header.' % magic.decode())
    count, strings_size = struct.unpack_from('<II', header, 4)
    stride = 24 if magic == b'PFS0' else 64
    table_size = 16 + count * stride + strings_size
    if count > MAX_FILES or table_size > MAX_TABLE or table_size > length:
        raise ValueError('Invalid or excessive container table.')
    table = read_at(stream, start + 16, count * stride)
    strings = read_at(stream, start + 16 + count * stride, strings_size)
    seen, intervals, entries = set(), [], []
    for i in range(count):
        offset, size, name_off = struct.unpack_from('<QQI', table, i * stride)
        if name_off >= len(strings):
            raise ValueError('Invalid container filename offset.')
        end = strings.find(b'\0', name_off)
        if end < 0:
            raise ValueError('Unterminated container filename.')
        try:
            name = strings[name_off:end].decode('utf-8')
        except UnicodeDecodeError as exc:
            raise ValueError('Invalid container filename encoding.') from exc
        # Flat containers: forbid traversal, ADS, controls and Windows device names.
        if (not re.fullmatch(r'[A-Za-z0-9_. -]{1,240}', name)
                or name in {'.', '..'} or name.endswith(('.', ' '))
                or name.split('.')[0].upper() in {'CON', 'PRN', 'AUX', 'NUL',
                    *('COM%d' % n for n in range(1, 10)), *('LPT%d' % n for n in range(1, 10))}):
            raise ValueError('Unsafe container filename.')
        if name.lower() in seen:
            raise ValueError('Duplicate container filename.')
        seen.add(name.lower())
        if offset > length - table_size or size > length - table_size - offset:
            raise ValueError('Container entry extends outside its partition.')
        if size:
            intervals.append((offset, offset + size))
        entries.append((name, start + table_size + offset, size))
    intervals.sort()
    if any(b[0] < a[1] for a, b in zip(intervals, intervals[1:])):
        raise ValueError('Overlapping container entries.')
    return entries


def package_entries(stream, extension, size):
    if extension == '.nsp':
        return partition(stream, 0, size, b'PFS0')
    if size < 0x200 or read_at(stream, 0x100, 4) != b'HEAD':
        raise ValueError('Invalid XCI header; compressed or split images are unsupported.')
    root = struct.unpack('<Q', read_at(stream, 0x130, 8))[0]
    if root < 0x200 or root > size - 16:
        raise ValueError('Invalid XCI root partition offset.')
    entries = partition(stream, root, size - root, b'HFS0')
    secure = [e for e in entries if e[0].lower() == 'secure']
    if len(secure) != 1:
        raise ValueError('XCI must have exactly one secure partition.')
    _, offset, length = secure[0]
    return partition(stream, offset, length, b'HFS0')


def key_text(path):
    data = Path(path).read_bytes()
    if len(data) > MAX_TABLE:
        raise ValueError('Key file is too large.')
    try:
        return data.decode('utf-16' if data.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
    except UnicodeError as exc:
        raise ValueError('Unsupported key-file encoding. Save it as UTF-8 or UTF-16 with BOM.') from exc


def normalize_keyset(source, target, say=lambda _: None):
    text = key_text(source)
    match = re.search(r'^\s*header_key\s*=\s*([0-9a-fA-F]+)\s*(?:#.*)?$', text, re.MULTILINE | re.IGNORECASE)
    if not match or len(match[1]) != 64:
        raise ValueError('prod.keys must contain a 64-hex-character header_key to read encrypted NCA headers. Check the key file selected in the app.')
    # Some exported keysets contain malformed hardware derivation sources even
    # though they already include the final master keys. Never truncate a key.
    entries = dict(re.findall(r'^\s*([a-z0-9_]+)\s*=\s*([0-9a-fA-F]+)\s*(?:#.*)?$',
                              text, re.MULTILINE | re.IGNORECASE))
    entries = {name.lower(): value for name, value in entries.items()}
    lines, skipped = [], 0
    for line in text.splitlines():
        assignment = re.fullmatch(r'\s*((?:mariko_)?master_kek_source_([0-9a-fA-F]{2}))\s*=\s*([^#]*?)(?:#.*)?',
                                  line, re.IGNORECASE)
        if assignment:
            name, generation, value = assignment.groups()
            if not re.fullmatch(r'[0-9a-fA-F]{32}', value.strip()):
                master = entries.get('master_key_' + generation.lower(), '')
                if not re.fullmatch(r'[0-9a-fA-F]{32}', master):
                    raise ValueError('Invalid %s: expected 32 hex digits and no valid derived master key is available.' % name.lower())
                skipped += 1
                continue
        lines.append(line)
    if skipped:
        say('Ignoring %d malformed, redundant derivation sources; using supplied derived master keys. Original key file is unchanged.' % skipped)
    target.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
    target.chmod(0o600)
    return target


def hactool_failure(output, code, inspection):
    # Only static descriptions are exposed. Do not print raw output: warnings
    # from the upstream key parser can include an entire supplied key value.
    lower = output.lower()
    phase = 'NCA header inspection' if inspection else 'NCA extraction'
    status = code & 0xffffffff
    if status in {0xc0000005, 0xc0000409, 0xc0000374}:
        reason = 'native extractor crashed; the NCA or its encryption/section format could not be processed'
    elif 'key (' in lower and 'hex digits' in lower:
        reason = 'a key entry has an invalid length or non-hexadecimal characters; check the selected key file'
    elif 'invalid nca header' in lower:
        reason = 'cannot decrypt/recognize the NCA header; check header_key and whether the dump is intact'
    elif 'failed to read nca header' in lower:
        reason = 'NCA header is truncated or unreadable'
    elif 'failed to read pfs0' in lower or 'pfs0 header size' in lower or 'npdm too big' in lower:
        reason = 'ExeFS/PFS0 could not be decrypted or read; check the matching title key and dump'
    elif 'romfs' in lower and ('failed' in lower or 'unable' in lower):
        reason = 'RomFS could not be read; an update may need the matching base game'
    elif 'failed to open' in lower or 'failed to write' in lower or 'permission denied' in lower:
        reason = 'extractor could not open/read/write a required file; check permissions and free disk space'
    elif 'unrecognized option' in lower or 'unknown option' in lower:
        reason = 'the selected extractor does not support the required hactool arguments'
    else:
        reason = 'extractor returned an unclassified failure; raw output remains hidden to protect keys'
    return '%s failed (exit 0x%08X): %s.' % (phase, status, reason)


def run_hactool(tool, keys, archive, options, title_key=None, encrypted_title_key=False):
    args = [str(tool), '--disablekeywarns', '--suppresskeys', '-k', str(keys), '-t', 'nca']
    inspection = options == ['-i']
    env = dict(os.environ)
    env.pop('SWR_HACTOOL_HEADER_ONLY', None)
    if inspection:
        env['SWR_HACTOOL_HEADER_ONLY'] = '1'
    if title_key:
        args.append(('--titlekey=' if encrypted_title_key else '--contentkey=') + title_key)
    args.extend(options)
    args.append(str(archive))
    # Never echo tool output or its command: both can contain cryptographic keys.
    with tempfile.TemporaryFile() as log:
        try:
            result = subprocess.run(args, stdin=subprocess.DEVNULL, stdout=log,
                                    stderr=subprocess.STDOUT, timeout=1800, check=False, env=env,
                                    creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise ValueError('Cannot run hactool, or extraction exceeded 30 minutes.') from exc
        log.seek(0)
        output = log.read(1024 * 1024).decode('utf-8', errors='replace')
    if result.returncode:
        raise ValueError(hactool_failure(output, result.returncode, inspection))
    return output


def read_title_keys(path):
    result = {}
    if not path:
        return result
    path = Path(path).expanduser()
    if not path.is_file() or path.stat().st_size > MAX_TABLE:
        raise ValueError('title.keys is missing or too large.')
    for line in key_text(path).splitlines():
        line = line.split('#', 1)[0].strip()
        if not line:
            continue
        match = re.fullmatch(r'([0-9a-fA-F]{32})\s*=\s*([0-9a-fA-F]{32})', line)
        if not match:
            raise ValueError('Invalid title.keys entry; expected rights_id = decrypted_title_key.')
        identity, value = match[1].lower(), match[2].lower()
        if identity in result and result[identity] != value:
            raise ValueError('Conflicting duplicate rights IDs in title.keys.')
        result[identity] = value
    return result


def read_ticket(data):
    """Read bounded common-ticket fields; no signature/authenticity claim."""
    if len(data) < 4 or len(data) > 0x10000:
        raise ValueError('Invalid ticket size.')
    # Switch tickets serialize the signature type little-endian (nxdumptool).
    signature = int.from_bytes(data[:4], 'little')
    offsets = {0x10000: 0x240, 0x10001: 0x140, 0x10002: 0x80,
               0x10003: 0x240, 0x10004: 0x140, 0x10005: 0x80, 0x10006: 0x40}
    base = offsets.get(signature)
    if base is None:
        raise ValueError('Unsupported ticket signature type (expected Switch little-endian signature identifier).')
    # TikCommonBlock is 0x180 bytes. RSA-2048 common tickets total 0x2c0,
    # not 0x140 + 0x2c0; optional section records can follow this block.
    if len(data) < base + 0x180:
        raise ValueError('Truncated ticket: %d bytes; this signature requires at least %d.' % (len(data), base + 0x180))
    if data[base + 0x140] != 2:
        raise ValueError('Unsupported ticket version.')
    kind = data[base + 0x141]
    if kind not in (0, 1):
        raise ValueError('Unsupported ticket title-key type.')
    rights = data[base + 0x160:base + 0x170].hex()
    if not int(rights, 16):
        raise ValueError('Ticket contains an empty rights ID.')
    # Personalized RSA-wrapped keys cannot be used as common AES keys.
    return rights, data[base + 0x40:base + 0x50].hex() if kind == 0 else None


def add_ticket(tickets, data):
    rights, key = read_ticket(data)
    if rights in tickets and tickets[rights] != key:
        raise ValueError('Conflicting tickets for the same rights ID.')
    tickets[rights] = key


@contextmanager
def import_game(source, *, hactool=None, keys=None, title_keys=None, ticket=None, program_nca=None, say=print):
    source = Path(source).expanduser().resolve()
    extension = source.suffix.lower()
    if source.is_dir() or extension == '.nro':
        yield str(source)
        return
    if extension not in PACKAGE_EXTENSIONS:
        raise ValueError('Use an extracted folder, .nro, .nsp, .xci or .nca. Compressed/split packages are unsupported.')
    if not source.is_file():
        raise ValueError('Package does not exist.')
    bundled = Path(__file__).resolve().parent / 'tools/hactool' / ('hactool.exe' if os.name == 'nt' else 'hactool')
    tool = hactool or os.environ.get('SWR_HACTOOL') or (str(bundled) if bundled.is_file() else None) or shutil.which('hactool')
    keys = keys or os.environ.get('SWR_PROD_KEYS') or str(Path.home() / '.switch' / 'prod.keys')
    keys = Path(keys).expanduser().resolve()
    if not tool or not Path(tool).is_file() or not keys.is_file():
        raise ValueError('Package import requires hactool and your prod.keys. Release builds include hactool; select prod.keys in the app.')
    title_keys = read_title_keys(title_keys or os.environ.get('SWR_TITLE_KEYS'))
    say('Loaded %d title.keys entries; matching is checked against the selected Program NCA.' % len(title_keys))
    tickets = {}
    skipped_tickets = 0
    if ticket:
        path = Path(ticket).expanduser()
        if not path.is_file() or path.stat().st_size > 0x10000:
            raise ValueError('Selected ticket is missing or too large.')
        add_ticket(tickets, path.read_bytes())
    say('Importing package (original file is read-only)...')
    with tempfile.TemporaryDirectory(prefix='switchrecomp-import-') as temp:
        temp = Path(temp)
        keys = normalize_keyset(keys, temp / '__keyset.keys', say)
        archives = []
        if extension == '.nca':
            archives = [source]
        else:
            with source.open('rb') as stream:
                all_entries = package_entries(stream, extension, source.stat().st_size)
                for name, offset, size in all_entries:
                    if name.lower().endswith('.tik'):
                        if size > 0x10000:
                            skipped_tickets += 1
                            say('Warning: oversized embedded ticket skipped; using another matching ticket or title.keys if available.')
                            continue
                        try:
                            rights, key = read_ticket(read_at(stream, offset, size))
                        except ValueError as exc:
                            skipped_tickets += 1
                            say('Warning: embedded ticket skipped: %s' % exc)
                            continue
                        if rights in tickets and tickets[rights] != key:
                            raise ValueError('Conflicting tickets for the same rights ID.')
                        tickets[rights] = key
                say('Found %d ticket rights IDs in selected package/ticket.' % len(tickets))
                entries = [e for e in all_entries if e[0].lower().endswith('.nca')]
                if not entries:
                    raise ValueError('No NCA archives found in the package.')
                needed = sum(e[2] for e in entries) + 2 * max(e[2] for e in entries)
                if shutil.disk_usage(temp).free < needed:
                    raise ValueError('Not enough free space for package extraction.')
                for name, offset, size in entries:
                    dest = temp / name
                    stream.seek(offset)
                    with dest.open('xb') as output:
                        remaining = size
                        while remaining:
                            chunk = stream.read(min(1024 * 1024, remaining))
                            if not chunk:
                                raise ValueError('Truncated NCA data.')
                            output.write(chunk)
                            remaining -= len(chunk)
                    archives.append(dest)
        if program_nca:
            archives = [archive for archive in archives if archive.name.lower() == program_nca.lower()]
        candidates = []
        for index, archive in enumerate(archives, 1):
            say('Inspecting NCA %d/%d (header only)...' % (index, len(archives)))
            info = run_hactool(tool, keys, archive, ['-i'])
            if not re.search(r'Content Type:\s*Program\b', info, re.IGNORECASE):
                continue
            rights = re.search(r'Rights ID:\s*([0-9a-fA-F]{32})', info)
            if not rights:
                raise ValueError('Extractor did not return a readable Program NCA rights ID; use the bundled hactool.')
            title_key, encrypted = None, False
            if rights and int(rights[1], 16):
                title_key = title_keys.get(rights[1].lower())
                if title_key is None:
                    identity = rights[1].lower()
                    title_key = tickets.get(identity)
                    if title_key is None:
                        detail = ('The matching ticket is personalized; export its decrypted title key from your console.'
                                  if identity in tickets else 'No matching common ticket was found in the package or selected .tik.')
                        raise ValueError('Program NCA requires its decrypted title key, but none of the %d loaded title.keys entries matches its rights ID. %s Skipped %d unreadable embedded ticket(s). Select the title.keys for this game or its own common .tik; renaming another entry does not supply the missing key.' % (len(title_keys), detail, skipped_tickets))
                    encrypted = True
                    say('Matching common ticket found; using its encrypted title key with prod.keys.')
                else:
                    say('Matching decrypted title key found in selected title.keys.')
            candidates.append((archive, title_key, encrypted))
        if not candidates:
            raise ValueError('No readable Program NCA found. Check keys/extractor support and --program-nca. Updates/DLC need an extracted, merged base game.')
        if len(candidates) != 1:
            raise ValueError('Expected one Program NCA, found %d. For multi-title packages select --program-nca NAME. Updates/DLC require an extracted, merged base game.' % len(candidates))
        archive, title_key, encrypted = candidates[0]
        game = temp / 'extracted'
        game.mkdir()
        run_hactool(tool, keys, archive, ['--exefsdir=' + str(game / 'exefs'),
                                        '--romfsdir=' + str(game / 'romfs')], title_key,
                    **({'encrypted_title_key': True} if encrypted else {}))
        main = game / 'exefs' / 'main'
        if not main.is_file() or read_at_file(main) != b'NSO0':
            raise ValueError('No decrypted NSO main found. Check keys; update-only/DLC packages are not standalone games.')
        if not (game / 'romfs').is_dir() or not any((game / 'romfs').iterdir()):
            raise ValueError('No extracted RomFS found. Update-only packages need a merged base dump.')
        say('Package imported; compiling does not guarantee runtime compatibility.')
        yield str(game)


def read_at_file(path):
    with path.open('rb') as stream:
        return stream.read(4)
