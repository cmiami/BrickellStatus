# Fedora packaging and releases

BrickellStatus ships one **unsigned** Linux artifact:

- `BrickellStatus_<version>_fedora44-x86_64.rpm`, for Fedora 44 on x86_64.

The project holds no GPG release key and publishes no repository, matching the
unsigned macOS and Windows policy in [`MACOS_RELEASE.md`](MACOS_RELEASE.md) and
[`WINDOWS_RELEASE.md`](WINDOWS_RELEASE.md). `dnf install ./…rpm` will note the
package is unsigned; that is expected. Signing later should be a separate,
explicit release-policy change, and would mean publishing a key people can
actually check.

The desktop shell is Tauri on **WebKitGTK 4.1** (GTK 3), which is the system
webview on Fedora. Nothing else is bundled — no Electron, no private Chromium.

## Why RPM and not Flatpak

Flatpak is the obvious first suggestion for a Linux desktop app, and it is the
wrong one here, for a reason specific to what this app does.

BrickellStatus does not only display things. It opens `/dev/ttyACM*` directly
to drive an e-paper panel, and it **flashes an ESP32-S3 over USB serial**
(`apps/desktop/src-tauri/src/firmware.rs`). Under Flatpak, raw serial access
means shipping `--device=all`, which hands the sandboxed app every device node
on the machine. That is a strictly larger hole than the RPM opens, while
costing a manifest, a freedesktop runtime, `flatpak-builder` in CI, and
realistically a Flathub submission before anyone benefits. Paying all of that
to end up with weaker isolation than the native package is a bad trade.

Two smaller reasons point the same way. Tauri bundles RPM natively and does not
bundle Flatpak at all. And the RPM can install the udev rule described below,
which is what actually makes the hardware usable; a Flatpak cannot.

AppImage was also considered and skipped: it needs FUSE, builds awkwardly in a
container, and offers nothing over an RPM on a release that already names
Fedora 44 as its target.

## Everything builds in a Fedora container

Unlike the Windows installer, this one **cannot be cross-built**. It links the
host's WebKitGTK, libudev and libdbus, and it names its runtime dependencies as
Fedora package names, so it is built on the distribution it targets. GitHub
offers no Fedora runner, so both the CI job and the release job run inside a
`fedora:44` container on an `ubuntu-latest` host, pinned by image digest the
same way every action in this repository is pinned by SHA.

The build dependencies, and why each is there:

| Package | Needed for |
| --- | --- |
| `webkit2gtk4.1-devel` | the webview the shell renders into |
| `systemd-devel` | `libudev`, which `serialport` uses to enumerate ports |
| `dbus-devel` | `libdbus-1`, reached by `tao` and by `btleplug` for BlueZ |
| `gcc`, `gcc-c++`, `make` | the C in `libsqlite3-sys` and `ring` |
| `libayatana-appindicator-gtk3-devel` | its `.pc` file, which the Tauri CLI probes while bundling |
| `pkgconf-pkg-config` | how every `-devel` package above is found |

Three packages from Tauri's published Fedora list are deliberately **absent**:

- `openssl-devel` — this workspace is rustls end to end. `cargo tree -i
  openssl-sys` must stay empty.
- `libxdo-devel` and `librsvg2-devel` — no crate in `Cargo.lock` links either.

### The appindicator trap

`libayatana-appindicator-gtk3-devel` looks like a fourth candidate for that
list and is not one. The reasoning that puts it there is seductive and wrong,
so it is written down here.

`libappindicator-sys` 0.9 `dlopen`s `libayatana-appindicator3.so.1` at runtime
rather than linking it. Nothing in `Cargo.lock` links the library, no `.so` is
needed to link, and `cargo build --release` produces a complete, working
binary without the `-devel` package installed. Every signal available from the
Rust side says the package is a runtime dependency of the *artifact*, not a
build dependency.

Then the Tauri CLI bundles, and runs this:

```sh
pkg-config --libs-only-L ayatana-appindicator3-0.1   # then appindicator3-0.1
```

It needs the library path to record which tray library the package depends on.
With no `.pc` file it finds neither and panics — `Can't detect any appindicator
library` — from `tauri-cli/src/interface/rust.rs`. So the `-devel` package is a
build-host requirement despite nothing linking against it, and the failure
lands **after** the full release compile, roughly seven minutes in.

`build-linux-rpm.mjs` therefore checks for that pkg-config module in its
preflight alongside the three it genuinely links, which turns seven wasted
minutes into an immediate error naming the `dnf` command. Either module
satisfies the CLI, so the preflight accepts either.

Which branch the CLI takes also decides what the package should require:
finding `ayatana-appindicator3-0.1` makes it record
`libayatana-appindicator3.so.1`, owned on Fedora 44 by
`libayatana-appindicator-gtk3` — which is what `bundle.linux.rpm.depends`
names.

