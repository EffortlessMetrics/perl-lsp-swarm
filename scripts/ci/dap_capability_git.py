"""Git-object and hosted-run binding for DAP capability receipts."""

from __future__ import annotations

import hashlib
import subprocess
from pathlib import Path
from typing import Any, Mapping, Sequence

from dap_capability_common import MatrixError, validate_run_identity


def _run(
    argv: Sequence[str], *, root: Path, text: bool = True
) -> subprocess.CompletedProcess[Any]:
    try:
        return subprocess.run(
            list(argv),
            cwd=root,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=text,
            timeout=20,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise MatrixError(f"cannot execute {' '.join(argv)!r}: {exc}") from exc


def run_text(argv: Sequence[str], *, root: Path) -> str:
    result = _run(argv, root=root, text=True)
    output = result.stdout.strip()
    if result.returncode != 0:
        raise MatrixError(
            f"command {' '.join(argv)!r} failed with exit {result.returncode}: "
            f"{output or '<no output>'}"
        )
    return output


def run_bytes(argv: Sequence[str], *, root: Path) -> bytes:
    result = _run(argv, root=root, text=False)
    output = bytes(result.stdout)
    if result.returncode != 0:
        rendered = output.decode("utf-8", errors="replace").strip()
        raise MatrixError(
            f"command {' '.join(argv)!r} failed with exit {result.returncode}: "
            f"{rendered or '<no output>'}"
        )
    return output


def verify_candidate(root: Path, repository_sha: str, run_id: str, run_attempt: str) -> None:
    validate_run_identity(repository_sha, run_id, run_attempt)
    head = run_text(["git", "rev-parse", "HEAD"], root=root)
    if head != repository_sha:
        raise MatrixError(f"checked-out HEAD differs from candidate: {head} != {repository_sha}")
    resolved = run_text(["git", "rev-parse", f"{repository_sha}^{{commit}}"], root=root)
    if resolved != repository_sha:
        raise MatrixError(
            f"candidate SHA does not resolve exactly: {resolved} != {repository_sha}"
        )


def assert_clean_tree(root: Path) -> None:
    result = _run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        root=root,
        text=True,
    )
    output = result.stdout.strip()
    if result.returncode != 0:
        raise MatrixError(
            f"git status failed with exit {result.returncode}: {output or '<no output>'}"
        )
    if output:
        raise MatrixError(f"candidate tree is not clean after validation:\n{output}")


def tracked_record(root: Path, path: Path, repository_sha: str) -> dict[str, str]:
    relative = path.as_posix()
    blob_sha = run_text(
        ["git", "rev-parse", f"{repository_sha}:{relative}"],
        root=root,
    )
    expected = run_bytes(["git", "show", f"{repository_sha}:{relative}"], root=root)
    try:
        actual = (root / path).read_bytes()
    except OSError as exc:
        raise MatrixError(f"cannot read tracked capability subject {relative}: {exc}") from exc
    if actual != expected:
        raise MatrixError(
            f"tracked capability subject differs from candidate Git object: {relative}"
        )
    return {
        "path": relative,
        "git_blob_sha1": blob_sha,
        "sha256": hashlib.sha256(actual).hexdigest(),
    }


def tracked_records(
    root: Path, paths: Sequence[Path], repository_sha: str
) -> list[Mapping[str, str]]:
    if len(paths) != len(set(paths)):
        raise MatrixError("duplicate capability receipt subject path")
    return [
        tracked_record(root, path, repository_sha)
        for path in sorted(paths, key=lambda candidate: candidate.as_posix())
    ]
