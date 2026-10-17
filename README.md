# SwitchRecomp — Package Import Development

Based on **SwitchRecomp by MZ Labs**. This development source adds experimental
`.nsp`, `.xci` and `.nca` import through the included hactool and your own
keys. See [PACKAGE_IMPORT.md](PACKAGE_IMPORT.md) for setup, supported inputs and
limitations, and [CODE_REVIEW.md](CODE_REVIEW.md) for review/test results.

**Fork:** https://github.com/kuaan08/Switchrecompz

Downloads below refer to this repository. A development executable is available
only after its GitHub Actions build succeeds; see the Actions tab for status.

Static recompiler for Nintendo Switch games: ARM64 → C → native.

Same idea as GBRecomp and PSXRecomp: the game **code** (exefs) is translated to C
once and compiled into a native Windows program. The game **data** (romfs: models,
textures, music) is not compiled; the game reads it as is from its `_data` folder.

Use only your own copy of the game. No games, keys or firmware are included.

**SwitchRecomp by MZ Labs.** Licensed under the Apache License 2.0
([LICENSE.txt](LICENSE.txt)): free to use, modify and share, also commercially, as
long as you credit "SwitchRecomp by MZ Labs" and keep [NOTICE.txt](NOTICE.txt) in
any copy, fork or derived work.

## Download