## The package's runtime dependencies are hand-written

This is the sharpest edge in the whole Linux leg and worth stating plainly.

Tauri writes RPMs with the pure-Rust `rpm` crate, **not** `rpmbuild`, so nothing
runs the automatic dependency generator that would normally read the built ELF
and emit a `Requires:` line per soname.

Tauri does add three sonames of its own — `libayatana-appindicator3.so.1`,
`libwebkit2gtk-4.1.so.0` and `libgtk-3.so.0` — but those are a fixed list it
knows about, not the result of inspecting this binary. The proof is what is
missing from a built package: `rpm -q --requires` shows no `libudev.so.1` and
no `libdbus-1.so.3`, even though the executable links both. `systemd-libs` and
`dbus-libs` appear in the package only because `depends` names them.

So the `depends` list in `bundle.linux.rpm` in
[`tauri.conf.json`](../apps/desktop/src-tauri/tauri.conf.json) is most of what
stands between a user and a package that installs cleanly and then fails to
start.

That list is `webkit2gtk4.1`, `gtk3`, `libayatana-appindicator-gtk3`,
`dbus-libs` and `systemd-libs`, plus a `bluez` recommendation for the BLE
transport. `libayatana-appindicator-gtk3` is there because the tray is dlopened:
nothing in the ELF records that dependency, so nothing but this line would.

Because a stale name here is invisible until release day, CI's `fedora-linux`
job **installs the package it just built** with `dnf`, queries it, and removes
it again. If a dependency is renamed or retired in a future Fedora, that job
fails months before a user would have found it.

## Serial access, and why a udev rule ships

Fedora creates `/dev/ttyACM*` as `root:dialout` mode 0660. Without intervention
the app lists a connected board and then cannot open it — the worst failure
shape available, because the hardware is visibly present and still unusable.

The package installs
[`70-brickellstatus-espressif.rules`](../apps/desktop/src-tauri/linux/70-brickellstatus-espressif.rules)
into `/usr/lib/udev/rules.d/`, tagging Espressif native USB (`303a`) and the
Wireless Paper's Silicon Labs CP2102 bridge (`10c4:ea60`) with `uaccess`.
systemd-logind then puts an ACL for the **active local session** on the node.

That is deliberately narrower than the usual advice of adding the account to
`dialout`, which grants that account every serial device on the machine,
permanently, whether or not it is the session sitting at the keyboard. It also
needs no logout — the post-install scriptlet reloads udev and retriggers `tty`
devices, so a board plugged in before the install works immediately.

Other USB-UART bridge families remain deliberately uncovered: the app does not
recognise them, so a rule would open devices that never appear in the picker.

## Wayland

GTK 3 selects the Wayland backend automatically when `WAYLAND_DISPLAY` is set,
and `gdkwayland-sys` is in `Cargo.lock`, so a Fedora 44 GNOME session runs this
natively with no `GDK_BACKEND` override and no XWayland fallback. Two things
still needed doing:

- **Window identity.** Wayland compositors match a window to its launcher by
  xdg-shell `app_id`, which GTK takes from the program name — the *binary*
  name, not the product name. The packaged desktop entry therefore sets
  `StartupWMClass=brickellstatus-desktop`. Without it the app gets a generic
  icon in the dash, appears as a second unnamed entry beside its own launcher,
  and "pin to dash" pins nothing.
- **Notifications.** `X-GNOME-UsesNotifications=true` puts the app in GNOME
  Settings → Notifications, so its native alerts can be tuned or silenced
  there rather than only inside the app.

The template that carries those two keys is deliberately free of comments.
Handlebars passes `#` lines straight through, so anything explanatory written
there is installed verbatim into `/usr/share/applications` on every machine —
the rationale belongs in this document instead.

Two known Wayland caveats belong in QA rather than in the package:

- **Tray icon.** GNOME has no built-in tray. The tray this app builds appears
  under KDE Plasma and under GNOME *with* the AppIndicator extension, and
  nowhere else. The window and notifications are unaffected. Test on both.
- **Blank window on some drivers.** WebKitGTK's DMABUF renderer misbehaves on
  a few driver and VM combinations, showing a white window. The escape hatch
  is `WEBKIT_DISABLE_DMABUF_RENDERER=1` in the environment. It is documented
  rather than baked into the launcher, because setting it unconditionally
  would cost every correctly-working machine its GPU compositing path.

## Build the package locally

On Fedora 44:

```sh
sudo dnf install webkit2gtk4.1-devel systemd-devel dbus-devel \
  libayatana-appindicator-gtk3-devel gcc gcc-c++ make pkgconf-pkg-config
npm --prefix apps/console ci
npm --prefix apps/console run tauri:build:linux
```

