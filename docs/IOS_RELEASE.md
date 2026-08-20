# iOS packaging and releases

**Status: not shipping.** The Rust builds for iOS and the app is correctly
feature-gated for it, but `tauri ios build` does not yet complete on this
project. What follows is exactly how far it gets, what had to be patched, and
the one thing still in the way — written down so the next attempt starts here
rather than at the beginning.

## What an iOS build can and cannot be

Two constraints shape this before any code is written.

**There is no unsigned distribution.** macOS, Windows and Fedora all ship
unsigned here, and each has an escape hatch: Gatekeeper's *Open Anyway*,
SmartScreen's *Run anyway*, dnf's missing-signature warning. iOS has none. An
unsigned `.ipa` cannot be installed at all. The options are a paid Apple
Developer account with TestFlight or the App Store, or sideloading with a free
Apple ID — which issues a **7-day** provisioning profile. Seven days, not
weeks: the app stops launching after that and has to be re-sideloaded from
Xcode. That is a materially weaker promise than every other platform here
makes, and it belongs in the README as such if this ever ships.

**There is no firmware flashing, ever.** The app writes firmware over a USB
serial bootloader. iOS exposes no USB serial interface to a sandboxed app, so
this is not a limitation to work around but a permanent absence. An iPad build
is a Bluetooth-only viewer: it can drive a panel, and can never flash one.

## What is already correct

The dependency gating predates this work and covers iOS properly:

```toml
[target.'cfg(not(any(target_os = "android", target_os = "ios")))'.dependencies]
espflash.workspace = true
serialport.workspace = true
```

`cargo check --target aarch64-apple-ios -p brickellstatus-desktop` passes with
no errors.

The capability gate was fixed alongside this: `usb_display` was
`cfg!(not(target_os = "android"))`, which would have had an iPad advertising
USB display and firmware flashing. It is now `cfg!(not(mobile))`, so any future
phone target inherits the honest answer. Both flashing surfaces — the startup
firmware prompt and the panel's "screen blank or scrambled" control — check
`firmwareFlashing` before offering anything, and a test covers the prompt
staying away.

`tauri.ios.conf.json` mirrors the Android one and drops `resources/firmware/`
from the bundle, so the images are absent rather than shipped unusable.

## Prerequisites, in the order they bite

Each of these produced a different, misleading error. In order:

1. **Xcode must be selected**, not just installed. With
   `xcode-select -p` pointing at `CommandLineTools`, every build script fails
   with `unable to locate xcodebuild` — including host ones, which makes it
   look like a Rust problem.

   ```sh
   sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
   ```

2. **CocoaPods.** `tauri ios init` wants to install it with `sudo gem install`.
   Homebrew avoids the sudo: `brew install cocoapods`.

3. **The iOS platform**, which is separate from the SDK. Without it,
   `xcodebuild` reports `Found no destinations for the scheme`, which reads
   like a signing failure and is not one. The real message is only visible via
   `xcodebuild -showdestinations`: *iOS 18.5 is not installed*.

   ```sh
   xcodebuild -downloadPlatform iOS
   ```

## Two hand-patches the generator needs

`tauri ios init` writes `gen/apple/project.yml` from its own template, and gets
two things wrong for this repository. Both must be re-applied after any
`ios init` re-run, exactly as `gen/android/app/build.gradle.kts` already
documents for Android.

**The npm invocation.** The generated Xcode pre-build phase is a bare
`npm run -- tauri ios xcode-script`, executed from `gen/apple`. That assumes
the frontend `package.json` sits beside `src-tauri`; here it is `apps/console`,
so npm looks for a `package.json` inside the Xcode project and fails before
Xcode reaches any signing question. This is the same assumption that made
Android need `rootDirRel`. Patch the script line to:

```yaml
- script: npm --prefix "$PROJECT_DIR/../../../../console" run -- tauri ios xcode-script ...
```

**The signing team.** `bundle.iOS.developmentTeam` is the schema-correct
setting — the whole config validates against `schema.tauri.app/config/2`, and
the upstream XcodeGen template has a `DEVELOPMENT_TEAM: {{apple.development-team}}`
slot. It still does not arrive: `tauri ios init` accepts no `--config`, reads
only `tauri.conf.json`, and does not carry the value through. Setting it in
either config file, or via `TAURI_APPLE_DEVELOPMENT_TEAM`, leaves
`project.yml` with no `DEVELOPMENT_TEAM` at all. Until that is fixed upstream,
add it under `settings.base:` by hand and re-run `xcodegen generate`.

Note the team id is the certificate's **OU**, not the identifier in its common
name:

```sh
security find-certificate -c "Apple Development" -p | openssl x509 -noout -subject
# CN=Apple Development: you@example.com (96RT7A56CL)   <- not this
# OU=53JHDJMD6N                                        <- this
```

## Where it stops

With both patches applied, the pre-build script runs — and the nested Tauri CLI
fails inside Xcode's build environment:

```
failed to compile iOS app: Failed to lookup Xcode version: Xcode doesn't appear to be installed.
Command PhaseScriptExecution failed with a nonzero exit code
```

`xcodebuild -version` answers correctly in a normal shell, and the script phase
does export `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`, so the
lookup is failing for a reason not yet identified — a Tauri CLI interaction
with Xcode 16.4's script-phase environment rather than anything in this
repository. That is the next thing to investigate.

## When it does build

`gen/apple` is currently ignored by the `gen/*` rule. If the hand-patches above
are still needed then, it should be committed and un-ignored the way
`gen/android` is, for the same reason: the fixes cannot survive otherwise. If
the generator is fixed upstream first, leave it generated.

A CI job should inject its own team rather than inherit a personal one, which
is why no team id is committed to `tauri.conf.json` today — a setting that
looks like it configures signing while doing nothing is worse than its absence.
