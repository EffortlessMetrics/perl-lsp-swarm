#!/usr/bin/env python3
"""Focused falsifiers for the release terminal-manifest boundary."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
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


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


def candidate(root: Path) -> Path:
    archive = root / "dist" / f"perllsp-0.18.0-rc.1-{TARGET}.tar.gz"
    archive.parent.mkdir(parents=True)
    archive.write_bytes(b"archive")
    (root / "dist" / "SHA256SUMS").write_text(
        f"{sha256(archive)}  {archive.name}\n", encoding="utf-8"
    )
    write_json(
        root / "dist" / "sbom-spdx.json",
        {"spdxVersion": "SPDX-2.3", "packages": [{"name": "perllsp"}]},
    )
    topology = root / "evidence" / TARGET / "release-topology.json"
    write_json(topology, {"schema": 1, "release": "0.18.0-rc.1"})
    identity = {
        "source_revision": SOURCE,
        "release_version": "0.18.0-rc.1",
        "target": TARGET,
        "release_topology_digest": sha256(topology),
    }
    write_json(root / "evidence" / TARGET / "release-build-identity.json", identity)
    write_json(root / "evidence" / TARGET / "release-build-receipt.json", {"input": identity})
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
            write_json(root / "dist" / "sbom-spdx.json", {"spdxVersion": "SPDX-2.3", "packages": []})
            with self.assertRaisesRegex(subject.ManifestError, "non-empty packages"):
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


if __name__ == "__main__":
    unittest.main()