The build script checks `pkg-config` for `webkit2gtk-4.1`, `libudev` and
`dbus-1` up front, so a missing library fails with its `dnf` command instead of
a pkg-config error a thousand lines into the cargo build. It prints the package
path, its exact size and its SHA-256.

From another OS, use the same container CI does:

```sh
docker run --rm -it -v "$PWD:/src" -w /src \
  fedora:44@sha256:6c75d5bf57cb0fa5aa4b92c6a83c86c791644496d9ac230de7711f5b8ec3b898
```

## Size policy

The release package must be at most **25 MiB** (26,214,400 bytes), the same
ceiling as the macOS DMG and the Windows installer. `size:linux` reports exact
executable and package bytes and fails the release gate above budget.

It also verifies the package *payload* — the executable, the desktop entry, the
udev rule, the firmware manifest and all four bundled licence files — by
listing the built RPM with `rpm -qpl`. That check exists because nothing else
would catch it: a typo in `bundle.linux.rpm.files` drops the udev rule silently,
and the first symptom is a user whose board is visible but cannot be opened.
Payload entries are matched by path suffix rather than absolute path, so an
upstream relayout of Tauri's resource root does not become a release blocker.

The RPM payload is zstd level 19 rather than Tauri's default gzip 6, which is
what keeps a bundle carrying firmware images and a compiled frontend inside the
budget.

## Secrets on Linux

Linux gets the same treatment as macOS, not the Windows one: `credentials.json`
is written with mode **0600** and is otherwise plaintext. There is no libsecret
or kwallet integration, so the data-at-rest boundary is the user account and
the filesystem — anything that can read the home directory as that user can
read the tokens. See `SECURITY.md`. Moving to the Secret Service API would be a
deliberate change, and would need to degrade gracefully on a machine running no
keyring daemon.

## QA protocol

Run this against the exact package the release will ship, on Fedora 44.

1. **Integrity** — `sha256sum BrickellStatus_<version>_fedora44-x86_64.rpm`
   matches the hash the build printed.
2. **Install** — `sudo dnf install ./BrickellStatus_<version>_fedora44-x86_64.rpm`.
   Confirm dnf resolves every dependency from the stock repositories and warns
   only about the missing signature.
3. **Launcher identity** — find the app in the GNOME overview by name, launch
   it from there, and confirm the running window shows the app's own icon and
   groups under the same dash entry. A generic icon means `StartupWMClass` no
   longer matches the binary name.
4. **Wayland session** — confirm `echo $XDG_SESSION_TYPE` reports `wayland`,
   then confirm the window renders. A white window is the DMABUF issue above,
   not a packaging failure; re-test with `WEBKIT_DISABLE_DMABUF_RENDERER=1` and
   record which path was needed.
5. **Tray** — under Plasma, or GNOME with the AppIndicator extension, confirm
   the tray icon appears and its menu opens and quits the app.
6. **Notifications** — trigger an alert and confirm it arrives, then confirm
   the app is listed in GNOME Settings → Notifications.
7. **Hardware, unplugged first** — with no board attached, confirm the panel
   picker reports nothing found rather than erroring.
8. **Hardware, plugged in after install** — attach a Vision Master E213/E290 or
   Wireless Paper and confirm it appears **without** replugging, which is what
   the post-install udev retrigger buys. Confirm `getfacl` on its `ttyACM` or
   `ttyUSB` node shows an ACL for your user.
9. **Flash** — confirm the app detects the board family and offers one Flash
   action, then flash the bundled firmware and confirm the app reports the
   build id it wrote. Wireless Paper must not show a display-revision choice.
   This is the step that proves serial write access, not just enumeration.
10. **Remove** — `sudo dnf remove brickellstatus` and confirm the udev rule is
    gone from `/usr/lib/udev/rules.d/`.

State whether QA ran on GNOME, Plasma, or both, and on bare metal or a VM —
items 4, 5 and 8 behave differently across all three.

## Release workflow

The Fedora leg lives in
[`release-desktop.yml`](../.github/workflows/release-desktop.yml) beside the
macOS DMGs and the Windows installer and runs under the same triggers and
version-tag verification. It builds in the pinned `fedora:44` container,
enforces the size and payload budget, and attaches
`BrickellStatus_<version>_fedora44-x86_64.rpm` to the GitHub release. Only the
package becomes a release asset; the SHA-256 and size report remain as internal
workflow artifacts. A manual `workflow_dispatch` run uploads workflow artifacts
without creating a release.

CI's `fedora-linux` job (in [`ci.yml`](../.github/workflows/ci.yml)) is the
complement, and does more than the other platforms' CI legs: it runs
`cargo test --workspace` on Linux, builds the real RPM, checks its payload,
then installs it with `dnf`, validates the installed desktop entry with
`desktop-file-validate`, and removes it. Linux is the only platform where the
package's dependency metadata is written by hand, so it is the only one where
CI has to prove the package installs.
