#!/usr/bin/env python3
"""Focused lineage falsifiers for post-strip release package evidence."""

from __future__ import annotations

import hashlib
import io
import importlib.util
import json
import os
import stat
import subprocess
import sys
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


def canonical(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def fixture(root: Path, target: str = TARGET) -> tuple[Path, Path, Path]:
    windows = "windows" in target
    build_dir = root / "target" / target / "release"
    package_dir = root / f"perllsp-{VERSION}-{target}"
    build_dir.mkdir(parents=True)
    package_dir.mkdir()
    identity = {
        "schema_version": "perl_lsp.release_build_identity.v1",
        "repository": "EffortlessMetrics/perl-lsp-swarm",
        "source_revision": SOURCE,
        "source_tree_digest": "b" * 64,
        "release_version": VERSION,
        "target": target,
        "profile": "release",
        "candidate_identity": "private-candidate-1",
        "artifact_role": "archive",
        "product_identity_contract_digest": "c" * 64,
        "release_topology_digest": "d" * 64,
        "toolchain_digest": "e" * 64,
    }
    binaries = []
    for executable, role in (("perllsp", "server"), ("perl-dap", "dap")):
        file_name = executable + (".exe" if windows else "")
        before = f"build-{executable}".encode()
        after = f"post-strip-{executable}".encode()
        (build_dir / file_name).write_bytes(before)
        (package_dir / file_name).write_bytes(after)
        packet = {
            "schema_version": "perl_lsp.binary_identity.v1",
            "product": {
                "name": "perl-lsp",
                "public_repository": "EffortlessMetrics/perl-lsp",
                "development_repository": "EffortlessMetrics/perl-lsp-swarm",
            },
            "binary": {
                "executable": executable,
                "cargo_package": executable,
                "role": role,
                "version": VERSION,
            },
            "build": {
                "source_revision": SOURCE,
                "source_tree_digest": identity["source_tree_digest"],
                "target": target,
                "profile": "release",
                "identity_state": "exact",
            },
            "artifact": {
                "role": "archive",
                "candidate_identity": identity["candidate_identity"],
            },
            "compatibility": {
                "expected_product_identity_version": 1,
                "dap_posture": "preview",
            },
            "limitations": ["artifact_digest_not_externally_bound"],
        }
        binaries.append(
            {
                "executable": executable,
                "role": role,
                "path_role": f"target/{target}/release/{file_name}",
                "file_sha256": sha(before),
                "packet_sha256": sha(canonical(packet)),
                "packet": packet,
            }
        )
    receipt = root / "receipt.json"
    receipt.write_text(
        json.dumps(
            {
                "schema_version": "perl_lsp.release_build_identity_receipt.v1",
                "status": "pass",
                "input": identity,
                "input_sha256": sha(canonical(identity)),
                "runner": "cargo",
                "build_execution": "external_release_workflow",
                "build_commands": [
                    ["cargo", "build", "--locked", "--release", "--target", target,
                     "-p", executable, "--bin", executable]
                    for executable in ("perllsp", "perl-dap")
                ],
                "binaries": binaries,
                "claim_boundary": "Synthetic receipt and literal file bytes; no compiler or installed process executed.",
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
    def assert_receipt_refused(self, mutate) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt, package_dir, archive = fixture(root)
            value = json.loads(receipt.read_text())
            mutate(value)
            receipt.write_bytes(canonical(value))
            previous = Path.cwd()
            os.chdir(root)
            try:
                with self.assertRaisesRegex(subject.PackageEvidenceError, "receipt"):
                    subject.build(receipt, package_dir, archive, SOURCE, VERSION, TARGET)
            finally:
                os.chdir(previous)

    def test_missing_or_unknown_receipt_fields_fail_closed(self) -> None:
        for field in ("schema_version", "input_sha256", "runner", "build_execution",
                      "build_commands", "claim_boundary"):
            with self.subTest(missing=field):
                self.assert_receipt_refused(lambda value: value.pop(field))
        self.assert_receipt_refused(lambda value: value.update(schema_version="future.v2"))
        self.assert_receipt_refused(lambda value: value.update(extra="unrecognized"))

    def test_input_digest_must_bind_the_complete_identity(self) -> None:
        self.assert_receipt_refused(lambda value: value.update(input_sha256="f" * 64))
        self.assert_receipt_refused(
            lambda value: value["input"].update(candidate_identity="other-candidate")
        )

    def test_invalid_input_cannot_pass_with_a_recomputed_digest(self) -> None:
        for field, invalid in (("profile", "debug"), ("schema_version", "future.v2"),
                               ("source_tree_digest", "not-a-digest")):
            def mutate(value):
                value["input"][field] = invalid
                value["input_sha256"] = sha(canonical(value["input"]))
            with self.subTest(field=field):
                self.assert_receipt_refused(mutate)

    def test_package_evidence_requires_archive_identity(self) -> None:
        for artifact_role in ("managed", "package_install"):
            def mutate(value):
                value["input"]["artifact_role"] = artifact_role
                value["input_sha256"] = sha(canonical(value["input"]))
            with self.subTest(artifact_role=artifact_role):
                self.assert_receipt_refused(mutate)

    def test_execution_and_commands_must_match_the_producer(self) -> None:
        for field, invalid in (("runner", "shell"), ("runner", []),
                               ("build_execution", "declared"), ("build_execution", {}),
                               ("build_commands", []), ("claim_boundary", "")):
            with self.subTest(field=field, invalid=invalid):
                self.assert_receipt_refused(lambda value: value.update({field: invalid}))

    def test_both_packet_digests_and_packet_subjects_are_checked(self) -> None:
        for index in (0, 1):
            with self.subTest(index=index, mutation="digest"):
                self.assert_receipt_refused(
                    lambda value: value["binaries"][index].update(packet_sha256="f" * 64)
                )
            for field, invalid in (("source_revision", "f" * 40),
                                   ("target", WINDOWS_TARGET), ("identity_state", "partial")):
                def mutate(value):
                    row = value["binaries"][index]
                    row["packet"]["build"][field] = invalid
                    row["packet_sha256"] = sha(canonical(row["packet"]))
                with self.subTest(index=index, field=field):
                    self.assert_receipt_refused(mutate)

    def test_binary_role_and_packet_presence_are_required(self) -> None:
        for index in (0, 1):
            for field in ("packet", "packet_sha256", "role"):
                with self.subTest(index=index, missing=field):
                    self.assert_receipt_refused(lambda value: value["binaries"][index].pop(field))
        self.assert_receipt_refused(lambda value: value["binaries"].reverse())
        self.assert_receipt_refused(lambda value: value["binaries"][0].update(role="dap"))

    def test_self_reported_executable_digest_is_not_provenance(self) -> None:
        def mutate(value):
            row = value["binaries"][0]
            row["packet"]["artifact"]["digest"] = row["file_sha256"]
            row["packet_sha256"] = sha(canonical(row["packet"]))
        self.assert_receipt_refused(mutate)

    def test_supported_build_routes_and_private_candidate_ids_remain_accepted(self) -> None:
        for runner in ("cargo", "cross"):
            for execution in ("adapter", "external_release_workflow"):
                with self.subTest(runner=runner, execution=execution):
                    with tempfile.TemporaryDirectory() as directory:
                        root = Path(directory)
                        receipt, package_dir, archive = fixture(root)
                        value = json.loads(receipt.read_text())
                        value["runner"] = runner
                        value["build_execution"] = execution
                        for command in value["build_commands"]:
                            command[0] = runner
                        receipt.write_bytes(canonical(value))
                        previous = Path.cwd()
                        os.chdir(root)
                        try:
                            result = subject.build(receipt, package_dir, archive, SOURCE, VERSION, TARGET)
                        finally:
                            os.chdir(previous)
                        self.assertEqual(result["status"], "pass")

    def test_cli_refuses_a_stale_receipt_without_writing_passing_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt, package_dir, archive = fixture(root)
            value = json.loads(receipt.read_text())
            value["binaries"][1]["packet"]["build"]["source_revision"] = "f" * 40
            value["binaries"][1]["packet_sha256"] = sha(canonical(value["binaries"][1]["packet"]))
            receipt.write_bytes(canonical(value))
            output = root / "package-evidence.json"
            result = subprocess.run(
                [sys.executable, "-B", str(MODULE.resolve()), "--receipt", str(receipt),
                 "--package-dir", str(package_dir), "--archive", str(archive),
                 "--source-sha", SOURCE, "--version", VERSION, "--target", TARGET,
                 "--output", str(output)],
                cwd=root, capture_output=True, text=True, check=False,
            )
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("NOT_PROVEN", result.stderr)
            self.assertIn("receipt", result.stderr)
            self.assertFalse(output.exists())

    def test_identical_duplicate_tar_member_is_rejected_by_build(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            receipt, package_dir, archive = fixture(root)
            package_name = package_dir.name
            with tarfile.open(archive, "w:gz") as bundle:
                for payload in (b"post-strip-perllsp", b"post-strip-perllsp"):
                    member = tarfile.TarInfo(f"{package_name}/perllsp")
                    member.size = len(payload)
                    bundle.addfile(member, io.BytesIO(payload))
                payload = b"post-strip-perl-dap"
                member = tarfile.TarInfo(f"{package_name}/perl-dap")
                member.size = len(payload)
                bundle.addfile(member, io.BytesIO(payload))
            previous = Path.cwd()
            os.chdir(root)
            try:
                with self.assertRaisesRegex(subject.PackageEvidenceError, "duplicate"):
                    subject.build(receipt, package_dir, archive, SOURCE, VERSION, TARGET)
            finally:
                os.chdir(previous)

    def test_archive_member_digest_rejects_ambiguous_and_nonregular_members(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            duplicate = root / "duplicate.zip"
            with zipfile.ZipFile(duplicate, "w") as bundle:
                bundle.writestr("perllsp.exe", b"same")
                bundle.writestr("perllsp.exe", b"same")
            with self.assertRaisesRegex(subject.PackageEvidenceError, "duplicate"):
                subject.archive_member_digest(duplicate, "perllsp.exe")

            link = root / "link.zip"
            info = zipfile.ZipInfo("perllsp.exe")
            info.create_system = 3
            info.external_attr = (stat.S_IFLNK | 0o777) << 16
            with zipfile.ZipFile(link, "w") as bundle:
                bundle.writestr(info, b"target")
            with self.assertRaisesRegex(subject.PackageEvidenceError, "regular"):
                subject.archive_member_digest(link, "perllsp.exe")

            creator_bypass = root / "creator-bypass.zip"
            info = zipfile.ZipInfo("perllsp.exe")
            info.create_system = 0
            info.external_attr = (stat.S_IFLNK | 0o777) << 16
            with zipfile.ZipFile(creator_bypass, "w") as bundle:
                bundle.writestr(info, b"target")
            with self.assertRaisesRegex(subject.PackageEvidenceError, "regular"):
                subject.archive_member_digest(creator_bypass, "perllsp.exe")

            dos_bypass = root / "dos-bypass.zip"
            info = zipfile.ZipInfo("perllsp.exe")
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | 0o755) << 16 | 0x10
            with zipfile.ZipFile(dos_bypass, "w") as bundle:
                bundle.writestr(info, b"regular")
            with self.assertRaisesRegex(subject.PackageEvidenceError, "regular"):
                subject.archive_member_digest(dos_bypass, "perllsp.exe")

            unix_regular = root / "unix-regular.zip"
            info = zipfile.ZipInfo("perllsp.exe")
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | 0o755) << 16
            with zipfile.ZipFile(unix_regular, "w") as bundle:
                bundle.writestr(info, b"regular")
            self.assertEqual(
                subject.archive_member_digest(unix_regular, "perllsp.exe"), sha(b"regular")
            )

            tar_link = root / "link.tar.gz"
            with tarfile.open(tar_link, "w:gz") as bundle:
                member = tarfile.TarInfo("perllsp")
                member.type = tarfile.SYMTYPE
                member.linkname = "target"
                bundle.addfile(member)
            with self.assertRaisesRegex(subject.PackageEvidenceError, "regular"):
                subject.archive_member_digest(tar_link, "perllsp")

            tar_hardlink = root / "hardlink.tar.gz"
            with tarfile.open(tar_hardlink, "w:gz") as bundle:
                member = tarfile.TarInfo("perllsp")
                member.type = tarfile.LNKTYPE
                member.linkname = "target"
                bundle.addfile(member)
            with self.assertRaisesRegex(subject.PackageEvidenceError, "regular"):
                subject.archive_member_digest(tar_hardlink, "perllsp")

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
