"""Injects the firmware version and source build into the device banner.

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

import hashlib
import subprocess
from pathlib import Path

Import("env")  # noqa: F821  (PlatformIO injects this)

# SCons `exec`s this file rather than importing it, so there is no `__file__` to
# locate the repository from -- asking for one fails the build outright. The
# project directory is what PlatformIO does hand us, and git itself resolves the
# rest, which also means no assumption about how deeply the firmware is nested.
PROJECT_DIR = env.subst("$PROJECT_DIR")  # noqa: F821
FIRMWARE_PATHS = [
    "firmware/panel/src",
    "firmware/panel/platformio.ini",
    "firmware/panel/scripts/build_id.py",
    "firmware/panel/version.txt",
]
VERSION_PATH = Path(PROJECT_DIR) / "version.txt"


def _git(*args: str, cwd: str) -> str:
    return subprocess.check_output(["git", *args], text=True, cwd=cwd).strip()


def _content_digest(root: Path) -> str:
    """Deterministic identity for dirty firmware inputs.

    A suffix such as ``abc1234-dirty`` collapses every uncommitted source state
    into one value and lets stale packaged binaries masquerade as current. The
    digest keeps dirty developer builds exact enough to verify without treating
    it as an ordered release number.
    """
    digest = hashlib.sha256()
    files: list[Path] = []
    for relative in FIRMWARE_PATHS:
        candidate = root / relative
        files.extend(
            path for path in (candidate.rglob("*") if candidate.is_dir() else [candidate])
            if path.is_file()
        )
    for path in sorted(files):
        relative = path.relative_to(root).as_posix().encode()
        digest.update(relative)
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()[:12]


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
        return f"{revision}-dirty-{_content_digest(Path(root))}" if dirty else revision
    except (subprocess.CalledProcessError, OSError):
        # A source tarball without git history still builds; it just cannot
        # claim an identity, and "unknown" is the honest answer. The app reads
        # "unknown" as "no build id", never as a build that differs from its
        # own, so such a board is not nagged to reflash.
        return "unknown"


def firmware_version() -> int:
    """Orderable release number shared by E213 and E290.

    Unlike a Git hash, this is deliberately monotonic. Refuse to build when it
    is absent or malformed: silently stamping an invented version would make a
    downgrade look like an upgrade.
    """
    value = int(VERSION_PATH.read_text(encoding="utf-8").strip())
    if value <= 0:
        raise ValueError("firmware version must be a positive integer")
    return value


env.Append(  # noqa: F821
    CPPDEFINES=[
        ("BRICKELLSTATUS_BUILD_ID", env.StringifyMacro(build_id())),
        ("BRICKELLSTATUS_FIRMWARE_VERSION", firmware_version()),
    ]
)
