# Graphics backends and Android export

SwitchRecomp by MZ Labs, with fork additions maintained by katakishi0.
These integrations are experimental and do not establish game compatibility.

| Target | Implementation | Validation boundary |
| --- | --- | --- |
| Windows OpenGL | Existing Win32/WGL renderer | Default; per-game correctness varies |
| Windows Vulkan | EGL/ANGLE context and the existing GLES renderer/shader path | Hardware needs ANGLE GLES 3.2; software CI probes GLES 3.1, not every hardware feature |
| Windows DirectX 11 | ANGLE GLES 3.0 compatibility profile, linked varyings, explicit sampler/UBO bindings | No compute/storage shaders or multiple render targets; limited shader/resource subset |
| DirectX 11 WARP | Same profile on Windows' software rasterizer | Diagnostic option, not a performance optimization |
| Android arm64 | NDK compilation, NativeActivity, GLES 3.2, touch/gamepad input, AAudio and signed APK export | Synthetic build/signature/ABI checks; device and game tests still needed |

## Windows

The runtime still decodes guest GPU commands and translates guest shaders.
ANGLE translates the resulting GLES operations to Vulkan or Direct3D 11.
These are API paths through a translation layer, not native rewrites of the
Maxwell renderer. Direct3D 12 is not implemented.

Choose the renderer in the build screen or game settings, then restart. INI
values: `opengl`, `vulkan`, `d3d11`, `d3d11-warp`. The CLI accepts `--renderer`.
The selected path never silently falls back to desktop OpenGL. Game builds
receive ANGLE DLLs in the adjacent `angle` directory.

D3D11 uses GLSL ES 3.0 because ANGLE D3D11 does not supply GLES 3.2. Its subset
covers basic vertex/fragment shaders, textures and constant buffers. Compute,
storage shaders and excess constant-buffer use are rejected; multiple render
targets stop with a diagnostic. Nonzero base vertex/instance and advanced
sampler/format behavior remain limitations. Unsupported shader operations may
fail compilation. Use Vulkan or OpenGL for titles needing these features.

Shader packs are separated by desktop/GLES profile; binary keys include GLSL
and driver identity. Incompatible packs are not reused across profiles.

## Dependency provenance and tests

`ci/fetch_angle.py` stages graphics libraries from official Chromium snapshot
**1400034**, checking both archive and individual SHA-256 digests. ANGLE revision:
`711b2c32641521c9e60a903b85c7367755c6448a`; SwiftShader revision:
`118f577b4188528566a1f23d88c4f0041372b83a`. No browser is bundled or launched.
`tools/angle/PROVENANCE.json` and license files accompany the libraries.
WARP is supplied by Windows.

The Windows probe translates a synthetic XYZ-only Maxwell shader, uploads a
texture/constant buffer, draws a triangle, reads the pixel and checks its color.
It also checks buffer readback and presentation, on D3D11 WARP and ANGLE
Vulkan/SwiftShader. These checks do not reproduce a retail GPU or game.

## Android

The desktop exports a game-specific APK; the phone runs the compiled title.
Choose Android APK and the installed SDK/NDK folders, or use `android_build.py`.
See [ANDROID.md](ANDROID.md).

The exporter reuses the importer/analyzer, compiles generated code and runtime
for arm64-v8a using NDK Clang, packages NativeActivity, and signs/verifies a debug
APK. Native ELF segments use 16 KiB alignment. No root or broad storage permission
is requested. Inputs unpack into private storage on a worker thread and are
cached by content. Saves/settings remain separate from packaged inputs.

AAudio supplies 48 kHz stereo output and stream reopening. Audio, surface
recreation, suspend/resume, touch/gamepad behavior, memory pressure and game
compatibility need physical-device tests. Android currently disables the
unavailable arm64 OpenH264 dependency, so H.264 video is not supported by this
build. ELF alignment alone does not validate 16 KiB host memory-page behavior.

## Primary references

- [ANGLE matrix](https://github.com/google/angle)
- [ANGLE platform extension](https://chromium.googlesource.com/angle/angle/+/main/extensions/EGL_ANGLE_platform_angle.txt)
- [ANGLE setup](https://chromium.googlesource.com/angle/angle/+/main/doc/DevSetup.md)
- [NDK build systems](https://developer.android.com/ndk/guides/other_build_systems)
- [NativeActivity](https://developer.android.com/ndk/reference/group/native-activity)
- [AAudio](https://developer.android.com/ndk/guides/audio/aaudio/aaudio)

Earlier architecture references included N64Recomp and RT64. No renderer code
from those projects was copied; their graphics models are not interchangeable
with this runtime's Switch GPU command handling.
