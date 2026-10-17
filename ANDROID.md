# Android APK export (experimental)

The Windows desktop app can compile a local title into an Android arm64 APK.
The phone runs it through NativeActivity, GLES 3.2, touch/gamepad input and AAudio.
Game compatibility needs device testing.

## Tools on the PC

Install a JDK with java/keytool on PATH, Android SDK tools and NDK r27c:

```text
sdkmanager "platforms;android-35" "build-tools;35.0.0" "ndk;27.2.12479018"
```

Select Android APK in the output panel and browse to SDK/NDK folders. Empty
paths use ANDROID_HOME / ANDROID_SDK_ROOT, ANDROID_NDK_HOME, or an installed NDK
inside the SDK. The usual Windows Android Studio SDK location is also detected.
SDK/NDK/JDK are not embedded in the single-file app.

## Export

Select input and keys normally, verify, choose Android APK, then recompile.
Android uses NDK Clang; the Windows TCC/GCC choice does not apply. Reveal the
completed APK in its folder and install on an arm64 phone with GLES 3.2.

```text
python android_build.py MyGame.nsp --keys C:\keys\prod.keys --title-keys C:\keys\title.keys --sdk C:\Android\Sdk --ndk C:\Android\Sdk\ndk\27.2.12479018 --outdir games -o MyGame --jobs 4
```

The program/runtime are arm64 libraries. Only selected title inputs enter the
APK; key files do not. Original inputs are unchanged. First launch requires
additional private storage for unpacked inputs; matching cached inputs are
reused later. PC compilation also needs substantial storage and memory.

## Signing

Exports are debug builds. A local debug keystore is created and reused at
`%LOCALAPPDATA%/SwitchRecomp/android/debug.keystore` on Windows (or
`$XDG_DATA_HOME/SwitchRecomp/android/debug.keystore`, defaulting to
`~/.local/share/SwitchRecomp/android/debug.keystore`, for source builds).
This location survives app updates. Keep it to install later exports over the
same app. A different signing key requires uninstalling the old APK, removing
its private saves. No signing key is uploaded. Production signing is not
implemented.

## Validation and limits

CI builds a synthetic homebrew APK, verifies its signature, and checks arm64 ABI,
non-executable stack and 16 KiB native load alignment. This does not establish
installation or execution on a physical phone. Use adb logcat with the
SwitchRecomp tag for startup reports.

Android H.264 decoding is not enabled. Complete 16 KiB host-page behavior has
not been validated, and lifecycle/input/audio require device tests. There is no
confirmed commercial-game compatibility list for this target.
Single-APK export is currently limited to packages below 2 GiB and 60000 assets;
external/split asset delivery is not implemented.
