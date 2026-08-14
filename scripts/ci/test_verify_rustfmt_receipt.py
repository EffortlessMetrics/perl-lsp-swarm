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
        self.metadata = Path(self.temp.name, "metadata.json")
        self._write_metadata(self._metadata_payload())
        self.cargo = self._cargo_tool()
        self.rustfmt = self._tool("rustfmt", "rustfmt 1.8.0-stable (fixture)")
        self.rustc_output = "rustc 1.95.0 (fixture 2026-08-01)\nbinary: rustc\ncommit-hash: 0123456789abcdef0123456789abcdef01234567\ncommit-date: 2026-08-01\nhost: x86_64-unknown-linux-gnu\nrelease: 1.95.0\nLLVM version: 20.1.0"
        self.rustc = self._tool("rustc", self.rustc_output)
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
            implementation = path.with_suffix(".py")
            implementation.write_text(
                "import sys\nprint(" + repr(version) + ")\n",
                encoding="utf-8",
            )
            path.write_text(f'@"{sys.executable}" "{implementation}" %*\n', encoding="utf-8")
        else:
            path.write_text(f"#!/bin/sh\necho '{version}'\n", encoding="utf-8")
            path.chmod(0o755)
        return path

    def _metadata_payload(self) -> dict[str, object]:
        package_id = "path+file:///fixture/pkg#0.1.0"
        return {
            "packages": [
                {
                    "id": package_id,
                    "name": "pkg",
                    "manifest_path": str(self.root / "pkg/Cargo.toml"),
                    "targets": [
                        {
                            "name": "pkg",
                            "kind": ["lib"],
                            "src_path": str(self.root / "pkg/src/lib.rs"),
                        }
                    ],
                }
            ],
            "workspace_members": [package_id],
            "workspace_root": str(self.root),
        }

    def _write_metadata(self, payload: dict[str, object]) -> None:
        self.metadata.write_text(json.dumps(payload), encoding="utf-8")

    def _cargo_tool(self) -> Path:
        suffix = ".cmd" if os.name == "nt" else ""
        path = Path(self.temp.name, "cargo" + suffix)
        implementation = path.with_suffix(".py") if os.name == "nt" else path
        implementation.write_text(
            """#!/usr/bin/env python3
import pathlib
import sys

args = sys.argv[1:]
if args == ["--version"]:
    print("cargo 1.95.0 (fixture)")
elif args == ["metadata", "--no-deps", "--locked", "--format-version", "1"]:
    print(pathlib.Path(%r).read_text(encoding="utf-8"))
else:
    print(f"unexpected cargo invocation: {args}", file=sys.stderr)
    raise SystemExit(2)
"""
            % str(self.metadata),
            encoding="utf-8",
        )
        if os.name == "nt":
            path.write_text(f'@"{sys.executable}" "{implementation}" %*\n', encoding="utf-8")
        else:
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
                "rustfmt_version": "rustfmt 1.8.0-stable (fixture)",
                "rustc_version_verbose": self.rustc_output,
            },
            "workspace": {
                "manifest_count": 1,
                "target_count": 1,
                "manifests": [{"manifest": "pkg/Cargo.toml", "package": "pkg"}],
                "targets": [
                    {
                        "package": "pkg",
                        "name": "pkg",
                        "kind": ["lib"],
                        "source": "pkg/src/lib.rs",
                        "manifest": "pkg/Cargo.toml",
                    }
                ],
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
        values = dict(receipt=self.receipt, root=self.root, candidate_sha=self.sha, candidate_tree_sha=self.tree, producer=Path("scripts/ci/rustfmt_check.py"), rustfmt=self.rustfmt, rustc=self.rustc, cargo=self.cargo)
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

    def test_non_mapping_receipt_sections_fail_controlled(self) -> None:
        self.receipt.write_text("[]", encoding="utf-8")
        with self.assertRaisesRegex(verifier.VerificationError, "receipt must be a JSON object"):
            verifier.verify(self._args())
        self.payload = self._valid_payload()
        self.assert_rejected(lambda value: value.update(inputs=[]))
        self.payload = self._valid_payload()
        self.assert_rejected(lambda value: value.update(workspace=[]))

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

    def test_wrong_selected_toolchain_is_rejected(self) -> None:
        wrong = self._tool("rustc-wrong", "rustc 1.94.0 (fixture)\nrelease: 1.94.0")
        with self.assertRaisesRegex(verifier.VerificationError, "selected rustc"):
            verifier.verify(self._args(rustc=wrong))
        self.payload["inputs"]["rustc_version_verbose"] = "rustc 1.94.0 (fixture)\nrelease: 1.94.0"
        self._resign(self.payload)
        self._write(self.payload)
        with self.assertRaisesRegex(verifier.VerificationError, "pinned release"):
            verifier.verify(self._args(rustc=wrong))

    def test_incomplete_and_failed_runs_are_rejected(self) -> None:
        self.assert_rejected(lambda value: value.update(runs=[]))
        self.payload = self._valid_payload()
        self.assert_rejected(lambda value: value["runs"][0].update(status="instrument_failure"))

    def test_malformed_nested_identities_are_rejected(self) -> None:
        mutations = (
            lambda value: (
                value["workspace"]["manifests"][0].update(manifest=None),
                value["runs"][0].update(manifest=None),
            ),
            lambda value: value["workspace"]["manifests"][0].update(package=""),
            lambda value: value["runs"][0].update(package=[]),
            lambda value: value["workspace"]["targets"][0].update(source={}),
            lambda value: value["workspace"]["targets"][0].update(package=None),
            lambda value: value["workspace"]["targets"][0].update(name=""),
            lambda value: value["workspace"]["targets"][0].update(manifest=[]),
            lambda value: value["workspace"]["targets"][0].update(kind=None),
            lambda value: value["workspace"]["targets"][0].update(kind=[]),
            lambda value: value["workspace"]["targets"][0].update(kind=[{}]),
        )
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.payload = self._valid_payload()
                self.assert_rejected(mutation)

    def test_incoherent_target_and_duplicate_package_are_rejected(self) -> None:
        self.assert_rejected(
            lambda value: value["workspace"]["targets"][0].update(
                package="absent", manifest="absent/Cargo.toml"
            )
        )

        def duplicate_package(value) -> None:
            value["workspace"]["manifests"].append(
                {"manifest": "other/Cargo.toml", "package": "pkg"}
            )
            value["workspace"]["manifest_count"] = 2
            value["runs"].append(
                {"manifest": "other/Cargo.toml", "package": "pkg", "status": "pass"}
            )

        self.payload = self._valid_payload()
        self.assert_rejected(duplicate_package)

    def test_incoherent_counts_findings_and_truncation_are_rejected(self) -> None:
        self.assert_rejected(lambda value: value["workspace"].update(manifest_count=2))
        self.payload = self._valid_payload()
        self.assert_rejected(lambda value: value.update(findings=[{"path": "pkg/src/lib.rs"}]))
        self.payload = self._valid_payload()
        self.assert_rejected(lambda value: value.update(findings_truncated=True))

    def test_canonically_resigned_target_omission_is_rejected(self) -> None:
        metadata = self._metadata_payload()
        extra_target = {
            "name": "pkg-bin",
            "kind": ["bin"],
            "src_path": str(self.root / "pkg/src/main.rs"),
        }
        (self.root / "pkg/src/main.rs").write_text("fn main() {}\n", encoding="utf-8")
        self._git("add", "pkg/src/main.rs")
        self._git("commit", "-m", "add second target")
        self.sha = self._git("rev-parse", "HEAD").stdout.strip()
        self.tree = self._git("rev-parse", "HEAD^{tree}").stdout.strip()
        metadata["packages"][0]["targets"].append(extra_target)
        self._write_metadata(metadata)
        self.payload["subject"] = {
            "repository_sha": self.sha,
            "repository_tree_sha": self.tree,
        }
        receipt_target = {
            "package": "pkg",
            "name": "pkg-bin",
            "kind": ["bin"],
            "source": "pkg/src/main.rs",
            "manifest": "pkg/Cargo.toml",
        }
        self.payload["workspace"]["targets"].append(receipt_target)
        self.payload["workspace"]["target_count"] = 2
        self.payload["workspace"]["targets"].pop()
        self.payload["workspace"]["target_count"] = 1
        self._resign(self.payload)
        self._write(self.payload)
        with self.assertRaisesRegex(verifier.VerificationError, "targets do not match"):
            verifier.verify(self._args())

    def test_canonically_resigned_manifest_run_and_targets_omission_is_rejected(self) -> None:
        for path, content in {
            "other/Cargo.toml": '[package]\nname="other"\nversion="0.1.0"\n',
            "other/src/lib.rs": "pub fn other() {}\n",
        }.items():
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8")
        self._git("add", "other")
        self._git("commit", "-m", "add second package")
        self.sha = self._git("rev-parse", "HEAD").stdout.strip()
        self.tree = self._git("rev-parse", "HEAD^{tree}").stdout.strip()
        metadata = self._metadata_payload()
        other_id = "path+file:///fixture/other#0.1.0"
        metadata["workspace_members"].append(other_id)
        metadata["packages"].append(
            {
                "id": other_id,
                "name": "other",
                "manifest_path": str(self.root / "other/Cargo.toml"),
                "targets": [
                    {
                        "name": "other",
                        "kind": ["lib"],
                        "src_path": str(self.root / "other/src/lib.rs"),
                    }
                ],
            }
        )
        self._write_metadata(metadata)
        self.payload["subject"] = {
            "repository_sha": self.sha,
            "repository_tree_sha": self.tree,
        }
        self.payload["workspace"]["manifests"].append(
            {"manifest": "other/Cargo.toml", "package": "other"}
        )
        self.payload["workspace"]["targets"].append(
            {
                "package": "other",
                "name": "other",
                "kind": ["lib"],
                "source": "other/src/lib.rs",
                "manifest": "other/Cargo.toml",
            }
        )
        self.payload["runs"].append(
            {"manifest": "other/Cargo.toml", "package": "other", "status": "pass"}
        )
        self.payload["workspace"]["manifest_count"] = 2
        self.payload["workspace"]["target_count"] = 2
        self.payload["workspace"]["manifests"].pop()
        self.payload["workspace"]["targets"].pop()
        self.payload["runs"].pop()
        self.payload["workspace"]["manifest_count"] = 1
        self.payload["workspace"]["target_count"] = 1
        self._resign(self.payload)
        self._write(self.payload)
        with self.assertRaisesRegex(verifier.VerificationError, "manifests do not match"):
            verifier.verify(self._args())

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
