# Focused source review — 2026-10-17

Scope: package admission, subprocess calls, key handling, build packaging,
selected filesystem/runtime code, and regression testing. This is a focused
review, not a full security audit or antivirus analysis of a new Windows binary.

## Findings and actions

1. **Original GUI rejected packages.** Added package choices, header checks,
   local extractor/key path settings, and integration with the actual Python
   import stage. Header recognition is explicitly separate from content/game
   validation.
2. **Archive paths and sizes need validation.** Added a bounded PFS0/HFS0 parser
   with flat filename validation (including Windows device names and alternate
   data stream characters), case-insensitive collision detection, bounds and
   overlap checks. No package filenames are used as shell commands.
3. **Extractor output can disclose keys.** hactool source `nca.c` prints title
   keys in its info output. Import captures and hides that output. Key contents
   are not echoed into the GUI job log. Paths remain visible in settings/logs.
4. **Title-key argument semantics matter.** hactool `--titlekey` expects an
   encrypted key, whereas this importer's `title.keys` accepts decrypted keys.
   The importer passes decrypted keys via hactool `--contentkey`, confirmed
   against upstream `nca.c`. The value is visible to local process inspection;
   no claim of secret process arguments is made.
5. **Single-file app payload must contain the new module.** Added
   `package_import.py` to `ci/stage-payload.ps1`. Upstream EXEs are unchanged.
6. **Temporary data lifetime matters.** A context manager keeps extraction data
   available until the compiler has copied game files, then cleans up. Tests
   cover cleanup on consumer exceptions and verify that original input bytes
   are preserved. Forced process termination may leave temporary directories.
7. **Do not mix multiple games or patches silently.** Require exactly one
   Program NCA unless explicitly selected. Reject missing executable/RomFS.
   Updates/DLC require an already merged extracted dump in this version.
8. **Runtime is not a security sandbox.** Existing `runtime/svc_fs.c` has a
   `path_escapes` check for `..` and colons, but this review does not establish
   confinement of all filesystem or memory accesses. Symlink/reparse-point
   behavior and generated native-code execution require a deeper audit.
   Use trusted dumps and a non-administrator account.
9. **Source control protection.** Ignore common ROM/key/ticket files and local
   path settings. This is an accidental-commit guard, not encryption or a full
   secret scanner.

## Verified locally

- Python syntax compilation passed.
- 15 synthetic importer/container/key-handling tests passed (including bundled-tool discovery).
- Existing synthetic homebrew NRO translated and compiled with GCC.
- Its Linux headless native program executed and logged `The game exited (code 0)`.
- `git diff --check` passed.

## Not verified

- Windows executable build, Rust GUI compilation, rustfmt and clippy (Rust is
  unavailable locally). Existing CI gates remain enabled.
- End-to-end hactool decryption of an encrypted NCA, or real commercial game
  compatibility; no relevant keys/game dumps were supplied.
- Compressed/split input, automatic ticket import, DLC or update merging.
- Full CPU/GPU/runtime security audit, dependency audit or binary antivirus scan.

## Sources inspected

- SwitchRecomp upstream source at commit `5c61944c395f072a699e081c704d616afe10332a`.
- https://github.com/SciresM/hactool/blob/master/README.md
- https://github.com/SciresM/hactool/blob/master/nca.c

The source package retains upstream LICENSE and NOTICE. No keys or game data are
included. Target repository: https://github.com/kuaan08/Switchrecompz.

## Bundled extractor and CI follow-up

The first Windows workflow stopped at rustfmt, before compiling the GUI. Corrected
those formatting changes. The hactool dependency is now built from pinned official
source, smoke-tested using a harmless PFS0 archive, and packaged with ISC and mbed
TLS licenses. The GUI exposes product-key and game selectors on the main screen;
the included extractor is detected automatically, with an optional override.
The native hactool build/PFS0 extraction test passed locally on Linux. Windows GUI
and complete single-file packaging still require the updated CI result.

## Header inspection / Windows key encoding follow-up

