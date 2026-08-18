"""Injects BRICKELLSTATUS_BUILD_ID so the device banner identifies its firmware.

The id is derived from the tracked firmware sources rather than from HEAD, so a
commit that touches nothing under firmware/ does not invalidate a device that is
genuinely up to date, and a dirty working tree is never reported as a release
build.

It must agree exactly with `bundle_firmware.py`, which stamps the same value
into the manifest as `sourceRevision`: the desktop app compares the two strings
to decide whether an attached board is running what it ships. The pathspecs
below are therefore repository-relative, and git is run from the repository
root -- PlatformIO runs extra scripts from the project directory, where a
repository-relative pathspec quietly matches nothing and `git log` exits zero
with no output. Every board built that way announced itself as "unknown" and was
offered a reflash on every launch, forever.

Verify a change here by building, not by reading. This script is `exec`'d by
SCons inside the build, where several ordinary assumptions -- `__file__` among
them -- do not hold.
"""

import subprocess

Import("env")  # noqa: F821  (PlatformIO injects this)

# SCons `exec`s this file rather than importing it, so there is no `__file__` to
# locate the repository from -- asking for one fails the build outright. The
# project directory is what PlatformIO does hand us, and git itself resolves the
# rest, which also means no assumption about how deeply the firmware is nested.
PROJECT_DIR = env.subst("$PROJECT_DIR")  # noqa: F821
FIRMWARE_PATHS = ["firmware/panel/src", "firmware/panel/platformio.ini"]


def _git(*args: str, cwd: str) -> str:
    return subprocess.check_output(["git", *args], text=True, cwd=cwd).strip()


def build_id() -> str:
    try:
        root = _git("rev-parse", "--show-toplevel", cwd=PROJECT_DIR)
        # A shallow clone has no parent to diff against, so git attributes every
        # path to the tip commit and the query below answers with whatever
        # commit is checked out. Claiming that as the firmware's identity is
        # worse than admitting we do not know it.
        if _git("rev-parse", "--is-shallow-repository", cwd=root) == "true":
            print("build_id: shallow clone, cannot identify the firmware source")
            return "unknown"
        revision = _git("log", "-1", "--format=%h", "--", *FIRMWARE_PATHS, cwd=root)
        if not revision:
            return "unknown"
        dirty = _git("status", "--porcelain", "--", *FIRMWARE_PATHS, cwd=root)
        return f"{revision}-dirty" if dirty else revision
    except (subprocess.CalledProcessError, OSError):
        # A source tarball without git history still builds; it just cannot
        # claim an identity, and "unknown" is the honest answer. The app reads
        # "unknown" as "no build id", never as a build that differs from its
        # own, so such a board is not nagged to reflash.
        return "unknown"


env.Append(CPPDEFINES=[("BRICKELLSTATUS_BUILD_ID", env.StringifyMacro(build_id()))])  # noqa: F821
