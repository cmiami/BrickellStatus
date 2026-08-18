"""Assembles the firmware images the desktop app ships and flashes.

Run from the repository root:

    python3 firmware/panel/scripts/bundle_firmware.py [--skip-build]

Writes `apps/desktop/src-tauri/resources/firmware/` containing one directory per
PlatformIO environment plus a `manifest.json` describing the flash layout.

PlatformIO is not required to *build* the desktop app. When it is unavailable
the script still writes a manifest, with no variants, and the app reports that
this build ships no firmware. That keeps `tauri.conf.json`'s declared resource
directory present: the Tauri build script rejects a missing bundle resource, and
a contributor without an embedded toolchain must still be able to build the app.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
FIRMWARE_ROOT = REPOSITORY_ROOT / "firmware" / "panel"
OUTPUT_ROOT = (
    REPOSITORY_ROOT / "apps" / "desktop" / "src-tauri" / "resources" / "firmware"
)

# Flash layout for the ESP32-S3. The bootloader sits at 0x0, not the 0x1000 used
# by the original ESP32 and the S2.
#
# boot_app0.bin at 0xe000 is not optional: the shipped partition table declares
# `otadata` there alongside app0/app1, so writing only the app leaves the old OTA
# selection in place and a board that had booted app1 keeps booting app1, which
# this flash never wrote. It ships with the Arduino framework rather than the
# build directory, which is exactly why it is easy to leave out.
LAYOUT = [
    ("0x0", "bootloader.bin"),
    ("0x8000", "partitions.bin"),
    ("0xe000", "boot_app0.bin"),
    ("0x10000", "firmware.bin"),
]

# One build per panel the app can drive. `panel` is the board a build is for,
# which the firmware's boot probe reports back so the app can tell whether it
# wrote the right one. `panelRevision` only means anything on the E213, whose
# two panel controllers are electrically indistinguishable and so remain the one
# thing hardware cannot answer for itself.
VARIANTS = [
    {
        "id": "vision-master-e213-v11",
        "label": "Vision Master E213 (panel v1.1)",
        "panel": "e213",
        "panelRevision": "v11",
    },
    {
        "id": "vision-master-e213",
        "label": "Vision Master E213 (original panel)",
        "panel": "e213",
        "panelRevision": "original",
    },
    {
        "id": "vision-master-e290",
        "label": "Vision Master E290",
        "panel": "e290",
        "panelRevision": None,
    },
]

BUILD_ID_PATHS = ["firmware/panel/src", "firmware/panel/platformio.ini"]


def git(*args: str) -> str:
    return subprocess.check_output(
        ["git", *args], text=True, cwd=REPOSITORY_ROOT
    ).strip()


def is_shallow() -> bool:
    """Whether this checkout lacks the history the build id is derived from.

    A shallow clone has no parent to diff against, so git attributes every
    tracked path to the tip commit and the query below answers with the release
    sha instead of the last firmware change. That is not a missing answer, it is
    a confidently wrong one: it changes on every release, marks every device
    outdated, and offers a reflash that writes byte-identical firmware.
    """
    try:
        return git("rev-parse", "--is-shallow-repository") == "true"
    except (subprocess.CalledProcessError, OSError):
        return False


def source_revision() -> str | None:
    """Identity of the firmware sources, matching what the device reports.

    Derived from the tracked firmware paths rather than HEAD so a commit that
    touches nothing under firmware/ does not make every device look outdated.

    This must stay identical to `build_id.py`, which stamps the same value into
    the banner the device speaks: the app compares the two strings, so any
    disagreement reports every board as running the wrong firmware.
    """
    try:
        revision = git("log", "-1", "--format=%h", "--", *BUILD_ID_PATHS)
        if not revision:
            return None
        dirty = git("status", "--porcelain", "--", *BUILD_ID_PATHS)
        return f"{revision}-dirty" if dirty else revision
    except (subprocess.CalledProcessError, OSError):
        return None


def platformio() -> str | None:
    return shutil.which("pio") or shutil.which("platformio")


def build(environment: str, executable: str) -> None:
    subprocess.run(
        [executable, "run", "-e", environment],
        cwd=FIRMWARE_ROOT,
        check=True,
    )


def boot_app0(environment: str) -> Path | None:
    """Locates boot_app0.bin inside the installed Arduino framework."""
    candidates = sorted(
        (Path.home() / ".platformio" / "packages").glob(
            "framework-arduinoespressif32*/tools/partitions/boot_app0.bin"
        )
    )
    return candidates[0] if candidates else None


def collect(environment: str) -> list[dict[str, str]] | None:
    build_dir = FIRMWARE_ROOT / ".pio" / "build" / environment
    target = OUTPUT_ROOT / environment
    target.mkdir(parents=True, exist_ok=True)

    images = []
    for offset, name in LAYOUT:
        source = boot_app0(environment) if name == "boot_app0.bin" else build_dir / name
        if source is None or not source.is_file():
            print(f"  missing {name} for {environment} (looked in {source})")
            return None
        shutil.copyfile(source, target / name)
        images.append({"offset": offset, "file": name})
    return images


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="package whatever is already in .pio/build instead of compiling",
    )
    arguments = parser.parse_args()

    if is_shallow():
        print("This is a shallow clone, so the firmware build id cannot be trusted.")
        print("Run `git fetch --unshallow`, or set fetch-depth: 0 on the checkout.")
        return 1

    if OUTPUT_ROOT.exists():
        shutil.rmtree(OUTPUT_ROOT)
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)

    executable = platformio()
    if executable is None and not arguments.skip_build:
        print("PlatformIO not found; writing an empty firmware bundle.")
        print("Install it, or pass --skip-build to package an existing .pio/build.")

    variants = []
    for variant in VARIANTS:
        environment = variant["id"]
        if executable is not None and not arguments.skip_build:
            print(f"building {environment}...")
            build(environment, executable)
        images = collect(environment)
        if images is None:
            print(f"  skipping {environment}: images unavailable")
            continue
        variants.append({**variant, "images": images})

    manifest = {
        # 2: variants name the board they drive, because there is now more than
        # one board and the app picks between them without asking.
        "schemaVersion": 2,
        "chip": "esp32s3",
        "sourceRevision": source_revision(),
        "variants": variants,
    }
    (OUTPUT_ROOT / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )

    if not variants:
        print(f"wrote an empty firmware manifest to {OUTPUT_ROOT}")
        print("this build of the app will report that it ships no firmware")
        return 0

    total = sum(
        (OUTPUT_ROOT / variant["id"] / image["file"]).stat().st_size
        for variant in variants
        for image in variant["images"]
    )
    print(
        f"bundled {len(variants)} variant(s), {total / 1024:.0f} KiB, "
        f"revision {manifest['sourceRevision']}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
