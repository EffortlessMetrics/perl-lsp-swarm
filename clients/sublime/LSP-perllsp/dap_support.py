from __future__ import annotations

import hashlib
import os
import shutil
from pathlib import Path
from typing import Callable, Iterable

AUTO = "auto"
DAP_EXECUTABLE = "perl-dap.exe" if os.name == "nt" else "perl-dap"


class DapPathError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _contains_path_separator(value: str) -> bool:
    return "/" in value or "\\" in value


def _resolve_explicit(
    configured: str,
    which: Callable[[str], str | None],
) -> Path:
    expanded = os.path.expandvars(os.path.expanduser(configured))
    candidate = Path(expanded)
    if candidate.is_absolute():
        resolved = candidate
    elif _contains_path_separator(expanded):
        raise DapPathError(
            "LSP-perllsp dap_path must be an absolute path or a bare executable name "
            "resolved through PATH."
        )
    else:
        found = which(expanded)
        if not found:
            raise DapPathError(f"Configured perl-dap executable was not found on PATH: {configured}")
        resolved = Path(found)

    if not resolved.is_file():
        raise DapPathError(f"Configured perl-dap executable was not found: {resolved}")
    return resolved.resolve()


def _sibling_candidates(server_path: str, which: Callable[[str], str | None]) -> Iterable[Path]:
    expanded = os.path.expandvars(os.path.expanduser(server_path))
    candidate = Path(expanded)
    if candidate.is_absolute():
        resolved = candidate
    elif _contains_path_separator(expanded):
        return ()
    else:
        found = which(expanded)
        if not found:
            return ()
        resolved = Path(found)
    return (resolved.resolve().with_name(DAP_EXECUTABLE),)


def resolve_dap_path(
    configured: str,
    *,
    server_path: str = AUTO,
    which: Callable[[str], str | None] = shutil.which,
) -> Path:
    if not isinstance(configured, str) or not configured:
        raise DapPathError("LSP-perllsp dap_path must be a non-empty string.")
    if configured != AUTO:
        return _resolve_explicit(configured, which)

    candidates: list[Path] = []
    if isinstance(server_path, str) and server_path and server_path != AUTO:
        candidates.extend(_sibling_candidates(server_path, which))

    path_candidate = which("perl-dap")
    if path_candidate:
        candidates.append(Path(path_candidate))

    for candidate in candidates:
        if candidate.is_file():
            return candidate.resolve()

    raise DapPathError(
        "perl-dap is not available. Install a matching perl-dap binary on PATH or set "
        "dap_path in Preferences: Package Settings: LSP-perllsp. The current managed "
        "perllsp release does not contain a verified perl-dap artifact."
    )


def dap_command(path: Path) -> list[str]:
    if not path.is_file():
        raise DapPathError(f"perl-dap executable is missing: {path}")
    return [str(path.resolve()), "--stdio"]