| | Latest release | Development build |
| --- | --- | --- |
| Windows | [SwitchRecomp.exe](https://github.com/kuaan08/Switchrecompz/releases/latest/download/SwitchRecomp.exe) | [SwitchRecomp.exe](https://github.com/kuaan08/Switchrecompz/releases/download/dev/SwitchRecomp.exe) |
| Linux (x86-64) | [SwitchRecomp-linux-x86_64.tar.gz](https://github.com/kuaan08/Switchrecompz/releases/latest/download/SwitchRecomp-linux-x86_64.tar.gz) | [SwitchRecomp-linux-x86_64.tar.gz](https://github.com/kuaan08/Switchrecompz/releases/download/dev/SwitchRecomp-linux-x86_64.tar.gz) |

The development build is rebuilt from every change to `master`. Every version, with
its notes and SHA-256 checksums, is on the
[Releases](https://github.com/kuaan08/Switchrecompz/releases) page.

**Windows:** the exe is a single file with everything inside: the app, the
recompiler, the runtimes, Python, TCC and GCC. On its first start it unpacks those
tools (about 430 MB) to `%LOCALAPPDATA%\SwitchRecomp`, which takes a few seconds.

**Linux:** the archive holds one program, `SwitchRecomp`, with the app, the
recompiler, the runtimes and TCC inside; it unpacks them to
`~/.local/share/SwitchRecomp` on its first start. Python 3.9 or newer and GCC come
from your distribution, for example:

```
sudo apt install python3 gcc            # Debian, Ubuntu, Mint
sudo dnf install python3 gcc            # Fedora
sudo pacman -S python gcc               # Arch
tar -xzf SwitchRecomp-linux-x86_64.tar.gz
./SwitchRecomp
```

Your games and settings are stored next to the program.

## Recompile a game

1. Dump the game from your own console and extract it with hactool and your own keys:

   ```
   hactool -k prod.keys --exefsdir=exefs --romfsdir=romfs game.nca
   ```

   With an update, extract the combined romfs:

   ```
   hactool -k prod.keys --basenca=base.nca --exefsdir=exefs --romfsdir=romfs update.nca
   ```

   You end up with a folder that has `exefs\` and `romfs\` inside.

2. Open SwitchRecomp and pick that folder (or drop it on the window). A
   homebrew `.nro` file works too. The app checks the folder right away and shows
   the game's name and title ID.

3. Choose the compiler (on Linux, GCC is the system's):
   - **TCC** compiles in seconds, but the game runs slower.
   - **GCC** takes minutes, but the game runs several times faster.

4. Press **Recompile**, then **Play**. Finished games are listed in the **Library**
   tab, where you can play them, change their settings, open their log and clear
   their build cache.

A big game takes several minutes with GCC the first time. Later runs only recompile
what changed. Recompiling after a game update also refreshes the copied exefs and
romfs.

### Output

Games are written to the `games\` folder:

| File | Contents |
| --- | --- |
| `MyGame.exe` (Linux: `MyGame`) | the runtime (kernel, services, GPU → OpenGL, video) |
| `MyGame_code.dll` (Linux: `MyGame_code.so`) | the game code translated to x86-64 |
| `MyGame_data\` | exefs, romfs and save data |
| `MyGame.ini` | settings |
| `MyGame_build\` | generated C and objects, only used to speed up the next recompile |

The runtime and the code library must come from the same game version. If you
update the game, recompile.

A game recompiled on Linux runs on other Linux PCs without any compiler, so you can
recompile it on a desktop and copy the folder to a Steam Deck.

### Command line

```
python\python.exe switchrecomp.py C:\path\to\game -o MyGame --outdir games
python\python.exe switchrecomp.py homebrew.nro
python3 switchrecomp.py ~/dumps/game -o MyGame --outdir games        # Linux
```

| Option | Meaning |
| --- | --- |
| `-o NAME` | output name |
| `--outdir DIR` | output folder (default: the current folder) |
| `--cc gcc` / `--cc tcc` | compiler; a name on `PATH`, in `gcc\` / `tcc\`, or a full path |
| `-O 0..3` | GCC optimization level (default 2) |
| `--native` | optimize for this CPU (the game may not start on other PCs) |
| `--jobs N` | files compiled at the same time |
| `--build-runtime` | build `bin\runtime_<win64/linux>_<backend>` with GCC and exit |

## Play

Requirements: a GPU with OpenGL 4.3 or newer (keep the driver up to date), and

- **Windows** 10/11 64-bit, or
- **Linux** x86-64 with an X11 or Wayland desktop (games use X11, through XWayland
  on Wayland) and glibc 2.35 or newer: Ubuntu 22.04, Debian 12, Fedora 36, SteamOS 3
  or later. Mesa 22+ or the NVIDIA driver. Controllers need SDL2
  (`libsdl2-2.0-0`, `SDL2`), which most desktops already have. Sound goes through
  SDL2, PulseAudio/PipeWire or ALSA, whichever is there.

The Windows build also works on Linux and the Steam Deck through Proton.

| Switch | Keyboard |
| --- | --- |
| D-Pad | Arrow keys |
| Left / right stick | W A S D / I J K L |
| A, B, X, Y | X or Space, Z, C, V |
| L, R, ZL, ZR | Q, E, 1, 3 |
| +, − | Enter, Backspace |

Controllers map by position, like the Switch controller: XInput (Xbox) ones on
Windows, anything SDL2 knows on Linux.

F1 help · F3 screenshot · F4 FPS limit 60 / unlimited · F5 diagnostic trace ·
F7 record .mp4 video (Windows) · F8 full-resolution PNG · F11 or Alt+Enter fullscreen · Esc quit

### Settings

Change them from the Library tab (**Settings**), or edit `MyGame.ini`. The game
rewrites the file on every start with all the available keys and their
descriptions. Most settings apply live while the game runs.

| Key | Meaning |
| --- | --- |
| `docked=1` | TV mode (1080p) instead of handheld (720p) |
| `resolution=1` | internal resolution scale (0.25 to 4) |
| `fps_limit=1` | 1 = 60 FPS like the console, 0 = unlimited |
| `shadows=1` | 0 skips shadow maps (faster on weak GPUs) |
| `volume=100` | game volume, 0 to 100 (0 = muted) |
| `lsfg=1` | frame generation with your own copy of Lossless Scaling (its `Lossless.dll` is read at run time, never included or copied); Windows only |
| `log=2` | log everything (for bug reports) |

If something goes wrong, send `MyGame.log` (next to the exe). The F5 trace goes to
the `captures\` folder, and `performance.txt` summarizes slow frames. For sound
problems, start the game with the environment variable `SWR_AUDIO_WAV` set to a file
name: everything the game plays is recorded there as a WAV file.

## Building from source

The git repository holds the sources only; the tools come from their official
downloads.

| Path | Contents |
| --- | --- |
| `switchrecomp.py`, `nxloader.py`, `codegen.py`, `a64.py` | the recompiler (Python 3) |
| `runtime\` | the C runtime linked into every game |
| `gui\` | the desktop app (Rust) |
| `ci\` | scripts that fetch the tools, test the recompiler and stage the single-file build |

**Windows**, in PowerShell 7 from the repository root:

```
./ci/toolchains.ps1        # python\, tcc\ and w64devkit\ (GCC), pinned and checksummed
python\python.exe switchrecomp.py --build-runtime --cc w64devkit\bin\gcc.exe
python\python.exe switchrecomp.py --build-runtime --backend headless --cc w64devkit\bin\gcc.exe
./ci/smoke-test.ps1 -Root .
```

**Linux**, from the repository root (Debian/Ubuntu package names):

```
sudo apt install python3 gcc g++ make curl nasm libx11-dev libgl-dev
./ci/toolchains.sh         # tcc/ and the OpenH264 decoder, built from pinned, checksummed sources
python3 switchrecomp.py --build-runtime
python3 switchrecomp.py --build-runtime --backend headless
pwsh ./ci/smoke-test.ps1 -Root .   # optional, needs PowerShell 7
```

That gives a working folder layout. Then build the app with Rust:

- **Folder build:** `cargo build --release` in `gui\`. The app looks for
  `switchrecomp.py` next to itself or in a parent folder, so it runs straight from
  `gui\target\release\`.
- **Single-file build:** `./ci/stage-payload.ps1 -Out dist/payload` (PowerShell 7,
  also on Linux), then in `gui\` set `SWR_PAYLOAD_DIR` to that folder and run
  `cargo build --release`. The build packs the folder into the program.

TCC can't build the runtime because parts of it are C++.

### Continuous integration

[.github/workflows/build.yml](.github/workflows/build.yml) runs those same steps for
Windows and Linux on every push and pull request, and smoke-tests the single-file
builds. Then it publishes both on the Releases page:

- **Every push to `master`** replaces the **Development build** (tag `dev`, marked
  as a pre-release). Its notes list the commits since the previous one.
- **A versioned release** comes from pushing a tag, or from running the workflow by
  hand (Actions → Build → Run workflow) with a version. GitHub writes the release
  notes from the changes since the last release. A version with a suffix, like
  `v2.0.0-rc.1`, is published as a pre-release.

  ```
  git tag v1.0.0
  git push origin v1.0.0
  ```

- **Pull requests** only build; the builds are attached to the run as artifacts.

To also announce versioned releases on Discord, add a repository secret named
`DISCORD_WEBHOOK` that holds a channel's webhook URL.

Linux builds on GitHub's Ubuntu 22.04 runners, so the binaries also run on older
distributions. Windows builds on GitHub's Windows runners. To build Windows on your
own machine instead, add a
Windows x64 [self-hosted runner](https://docs.github.com/actions/hosting-your-own-runners)
that has PowerShell 7 and rustup. Then either pick it when you start the workflow
by hand, or set the repository variable `SWR_RUNNER` to `self-hosted` (or a JSON
list of labels such as `["self-hosted", "windows"]`).

## Third-party components

| Component | License | Used for |
| --- | --- | --- |
| hactool (SciresM), pinned source | ISC | included NSP/XCI/NCA extraction |
| mbed TLS, vendored with hactool | Apache 2.0 / GPL 2.0 dual license | hactool crypto; license included |
| OpenH264 (Cisco) 2.4.1 | BSD 2-clause, `runtime\oh264\LICENSE` | game videos |
| SPIRV-Cross (Khronos) | Apache 2.0, `runtime\spvc\LICENSE-SPIRV-Cross.txt` | LSFG shaders (Windows) |
| texture2ddecoder | MIT, `runtime\bcn\LICENSE_texture2ddecoder.txt` | BCn textures |
| TCC 0.9.27 | LGPL, `tcc\COPYING.txt` | quick builds |
| Python 3.14 (embeddable) | PSF, `python\LICENSE.txt` | the recompiler (Windows) |
| [w64devkit](https://github.com/skeeto/w64devkit) 2.10: GCC 16, binutils, MinGW-w64 | GPL, LGPL and others; source in the release's `source.tar` | fast builds, the runtime (Windows) |
| SDL2 | zlib; not included, loaded from the system if installed | controllers (Linux) |
| egui / eframe | MIT or Apache 2.0 | the desktop app |

The Apache 2.0 license in LICENSE.txt covers only the SwitchRecomp code. Each
component above keeps its own license.
