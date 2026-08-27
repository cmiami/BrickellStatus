# Windows packaging and releases

BrickellStatus ships one **unsigned** Windows artifact:

- `BrickellStatus_<version>_windows-x64-setup.exe`, an NSIS installer for x64
  Windows 10/11.

The project holds no Authenticode certificate and submits nothing to
Microsoft, matching the unsigned macOS policy in
[`MACOS_RELEASE.md`](MACOS_RELEASE.md). SmartScreen will warn on first run;
the approved walkthrough is in the README and below. Adding signing later
should be a separate, explicit release-policy change.

The desktop shell is Tauri on WebView2. Windows 11 ships the WebView2 runtime;
the installer's silent `downloadBootstrapper` step covers machines without it.
Nothing else is bundled — no Electron, no private Chromium.

## Everything builds on macOS

The executable and installer are **cross-compiled from macOS**; a Windows
machine is only needed for QA. The pieces:

- `rustup target add x86_64-pc-windows-msvc` — pinned through
  [`rust-toolchain.toml`](../rust-toolchain.toml);
- **cargo-xwin 0.23.1** (`cargo install --locked cargo-xwin`) drives the build.
  On first use it downloads the Microsoft CRT and Windows SDK
  (~1.5–2.5 GB into `~/Library/Caches/cargo-xwin`); running it implies
  accepting Microsoft's SDK license, which the build script acknowledges by
  exporting `XWIN_ACCEPT_LICENSE=1`;
- **LLVM from Homebrew** (`brew install llvm`) supplies `clang-cl` and
  `lld-link`. The keg is deliberately not on PATH; the build script prepends
  `$(brew --prefix llvm)/bin` itself;
- **NSIS from Homebrew** (`brew install nsis`) supplies `makensis`, which
  compiles Windows installers on any host. On the first bundle, the Tauri CLI
  also fetches its pinned, checksummed `nsis_tauri_utils` plugin into
  `~/Library/Caches/tauri` — a one-time network access worth knowing about in
  a supply-chain review.

TLS note: the workspace pins rustls to the `ring` provider (installed as the
process default at startup) precisely so no C/CMake dependency stands between
macOS and a Windows target. `cargo tree -i aws-lc-sys` must stay empty.

Apostrophe note: Tauri's NSIS shortcut helpers single-quote their COM
parameters, and the `'` in "BrickellStatus.lnk" terminates that quote early
(`macro "NSISCOMCALL" requires 4 parameter(s), passed 7`). The build script
interposes a `makensis` shim that re-quotes the generated `utils.nsh` with
NSIS backticks before compiling. If a Tauri CLI upgrade fixes the template,
the shim becomes a harmless no-op and can be removed.

## Build the unsigned installer locally

```sh
brew install nsis llvm                      # one-time
cargo install --locked cargo-xwin           # one-time
npm --prefix apps/console ci
npm --prefix apps/console run tauri:build:win
npm --prefix apps/console run size:win
```

`tauri:build:win` preflights every tool above with an actionable error before
building, then produces:

```text
target/x86_64-pc-windows-msvc/release/bundle/nsis/BrickellStatus_<version>_x64-setup.exe
```

and prints the installer's byte size and SHA-256. Re-verify that hash inside
the QA VM with `certutil -hashfile <installer> SHA256` before installing.

The installer runs per-user (`installMode: currentUser`): no UAC elevation, app
under `%LOCALAPPDATA%\Programs`, Start Menu entry, standard uninstaller. As on
macOS, every production build bundles the license inventory; `size:win` fails
if any required legal resource is missing, and the same rules apply.

## Size policy

The release installer must be at most **25 MiB** (26,214,400 bytes), the same
ceiling as the macOS DMG. `size:win` reports exact executable and installer
bytes and fails the release gate above budget. A larger budget requires a
deliberate policy change backed by a measured artifact, not an estimate.

## Secrets on Windows

Windows has no Unix file mode, so `credentials.json` is sealed with
**user-scope DPAPI** (`CryptProtectData`, prompts forbidden): encryption is
keyed to the logged-in Windows account, entirely silent, and a copied file is
useless on another machine or account. A pre-DPAPI plaintext file still loads
once and is re-sealed on the next write. See `SECURITY.md` for the data-at-rest
boundary.

