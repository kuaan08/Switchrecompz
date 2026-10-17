# Android APK export (experimental)

The Windows desktop app can compile a local title into an Android arm64 APK.
The phone runs it through NativeActivity, GLES 3.2, touch/gamepad input and AAudio.
Game compatibility needs device testing.

## Tools on the PC

On Windows, choose **Android APK** and enable **Download missing Android SDK,
NDK and JDK automatically** if you do not already have these tools. This is
opt-in because it transfers approximately 1.1 GB and requires several GB of
free space. The tools are downloaded from Google and Eclipse Adoptium,
checked against fixed SHA-256 digests, and installed without administrator
access into `%LOCALAPPDATA%/SwitchRecomp/android/toolchains`. This stable cache
survives updates of SwitchRecomp.exe. Android Studio itself is not required.
The app includes the Python exporter, verified graphics header and installer,
but not the large SDK/NDK/JDK archives. No automatic download occurs unless
this option is enabled. Android SDK/NDK and Temurin 17 remain subject to their
respective vendor licenses; please review them before enabling downloads:
[Android SDK terms](https://developer.android.com/studio/terms) and
[Temurin licenses](https://adoptium.net/temurin/licensing/).

Alternatively, install a JDK with java/keytool on PATH, Android SDK build tools
and NDK r27c yourself:

```text
sdkmanager "platforms;android-35" "build-tools;35.0.0" "ndk;27.2.12479018"
```

Select the SDK/NDK folders in the GUI or leave them blank to detect installed
tools (ANDROID_HOME / ANDROID_SDK_ROOT / ANDROID_NDK_HOME or Android Studio's
Windows SDK folder). An explicitly selected invalid folder is an error rather
than being silently replaced. Automatic downloads currently support Windows
x64 only; the existing Linux CI path installs tools separately.

## Export

Select input and keys normally, verify, choose Android APK, then recompile.
Android uses NDK Clang; the Windows TCC/GCC choice does not apply. Reveal the
completed APK in its folder and install on an arm64 phone with GLES 3.2.

```text
python android_build.py MyGame.nsp --keys C:\keys\prod.keys --title-keys C:\keys\title.keys --sdk C:\Android\Sdk --ndk C:\Android\Sdk\ndk\27.2.12479018 --outdir games -o MyGame --jobs 4
```

For automatic Windows setup from the command line, omit `--sdk` and `--ndk`
and add `--auto-tools`. You can also keep manually supplied SDK/NDK paths.

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