A reported NSP import failed inside hactool but the old message concealed all
useful diagnostics. The exact failure in that user dump is not known from that
message alone. Fixed an independent flaw in the discovery path: upstream hactool
can parse sections during `-i` before our importer chooses the title key. A
bounded fork patch adds header-only discovery, validated with a synthetic NCA
whose sections must not be parsed. Invalid headers now return a failing status.
Also added key-file encoding normalization and validated the required header key
format. Diagnostics use static categories plus exit code, never raw secret-bearing
output. Eighteen Python tests and the native Linux extractor smoke tests passed;
commercial-game decryption still needs the user's local retest.

## Malformed redundant derivation-source follow-up

Reproduced a native key-parser exit before NCA inspection using a privately supplied
keyset. Some indexed master KEK source entries have 34 hex digits instead of 32.
The importer now omits such an entry only when a syntactically valid corresponding
derived master key exists, reports the omitted count, and leaves the original file
unchanged. It never truncates or invents key material. Missing replacements fail
with a key-name-only diagnostic. No supplied values are included in this repository.
Nineteen Python tests pass. A synthetic plaintext NCA header probe with the
normalized supplied keyset passes the native extractor; this verifies parser
progress, not commercial-game decryption or the correctness of the supplied keys.

## Rights-managed import and validation review

Reviewed package parsing, title-key matching, hactool argument semantics, GUI
settings/pickers, worker lifecycle, CLI compile entry and nxloader module-loading
checks for the reported failure. Existing runtime/translation sources remain an
experimental compatibility implementation; this is not a full security audit of
the entire vendored codebase or proof of commercial-game compatibility.

The previous importer discarded embedded tickets and reported an absent match as
though no title.keys file had been selected. Common tickets are now parsed with
bounds and exact rights-ID matching, with encrypted and decrypted keys kept
distinct. Personalized tickets and conflicts stop with actionable static errors.
Missing matches report entry counts without logging key material. GUI selectors
for product keys, title keys and optional external tickets are directly visible.
Verification uses the normal importer and module loader without compilation.
Synthetic tests cover signature layouts, truncation, common/personalized ticket
behavior, matching, precedence, duplicates and correct hactool arguments.

## Native Switch ticket layout regression

Corrected two incorrect assumptions in our first ticket reader and its synthetic
fixtures: the signature type is little-endian for Switch tickets, and the common
data block is 0x180 bytes. An RSA-2048 common ticket is 0x2c0 bytes total, including
the 0x140-byte signature block. The previous reader incorrectly required 0x400
bytes and read the signature identifier big-endian. Checked these fields against
nxdumptool include/core/tik.h (TikCommonBlock and TikSigRsa2048 size assertions)
and include/core/signature.h (signatureGetTypeFromSignedBlob, called with false
by tikGetCommonBlockFromSignedTicketBlob).

Regression tests now use little-endian 0x10004 / a 0x2c0-byte signed common ticket,
check fixed wire offsets, and reject truncated layouts. Unreadable embedded
tickets generate static diagnostics and are skipped so that a valid selected
title.keys entry or another matching ticket can still work. An explicitly chosen
invalid external ticket and conflicting tickets still fail. No guess is made for
unknown signature types, missing fields or personalized key material.
Twenty-seven Python tests pass; the user's NSP is unavailable locally.
Reference: https://github.com/DarkMatterCore/nxdumptool/tree/rewrite/include/core

## Imported game data copy and fork UI

The user's successful import and ARM64 translation exposed a later data-copy bug:
main passed the original NSP path to copy_data, so find_exefs returned None.
The importer already kept its temporary folder alive for the full build; main now
passes that imported folder and copies data before expensive analysis/translation.
An absent ExeFS produces a clear ValueError. A synthetic NSO package-import
fixture exercises real translation, data copy, linking and a headless runtime
execution. CI runs this fixture with both bundled Windows compilers.

