# SwitchRecompz

A development fork of [SwitchRecomp by MZ Labs](https://github.com/Mythrax-ZS/SwitchRecomp), with fork additions maintained by [katakishi0](https://github.com/katakishi0).

SwitchRecomp translates Nintendo Switch ARM64 code to C ahead of time and compiles it into native host code. A separate runtime supplies the system services, graphics, audio and input that the translated program needs. The Windows desktop app also has an experimental Android arm64 APK export target.

> **Work in progress.** This is an experimental recompiler and runtime. Importing or compiling a title successfully does not establish gameplay compatibility. Bring your own game dumps and keys; no games, keys or firmware are included.

**Contents:** [Progress](#progress) · [How it works](#how-it-works) · [Architecture](#architecture) · [Desktop app](#desktop-app) · [Build caches](#build-caches) · [Download](#download) · [Getting started](#getting-started) · [Building](#building) · [Documentation](#documentation) · [Acknowledgements](#acknowledgements) · [License](#license)

## Progress

### Implemented features

The table describes code and tools present in this fork. It is not a list of games confirmed playable.

| Area | Current implementation | Practical limit |
| --- | --- | --- |
| CPU translation | ARM64 analysis, C generation, parallel compilation and native module linking | Unsupported instructions and undiscovered code paths can still prevent execution |
| Interpreter fallback | Generated from the same instruction templates as the recompiler; runtime hints can feed a later recompile | Does not fix missing instruction semantics or services |
| Package import | Experimental NSP, XCI and standalone Program NCA input through bundled hactool; NRO homebrew input is also available | Compressed or split dumps and automatic update/DLC combining are not supported |
| Key selection | Separate product-key, title-key and optional common-ticket selectors; full rights-ID matching | A selected key file is not proof that it contains the correct key for a title |
| Verification | Import, decryption and module-loading checks without C generation, compilation or launch | Does not test gameplay or establish ticket-signature authenticity |
| Graphics | OpenGL plus experimental ANGLE Vulkan and DirectX 11 paths | DirectX is a limited GLES 3.0 compatibility profile; all paths need per-game validation |
| Audio | PCM/ADPCM decoding, mixing and resampling; Windows waveOut and Android AAudio | Some effects are approximated; Android output needs device tests |
| Video | H.264 jobs decoded on the CPU through OpenH264 | Not a claim of complete NVDEC or codec compatibility |
| Input | Keyboard and XInput controller support on Windows | Controller mappings and game behavior still need testing |
| Desktop app | Rust/egui interface, background jobs, progress log, library and settings | A completed build may still fail at runtime |
| Updates | Manual GitHub release check with a link to the fork's release page | Does not download, install or replace the app automatically |
| Build reuse | Local object reuse plus CI caches for dependencies and native tools | Cache reuse depends on matching inputs and cache availability |

### Validation and compatibility

Synthetic tests cover package admission, key/ticket handling, update metadata and the imported-input build path. The build-pipeline fixture generates a small ARM64 program, translates and compiles it, then checks that the headless native runtime exits successfully. On Windows, that fixture exercises both bundled compilers when available.

The Windows workflow also runs GUI formatting, lint and unit tests, recompiler smoke tests, and a smoke test after unpacking the single-file application. These checks catch regressions in the build and packaging path; they do not measure commercial-game compatibility.

User testing has shown an NSP progressing through import and ARM64-to-C translation. That progress alone is not a confirmed gameplay result. There is no verified commercial-game compatibility or FPS table published here yet.

### Platform status

| Target | Status |
| --- | --- |
| Windows x64 | Current build and release target |
| Windows OpenGL | Current graphics backend; requires OpenGL 4.3 or newer |
| Windows Vulkan | Experimental ANGLE backend; hardware requires GLES 3.2 |
| Windows DirectX 11 | Experimental ANGLE GLES 3.0 subset; hardware and WARP options |
| Android arm64 | SDK/NDK APK export from the desktop app; physical-device validation pending |

Vulkan and DirectX reuse the GLES renderer through ANGLE. DirectX 11 now negotiates GLES 3.2/3.1 when supported (enabling compute/SSBO/MRT paths) and otherwise falls back to the limited GLES 3.0 profile; full game support still requires validation. Android exports a game-specific APK from the PC; SDK/NDK/JDK can be selected from existing installations or downloaded on demand after opting in. See [PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md) for the tested boundary and [ANDROID.md](ANDROID.md) for setup.

## How it works

Recompilation happens before the program runs:

1. **Import and check.** The app passes the selected input and key paths to the Python pipeline. Package inputs use the bundled extractor.
2. **Load modules.** The loader reads executable modules, decompresses them where needed, and resolves the metadata used for analysis.
3. **Analyze ARM64.** The analyzer finds functions and control-flow blocks using unwind information, symbols, relocation targets, branch targets and any previous execution hints.
4. **Generate C.** Guest functions become C functions. Output is divided into chunks so the C compiler can process several chunks at once.
5. **Compile and link.** GCC, TCC or Android NDK Clang compiles the generated code into a native module. The runtime and translated module form the runnable build.
6. **Run through the runtime.** The runtime starts the translated entry point and handles guest memory, threads, service calls, graphics, sound and input.

**Static CPU recompilation does not remove the runtime.** The guest program still expects Switch behavior from system services and the GPU. Missing or inaccurate runtime behavior can therefore break a title even when every C chunk compiles.

The fallback interpreter handles execution that reaches code outside the statically generated entry points. Recorded hints can help the analyzer discover that code on the next recompile. An unsupported instruction can still stop execution.

## Architecture

### Recompiler and application

| Component | Responsibility | Source |
| --- | --- | --- |
| Desktop interface | Input/key selection, build settings, library, progress and launch actions | [gui/src/app.rs](gui/src/app.rs), [gui/src/library.rs](gui/src/library.rs) |
| Job runner | Starts Python in the background and streams progress and diagnostics to the interface | [gui/src/job.rs](gui/src/job.rs) |
| Bundled payload | Unpacks the embedded tools and runtime into a local cache | [gui/src/payload.rs](gui/src/payload.rs), [ci/stage-payload.ps1](ci/stage-payload.ps1) |
| Import and verification | Validates supported inputs, matches keys/tickets and coordinates hactool | [package_import.py](package_import.py) |
| Executable loader | Reads NSO/NRO modules, decompression, relocation and module metadata | [nxloader.py](nxloader.py) |
| ARM64 semantics | Instruction templates shared by static translation and interpreter generation | [a64.py](a64.py) |
| Analysis and C generation | Function discovery, control flow, generated functions and interpreter | [codegen.py](codegen.py) |
| Build driver | Orchestrates analysis, generation, compiler jobs, object reuse and linking | [switchrecomp.py](switchrecomp.py) |
| Release checker | Fetches bounded release metadata and compares the installed development build | [update_check.py](update_check.py), [gui/src/updates.rs](gui/src/updates.rs) |

### Native runtime

The translated code calls a runtime API rather than implementing the console's operating environment itself. The runtime loads the translated module, initializes guest state and dispatches guest calls to its own implementations.

| Subsystem | What it does | Source |
| --- | --- | --- |
| Startup and dispatch | Loads the native module, checks its interface and starts guest execution | [runtime/main.c](runtime/main.c), [runtime/entry.c](runtime/entry.c) |
| CPU helpers | Guest register state and operations needed by generated C | [runtime/cpu.h](runtime/cpu.h), [runtime/cpu_helpers.c](runtime/cpu_helpers.c) |
| Memory and kernel | Guest memory, threads, synchronization and core service calls | [runtime/mem.c](runtime/mem.c), [runtime/kernel.c](runtime/kernel.c), [runtime/svc_core.c](runtime/svc_core.c) |
| IPC and services | Dispatches guest service requests to high-level implementations | [runtime/ipc.c](runtime/ipc.c) |
| GPU command processing | Handles guest GPU commands and connects them to rendering | [runtime/gpu.c](runtime/gpu.c), [runtime/gpu_engines.c](runtime/gpu_engines.c), [runtime/nvdrv.c](runtime/nvdrv.c) |
| OpenGL rendering | Host GPU resources, draws and queued OpenGL calls | [runtime/gpu_gl.c](runtime/gpu_gl.c), [runtime/glq.c](runtime/glq.c) |
| Shader translation | Translates guest GPU instructions to GLSL for the host driver | [runtime/shader.c](runtime/shader.c) |
| Sound and video | Guest audio mixing/output and the H.264 decoding path | [runtime/audio.c](runtime/audio.c), [runtime/host1x.c](runtime/host1x.c) |
| Windows integration | Window creation, WGL or EGL/ANGLE context, keyboard and XInput | [runtime/backend_win32.c](runtime/backend_win32.c), [runtime/angle_win32.c](runtime/angle_win32.c) |
| Android export | NDK arm64 runtime/module compilation, APK packaging and debug signing | [android_build.py](android_build.py), [runtime/backend_android.c](runtime/backend_android.c) |
| Headless backend | Runs synthetic fixtures without a graphics window | [runtime/backend_headless.c](runtime/backend_headless.c) |

The CPU recompiler and GPU shader translator are separate systems. Generating native CPU code does not precompile every graphics shader or guarantee correct rendering.

## Desktop app

The interface groups the workflow into input, keys and output/build settings. Long-running work runs in a child process, with phase and progress information shown in the app.

- **Input and keys:** choose the input, `prod.keys`, `title.keys` and an optional external common ticket. The bundled extractor is detected automatically; an override is available for development.
- **Verification:** use **Verify game and keys** before a long build. It runs the import and loader checks without starting compilation.
- **Output target:** choose Windows or Android APK. Windows offers OpenGL, experimental Vulkan and limited DirectX 11/WARP choices. Android uses NDK Clang and exposes SDK/NDK folder selectors or an optional verified automatic tool download.
- **Compiler options:** choose TCC for quick iteration or GCC for optimized output. GCC exposes optimization level and an optional host-CPU optimization choice. A build optimized for one CPU may not run on another.
- **Library:** launch completed builds, open their settings or logs, and delete their build cache when space is needed.
- **Updates:** **Check for updates** compares the installed development build with this fork's published development release and opens GitHub. Updating remains a manual step.

Key-file paths are remembered in local settings. Key contents are not stored in those settings or printed in the job log. Extractor failures use categorized diagnostics because raw output can contain key material.

## Build caches

There are two different kinds of build reuse:

| Cache | What it saves | When it helps |
| --- | --- | --- |
| Local recompilation | Unchanged generated C keeps its timestamp; matching compiled objects can be reused | Rebuilding the same title with compatible compiler settings |
| GitHub Actions | Verified tool downloads, compiled runtimes/hactool and Rust build dependencies | Subsequent CI runs with matching cache inputs |

Local object names include a signature derived from the compiler command, flags and shared CPU headers, keeping different compiler configurations separate. Objects are replaced only after compilation succeeds.

A local rebuild can still repeat import, analysis and C generation. The object cache reduces C compilation work; it does not skip the entire pipeline. Changing code, compiler options or shared headers may require recompilation. Clearing the build cache makes a later build compile those objects again.

CI native caches are keyed by the relevant source and build inputs. The workflow validates required outputs after a restore. Cache availability is managed by GitHub and is not a guarantee that every run will be faster.

## Download

| Windows x64 | Link |
| --- | --- |
| Latest release | [SwitchRecomp.exe](https://github.com/kuaan08/Switchrecompz/releases/latest/download/SwitchRecomp.exe) |
| Development build | [SwitchRecomp.exe — development](https://github.com/kuaan08/Switchrecompz/releases/download/dev/SwitchRecomp.exe) |
| Build results | [GitHub Actions](https://github.com/kuaan08/Switchrecompz/actions) |

Release notes and SHA-256 checksums are on the [Releases page](https://github.com/kuaan08/Switchrecompz/releases). The development package is replaced only after the Windows build succeeds.

The single-file package includes the app, recompiler, runtimes, Python, hactool, TCC, GCC and ANGLE graphics libraries. Android SDK/NDK/JDK are not embedded; the optional Android tool setup downloads them to a persistent private cache. First launch unpacks the embedded payload under `%LOCALAPPDATA%\SwitchRecomp`.

## Getting started

Requirements: Windows 10/11 x64, a GPU/driver suitable for the selected graphics API (OpenGL 4.3 by default), and enough free storage and memory for the title's generated code and compilation. Large titles can generate substantial amounts of C; there is no single tested hardware minimum for every title.

1. Launch the app and select your own input.
2. Select `prod.keys` and, when required, a matching `title.keys` entry or common ticket.
3. Run **Verify game and keys** and resolve any reported issue.
4. Choose Windows or Android APK, set the output name and parallel-job count, then start recompilation.
5. Launch a Windows build from the library, or reveal/install the exported APK on an Android arm64 device. Check the runtime log if it fails.

More parallel jobs can increase memory use. TCC favors compile speed; GCC takes longer to compile but can produce faster code. Neither compiler choice establishes compatibility.

Compressed `.nsz`/`.xcz`, split dumps and automatic update/DLC combining are not supported. Multiple Program NCAs require an explicit CLI selection; there is no selection dialog for that case. See [PACKAGE_IMPORT.md](PACKAGE_IMPORT.md) for the full input limits.

### Reporting problems

Open an issue in [this fork](https://github.com/kuaan08/Switchrecompz/issues) with the build/commit, Windows version, CPU/GPU and driver, selected compiler, failing phase and relevant diagnostic. Distinguish import, translation, compilation and runtime failures: they involve different parts of the project.

Do not attach keys or game dumps. Review logs before sharing them; selected local paths can appear. Please report fork-specific problems here rather than to upstream.

## Building

Use Windows, PowerShell 7, Rust and the Windows build tools required by eframe. From the repository root:

```powershell
# Fetch the pinned, checksummed toolchains
./ci/toolchains.ps1
python ci/fetch_angle.py

# Build the pinned extractor
git clone https://github.com/SciresM/hactool.git .ci-build/hactool
git -C .ci-build/hactool checkout 1d64a83450e025622f3468c28fc4164dad2c5ef6
./python/python.exe ci/build_hactool.py --source .ci-build/hactool

# Build graphical and headless runtimes with GCC
./python/python.exe switchrecomp.py --build-runtime --cc w64devkit/bin/gcc.exe
./python/python.exe switchrecomp.py --build-runtime --backend headless --cc w64devkit/bin/gcc.exe

# Run Python and recompiler checks
./python/python.exe -m unittest discover -s tests -v
./ci/smoke-test.ps1 -Root .

# Check the desktop app
cargo fmt --manifest-path gui/Cargo.toml --check
cargo clippy --manifest-path gui/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path gui/Cargo.toml --locked

# Embed the payload and build the single-file app
./ci/stage-payload.ps1 -Out dist/payload
$env:SWR_PAYLOAD_DIR = (Resolve-Path dist/payload).Path
cargo build --release --locked --manifest-path gui/Cargo.toml
```

The result is `gui/target/release/SwitchRecomp.exe`. TCC is available for translated code, but the runtime itself requires GCC because parts of it are C++.

### Continuous integration

The [Build workflow](.github/workflows/build.yml) builds and tests Windows x64, probes ANGLE D3D11 WARP and Vulkan/SwiftShader, and builds a synthetic Android arm64 APK. Android build checks do not replace device tests. Pushes to `main`/`master` publish the development release after successful checks; version tags publish versioned releases. Pull requests produce build artifacts without publishing a release. The workflow can also be started manually.

## Documentation

| Document | Contents |
| --- | --- |
| [Package import](PACKAGE_IMPORT.md) | Supported inputs, key/ticket setup, verification and import limits |
| [Platform support](PLATFORM_SUPPORT.md) | Graphics integration, constraints and validation boundary |
| [Android export](ANDROID.md) | SDK/NDK setup, APK generation, signing and limitations |
| [Code review](CODE_REVIEW.md) | Review findings, regression fixes and validation history |
| [Build workflow](.github/workflows/build.yml) | Toolchain setup, checks, caches, packaging and release publication |

## Acknowledgements

- **[SwitchRecomp by MZ Labs](https://github.com/Mythrax-ZS/SwitchRecomp):** original recompiler, runtime and desktop application.
- **[katakishi0](https://github.com/katakishi0):** fork additions and maintenance. Original SwitchRecomp authorship remains with MZ Labs.
- **[kuaan08](https://github.com/kuaan08):** hosting this fork at [Switchrecompz](https://github.com/kuaan08/Switchrecompz).

The project also depends on third-party work:

| Component | Role | License |
| --- | --- | --- |
| [ANGLE](https://github.com/google/angle), Vulkan loader and SwiftShader | GLES translation to Vulkan/DirectX; software graphics diagnostics | BSD / Apache 2.0 and component-specific notices |
| [hactool](https://github.com/SciresM/hactool) and its vendored mbed TLS | Package import and cryptographic operations | ISC / Apache 2.0 |
| [OpenH264](https://github.com/cisco/openh264) | H.264 decoding | BSD 2-clause |
| [SPIRV-Cross](https://github.com/KhronosGroup/SPIRV-Cross) | Shader tooling used by the optional Windows LSFG integration | Apache 2.0 |
| texture2ddecoder | BCn texture decoding | MIT |
| TCC | Fast C compilation | LGPL |
| Python | Recompiler and tooling | PSF |
| [w64devkit](https://github.com/skeeto/w64devkit) | GCC, binutils and MinGW-w64 toolchain | GPL, LGPL and component-specific licenses |
| egui / eframe | Desktop interface | MIT / Apache 2.0 |

Third-party components retain their own licenses and notices. This is an independent community fork, not affiliated with Nintendo or endorsed by MZ Labs.

## License

SwitchRecomp code is licensed under the **Apache License 2.0**. Retain the attribution **SwitchRecomp by MZ Labs**, [LICENSE.txt](LICENSE.txt) and [NOTICE.txt](NOTICE.txt) in copies and derived work. The project license does not replace the licenses of bundled third-party components.
