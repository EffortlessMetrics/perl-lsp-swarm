#!/usr/bin/env python3
"""Fixture proof for omitted and reused 11983 reject identities."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).parents[2]
MODULE_PATH = ROOT / "scripts/maintenance/verify_11983_reject_identities.py"
SPEC = importlib.util.spec_from_file_location("verify_11983", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load {MODULE_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def write_manifest(root: Path, *, duplicate_first: bool = False, omit_identity: bool = False) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    manifest = root / "manifest.tsv"
    rows = []
    for index, (path, identities) in enumerate(MODULE.EXPECTED_REJECT_IDENTITIES.items()):
        patch = root / f"patch-{index}.diff"
        log = root / f"log-{index}.txt"
        reject = root / f"reject-{index}.rej"
        segments = [f"{identity.hunk}\n {identity.old_anchor}\n" for identity in identities]
        if omit_identity and index == 4:
            segments.pop()
        if duplicate_first and index == 0:
            segments.append(segments[0])
        patch.write_text("\n".join(segments), encoding="utf-8")
        reject.write_text("\n".join(segments), encoding="utf-8")
        log.write_text("".join(f"Rejected hunk #{number}.\n" for number in range(1, len(segments) + 1)), encoding="utf-8")
        rows.append(f"{path}\t{patch}\t{log}\t{reject}\n")
    manifest.write_text("".join(rows), encoding="utf-8")
    return manifest


def expect_rejection(manifest: Path, evidence: Path, phrase: str) -> None:
    try:
        MODULE.validate_manifest(manifest, evidence)
    except ValueError as error:
        if phrase not in str(error):
            raise RuntimeError(f"unexpected rejection: {error}") from error
    else:
        raise RuntimeError(f"fixture unexpectedly passed: {phrase}")


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="rebuild-11983-identities-") as directory:
        root = Path(directory)
        MODULE.validate_manifest(write_manifest(root / "positive"), root / "positive-evidence")
        expect_rejection(
            write_manifest(root / "duplicate", duplicate_first=True),
            root / "duplicate-evidence",
            "exactly match",
        )
        expect_rejection(
            write_manifest(root / "missing", omit_identity=True),
            root / "missing-evidence",
            "exactly match",
        )
    print("11983 reject-identity fixtures passed")


if __name__ == "__main__":
    main()
