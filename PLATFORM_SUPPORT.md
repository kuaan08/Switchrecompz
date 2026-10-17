# Rendering and Android development status

This fork is maintained by katakishi0 and is based on SwitchRecomp by MZ Labs.
This document records the implementation review for the next platform work.

| Target | Present in this source | Missing before claiming support |
| --- | --- | --- |
| Windows OpenGL | Win32 context, GL call queue, guest shader-to-GLSL translator, GPU resources and draw path | Per-game compatibility testing |
| Windows Vulkan | No Vulkan renderer wired to the runtime | Resource/pipeline implementation, descriptors, barriers, shader generation, presentation and tests |
| Windows DirectX | No D3D renderer wired to the runtime | D3D device/resources, shader path, state translation, synchronization, presentation and tests |
| Android GLES | NativeActivity backend, EGL ES 3.2, touch overlay, Android filesystem and loader branches | NDK toolchain/build rules, arm64 libraries, APK packaging, lifecycle/input/storage tests and on-device game validation |

## Renderer review

`runtime/gpu_gl.c`, `runtime/shader.c`, `runtime/gl_load.c` and the GL queue
implement the current graphics path. A window/device selector cannot substitute
for this renderer. The software fallback in `gpu_3d.c` only implements clears.

N64Recomp separates CPU recompilation from the runtime. RT64 is useful as a design
reference for Vulkan/D3D abstractions, resource lifetime, synchronization and
presentation. Its N64 graphics model is not a replacement for Switch Maxwell/NVN
command decoding and guest shader translation. No third-party renderer code has
been copied into this fork.

ANGLE is a candidate to investigate for reusing the GLES path on Vulkan. Its
published support matrix has ES 3.2 support on Vulkan, while D3D11 support is more
limited. This runtime emits `#version 320 es` in the GLES shader path; feature and
extension coverage must be tested before selecting ANGLE as a supported renderer.
A native D3D12 backend would require its own implementation or a suitable
cross-API rendering layer, plus translated shaders and synchronization tests.

Next implementation gate: isolate the existing renderer's device, resource and
presentation calls behind a backend contract and exercise clear/upload/draw/
readback/synchronization fixtures. Keep OpenGL available until the new path passes
those fixtures and real-game comparisons.

## Android review

`backend_android.c` already exports `ANativeActivity_onCreate`, creates an EGL
context and loads assets. `main.c` expects a recompiled Android `.so` or a static
module. The current desktop CLI's Linux executable link rules are not an Android
APK builder. The generated C must be compiled for the same Android ABI as the
runtime; a Windows DLL cannot be placed in an arm64-v8a APK.

Next implementation gate: build a synthetic homebrew and runtime with the NDK for
arm64-v8a, package/sign a minimal NativeActivity APK, and validate storage access,
activity recreation, surface loss, touch/gamepad input and exit on a device. An
Android source file alone does not establish this gate. No Android APK is
published by the Windows-only workflow.

## Primary references consulted

- https://github.com/N64Recomp/N64Recomp
- https://github.com/rt64/rt64
- https://github.com/google/angle (README support matrix)
- https://github.com/google/angle/blob/main/doc/DevSetup.md
- https://developer.android.com/ndk/guides/abis

Vulkan, DirectX and Android remain development work. They are not exposed as
working graphics/output choices in this release.