Fork credits identify katakishi0 as maintainer of fork additions while retaining
MZ Labs authorship, LICENSE and NOTICE. The interface groups game/key/output steps
and uses a teal accent. A manual update action invokes bundled Python, fetches
only the public fork release metadata over HTTPS with a timeout/size limit, checks
fixed release/asset URLs, compares development commit IDs and opens a release
link. It does not install or execute downloaded files. Offline/API errors remain
visible and retryable. Platform renderer/Android findings are in PLATFORM_SUPPORT.md.

## Partial vertex-position exports — 2026-10-17

A user report for Arcade Archives CLU CLU LAND included a frame trace, generated
GLSL and the original vertex-shader instruction stream. The captured program
exports position XYZ and texture-coordinate XY, but never exports position W.
The old translator consequently emitted a GLSL vertex shader with undefined W.
The report shows Intel HD Graphics Gen11/OpenGL 4.6 and no untranslated or failed
shaders for that traced draw. Its log identifies the native module as TCC-built,
regardless of the compiler choice recalled in the report.

Initialize vertex position to (0, 0, 0, 1) before guest stores. Explicit guest W
exports continue to override the default, including stores on conditional paths.
This addresses a concrete undefined-output defect; the original game's complete
rendering on the user's Intel driver has not been retested here.

Bump the shader warm-up pack path from v3 to v4 so stale translated GLSL is not
precompiled. Native shader binary keys already include generated GLSL source, so
changed vertex source receives a new binary key without deleting unrelated cache
files. The user needs an updated runtime in the game's build; replacing only the
desktop app does not update an already generated game executable.

Validation: synthetic XYZ/XYZW Maxwell programs exercise the real C translator.
The XYZ regression fails against the old translator on desktop GLSL and GLES;
the new translator supplies W=1 and preserves an explicit W export. An offscreen
OpenGL 4.4 test on Mesa llvmpipe rendered a synthetic triangle using translated
XYZ-only output: the center pixel was black before initialization and red after.
All 34 Python tests passed locally. No uploaded game shaders, captures, logs, keys
or game content are committed. Windows packaging remains subject to CI, and this
is not a claim of complete game compatibility.

## Experimental ANGLE graphics and Android export — 2026-10-17

Added explicit Win32 EGL/ANGLE display selection for Vulkan, D3D11 and D3D11 WARP.
The loader does not mix WGL functions into an active GLES context. D3D11 uses a
restricted GLSL ES 3.0 profile with matching varying names and explicit uniform
bindings; compute/storage resources and multiple render targets are unsupported.
Vulkan requests GLES 3.2. Shader warm-up packs now distinguish desktop and GLES
profiles. GLES buffer readback maps actual data instead of returning zeros.

Pinned ANGLE binaries come from official Chromium snapshot 1400034. Archive and
component SHA-256 checks are required; libraries and third-party license notices
are staged into the Windows release. The standalone graphics probe uses synthetic
instructions/assets only, and checks texture/UBO rendering plus pixel/buffer
readback and presentation on WARP and Vulkan/SwiftShader. Software Vulkan CI uses
GLES 3.1, so it does not validate the complete hardware GLES 3.2 runtime.

Android export compiles generated code and the runtime for arm64 with the NDK,
packages NativeActivity and local inputs, aligns native load segments to 16 KiB,
and signs/verifies a debug APK. SDK/NDK/JDK are external requirements. Only title
inputs and licenses enter the APK; key paths/contents and the debug signing key
do not. Native asset extraction runs outside the UI thread, checks manifest paths
and caches inputs by content in private storage. AAudio replaces the previously
silent Android audio stub. No broad-storage or root permission is requested.

Locally, 39 Python tests pass, the Rust interface passes cargo check/Clippy, and a
synthetic Android APK builds, verifies its v2/v3 signature and passes arm64/native
alignment checks. Physical-device audio/input/lifecycle, game compatibility and
complete 16 KiB host-page behavior remain unverified. Android H.264 decoding is
not enabled. See PLATFORM_SUPPORT.md and ANDROID.md for the supported boundary.
