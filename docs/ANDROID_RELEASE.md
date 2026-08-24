# Android packaging and releases

BrickellStatus ships two **signed** Android artifacts per release:

- `BrickellStatus_<version>_android-arm64-v8a.apk` — every phone sold in the
  last decade.
- `BrickellStatus_<version>_android-armeabi-v7a.apk` — older 32-bit hardware.

Unlike the macOS DMG and the Windows installer, these are signed, with a
self-held upload key rather than a purchased certificate. Android has no
Gatekeeper or SmartScreen equivalent to talk a user past; it simply requires
that an APK be signed by *something* before it will install. The user-facing
step is allowing installation from an unknown source, which the README covers.

x86 and x86_64 are deliberately not published. They exist only for emulators,
and a universal APK carrying all four ABIs would be roughly three times the
size budget.

## What the phone build is, and is not

The Android app is the same local-first runtime as the desktop: collectors,
SQLite, the policy engine, notifications and the console UI all run on the
device. There is no server and no companion process; the frontend reaches Rust
over the same Tauri IPC it uses on the desktop.

Three desktop capabilities are absent:

| Capability | Android | Why |
| --- | --- | --- |
| System tray | Gone | No such surface. The status cache that used to live in the tray state is now its own managed state, and the frontend still receives the same `display-connection-status` event. |
| USB firmware flashing | Gone | Flashing drives a USB serial bootloader, which an unprivileged Android app cannot open. The APK bundles no firmware at all. |
| USB e-paper transport | Gone | Same reason. `serialport` compiles for Android but `available_ports()` answers *"Not implemented for this OS"*, so a scan reports Bluetooth devices only, and the console hides the USB and Automatic transport choices. |

Bluetooth e-paper **does** work. See *The Bluetooth bridge* below, because it is
the one part of this build with a moving part worth understanding.

## Collecting in the background

`install_runtime` spawns two five-second-tick workers and the scheduler as plain
tokio tasks. They belong to the *process*, not to the activity, so they keep
running for exactly as long as the process is allowed to run — and a
backgrounded Android app is cached and then frozen within moments. Without
intervention the collectors stop, no notification fires and no frame reaches the
panel, which reduces an advance-warning app to one that confirms what you can
already see out of the window.

`WatchService` is what buys the process out of that. It is a foreground service
with types `dataSync|connectedDevice`: the first covers the feed polling, the
second is what permits an open BLE connection to the panel while backgrounded.
It is started from `MainActivity.onCreate` — Android forbids launching a
foreground service from the background — and declared `stopWithTask="false"`, so
swiping the app out of Recents leaves the watch running. `START_STICKY` brings
it back if the system kills it for memory.

It also holds a **partial wake lock**. Doze parks timers between maintenance
windows, and a five-second poll that actually fires every fifteen minutes warns
you after the bridge has opened. The battery cost is real and is the honest
price of the product's central claim; the notification carries a **Stop
watching** action so it stays the user's call.

The ongoing notification a foreground service must display is not wasted on a
"running" line. The dispatch worker publishes the current decision into it every
tick that changes, picking the channel the engine already ranked highest by
`priority.score` — the same ranking the panel and the alerts use. So the cost of
a permanent notification is repaid as the cheapest way to read the river.

That path crosses JNI in the direction Android makes awkward. Rust holds a
global reference to the `WatchService` class, resolved once during
`NativeBridge.initStatusBridge()` — on a thread the JVM called into, for the
same class-loader reason the Bluetooth handshake has — and calls
`publishStatus` from the worker thread later. Updates are suppressed when the
text has not changed, so a quiet river costs one string comparison per tick
rather than a JNI round trip and a notification redraw.

**What is still not covered.** A cold boot does not start the watch: the service
is launched by the activity, so the app must be opened once after a reboot.
Starting headless would mean the Rust runtime being constructible outside
`tauri::Builder::setup`, which it is not today.

## Toolchain

Pinned so that a local build and CI use the same compiler:

| Component | Version | Note |
| --- | --- | --- |
| JDK | 21 | AGP supports 17 and 21. It rejects 25. |
| Android SDK platform | 36 | `tauri-plugin-notification`'s AAR sets `compileSdk = 36`. |
| Build tools | 36.0.0 | |
| NDK | 27.3.13750724 | Matches `ANDROID_NDK_HOME` on GitHub's ubuntu runners. |
| Rust | 1.97.1 | Workspace pin. |

Command-line setup on Linux, without Android Studio:

