"""Writes a tiny homebrew NRO for smoke tests: a call, a conditional branch, a few
SIMD instructions, then svcExitProcess. No game or key needed.

usage: make_test_nro.py OUT.nro
"""
import hashlib, struct, sys

TEXT, RO, DATA = 0x1000, 0x1000, 0x1000
img = bytearray(TEXT + RO + DATA)


def put(off, *words):
    for i, w in enumerate(words):
        struct.pack_into('<I', img, off + i * 4, w)


put(0x00, 0x14000020)                    # b 0x80 (over the header)
img[0x10:0x14] = b'NRO0'
struct.pack_into('<I', img, 0x18, len(img))
struct.pack_into('<IIIIII', img, 0x20, 0, TEXT, TEXT, RO, TEXT + RO, DATA)
struct.pack_into('<I', img, 0x38, 0)     # bss
img[0x40:0x60] = hashlib.sha256(b'swr smoke test').digest()   # build id

put(0x80,
    0x94000020,   # bl   0x100
    0x6F00E400,   # movi v0.2d, #0
    0x4E20D401,   # fadd v1.4s, v0.4s, v0.4s
    0xD40000E1,   # svc  #0x7 (ExitProcess)
    0x14000000)   # b    .
put(0x100,
    0xD2800020,   # movz x0, #1
    0x91001400,   # add  x0, x0, #5
    0xF100181F,   # cmp  x0, #6
    0x54000041,   # b.ne 0x114
    0x91000400,   # add  x0, x0, #1
    0xD65F03C0)   # ret

with open(sys.argv[1], 'wb') as f:
    f.write(img)
