#!/usr/bin/env python3
"""Focused falsifiers for the release terminal-manifest boundary."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
import tarfile
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("release_terminal_manifest.py")
SPEC = importlib.util.spec_from_file_location("release_terminal_manifest", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
subject = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = subject
SPEC.loader.exec_module(subject)

SOURCE = "a" * 40
TAG = "v0.18.0-rc.1"
TARGET = "x86_64-unknown-linux-gnu"
VERSION = "0.18.0-rc.1"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


def packet(executable: str, package: str, role: str) -> dict[str, object]:
    return {
        "schema_version": "perl_lsp.binary_identity.v1",
        "product": {
            "name": "perl-lsp",
            "public_repository": "EffortlessMetrics/perl-lsp",
            "development_repository": "EffortlessMetrics/perl-lsp-swarm",
        },
        "binary": {
            "executable": executable,
            "cargo_package": package,
            "role": role,
            "version": VERSION,
        },
        "build": {
            "source_revision": SOURCE,
            "source_tree_digest": "b" * 64,
            "target": TARGET,
            "profile": "release",
            "identity_state": "exact",
        },
        "artifact": {"role": "archive", "digest": None, "candidate_identity": TAG},
        "compatibility": {"expected_product_identity_version": 1, "dap_posture": "preview"},
        "limitations": [],
    }


def binary_row(executable: str, package: str, role: str, build_bytes: bytes) -> dict[str, object]:
    identity_packet = packet(executable, package, role)
    return {
        "role": role,
        "executable": executable,
        "path_role": f"target/{TARGET}/release/{executable}",
        "file_sha256": hashlib.sha256(build_bytes).hexdigest(),
        "packet_sha256": hashlib.sha256(subject.canonical(identity_packet)).hexdigest(),
        "packet": identity_packet,
    }


def candidate(root: Path) -> Path:
    package_name = f"perllsp-{VERSION}-{TARGET}"
    package_dir = root / "package" / package_name
    package_dir.mkdir(parents=True)
    packaged = {"perllsp": b"post-strip-server", "perl-dap": b"post-strip-dap"}
    for executable, payload in packaged.items():
        (package_dir / executable).write_bytes(payload)
    archive = root / "dist" / f"{package_name}.tar.gz"
    archive.parent.mkdir(parents=True)
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(package_dir, arcname=package_name)
    (root / "dist" / "SHA256SUMS").write_text(
        f"{sha256(archive)}  {archive.name}\n", encoding="utf-8"
    )
    write_json(
        root / "dist" / "sbom-spdx.json",
        {
            "spdxVersion": "SPDX-2.3",
            "dataLicense": "CC0-1.0",
            "SPDXID": "SPDXRef-DOCUMENT",
            "name": "perl-lsp",
            "documentNamespace": "https://example.invalid/spdx/perl-lsp/0.18.0-rc.1",
            "creationInfo": {"created": "2026-08-30T00:00:00Z", "creators": ["Tool: cargo-sbom-0.9.1"]},
            "packages": [{
                "SPDXID": "SPDXRef-Package-perllsp",
                "name": "perllsp",
                "downloadLocation": "NOASSERTION",
                "filesAnalyzed": False,
                "copyrightText": "NOASSERTION",
                "licenseConcluded": "NOASSERTION",
                "licenseDeclared": "NOASSERTION",
            }],
        },
    )
    topology = root / "evidence" / TARGET / "release-topology.json"
    write_json(topology, {"schema": 1, "release": "0.18.0-rc.1"})
    identity = {
        "schema_version": "perl_lsp.release_build_identity.v1",
        "repository": "EffortlessMetrics/perl-lsp-swarm",
        "source_revision": SOURCE,
        "source_tree_digest": "b" * 64,
        "release_version": VERSION,
        "target": TARGET,
        "profile": "release",
        "candidate_identity": TAG,
        "artifact_role": "archive",
        "product_identity_contract_digest": "c" * 64,
        "release_topology_digest": sha256(topology),
        "toolchain_digest": "d" * 64,
    }
    build_payloads = {"perllsp": b"build-server", "perl-dap": b"build-dap"}
    binaries = [
        binary_row("perllsp", "perllsp", "server", build_payloads["perllsp"]),
        binary_row("perl-dap", "perl-dap", "dap", build_payloads["perl-dap"]),
    ]
    write_json(root / "evidence" / TARGET / "release-build-identity.json", identity)
    write_json(
        root / "evidence" / TARGET / "release-build-receipt.json",
        {
            "schema_version": "perl_lsp.release_build_identity_receipt.v1",
            "status": "pass",
            "input_sha256": hashlib.sha256(subject.canonical(identity)).hexdigest(),
            "input": identity,
            "runner": "cargo",
            "build_execution": "external_release_workflow",
            "build_commands": ["cargo build perllsp", "cargo build perl-dap"],
            "binaries": binaries,
            "claim_boundary": "test fixture",
        },
    )
    write_json(
        root / "evidence" / TARGET / "release-package-evidence.json",
        {
            "schema_version": "perl_lsp.release_package_evidence.v1",
            "status": "pass",
            "source_revision": SOURCE,
            "release_version": VERSION,
            "target": TARGET,
            "archive": {"name": archive.name, "sha256": sha256(archive)},
            "binaries": [
                {
                    "executable": row["executable"],
                    "member_path": f"{package_name}/{row['executable']}",
                    "pre_strip_sha256": row["file_sha256"],
                    "post_strip_sha256": hashlib.sha256(packaged[str(row["executable"])]).hexdigest(),
                }
                for row in binaries
            ],
        },
    )
    return root


class ReleaseTerminalManifestTests(unittest.TestCase):
    def test_complete_candidate_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            first = subject.canonical(subject.build_manifest(root, SOURCE, TAG))
            second = subject.canonical(subject.build_manifest(root, SOURCE, TAG))
            self.assertEqual(first, second)
            self.assertEqual(json.loads(first)["status"], "eligible")

    def test_empty_sbom_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            sbom_path = root / "dist" / "sbom-spdx.json"
            sbom = json.loads(sbom_path.read_text(encoding="utf-8"))
            sbom["packages"] = []
            write_json(sbom_path, sbom)
            with self.assertRaisesRegex(subject.ManifestError, "nonempty packages"):
                subject.build_manifest(root, SOURCE, TAG)

    def test_omitted_or_forged_checksum_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            (root / "dist" / "SHA256SUMS").write_text(f"{'f' * 64}  missing.tar.gz\n", encoding="utf-8")
            with self.assertRaises(subject.ManifestError):
                subject.build_manifest(root, SOURCE, TAG)

    def test_wrong_source_evidence_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            identity_path = root / "evidence" / TARGET / "release-build-identity.json"
            identity = json.loads(identity_path.read_text(encoding="utf-8"))
            identity["source_revision"] = "b" * 40
            write_json(identity_path, identity)
            with self.assertRaisesRegex(subject.ManifestError, "another source"):
                subject.build_manifest(root, SOURCE, TAG)

    def test_malformed_nonempty_spdx_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            write_json(root / "dist" / "sbom-spdx.json", {"spdxVersion": "SPDX-2.3", "packages": [{"name": "perllsp"}]})
            with self.assertRaisesRegex(subject.ManifestError, "SPDX"):
                subject.build_manifest(root, SOURCE, TAG)

    def test_forged_receipt_hash_or_binary_digest_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            receipt_path = root / "evidence" / TARGET / "release-build-receipt.json"
            receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
            receipt["input_sha256"] = "f" * 64
            write_json(receipt_path, receipt)
            with self.assertRaisesRegex(subject.ManifestError, "input hash"):
                subject.build_manifest(root, SOURCE, TAG)

    def test_nonpassing_or_wrong_schema_receipt_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            receipt_path = root / "evidence" / TARGET / "release-build-receipt.json"
            receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
            receipt["status"] = "not_proven"
            write_json(receipt_path, receipt)
            with self.assertRaisesRegex(subject.ManifestError, "not a passing"):
                subject.build_manifest(root, SOURCE, TAG)

    def test_forged_binary_packet_digest_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            receipt_path = root / "evidence" / TARGET / "release-build-receipt.json"
            receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
            receipt["binaries"][0]["packet_sha256"] = "f" * 64
            write_json(receipt_path, receipt)
            with self.assertRaisesRegex(subject.ManifestError, "packet digest"):
                subject.build_manifest(root, SOURCE, TAG)

    def test_archive_member_drift_from_post_strip_evidence_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            evidence_path = root / "evidence" / TARGET / "release-package-evidence.json"
            evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
            evidence["binaries"][0]["post_strip_sha256"] = "e" * 64
            write_json(evidence_path, evidence)
            with self.assertRaisesRegex(subject.ManifestError, "archive member"):
                subject.build_manifest(root, SOURCE, TAG)

    def test_attestation_inventory_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            subject.write_outputs(root, SOURCE, TAG)
            inventory = root / "attestation-subjects.sha256"
            inventory.write_text(inventory.read_text(encoding="utf-8") + f"{'0' * 64}  unexpected.bin\n", encoding="utf-8")
            with self.assertRaisesRegex(subject.ManifestError, "attestation subject inventory"):
                subject.check_outputs(root, SOURCE, TAG)

    def test_unlisted_candidate_file_is_rejected_as_attestation_drift(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = candidate(Path(directory))
            (root / "unexpected.bin").write_bytes(b"drift")
            with self.assertRaisesRegex(subject.ManifestError, "unadmitted attestation drift"):
                subject.build_manifest(root, SOURCE, TAG)


if __name__ == "__main__":
    unittest.main()