```bash
export JAVA_HOME=/path/to/jdk-21
export ANDROID_HOME="$HOME/Android/Sdk"
mkdir -p "$ANDROID_HOME/cmdline-tools"
# Unzip the Linux command-line tools so that sdkmanager lands at
# $ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager -- it resolves the SDK root
# from its own path and will not work from anywhere else.
yes | "$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" --licenses
"$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager" --install \
  "platform-tools" "platforms;android-36" "build-tools;36.0.0" \
  "ndk;27.3.13750724"

export NDK_HOME="$ANDROID_HOME/ndk/27.3.13750724"   # the Tauri CLI reads this
export PATH="$ANDROID_HOME/platform-tools:$PATH"
```

Rust targets are installed by `tauri android init`; to add them by hand:

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi --toolchain 1.97.1
```

They are deliberately **not** listed in `rust-toolchain.toml`. That file's
`targets` are fetched by every `rustup` invocation on every machine, so putting
them there would make the macOS and Windows release jobs download four Android
standard libraries they never use.

## Build locally

```bash
npm --prefix apps/console ci
npm --prefix apps/console run tauri:android:build   # APKs, arm64 + armv7
npm --prefix apps/console run tauri:android:bundle  # AAB, for the Play Store
npm --prefix apps/console run tauri:android:dev     # live reload onto a device
```

Output lands in
`apps/desktop/src-tauri/gen/android/app/build/outputs/apk/<abi>/release/`.

`tauri:android:build` runs `android:prepare` first, which checks the vendored
droidplug Java, bundles the licence texts and builds the frontend. It does
**not** run `firmware:bundle`: that needs Python and PlatformIO to produce ESP32
images Android can never flash.

### One thing to know about the staging directory

Tauri copies `bundle.resources` into
`gen/android/app/src/main/assets/` and never prunes what it finds there. If you
change which resources the Android bundle carries, delete the stale directory by
hand — otherwise the old files keep shipping. `check-android-artifact-size.mjs`
fails the build if firmware assets reappear, which is exactly the mistake this
guards against.

## Size policy

The repository's 25 MiB release budget (`CONTRIBUTING.md`) applies **per ABI**.
As of the first Android build:

| Artifact | Size |
| --- | --- |
| `android-arm64-v8a.apk` | 19.55 MiB |
| `android-armeabi-v7a.apk` | 14.84 MiB |

`npm --prefix apps/console run size:android` reports both, asserts the four
bundled licence files are present inside each APK, and fails if firmware assets
have crept back in. If arm64 ever crosses the budget, the levers in order are:
drop `armeabi-v7a`, enable `isShrinkResources`, then reconsider the ceiling —
in that order, and as a deliberate decision recorded in `CONTRIBUTING.md`, not a
quiet edit.

## Signing

The upload key is **not** in the repository and must never be. Generate one:

```bash
keytool -genkeypair -v \
  -keystore ~/keys/brickellstatus-release.jks \
  -alias brickellstatus \
  -keyalg RSA -keysize 4096 -validity 10000 -storetype PKCS12 \
  -dname "CN=BrickellStatus, O=BrickellStatus contributors, C=US"
