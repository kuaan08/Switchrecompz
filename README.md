# SwitchRecompz

A Windows-focused development fork of [SwitchRecomp by MZ Labs](https://github.com/MZLabs/SwitchRecomp), maintained by [katakishi0](https://github.com/katakishi0).

SwitchRecomp translates supported Nintendo Switch ARM64 code to C and builds a native Windows program. Compatibility depends on the game and the current runtime; a successful build does not guarantee that a game will run correctly.

> **Work in progress.** This fork is experimental. Import and verification support are still being tested, and game compatibility varies. Bring your own legally obtained game dump and keys. This repository and its releases contain no games, keys or firmware.

**Contents:** [Features](#features) · [Project status](#project-status) · [Download](#download) · [Getting started](#getting-started) · [Build from source](#build-from-source) · [Credits and license](#credits-and-license)

## Features

- Windows desktop interface for selecting an input, keys, compiler and output name.
- Experimental import support for `.nsp`, `.xci` and `.nca`, alongside `.nro` homebrew.
- hactool is included in the Windows release; a separate download is not normally needed.
- Separate selectors for your `prod.keys`, `title.keys` and optional ticket.
- **Verify game and keys** checks import, decryption and module loading before compilation.
- GCC for optimized builds and TCC for quicker development builds.
- Build cache to reduce work when recompiling the same title.

Import support has limits. Compressed or split dumps are not supported, and updates or DLC are not automatically combined. See [PACKAGE_IMPORT.md](PACKAGE_IMPORT.md) for the current details and key setup.

## Project status

| Area | Status |
| --- | --- |
| Windows x64 app | Available through GitHub Actions releases |
| Graphics | Current runtime uses OpenGL |
| NSP / XCI / NCA import | Experimental; use the verification option first |
| Vulkan and DirectX | Not implemented as working renderer choices |
| Android | Not available as a build or installable app |

Vulkan, DirectX and Android are research and development goals, not supported options in the current release. See [PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md) for the implementation review.

## Download

| Windows x64 | Link |
| --- | --- |
| Latest release | [Download SwitchRecomp.exe](https://github.com/kuaan08/Switchrecompz/releases/latest/download/SwitchRecomp.exe) |
| Development build | [Download the latest development build](https://github.com/kuaan08/Switchrecompz/releases/download/dev/SwitchRecomp.exe) |
| Build status | [GitHub Actions](https://github.com/kuaan08/Switchrecompz/actions) |

The development build is published from the latest successful push to the default branch. Release notes and checksums are listed on the [Releases page](https://github.com/kuaan08/Switchrecompz/releases).

On first launch, the app unpacks its bundled tools under `%LOCALAPPDATA%\SwitchRecomp`. The package includes the app, recompiler, Python, hactool, TCC and GCC.

## Getting started

1. Launch SwitchRecompz and select your own `prod.keys`. Select `title.keys` as well when the title needs a decrypted title key.
2. Choose your own `.nsp`, `.xci`, `.nca` or `.nro` input in the app.
3. Select **Verify game and keys** and resolve any reported issue before building.
4. Choose GCC for an optimized build or TCC for a faster compile, set the output name, and start recompilation.
5. Use the library to open a completed build.

Key files remain on your computer. The application remembers selected paths, not key contents. Handle key files privately; do not include them in bug reports.

See [PACKAGE_IMPORT.md](PACKAGE_IMPORT.md) for supported inputs, verification details and troubleshooting. The upstream [SwitchRecomp documentation](https://github.com/MZLabs/SwitchRecomp) covers the original recompilation workflow and runtime.

## Build from source

The GitHub Actions workflow builds the Windows x64 application and runs the available checks. To build locally, use Windows with PowerShell 7, Rust and the required Windows build tools:

```powershell
./ci/toolchains.ps1
python\python.exe ci/build_hactool.py --source C:\path\to\hactool
cargo test --manifest-path gui/Cargo.toml
cargo build --release --manifest-path gui/Cargo.toml
```

The hactool source must match the pinned revision described in [PACKAGE_IMPORT.md](PACKAGE_IMPORT.md). See the [Build workflow](.github/workflows/build.yml) for the complete release process.

## Credits and license

This fork builds on [SwitchRecomp by MZ Labs](https://github.com/MZLabs/SwitchRecomp). The upstream project and its contributors retain their original authorship. Fork additions and maintenance are credited to [katakishi0](https://github.com/katakishi0); the repository is hosted by [kuaan08](https://github.com/kuaan08).

SwitchRecomp is distributed under the Apache License 2.0. Keep the upstream license and attribution notices with copies and derived versions. See [LICENSE.txt](LICENSE.txt) and [NOTICE.txt](NOTICE.txt).

This is an independent community fork and is not affiliated with Nintendo or endorsed by MZ Labs.