## QA protocol (Parallels VM)

Run this against the exact installer the release will ship, inside an x64
Windows 11 VM.

1. **Integrity** — copy the setup exe in via a shared folder;
   `certutil -hashfile "BrickellStatus_<version>_windows-x64-setup.exe" SHA256`
   matches the hash the build printed.
2. **SmartScreen** — "Windows protected your PC" → **More info** → **Run
   anyway**. Never disable SmartScreen globally.
3. **Install** — per-user, no UAC prompt; app in
   `%LOCALAPPDATA%\Programs\BrickellStatus`; Start Menu and Apps-list uninstall
   entries present.
4. **WebView2** — the app launches on stock Windows 11. Optionally, in a VM
   snapshot, uninstall the WebView2 Runtime and confirm the installer's
   bootstrapper fetches it.
5. **Window and tray** — closing the window hides it and the runtime keeps
   working; tray left-click opens the menu with live status lines;
   double-clicking the tray icon opens the window; the tooltip carries
   "BrickellStatus · status · detail"; the icon is legible on the dark taskbar;
   **Quit** exits; relaunching while running focuses the existing instance.
6. **Notifications** — trigger a notice; a toast appears attributed to
   BrickellStatus. Toasts need the Start Menu shortcut, so never QA them from a
   bare unpacked exe.
7. **External links** — map attribution and release links open the default
   browser, including URLs containing `&`.
8. **Geolocation** — "Locate this computer once" produces either a position or
   the documented error copy; search and pin-drop still work without one.
9. **USB / e-ink** — test both interfaces: Vision Master as "Espressif USB
   JTAG/serial debug unit" (VID 0x303A, inbox `usbser.sys`) and Wireless Paper
   as a Silicon Labs CP210x COM port (VID 0x10C4 / PID 0xEA60). The in-app scan
   finds each; connect; the test frame returns `ACK INK1`; rotation renders. If
   Windows does not expose the CP210x COM port, install Silicon Labs' official
   CP210x VCP driver before treating discovery as an app failure.
10. **Firmware flash** — flash each board family and watch progress. On Wireless
    Paper, confirm the exact CP2102 interface produces one Wireless Paper offer
    and one Flash action, with no display-revision choice. Boot verification
    returns over the same CP2102 UART. Set the Parallels
    USB assignment to reconnect to Windows automatically: the board
    re-enumerates after the post-flash reset, and if the Mac reclaims it the
    QA run stalls. Windows keeps COM numbers sticky per device serial, so the
    app's reopen-by-name is expected to hold; if the COM number does change,
    file a follow-up rather than blocking the release.
11. **Bluetooth LE** — Parallels' shared Mac Bluetooth does not reliably
    expose an LE radio to Windows, so an empty `btleplug` scan inside the VM
    proves nothing. Pass a USB Bluetooth dongle through so Windows owns a real
    radio, or record BLE as "verified on physical hardware only".
12. **Secrets** — save a WhatsApp token; `%APPDATA%\com.cmiami.brickellstatus\
    credentials.json` contains a `dpapiCiphertext` envelope, not plaintext,
    and the token survives an app restart.
13. **Uninstall** — app and shortcuts removed; per-user app data under
    `%APPDATA%` is retained, which is expected.

## Release workflow

The Windows leg lives in
[`release-desktop.yml`](../.github/workflows/release-desktop.yml) beside the
macOS DMGs and runs under the same triggers and version-tag verification. It
cross-compiles on a **macOS** runner — the same toolchain as the local build —
enforces the size budget, and attaches
`BrickellStatus_<version>_windows-x64-setup.exe` to the GitHub release. Only the
installer becomes a release asset; the SHA-256 and size report remain as
internal workflow artifacts. A manual `workflow_dispatch` run uploads workflow
artifacts without creating a release.

CI's `windows-native` job (in [`ci.yml`](../.github/workflows/ci.yml)) is the
complement: it runs `cargo test --workspace` on a real Windows runner, because
cross-compilation proves the code builds, not that it behaves — the DPAPI
round-trip tests execute nowhere else.
