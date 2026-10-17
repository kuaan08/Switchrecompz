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
| `.nsp` | Bounded PFS0 parser, streamed NCA extraction, included hactool | Base game, one Program NCA by default |
| `.xci` | HEAD/root HFS0/secure HFS0 parser, streamed NCA extraction, included hactool | Uncompressed, unsplit image |
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

1. Click **Select prod.keys…** on the main screen and select your own product-key file.
2. Click **Open NSP / XCI / NCA / NRO…** and select your own game dump, or drag it onto the app.
3. Choose the compiler and press **Recompile**. The included hactool is selected automatically.
4. Rights-managed games also need your own decrypted `title.keys`; select it under
   **Advanced import options / title.keys**. Each entry is `rights_id = key`,
   with 32 hexadecimal characters on each side. Tickets are not imported automatically.

Release builds include hactool, built from the official SciresM repository at
`1d64a83450e025622f3468c28fc4164dad2c5ef6`, plus its vendored mbed TLS and licenses.
No separately downloaded hactool executable is needed. For local source builds,
check out that exact source revision and run
`python3 ci/build_hactool.py --source /path/to/hactool` (after toolchain setup on Windows).
The CI builds and smoke-tests the extractor, then embeds it in the application's
single-file payload. That payload is unpacked locally at startup, like GCC/Python.

The advanced extractor path is an optional override for development. hactoolnet
has different arguments and is not a compatible override. Keys, games and firmware
are never included. Settings remember key-file paths, not key contents.
The extractor and key generation support can limit which dumps work.

## Command line

```text
python switchrecomp.py "MyGame.nsp" --hactool "C:\tools\hactool.exe" --keys "C:\keys\prod.keys" --title-keys "C:\keys\title.keys" --cc gcc --outdir games
python3 switchrecomp.py MyGame.xci --hactool /opt/hactool --keys /private/prod.keys --cc gcc --outdir games
python3 switchrecomp.py MyGame.nca --hactool /opt/hactool --keys /private/prod.keys --no-build
```

Optional environment defaults: `SWR_HACTOOL`, `SWR_PROD_KEYS`, `SWR_TITLE_KEYS`.
The bundled extractor is preferred over PATH; `~/.switch/prod.keys` is also detected.
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

### Import diagnostics update

The bundled hactool has a small, documented fork patch for header-only NCA
inspection. Discovery reads content type/Rights ID without parsing encrypted
sections; only the selected Program NCA is extracted with its matching title key.
The extractor also receives `--suppresskeys`. Failed subprocesses report a static
error category, phase and exit code; raw key-bearing output is never printed.

UTF-8 with/without BOM and UTF-16 BOM key files are accepted. `prod.keys` must
contain a 64-hex-character `header_key`. A UTF-8 normalized copy is created only
inside the private temporary import directory and removed with that directory.
No key material is copied to game output or sent to GitHub.

Malformed indexed master KEK derivation sources are omitted from the temporary
keyset only if the corresponding valid-length derived master key is supplied.
The log reports the count. Values are never truncated, and the original file is
unchanged. Otherwise import stops with an entry-name-only format diagnostic.
