"""Check the synthetic APK's ABI, library alignment and NativeActivity payload."""
from pathlib import Path
import struct
import sys
import zipfile


def verify(path):
    with zipfile.ZipFile(path) as apk:
        for name in ('lib/arm64-v8a/libgame_code.so', 'lib/arm64-v8a/libswr_runtime.so'):
            data = apk.read(name)
            if data[:6] != b'\x7fELF\x02\x01' or struct.unpack_from('<H', data, 18)[0] != 183:
                raise ValueError('APK library is not arm64: ' + name)
            off = struct.unpack_from('<Q', data, 32)[0]
            size, count = struct.unpack_from('<HH', data, 54)
            for index in range(count):
                entry = off + index * size
                kind, flags = struct.unpack_from('<II', data, entry)
                if kind == 1:
                    alignment = struct.unpack_from('<Q', data, entry + 48)[0]
                    if alignment < 16384:
                        raise ValueError('Native library lacks 16 KiB load alignment')
                if kind == 0x6474e551 and flags & 1:
                    raise ValueError('Executable native stack is not allowed')
        if apk.read('assets/switchrecomp/name.txt').strip() != b'game':
            raise ValueError('Native module name mismatch')
        names = apk.read('assets/switchrecomp/assets.txt').decode().splitlines()
        if not names or not all('assets/game/' + name in apk.namelist() for name in names):
            raise ValueError('Incomplete game asset manifest')
    print('PASS: arm64 APK payload and 16 KiB native load alignment')


if __name__ == '__main__':
    verify(Path(sys.argv[1]))
