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
- 14 synthetic importer/container/key-handling tests passed.
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
