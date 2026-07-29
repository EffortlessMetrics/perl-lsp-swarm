#!/usr/bin/env python3
"""Materialize the modern agent-flow tree from a reviewable bootstrap archive."""

from __future__ import annotations

import base64
import io
import shutil
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PARTS = ROOT / ".agent-flow-bootstrap"
WORKFLOW = ROOT / ".github" / "workflows" / "materialize-agent-flow.yml"
EXPECTED_SHA256 = "50a1f3d57793495731d35b0f9cdce2839588451facb63d7caf55d8bab8231a21"


def safe_member(member: tarfile.TarInfo) -> None:
    path = Path(member.name)
    if path.is_absolute() or ".." in path.parts:
        raise RuntimeError(f"unsafe archive path: {member.name}")
    if not member.isfile():
        raise RuntimeError(f"unexpected non-file archive member: {member.name}")


def main() -> int:
    import hashlib

    encoded = "".join(
        path.read_text(encoding="ascii").strip()
        for path in sorted(PARTS.glob("part-*.b64"))
    )
    archive = base64.b64decode(encoded, validate=True)
    actual = hashlib.sha256(archive).hexdigest()
    if actual != EXPECTED_SHA256:
        raise RuntimeError(f"archive digest mismatch: expected {EXPECTED_SHA256}, got {actual}")

    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
        members = bundle.getmembers()
        for member in members:
            safe_member(member)
        bundle.extractall(ROOT, members=members, filter="data")

    shutil.rmtree(PARTS)
    Path(__file__).unlink()
    WORKFLOW.unlink()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