```

Back it up somewhere durable. Losing it means never shipping an update that
Android will accept as the same app.

Point the local build at it with
`apps/desktop/src-tauri/gen/android/keystore.properties` (gitignored):

```properties
storeFile=/home/you/keys/brickellstatus-release.jks
storePassword=…
keyAlias=brickellstatus
keyPassword=…
```

Absent that file the release build still succeeds and produces an *unsigned*
APK, so a fresh clone and every fork can build without holding the key.

For CI, set four repository secrets:

| Secret | Value |
| --- | --- |
| `ANDROID_KEYSTORE_BASE64` | `base64 -w0 ~/keys/brickellstatus-release.jks` |
| `ANDROID_KEYSTORE_PASSWORD` | store password |
| `ANDROID_KEY_ALIAS` | `brickellstatus` |
| `ANDROID_KEY_PASSWORD` | key password |

The release job writes `keystore.properties`, builds, and removes it in an
`if: always()` step. A missing `ANDROID_KEYSTORE_BASE64` **fails** the job on
this repository; only a fork falls through to an unsigned build. That asymmetry
exists because v0.1.27 through v0.1.32 shipped unsigned, uninstallable APKs
while the job stayed green on a warning.

The job then verifies the artifact rather than the inputs: every published APK
must pass `apksigner verify`, and its DEX must still contain each method Rust
resolves by JNI name (`publishStatus`, `initBluetooth`, `initStatusBridge`).
The second check exists because R8 strips whatever it sees no caller for, and
a name lookup from Rust is not a caller it can see -- a keep rule is the only
thing standing between minification and a `NoSuchMethodError` on a build that
otherwise looks healthy.

## The Bluetooth bridge

btleplug's Android backend is half Rust and half Java, and both halves need
help that no other platform does.

**The Java half is vendored.** `apps/desktop/src-tauri/android/droidplug/java/`
is a copy of btleplug's own Java sources, which are not published to Maven. They
are added to the app's `main` source set by `gen/android/app/build.gradle.kts`.
`apps/console/scripts/sync-droidplug-java.mjs --check` — run by `android:prepare`
and by CI — fails if that copy drifts from the btleplug version the lockfile
resolves. A drift there would otherwise surface as a `NoSuchMethodError` on a
device, which is a miserable way to find out.

**The handshake has to come from Java.** btleplug resolves its classes with JNI
`FindClass`, which searches the class loader of the nearest Java frame on the
calling thread. Tauri runs the Rust main loop on a thread it spawned, whose
loader cannot see anything in the APK. So `MainActivity.onCreate` calls
`NativeBridge.initBluetooth()` — an app-package native method, therefore the app
class loader — *before* `super.onCreate` starts Rust. Failure is recorded, not
thrown: a device with Bluetooth off loses the panel output and nothing else.

**R8 would delete all of it.** Every one of those classes is reached only
through JNI, so the release build's `isMinifyEnabled = true` sees no caller.
`gen/android/app/proguard-rules.pro` keeps `com.nonpolynomial.**`,
`io.github.gedgygedgy.**` and `NativeBridge`. Remove those and Bluetooth breaks
in release builds only.

## TLS on Android is not the platform verifier

`reqwest` 0.13 reaches for `rustls-platform-verifier` whenever a client names no
roots. Its Android backend calls into the JVM through a Kotlin component that is
distributed separately from the crate and that this app does not ship — so the
client builds cleanly and then panics inside the first handshake.

`crates/tls` settles it once for every HTTP client in the workspace: on Android
it hands reqwest the Mozilla root program explicitly via `tls_certs_only`, which
takes the platform verifier out of the path entirely. That is the same root set
the AIS websocket already trusts through `tokio-tungstenite`, so every
connection the app opens now agrees on one list.

The trade: CAs a user or employer installed on the device are not honoured, and
the roots move only when the app is updated. For a client that talks to five
fixed public hosts, that is the cheaper side of shipping a JVM bridge for TLS.

## Re-running `tauri android init`

`gen/android/` is committed, because four things in it are hand edits:

- `app/build.gradle.kts` — the droidplug source set, the signing config, and
  `rust { rootDirRel }`.
- `app/src/main/AndroidManifest.xml` — the Bluetooth, notification and location
  permissions.
- `app/src/main/java/com/cmiami/brickellstatus/MainActivity.kt` — the Bluetooth
  handshake and the runtime permission request.
- `app/proguard-rules.pro` — the JNI keep rules.

`NativeBridge.kt` is ours outright. Re-running `android init` will overwrite the
generated files, so diff the result before committing.

`rootDirRel` deserves a note: `android init` sets it to `src-tauri`'s own
directory, on the assumption that the frontend `package.json` sits beside
`src-tauri`. In this repo the npm project is `apps/console`, so the generated
value points at a directory with no `package.json` and the Gradle build fails
with a bare `ENOENT`. It is set to `../../../../../console` instead.

## Release workflow

`.github/workflows/release-desktop.yml` (*Release installers*) builds Android
alongside the DMG and the NSIS installer on a `v*` tag, and the `publish` job
attaches all of them to one release. Android builds on `ubuntu-latest`, which
already carries the SDK and the pinned NDK — no cross-compilation gymnastics,
unlike the Windows leg.

`.github/workflows/ci.yml` gains an *Android cross-compile* job on every PR. It
builds the library for `aarch64-linux-android` and stops — no Gradle, no APK.
That is enough to catch a `#[cfg(desktop)]` that no longer lines up, in a couple
of minutes rather than half an hour. It is a required check.

## On-device QA

```bash
adb install -r apps/desktop/src-tauri/gen/android/app/build/outputs/apk/arm64/release/app-arm64-release.apk
adb logcat -s RustStdoutStderr:V chromium:V
```

Worth walking, in this order — the first two are where this port can fail in
ways the build cannot tell you about:

1. **The console renders.** A blank white screen means CSP. Attach
   `chrome://inspect` and read the console; the violation names the missing
   source. `tauri.android.conf.json` carries an Android-specific policy because
   the WebView serves the app from `http://tauri.localhost`, not the desktop's
   `asset:` origin.
2. **A signal actually refreshes.** This is the TLS path. If every collector
   errors, the `crates/tls` Android branch is not doing its job.
3. Preferences save and survive a restart (SQLite, and the `0600` secret store).
4. Map tiles and the radar layer load — more CSP surface.
5. Outputs → E-paper → Scan: the OS asks for Bluetooth, the INK1 panel appears,
   connect, then **Send test frame** and watch the panel redraw. This is the
   btleplug proof.
6. A notification fires when a channel goes active.
7. The firmware prompt never appears, and the USB and Automatic transport
   choices are absent.
8. Background the app for two minutes and return: no crash, one catch-up tick.
