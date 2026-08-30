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
import zipfile
from pathlib import Path

MODULE = Path(__file__).with_name("release_package_evidence.py")
SPEC = importlib.util.spec_from_file_location("release_package_evidence", MODULE)
assert SPEC is not None and SPEC.loader is not None
subject = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(subject)

SOURCE = "a" * 40
VERSION = "0.18.0-rc.1"
TARGET = "x86_64-unknown-linux-gnu"
WINDOWS_TARGET = "x86_64-pc-windows-msvc"


def sha(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def fixture(root: Path, target: str = TARGET) -> tuple[Path, Path, Path]:
    windows = "windows" in target
    build_dir = root / "target" / target / "release"
    package_dir = root / f"perllsp-{VERSION}-{target}"
    build_dir.mkdir(parents=True)
    package_dir.mkdir()
    binaries = []
    for executable, role in (("perllsp", "server"), ("perl-dap", "dap")):
        file_name = executable + (".exe" if windows else "")
        before = f"build-{executable}".encode()
        after = f"post-strip-{executable}".encode()
        (build_dir / file_name).write_bytes(before)
        (package_dir / file_name).write_bytes(after)
        binaries.append(
            {
                "executable": executable,
                "role": role,
                "path_role": f"target/{target}/release/{file_name}",
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
                    "target": target,
                },
                "binaries": binaries,
            }
        ),
        encoding="utf-8",
    )
    if windows:
        archive = root / f"{package_dir.name}.zip"
        with zipfile.ZipFile(archive, "w") as bundle:
            for path in package_dir.iterdir():
                bundle.write(path, arcname=path.name)
    else:
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

    def test_windows_zip_uses_flat_binary_members(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt, package_dir, archive = fixture(root, WINDOWS_TARGET)
            previous = Path.cwd()
            os.chdir(root)
            try:
                value = subject.build(
                    receipt, package_dir, archive, SOURCE, VERSION, WINDOWS_TARGET
                )
            finally:
                os.chdir(previous)
            self.assertEqual(
                [row["member_path"] for row in value["binaries"]],
                ["perllsp.exe", "perl-dap.exe"],
            )

    def test_windows_zip_member_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt, package_dir, archive = fixture(root, WINDOWS_TARGET)
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("perllsp.exe", b"drift")
                bundle.write(package_dir / "perl-dap.exe", arcname="perl-dap.exe")
            previous = Path.cwd()
            os.chdir(root)
            try:
                with self.assertRaisesRegex(subject.PackageEvidenceError, "archive member"):
                    subject.build(
                        receipt, package_dir, archive, SOURCE, VERSION, WINDOWS_TARGET
                    )
            finally:
                os.chdir(previous)


if __name__ == "__main__":
    unittest.main()
