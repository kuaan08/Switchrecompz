"""Exercise real Maxwell translation with synthetic partial position exports."""
import ctypes
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


def position_shader(components):
    # Four-word ALD from generic input 0, followed by an XYZ or XYZW AST.
    # Each 32-byte instruction group begins with a scheduling control word.
    words = [0] * 14
    words[10] = 0
    words[11] = 0xEFD87F800807FF00 | (3 << 47)
    words[12] = 0xEFF07F800707FF00 | ((components - 1) << 47)
    words[13] = 0xE30000000007000F  # EXIT
    return struct.pack('<' + 'Q' * len(words), *words)


class ShaderTranslationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cc = ROOT / 'w64devkit/bin/gcc.exe' if os.name == 'nt' else None
        cc = str(cc) if cc and cc.is_file() else shutil.which('gcc')
        if not cc:
            raise unittest.SkipTest('GCC is required for the native shader translator')
        cls.tmp = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.tmp.cleanup)
        root = Path(cls.tmp.name)
        wrapper = root / 'wrapper.c'
        wrapper.write_text('''#include "shader.h"
#include <stdlib.h>
int g_texoff_emul = 0;
#ifdef _WIN32
__declspec(dllexport)
#endif
char* translate_test(const u8* code, u32 size, int stage, int es, int* unsupported) {
    ShaderInfo si = {0};
    extern int g_glsl_es;
    g_glsl_es = es;
    char* result = shader_to_glsl(code, size, stage, &si);
    *unsupported = si.unsupported;
    return result;
}
#ifdef _WIN32
__declspec(dllexport)
#endif
void free_test(char* text) { free(text); }
''')
        lib = root / ('translator.dll' if os.name == 'nt' else 'translator.so')
        flags = [] if os.name == 'nt' else ['-fPIC']
        subprocess.run([cc, '-shared', '-O2', *flags, '-I', str(ROOT / 'runtime'),
                        str(wrapper), str(ROOT / 'runtime/shader.c'), '-o', str(lib)],
                       check=True, capture_output=True)
        cls.lib = ctypes.CDLL(str(lib))
        if os.name == 'nt':
            free_library = ctypes.WinDLL('kernel32', use_last_error=True).FreeLibrary
            free_library.argtypes = [ctypes.c_void_p]
            cls.addClassCleanup(free_library, cls.lib._handle)
        cls.lib.translate_test.argtypes = [ctypes.c_void_p, ctypes.c_uint32,
                                           ctypes.c_int, ctypes.c_int,
                                           ctypes.POINTER(ctypes.c_int)]
        cls.lib.translate_test.restype = ctypes.c_void_p
        cls.lib.free_test.argtypes = [ctypes.c_void_p]

    def translate(self, components, es=0):
        ctypes.c_int.in_dll(self.lib, 'g_glsl_es_version').value = 320
        code = position_shader(components)
        buf = ctypes.create_string_buffer(code)
        unsupported = ctypes.c_int()
        ptr = self.lib.translate_test(buf, len(code), 0, es, ctypes.byref(unsupported))
        self.assertTrue(ptr)
        try:
            text = ctypes.string_at(ptr).decode()
        finally:
            self.lib.free_test(ptr)
        self.assertEqual(unsupported.value, 0)
        return text

    def test_xyz_export_has_defined_homogeneous_w(self):
        for es in (0, 1):
            with self.subTest(es=es):
                text = self.translate(3, es)
                default = 'gl_Position = vec4(0.0, 0.0, 0.0, 1.0);'
                self.assertIn(default, text)
                self.assertNotIn('gl_Position.w =', text)
                for component in 'xyz':
                    self.assertLess(text.index(default), text.index('gl_Position.' + component + ' ='))

    def test_explicit_w_export_overrides_default(self):
        text = self.translate(4)
        self.assertLess(text.index('gl_Position = vec4('),
                        text.index('gl_Position.w = uintBitsToFloat(r3);'))

    def test_gles_300_vertex_profile(self):
        code = position_shader(3)
        buf = ctypes.create_string_buffer(code)
        unsupported = ctypes.c_int()
        ctypes.c_int.in_dll(self.lib, 'g_glsl_es_version').value = 300
        ptr = self.lib.translate_test(buf, len(code), 0, 1, ctypes.byref(unsupported))
        self.assertTrue(ptr)
        try:
            text = ctypes.string_at(ptr).decode()
        finally:
            self.lib.free_test(ptr)
        self.assertTrue(text.startswith('#version 300 es'))
        self.assertNotIn('samplerCubeArray', text)
        self.assertEqual(unsupported.value, 0)


if __name__ == '__main__':
    unittest.main()
