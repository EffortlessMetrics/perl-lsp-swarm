#!/usr/bin/env python3
"""Resolve the bash interpreter scripts in this repository should spawn.

Shared helper for every script and oracle that shells out to bash (#15889).

Why this exists: on Windows, ``CreateProcess`` resolves the bare name
``bash`` against System32 *before* PATH, so a host with WSL installed
silently runs WSL bash — where native ``F:/...`` paths do not exist and
every invocation fails 127 — while ``shutil.which("bash")`` points at the
Git/MSYS bash the caller intended (root-caused in #15868 for
``test_verify_staged_binaries.py``).

Usage from repository scripts (the directory layout keeps this module
importable from ``scripts/`` and ``scripts/ci/`` and ``scripts/tests/``)::

    sys.path.insert(0, str(Path(__file__).resolve().parent))        # scripts/
    from bash_binary import bash_binary, bash_path

    subprocess.run([bash_binary(), bash_path(WRAPPER), ...], ...)
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

_WIN_DRIVE_PREFIX = re.compile(r"^[A-Za-z]:[\\/]")

__all__ = ["bash_binary", "bash_path"]


def bash_binary() -> str:
    """Return the bash argv[0] this host should spawn.

    POSIX hosts keep the bare name. On Windows the resolver walks PATH
    explicitly (skipping System32, whose ``bash.exe`` is WSL), then falls
    back to the standard Git-for-Windows install locations, and only then
    returns the bare name so the eventual failure names the real gap.
    """
    if sys.platform != "win32":
        return "bash"
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        # PATH entries may carry surrounding quotes when they contain `;`
        # or spaces; a quoted entry would never match an existing file.
        stripped = entry.strip().strip('"')
        if "system32" in stripped.lower():
            continue
        candidate = Path(stripped) / "bash.exe"
        if candidate.is_file():
            return str(candidate)
    program_files = os.environ.get("ProgramFiles", r"C:\Program Files")
    for candidate in (
        Path(program_files) / "Git" / "bin" / "bash.exe",
        Path(program_files) / "Git" / "usr" / "bin" / "bash.exe",
        Path(r"C:\Program Files (x86)\Git") / "usr" / "bin" / "bash.exe",
    ):
        if candidate.is_file():
            return str(candidate)
    return "bash"


def bash_path(path: Path) -> str:
    r"""Render *path* so bash resolves it on every host.

    Backslashes inside a bash argument are escape characters, so a native
    Windows path such as ``F:\code\x.sh`` must reach bash with forward
    separators. ``Path.as_posix()`` handles the host-native form on both
    platforms; the drive-prefix discrimination below additionally repairs
    Windows-persisted strings seen from POSIX hosts — without touching
    POSIX paths whose components legitimately contain backslashes
    (#15435).
    """
    posix = path.as_posix()
    # A drive-prefixed path ("F:" followed by a separator) persisted by a
    # Windows host can reach this helper through a POSIX harness whose
    # Path keeps the backslashes literal; bash cannot consume that form,
    # so it is rewritten in full. Everything else — including a POSIX
    # path whose components contain backslashes — is returned exactly as
    # as_posix() rendered it, so normalization never corrupts the running
    # host's own paths.
    if _WIN_DRIVE_PREFIX.match(posix):
        return posix.replace("\\", "/")
    return posix
