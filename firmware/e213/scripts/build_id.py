"""Injects TENDERS_LOG_BUILD_ID so the device banner identifies its firmware.

The id is derived from the tracked firmware sources rather than from HEAD, so a
commit that touches nothing under firmware/ does not invalidate a device that is
genuinely up to date, and a dirty working tree is never reported as a release
build.
"""

import subprocess

Import("env")  # noqa: F821  (PlatformIO injects this)

FIRMWARE_PATHS = ["firmware/e213/src", "firmware/e213/platformio.ini"]


def _git(*args: str) -> str:
    return subprocess.check_output(["git", *args], text=True).strip()


def build_id() -> str:
    try:
        revision = _git("log", "-1", "--format=%h", "--", *FIRMWARE_PATHS)
        if not revision:
            return "unknown"
        dirty = _git("status", "--porcelain", "--", *FIRMWARE_PATHS)
        return f"{revision}-dirty" if dirty else revision
    except (subprocess.CalledProcessError, OSError):
        # A source tarball without git history still builds; it just cannot
        # claim an identity, and "unknown" is the honest answer.
        return "unknown"


env.Append(CPPDEFINES=[("TENDERS_LOG_BUILD_ID", env.StringifyMacro(build_id()))])  # noqa: F821
