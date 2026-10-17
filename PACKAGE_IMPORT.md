# Package import development fork

Based on **SwitchRecomp by MZ Labs**, upstream commit
`5c61944c395f072a699e081c704d616afe10332a`.
Apache-2.0 license and upstream NOTICE are retained.

This change adds a local import stage before the existing ARM64-to-C recompiler.
It does not add game compatibility to the CPU, GPU or services runtime.

## Input support

| Input | Implementation | Limits |
| --- | --- | --- |
| Extracted `exefs/romfs` | Existing loader | Same compatibility as upstream |
| `.nro` | Existing homebrew loader | Same compatibility as upstream |
| `.nsp` | Bounded PFS0 parser, streamed NCA extraction, external hactool | Base game, one Program NCA by default |
| `.xci` | HEAD/root HFS0/secure HFS0 parser, streamed NCA extraction, external hactool | Uncompressed, unsplit image |
| `.nca` | External hactool extracts ExeFS and RomFS | Standalone Program NCA only |
| `.nsz`, `.xcz`, split dumps | Explicit rejection | Decompress/merge your own dump first |
| Updates and DLC | Not applied automatically | Supply a correctly merged, extracted base dump |
| Multiple Program NCAs | Refused unless selected | CLI `--program-nca FILENAME`; no selection dialog yet |

Only container parsing and the import orchestration have been tested with
synthetic fixtures. No commercial game, encrypted NCA, real XCI or hactool binary
was supplied for an end-to-end decryption/gameplay test. These import paths must
be treated as experimental until tested with your own dumps.

## Setup and GUI

Build the modified application using the upstream build instructions. The
upstream release EXE does not include these changes. In the modified GUI:

1. Open **Package import (.nsp / .xci / .nca)**.
2. Select a trusted local [hactool](https://github.com/SciresM/hactool) executable.
   hactoolnet is a different tool and is not a drop-in replacement here.
3. Select your own `prod.keys`. Rights-managed titles also need your own
   `title.keys` with **decrypted** title keys, `rights_id = key` (32 hex characters
   on each side). Tickets inside NSPs are not automatically imported.
4. Select a game file using **Game file…**, or drag it onto the window.
5. Select the compiler and choose **Recompile**. Import failures are shown in the
   job log. A recognized container header is not a compatibility confirmation.

The tool is not bundled and no keys, games or firmware are downloaded. Settings
store only paths, not the key contents. Keys are not copied into game output.
The extractor version and key generation support can limit which dumps work.

## Command line

```text
python switchrecomp.py "MyGame.nsp" --hactool "C:\tools\hactool.exe" --keys "C:\keys\prod.keys" --title-keys "C:\keys\title.keys" --cc gcc --outdir games
python3 switchrecomp.py MyGame.xci --hactool /opt/hactool --keys /private/prod.keys --cc gcc --outdir games
python3 switchrecomp.py MyGame.nca --hactool /opt/hactool --keys /private/prod.keys --no-build
```

Optional environment defaults: `SWR_HACTOOL`, `SWR_PROD_KEYS`, `SWR_TITLE_KEYS`.
`hactool` on PATH and `~/.switch/prod.keys` are also detected.
All key files stay local. The decrypted title key, when needed, is supplied to
hactool via its `--contentkey` argument. Local process inspection by the same
user or an administrator can see that argument. Do not share process traces.

## Validation and disk use

The importer rejects unsafe/duplicate filenames, excessive tables, truncation,
entries outside partitions and overlapping file ranges. NSP/XCI tables are
bounded to 16 MiB and 4096 entries. NCA bytes are copied in 1 MiB chunks. The
original source is opened read-only. Free space is checked before container
extraction; large games also need space for generated C and final game data.

Temporary NCA/game directories are removed on normal completion and Python
exceptions. Force-killing the app or process tree may leave temporary files.
There is no extraction cache in this first implementation.
External tool output is captured instead of printed because hactool can print
cryptographic keys. A tool error is intentionally summarized without its raw
output. Child processes are invoked with argument lists, without a shell.

## Tests

```text
python3 -m unittest discover -s tests -v
python3 ci/make_test_nro.py /tmp/hello.nro
python3 switchrecomp.py /tmp/hello.nro -o hello --outdir /tmp/check --backend headless --cc gcc --jobs 2
cd /tmp/check
./hello
```

The import unit tests use synthetic containers and a mocked extractor. The
homebrew smoke test compiles and executes the existing runtime without keys or
copyrighted game assets. Rust GUI compilation, formatting, lint and unit tests
must run in CI; this development environment has no Rust toolchain.
