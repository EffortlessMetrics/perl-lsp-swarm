#!/usr/bin/env python3
"""Focused lineage falsifiers for post-strip release package evidence."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import tarfile
import tempfile
import unittest
from pathlib import Path

MODULE = Path(__file__).with_name("release_package_evidence.py")
SPEC = importlib.util.spec_from_file_location("release_package_evidence", MODULE)
assert SPEC is not None and SPEC.loader is not None
subject = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(subject)

SOURCE = "a" * 40
VERSION = "0.18.0-rc.1"
TARGET = "x86_64-unknown-linux-gnu"


def sha(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def fixture(root: Path) -> tuple[Path, Path, Path]:
    build_dir = root / "target" / TARGET / "release"
    package_dir = root / f"perllsp-{VERSION}-{TARGET}"
    build_dir.mkdir(parents=True)
    package_dir.mkdir()
    binaries = []
    for executable, role in (("perllsp", "server"), ("perl-dap", "dap")):
        before = f"build-{executable}".encode()
        after = f"post-strip-{executable}".encode()
        (build_dir / executable).write_bytes(before)
        (package_dir / executable).write_bytes(after)
        binaries.append(
            {
                "executable": executable,
                "role": role,
                "path_role": f"target/{TARGET}/release/{executable}",
                "file_sha256": sha(before),
            }
        )
    receipt = root / "receipt.json"
    receipt.write_text(
        json.dumps(
            {
                "status": "pass",
                "input": {
                    "source_revision": SOURCE,
                    "release_version": VERSION,
                    "target": TARGET,
                },
                "binaries": binaries,
            }
        ),
        encoding="utf-8",
    )
    archive = root / f"{package_dir.name}.tar.gz"
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(package_dir, arcname=package_dir.name)
    return receipt, package_dir, archive


class ReleasePackageEvidenceTests(unittest.TestCase):
    def test_post_strip_members_are_bound_to_pre_strip_receipt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt, package_dir, archive = fixture(root)
            previous = Path.cwd()
            os.chdir(root)
            try:
                value = subject.build(receipt, package_dir, archive, SOURCE, VERSION, TARGET)
            finally:
                os.chdir(previous)
            self.assertEqual(value["status"], "pass")
            self.assertEqual(len(value["binaries"]), 2)

    def test_pre_strip_digest_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt, package_dir, archive = fixture(root)
            (root / "target" / TARGET / "release" / "perllsp").write_bytes(b"drift")
            previous = Path.cwd()
            os.chdir(root)
            try:
                with self.assertRaisesRegex(subject.PackageEvidenceError, "pre-strip"):
                    subject.build(receipt, package_dir, archive, SOURCE, VERSION, TARGET)
            finally:
                os.chdir(previous)


if __name__ == "__main__":
    unittest.main()
