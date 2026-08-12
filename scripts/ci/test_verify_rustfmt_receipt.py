#!/usr/bin/env python3

from __future__ import annotations

import copy
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path

from scripts.ci import verify_rustfmt_receipt as verifier


class VerifyRustfmtReceiptTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name, "repo").resolve()
        self.root.mkdir()
        for path, content in {
            "Cargo.toml": "[workspace]\nmembers=[]\n",
            "Cargo.lock": "# lock\n",
            "rust-toolchain.toml": '[toolchain]\nchannel="1.95.0"\n',
            "rustfmt.toml": "max_width = 100\n",
            "scripts/ci/rustfmt_check.py": "# producer\n",
            "pkg/Cargo.toml": '[package]\nname="pkg"\nversion="0.1.0"\n',
            "pkg/src/lib.rs": "pub fn value() {}\n",
        }.items():
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8")
        self._git("init", "--initial-branch=main")
        self._git("config", "user.name", "fixture")
        self._git("config", "user.email", "fixture@example.invalid")
        self._git("add", ".")
        self._git("commit", "-m", "fixture")
        self.sha = self._git("rev-parse", "HEAD").stdout.strip()
        self.tree = self._git("rev-parse", "HEAD^{tree}").stdout.strip()
        self.cargo = self._tool("cargo", "cargo 1.95.0 (fixture)")
        self.rustfmt = self._tool("rustfmt", "rustfmt 1.95.0-stable (fixture)")
        self.receipt = Path(self.temp.name, "receipt.json")
        self.payload = self._valid_payload()
        self._write(self.payload)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _git(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["git", "-c", "commit.gpgsign=false", *args], cwd=self.root, text=True, capture_output=True, check=True)

    def _tool(self, name: str, version: str) -> Path:
        suffix = ".cmd" if os.name == "nt" else ""
        path = Path(self.temp.name, name + suffix)
        if os.name == "nt":
            path.write_text(f"@echo {version}\n", encoding="utf-8")
        else:
            path.write_text(f"#!/bin/sh\necho '{version}'\n", encoding="utf-8")
            path.chmod(0o755)
        return path

    def _digest(self, relative: str) -> str:
        return "sha256:" + hashlib.sha256((self.root / relative).read_bytes()).hexdigest()

    def _valid_payload(self) -> dict[str, object]:
        payload: dict[str, object] = {
            "schema_version": "rustfmt_check.v1",
            "receipt_kind": "rustfmt_check",
            "result": "pass",
            "subject": {"repository_sha": self.sha, "repository_tree_sha": self.tree},
            "inputs": {
                "cargo_toml_sha256": self._digest("Cargo.toml"),
                "cargo_lock_sha256": self._digest("Cargo.lock"),
                "rust_toolchain_sha256": self._digest("rust-toolchain.toml"),
                "rustfmt_toml_sha256": self._digest("rustfmt.toml"),
                "producer_sha256": self._digest("scripts/ci/rustfmt_check.py"),
                "cargo_version": "cargo 1.95.0 (fixture)",
                "rustfmt_version": "rustfmt 1.95.0-stable (fixture)",
            },
            "workspace": {
                "manifest_count": 1,
                "target_count": 1,
                "manifests": [{"manifest": "pkg/Cargo.toml", "package": "pkg"}],
                "targets": [{"source": "pkg/src/lib.rs", "package": "pkg"}],
            },
            "runs": [{"manifest": "pkg/Cargo.toml", "package": "pkg", "status": "pass"}],
            "findings": [],
            "instrument_failures": [],
            "findings_truncated": False,
        }
        self._resign(payload)
        return payload

    def _resign(self, payload: dict[str, object]) -> None:
        unsigned = {key: value for key, value in payload.items() if key != "evidence_sha256"}
        payload["evidence_sha256"] = "sha256:" + hashlib.sha256(verifier.canonical_json(unsigned)).hexdigest()

    def _write(self, payload: dict[str, object]) -> None:
        self.receipt.write_text(json.dumps(payload), encoding="utf-8")

    def _args(self, **changes: object) -> Namespace:
        values = dict(receipt=self.receipt, root=self.root, candidate_sha=self.sha, candidate_tree_sha=self.tree, producer=Path("scripts/ci/rustfmt_check.py"), rustfmt=self.rustfmt, cargo=self.cargo)
        values.update(changes)
        return Namespace(**values)

    def assert_rejected(self, mutate) -> None:
        broken = copy.deepcopy(self.payload)
        mutate(broken)
        self._resign(broken)
        self._write(broken)
        with self.assertRaises(verifier.VerificationError):
            verifier.verify(self._args())

    def test_valid_receipt_passes(self) -> None:
        verifier.verify(self._args())

    def test_recomputed_digest_non_pass_is_rejected(self) -> None:
        self.assert_rejected(lambda value: value.update(result="format_failure"))

    def test_schema_kind_and_canonical_digest_are_rejected(self) -> None:
        self.assert_rejected(lambda value: value.update(schema_version="rustfmt_check.v0"))
        self.payload = self._valid_payload()
        self.assert_rejected(lambda value: value.update(receipt_kind="other"))
        self.payload = self._valid_payload()
        self.payload["evidence_sha256"] = "sha256:" + "0" * 64
        self._write(self.payload)
        with self.assertRaisesRegex(verifier.VerificationError, "canonical evidence"):
            verifier.verify(self._args())

    def test_stale_identity_is_rejected(self) -> None:
        self.assert_rejected(lambda value: value["subject"].update(repository_sha="a" * 40))

    def test_input_digest_mismatch_is_rejected(self) -> None:
        self.assert_rejected(lambda value: value["inputs"].update(rustfmt_toml_sha256="sha256:" + "0" * 64))

    def test_tool_mismatch_is_rejected(self) -> None:
        self.assert_rejected(lambda value: value["inputs"].update(rustfmt_version="rustfmt 1.94.0"))

    def test_incomplete_and_failed_runs_are_rejected(self) -> None:
        self.assert_rejected(lambda value: value.update(runs=[]))
        self.payload = self._valid_payload()
        self.assert_rejected(lambda value: value["runs"][0].update(status="instrument_failure"))

    def test_incoherent_counts_findings_and_truncation_are_rejected(self) -> None:
        self.assert_rejected(lambda value: value["workspace"].update(manifest_count=2))
        self.payload = self._valid_payload()
        self.assert_rejected(lambda value: value.update(findings=[{"path": "pkg/src/lib.rs"}]))
        self.payload = self._valid_payload()
        self.assert_rejected(lambda value: value.update(findings_truncated=True))

    def test_dirty_tree_is_rejected(self) -> None:
        (self.root / "pkg/src/lib.rs").write_text("dirty\n", encoding="utf-8")
        with self.assertRaisesRegex(verifier.VerificationError, "not clean"):
            verifier.verify(self._args())

    def test_symlinked_input_is_rejected(self) -> None:
        producer = self.root / "scripts/ci/rustfmt_check.py"
        replacement = self.root / "producer-real.py"
        replacement.write_bytes(producer.read_bytes())
        producer.unlink()
        try:
            producer.symlink_to(replacement)
        except OSError as error:
            self.skipTest(f"symlinks unavailable: {error}")
        self._git("add", "scripts/ci/rustfmt_check.py", "producer-real.py")
        self._git("commit", "-m", "symlink fixture")
        self.sha = self._git("rev-parse", "HEAD").stdout.strip()
        self.tree = self._git("rev-parse", "HEAD^{tree}").stdout.strip()
        self.payload["subject"] = {
            "repository_sha": self.sha,
            "repository_tree_sha": self.tree,
        }
        self._resign(self.payload)
        self._write(self.payload)
        with self.assertRaisesRegex(verifier.VerificationError, "non-symlink"):
            verifier.verify(self._args())


if __name__ == "__main__":
    unittest.main()
