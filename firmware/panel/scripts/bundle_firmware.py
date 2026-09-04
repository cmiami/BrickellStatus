"""Assembles the firmware images the desktop app ships and flashes.

Run from the repository root:

    python3 firmware/panel/scripts/bundle_firmware.py [--skip-build]

Writes `apps/desktop/src-tauri/resources/firmware/` containing one directory per
PlatformIO environment plus a `manifest.json` describing the flash layout.

PlatformIO is not required to *build* the desktop app. Without current cached
images the script writes an empty manifest, and the app reports that this
build ships no firmware. That keeps `tauri.conf.json`'s declared resource
directory present: the Tauri build script rejects a missing bundle resource, and
a contributor without an embedded toolchain must still be able to build the app.
"""

from __future__ import annotations

import argparse
import hashlib
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

# E213 is one product-facing panel with two internal controller images. The app
# never asks a reader to choose between them: it requires READY from the first
# and tries the other once on silence. Wireless Paper controller detection runs
# inside its single firmware image, so that board also remains one product and
# one Flash action.
VARIANTS = [
    {
        "id": "vision-master-e213-v11",
        "label": "Vision Master E213",
        "panel": "e213",
        "hardware": "visionMasterE213",
        "panelRevision": "v11",
    },
    {
        "id": "vision-master-e213",
        "label": "Vision Master E213",
        "panel": "e213",
        "hardware": "visionMasterE213",
        "panelRevision": "original",
    },
    {
        "id": "vision-master-e290",
        "label": "Vision Master E290",
        "panel": "e290",
        "hardware": "visionMasterE290",
        "panelRevision": None,
    },
    {
        "id": "wireless-paper",
        "label": "Wireless Paper",
        "panel": "e213",
        "hardware": "wirelessPaper",
        "panelRevision": None,
    },
]

BUILD_ID_PATHS = [
    "firmware/panel/src",
    "firmware/panel/platformio.ini",
    "firmware/panel/scripts/build_id.py",
    "firmware/panel/version.txt",
]
FIRMWARE_VERSION_PATH = FIRMWARE_ROOT / "version.txt"


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


def content_digest() -> str:
    """Matches build_id.py's deterministic identity for dirty inputs."""
    digest = hashlib.sha256()
    files: list[Path] = []
    for relative in BUILD_ID_PATHS:
        candidate = REPOSITORY_ROOT / relative
        files.extend(
            path for path in (candidate.rglob("*") if candidate.is_dir() else [candidate])
            if path.is_file()
        )
    for path in sorted(files):
        relative = path.relative_to(REPOSITORY_ROOT).as_posix().encode()
        digest.update(relative)
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()[:12]


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
        return f"{revision}-dirty-{content_digest()}" if dirty else revision
    except (subprocess.CalledProcessError, OSError):
        return None


def firmware_version() -> int:
    """Monotonic firmware release shared with the compiled device images."""
    value = int(FIRMWARE_VERSION_PATH.read_text(encoding="utf-8").strip())
    if value <= 0:
        raise ValueError("firmware version must be a positive integer")
    return value


def platformio() -> str | None:
    return shutil.which("pio") or shutil.which("platformio")


def build(environment: str, executable: str) -> None:
    subprocess.run(
        [executable, "run", "-e", environment],
        cwd=FIRMWARE_ROOT,
        check=True,
    )


def boot_app0() -> Path | None:
    """Locates boot_app0.bin inside the installed Arduino framework."""
    candidates = sorted(
        (Path.home() / ".platformio" / "packages").glob(
            "framework-arduinoespressif32*/tools/partitions/boot_app0.bin"
        )
    )
    return candidates[0] if candidates else None


def collect(environment: str, revision: str | None) -> list[dict[str, str]] | None:
    build_dir = FIRMWARE_ROOT / ".pio" / "build" / environment
    target = OUTPUT_ROOT / environment

    sources = []
    for offset, name in LAYOUT:
        source = boot_app0() if name == "boot_app0.bin" else build_dir / name
        if source is None or not source.is_file():
            print(f"  missing {name} for {environment} (looked in {source})")
            return None
        sources.append((offset, name, source))

    # A cached build can predate the checkout. The manifest must identify the
    # image actually flashed, not relabel an old binary with today's revision.
    # Match the complete C string so a dirty build cannot pass as its clean base.
    application = (build_dir / "firmware.bin").read_bytes()
    if not revision or revision == "unknown" or revision.encode() + b"\0" not in application:
        print(f"  {environment}: cached image does not announce source revision {revision!r}")
        return None

    target.mkdir(parents=True, exist_ok=True)
    images = []
    for offset, name, source in sources:
        shutil.copyfile(source, target / name)
        images.append({"offset": offset, "file": name})
    return images


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="package existing .pio/build images only when their build id matches the sources",
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
        print("PlatformIO not found; only current cached firmware can be bundled.")
        print("Install it to rebuild missing or outdated images.")

    revision = source_revision()
    variants = []
    for variant in VARIANTS:
        environment = variant["id"]
        if executable is not None and not arguments.skip_build:
            print(f"building {environment}...")
            build(environment, executable)
        images = collect(environment, revision)
        if images is None:
            if executable is not None and not arguments.skip_build:
                raise RuntimeError(f"{environment}: freshly built images are missing or have the wrong build id")
            print(f"  skipping {environment}: current images unavailable; rebuild with PlatformIO")
            continue
        variants.append({**variant, "images": images})

    manifest = {
        # 4: exact board hardware is separate from framebuffer geometry, and
        # the manifest carries the device banner's monotonic firmware version.
        "schemaVersion": 4,
        "chip": "esp32s3",
        "firmwareVersion": firmware_version(),
        "sourceRevision": revision,
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
        f"firmware v{manifest['firmwareVersion']} · revision {manifest['sourceRevision']}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
