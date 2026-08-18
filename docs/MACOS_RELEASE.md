# macOS packaging and releases

Tender's Log currently ships two **unsigned** disk images:

- `arm64` for Apple Silicon Macs;
- `x86_64` for Intel Macs.

The project does not import a Developer ID certificate, submit builds to Apple,
or read signing/notarization secrets. This is intentional for the initial
friend-to-friend distribution.

The desktop shell is Tauri on the system WKWebView. It does not package
Electron or a private Chromium runtime. MapLibre is a route-level JavaScript
chunk and OpenFreeMap tiles remain network resources; no map tiles are copied
into the app or DMG.

## Build an unsigned DMG locally

Requirements are macOS 12 or newer, Node.js 24, npm 11.18.0, the checked-in
Rust toolchain, Xcode Command Line Tools, and JavaScript dependencies installed
from the lockfile.

```sh
npm --prefix apps/console ci
npm --prefix apps/console run assets:mac
npm --prefix apps/console run tauri:build:mac
npm --prefix apps/console run size:mac
```

Without an explicit Rust target, the native-architecture DMG is written beneath:

```text
target/release/bundle/dmg/
```

Passing `-- --target <rust-target>` places it under
`target/<rust-target>/release/bundle/dmg/`, as the release workflow does.

`tauri:build:mac` first asks Tauri for the native `.app`, then creates the
install image with macOS's built-in `hdiutil`. The image always contains the
app, an **Applications** shortcut, and the proper volume/app icon. A bounded
20-second Finder pass adds the custom Tender's Log background and icon
positions when Finder automation is available. If it is unavailable (common
on headless CI runners), packaging removes the unused background and emits a
plain install image instead. It never waits indefinitely for Finder: the
wrapper checks the correctly cased `.DS_Store`, detaches the temporary volume,
compresses the image, and verifies its checksum in either path.

To test only the compiled application and skip the disk-image step:

```sh
npm --prefix apps/console run tauri:build -- --no-bundle --no-sign --ci
```

`assets:mac` is offline after `npm ci`: it uses the installed Tauri CLI plus
macOS's built-in `sips` tool to regenerate `.icns`, PNG icons, and the DMG
background from the checked-in Tender's Log SVG sources.

Every production Tauri build also runs `licenses:bundle`. It reads the locked
Rust graph for both macOS architectures and the installed locked npm graph,
then writes a deduplicated legal-text inventory into the application at:

```text
Tender's Log.app/Contents/Resources/licenses/
```

That directory contains `THIRD_PARTY_NOTICES.md`, both project license texts,
and `DEPENDENCY_LICENSES.txt` with package/version/SPDX inventory, lockfile
fingerprints, and the complete license/notice text shipped by each dependency.
If an upstream archive omits its license file, the report says so and includes
the permitted standard text identified by the package's declared license. The
artifact size check also fails when any required legal resource is missing or
empty, so a DMG cannot pass the release gate without them.

## Size policy

Every release DMG must be at most **25 MiB** (26,214,400 bytes). The
`size:mac` script reports exact DMG bytes and the summed app payload; the
release workflow runs the same check and fails before upload when the budget
is exceeded. The workspace release profile uses thin LTO, one codegen unit,
and symbol stripping. A larger budget requires a deliberate policy change
backed by a measured artifact, not an estimate.

## Open an unsigned build

Because the app is not signed or notarized, macOS Gatekeeper may block the
first ordinary double-click. Do not disable Gatekeeper and do not strip the
quarantine attribute globally.

1. Open the DMG and drag **Tender's Log** to **Applications**.
2. In Finder, open **Applications**.
3. Control-click or right-click **Tender's Log**, then choose **Open**.
4. Confirm **Open** in the macOS dialog.

After that explicit first launch, the app opens normally for that user.

Closing the main window hides it; it does not stop collection, prediction,
delivery, or E213 rotation. Use the Tender's Log menu-bar item to see the
current USB/BLE state and choose **Open Tender's Log** or **Quit Tender's
Log**. Only the explicit Quit action stops the background runtime.

The Windows installer has its own document mirroring this one:
[`WINDOWS_RELEASE.md`](WINDOWS_RELEASE.md). It is cross-compiled from macOS,
so both platforms release from the same workflow and toolchain family.

## Release workflow

`.github/workflows/release-desktop.yml` runs only in either of these cases:

- a version tag such as `v0.1.0` is pushed;
- a maintainer starts it manually with `workflow_dispatch`.

A tag build must exactly match `apps/console/package.json`; lightweight and
annotated tags are both accepted and neither requires a signature. It builds separate
Apple Silicon and Intel DMGs, enforces the 25 MiB budget, records exact size
reports and SHA-256 checksums, and attaches both to a GitHub release. A manual
run uploads temporary workflow artifacts but does not create a release.

Example unsigned lightweight tag creation:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The workflow always passes Tauri's `--no-sign` flag and contains no tag-signing,
Developer ID, or notarization step. Adding any signing later should be a
separate, explicit release-policy change.
