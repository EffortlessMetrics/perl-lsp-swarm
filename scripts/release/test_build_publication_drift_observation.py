#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("build_publication_drift_observation.py")
SPEC = importlib.util.spec_from_file_location("build_publication_drift_observation", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
observation_module = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = observation_module
SPEC.loader.exec_module(observation_module)

ObservationError = observation_module.ObservationError
build_observation = observation_module.build_observation
checkout_identity = observation_module._checkout_identity
tree_entry = observation_module._tree_entry
main = observation_module.main


def git(root: Path, *arguments: str) -> str:
    completed = subprocess.run(
        ["git", "-C", str(root), *arguments],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
        text=True,
    )
    return completed.stdout.strip()


def repository(root: Path, remote: str, files: dict[str, str]) -> str:
    root.mkdir()
    git(root, "init", "-b", "main")
    git(root, "config", "user.name", "Fixture")
    git(root, "config", "user.email", "fixture@example.invalid")
    git(root, "remote", "add", "origin", f"https://github.com/{remote}.git")
    for name, content in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    git(root, "add", ".")
    git(root, "commit", "-m", "fixture")
    return git(root, "rev-parse", "HEAD")


class PublicationDriftObservationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.control = self.root / "control"
        self.control.mkdir()
        self.swarm = self.root / "swarm"
        self.public = self.root / "public"
        self.swarm_sha = repository(
            self.swarm,
            "EffortlessMetrics/perl-lsp-swarm",
            {"same.txt": "same", "README.md": "swarm", "new.txt": "new"},
        )
        self.public_sha = repository(
            self.public,
            "EffortlessMetrics/perl-lsp",
            {"same.txt": "same", "README.md": "public"},
        )
        self.swarm_identity = checkout_identity(
            self.swarm, "EffortlessMetrics/perl-lsp-swarm", self.swarm_sha
        )
        self.public_identity = checkout_identity(
            self.public, "EffortlessMetrics/perl-lsp", self.public_sha
        )
        self.manifest = self.control / "manifest.json"
        self.invariants = self.control / "invariants.json"
        self._write_authorities()

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _write_authorities(self) -> None:
        self.manifest.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "swarm_repository": "EffortlessMetrics/perl-lsp-swarm",
                    "public_repository": "EffortlessMetrics/perl-lsp",
                    "swarm_sha": self.swarm_sha,
                    "public_sha": self.public_sha,
                    "swarm_tree_digest": self.swarm_identity["tree_digest"],
                    "public_tree_digest": self.public_identity["tree_digest"],
                    "version": "0.18.0",
                    "rules": [
                        {
                            "id": "publication_context.readme",
                            "path": "README.md",
                            "classification": "expected_publication_translation",
                            "owner": "release-engineering",
                        }
                    ],
                    "required_invariants": [
                        {"id": "product_path_coverage_complete", "owner": "release-engineering"}
                    ],
                }
            ),
            encoding="utf-8",
        )
        self.invariants.write_text(
            json.dumps(
                {
                    "schema_version": "perl_lsp.publication_drift_invariants.v1",
                    "swarm_sha": self.swarm_sha,
                    "swarm_tree_digest": self.swarm_identity["tree_digest"],
                    "public_sha": self.public_sha,
                    "public_tree_digest": self.public_identity["tree_digest"],
                    "invariants": [
                        {
                            "id": "product_path_coverage_complete",
                            "status": "pass",
                            "owner": "release-engineering",
                            "evidence": ["tracked path inventory complete"],
                        }
                    ],
                }
            ),
            encoding="utf-8",
        )

    def build(self) -> dict[str, object]:
        return build_observation(
            self.swarm,
            self.public,
            self.control,
            self.manifest,
            self.invariants,
            self.swarm_sha,
            self.public_sha,
            "0.18.0",
        )

    def test_exact_checkouts_produce_rule_bound_and_unknown_rows(self) -> None:
        result = self.build()
        rows = {item["path"]: item for item in result["differences"]}
        self.assertEqual(
            rows["README.md"]["classification"], "expected_publication_translation"
        )
        self.assertEqual(rows["README.md"]["manifest_rule"], "publication_context.readme")
        self.assertEqual(rows["new.txt"]["classification"], "unknown_or_not_proven")
        self.assertIn("public_object=absent", rows["new.txt"]["evidence"])
        self.assertIn("mode:", rows["README.md"]["evidence"][0])
        self.assertEqual(result["invariants"][0]["status"], "pass")

    def test_missing_object_is_distinct_from_a_git_tool_failure(self) -> None:
        self.assertIsNone(tree_entry(self.public, "new.txt"))
        with mock.patch.object(
            observation_module,
            "_run",
            side_effect=ObservationError("git ls-tree failed: redacted"),
        ):
            with self.assertRaisesRegex(ObservationError, "git ls-tree failed"):
                tree_entry(self.public, "README.md")

    def test_same_bytes_with_different_mode_remain_a_difference(self) -> None:
        git(self.swarm, "update-index", "--chmod=+x", "same.txt")
        git(self.swarm, "commit", "-m", "make same object executable")
        self.swarm_sha = git(self.swarm, "rev-parse", "HEAD")
        self.swarm_identity = checkout_identity(
            self.swarm, "EffortlessMetrics/perl-lsp-swarm", self.swarm_sha
        )
        self._write_authorities()

        result = self.build()
        row = {item["path"]: item for item in result["differences"]}["same.txt"]
        self.assertTrue(any(item.startswith("public_object=mode:100644,") for item in row["evidence"]))
        self.assertTrue(any(item.startswith("swarm_object=mode:100755,") for item in row["evidence"]))

    def test_acquisition_failure_writes_redacted_not_proven_observation(self) -> None:
        out = self.root / "target" / "observation.json"
        exit_code = main(
            [
                "--swarm-root",
                str(self.swarm),
                "--public-root",
                str(self.root / "missing-public"),
                "--control-root",
                str(self.control),
                "--manifest",
                str(self.manifest),
                "--invariants",
                str(self.invariants),
                "--swarm-sha",
                self.swarm_sha,
                "--public-sha",
                self.public_sha,
                "--version",
                "0.18.0",
                "--out",
                str(out),
            ]
        )
        self.assertEqual(exit_code, 2)
        failure = json.loads(out.read_text(encoding="utf-8"))
        evidence = failure["differences"][0]["evidence"]
        self.assertEqual(failure["differences"][0]["classification"], "unknown_or_not_proven")
        self.assertIn("acquisition_failure=checkout_missing_or_unreadable", evidence)
        self.assertIn("cause=redacted", evidence)
        self.assertNotIn(str(self.root), " ".join(evidence))

    def test_git_tool_failure_writes_redacted_not_proven_observation(self) -> None:
        out = self.root / "target" / "tool-failure-observation.json"
        with mock.patch.object(
            observation_module,
            "_run",
            side_effect=ObservationError("git cat-file failed: secret path"),
        ):
            exit_code = main(
                [
                    "--swarm-root",
                    str(self.swarm),
                    "--public-root",
                    str(self.public),
                    "--control-root",
                    str(self.control),
                    "--manifest",
                    str(self.manifest),
                    "--invariants",
                    str(self.invariants),
                    "--swarm-sha",
                    self.swarm_sha,
                    "--public-sha",
                    self.public_sha,
                    "--version",
                    "0.18.0",
                    "--out",
                    str(out),
                ]
            )
        self.assertEqual(exit_code, 2)
        failure = json.loads(out.read_text(encoding="utf-8"))
        evidence = failure["differences"][0]["evidence"]
        self.assertIn("acquisition_failure=git_tool_failure", evidence)
        self.assertNotIn("secret path", " ".join(evidence))

    def test_branch_movement_after_subject_selection_is_rejected(self) -> None:
        (self.swarm / "later.txt").write_text("later", encoding="utf-8")
        git(self.swarm, "add", ".")
        git(self.swarm, "commit", "-m", "later")
        with self.assertRaises(ObservationError):
            self.build()

    def test_wrong_repository_remote_is_rejected(self) -> None:
        git(self.public, "remote", "set-url", "origin", "https://github.com/example/wrong.git")
        with self.assertRaises(ObservationError):
            self.build()

    def test_manifest_for_another_tree_is_rejected(self) -> None:
        value = json.loads(self.manifest.read_text(encoding="utf-8"))
        value["public_tree_digest"] = "0" * 64
        self.manifest.write_text(json.dumps(value), encoding="utf-8")
        with self.assertRaises(ObservationError):
            self.build()

    def test_invariant_packet_for_another_sha_is_rejected(self) -> None:
        value = json.loads(self.invariants.read_text(encoding="utf-8"))
        value["public_sha"] = "0" * 40
        self.invariants.write_text(json.dumps(value), encoding="utf-8")
        with self.assertRaises(ObservationError):
            self.build()

    def test_authority_path_must_remain_in_control_checkout(self) -> None:
        escaped = self.root / "escaped.json"
        escaped.write_text(self.manifest.read_text(encoding="utf-8"), encoding="utf-8")
        with self.assertRaises(ObservationError):
            build_observation(
                self.swarm,
                self.public,
                self.control,
                escaped,
                self.invariants,
                self.swarm_sha,
                self.public_sha,
                "0.18.0",
            )


if __name__ == "__main__":
    unittest.main()
