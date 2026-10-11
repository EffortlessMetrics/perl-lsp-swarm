"""Discriminators for #11266 research, separate from production policy."""

import copy
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "selection_research", ROOT / "scripts/characterize_project_selection.py"
)
RESEARCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESEARCH)


class SelectionResearchTests(unittest.TestCase):
    def setUp(self):
        self.policy = RESEARCH.read_json(RESEARCH.POLICY)
        self.manifest = RESEARCH.read_json(RESEARCH.DATA / "fixtures.v1.json")
        self.receipt = RESEARCH.read_json(
            RESEARCH.DATA / "linux-decision.v1.json"
        )

    def validate(self, policy=None, manifest=None, receipt=None):
        RESEARCH.validate_artifacts(
            self.policy if policy is None else policy,
            self.manifest if manifest is None else manifest,
            self.receipt if receipt is None else receipt,
        )

    def test_checked_artifacts_and_exact_source_are_current(self):
        self.validate()
        self.assertEqual(
            self.receipt["source_sha256"], RESEARCH.digest(RESEARCH.SOURCE)
        )
        self.assertEqual(
            self.receipt["fixture_sha256"],
            RESEARCH.digest(RESEARCH.DATA / "fixtures.v1.json"),
        )

    def test_matching_nonexistent_commit_cannot_stamp_a_receipt(self):
        manifest = copy.deepcopy(self.manifest)
        receipt = copy.deepcopy(self.receipt)
        manifest["source_commit"] = receipt["source_commit"] = "0" * 40
        with self.assertRaisesRegex(ValueError, "not an available commit"):
            self.validate(manifest=manifest, receipt=receipt)

    def test_available_commit_must_match_observed_walker(self):
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / "different-walker.rs"
            source.write_text("// different source subject\n")
            with patch.object(RESEARCH, "SOURCE", source):
                with self.assertRaisesRegex(ValueError, "walker differs"):
                    self.validate()

    def test_fixture_entry_and_selection_ceilings_are_admission_checks(self):
        for mutation in ("entry", "scanned"):
            manifest = copy.deepcopy(self.manifest)
            case = next(
                c for c in manifest["cases"] if c["id"] == "bounded-large"
            )
            if mutation == "entry":
                case["entries"].append(
                    {"path": "root/f256.pm", "kind": "file"}
                )
            else:
                case["current"]["scanned"] = 257
            with self.subTest(mutation=mutation), self.assertRaises(
                RESEARCH.jsonschema.ValidationError
            ):
                self.validate(manifest=manifest)
            with patch.object(RESEARCH, "run_binary") as run:
                with self.assertRaises(RESEARCH.jsonschema.ValidationError):
                    RESEARCH.characterize(
                        Path("/unexecuted"), self.policy, manifest
                    )
                run.assert_not_called()

    def test_unknown_missing_and_wrong_generation_are_rejected(self):
        for name in ("policy", "manifest", "receipt"):
            original = getattr(self, name)
            for mutation in ("unknown", "missing", "generation"):
                with self.subTest(document=name, mutation=mutation):
                    candidate = copy.deepcopy(original)
                    if mutation == "unknown":
                        candidate["another_project_envelope"] = {}
                    elif mutation == "missing":
                        del candidate["schema"]
                    else:
                        candidate["schema"] += ".next"
                    with self.assertRaises(
                        RESEARCH.jsonschema.ValidationError
                    ):
                        self.validate(**{name: candidate})

    def test_semantic_change_requires_policy_identity_movement(self):
        for key in ("rule", "rationale", "disposition"):
            candidate = copy.deepcopy(self.policy)
            candidate["rows"][0][key] = (
                "correct" if key == "disposition" else "silently changed"
            )
            with self.subTest(key=key), self.assertRaisesRegex(
                ValueError, "identity movement"
            ):
                self.validate(policy=candidate)

    def test_current_defects_have_explicit_non_retention_dispositions(self):
        # Protect the decision boundary, never current walker behavior.
        dispositions = {
            row["id"]: row["disposition"] for row in self.policy["rows"]
        }
        for row in (
            "exclude-auto",
            "unreadable",
            "no-input",
            "ordering",
            "path-identity",
            "budget-cancellation",
        ):
            self.assertEqual(dispositions[row], "correct", row)
        for row in ("symlinks", "outside-loop"):
            self.assertEqual(dispositions[row], "refuse", row)
        self.assertEqual(dispositions["platforms"], "defer")

    def test_missing_disposition_and_duplicate_policy_row_are_rejected(self):
        candidate = copy.deepcopy(self.policy)
        del candidate["rows"][0]["disposition"]
        with self.assertRaises(RESEARCH.jsonschema.ValidationError):
            self.validate(policy=candidate)
        candidate = copy.deepcopy(self.policy)
        candidate["rows"].append(candidate["rows"][0])
        with self.assertRaisesRegex(ValueError, "duplicate policy row"):
            self.validate(policy=candidate)

    def test_unreadable_outside_alias_and_empty_observations_cannot_vanish(
        self,
    ):
        for case_id in (
            "links",
            "unreadable-file",
            "unreadable-subtree",
            "unreadable-root",
            "empty",
        ):
            candidate = copy.deepcopy(self.receipt)
            candidate["observations"] = [
                o for o in candidate["observations"] if o["id"] != case_id
            ]
            with self.subTest(case=case_id), self.assertRaisesRegex(
                ValueError, "fixture coverage"
            ):
                self.validate(receipt=candidate)
        candidate = copy.deepcopy(self.receipt)
        row = next(
            o
            for o in candidate["observations"]
            if o["id"] == "unreadable-subtree"
        )
        row["current"]["unscanned_display"] = []
        with self.assertRaisesRegex(ValueError, "characterized fixture"):
            self.validate(receipt=candidate)

    def test_observations_require_their_own_policy_rows(self):
        candidate = copy.deepcopy(self.receipt)
        candidate["observations"][0]["policy_rows"] = ["extensions"]
        with self.assertRaisesRegex(
            ValueError, "explicit policy dispositions"
        ):
            self.validate(receipt=candidate)

    def test_duplicate_fixture_or_observation_cannot_hide_another_subject(
        self,
    ):
        manifest = copy.deepcopy(self.manifest)
        manifest["cases"].append(manifest["cases"][0])
        with self.assertRaisesRegex(ValueError, "duplicate fixture"):
            self.validate(manifest=manifest)
        receipt = copy.deepcopy(self.receipt)
        receipt["observations"].append(receipt["observations"][0])
        with self.assertRaisesRegex(ValueError, "duplicate observation"):
            self.validate(receipt=receipt)

    def test_linux_receipt_cannot_qualify_windows_or_drop_limits(self):
        candidate = copy.deepcopy(self.receipt)
        candidate["platform"] = "windows"
        with self.assertRaises(RESEARCH.jsonschema.ValidationError):
            self.validate(receipt=candidate)
        candidate = copy.deepcopy(self.receipt)
        candidate["not_proven"] = []
        with self.assertRaisesRegex(ValueError, "platform limitations"):
            self.validate(receipt=candidate)

    def test_receipt_cannot_substitute_another_discovery_authority(self):
        candidate = copy.deepcopy(self.receipt)
        candidate["architecture"]["chosen"] = "workspace index default"
        with self.assertRaisesRegex(ValueError, "architecture differs"):
            self.validate(receipt=candidate)

    def test_empty_policy_mapping_and_missing_finite_budget_are_rejected(self):
        manifest = copy.deepcopy(self.manifest)
        manifest["cases"][0]["policy_rows"] = []
        with self.assertRaises(RESEARCH.jsonschema.ValidationError):
            self.validate(manifest=manifest)
        policy = copy.deepcopy(self.policy)
        del policy["discovery_budget"]
        with self.assertRaises(RESEARCH.jsonschema.ValidationError):
            self.validate(policy=policy)

    def test_fixture_creation_cannot_escape_the_owned_tree(self):
        manifest = copy.deepcopy(self.manifest)
        manifest["cases"][0]["root"] = "../unowned"
        with self.assertRaisesRegex(ValueError, "fixture root escapes"):
            self.validate(manifest=manifest)
        for entry in (
            {"path": "../escape.pm", "kind": "file"},
            {"path": "root/link", "kind": "symlink", "target": "../../escape"},
            {
                "path": "root/link",
                "kind": "symlink",
                "target": "/fixture/escape",
            },
            {
                "path": "root/name",
                "kind": "file",
                "native_name_hex": "2e2e2f657363617065",
            },
        ):
            manifest = copy.deepcopy(self.manifest)
            manifest["cases"][0]["entries"].append(entry)
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                self.validate(manifest=manifest)

    def test_duplicate_json_keys_are_not_last_writer_wins(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "duplicate.json"
            path.write_text('{"disposition":"correct","disposition":"retain"}')
            with self.assertRaisesRegex(ValueError, "duplicate key"):
                RESEARCH.read_json(path)

    def test_lossy_display_does_not_masquerade_as_physical_identity(self):
        case = next(
            c for c in self.manifest["cases"] if c["id"] == "unicode-bytes"
        )
        self.assertEqual(case["current"]["scanned"], 6)
        self.assertEqual(len(case["current"]["selected_display"]), 5)
        self.assertEqual(
            len([e for e in case["entries"] if "native_name_hex" in e]), 2
        )

    @unittest.skipUnless(
        sys.platform == "linux", "real Linux fixture instrument"
    )
    def test_fake_empty_success_cannot_pass_the_characterization_instrument(
        self,
    ):
        # A controlled wrong executable passes the strict blocking control and
        # empty fixture, then fails the first nonempty selection observation.
        with tempfile.TemporaryDirectory() as temp:
            binary = Path(temp) / "empty-success"
            binary.write_text(
                f"#!{sys.executable}\nimport sys\n"
                "if '--check' in sys.argv:\n"
                " print('error: controlled blocker'); sys.exit(1)\n"
                "print('Files scanned: 0\\n"
                "No Perl files (.pm, .pl, .t) found.')\n"
            )
            binary.chmod(0o700)
            with self.assertRaisesRegex(
                AssertionError, "CHARACTERIZATION changed for extensions"
            ):
                RESEARCH.characterize(binary, self.policy, self.manifest)

    def test_text_instrument_separates_traversal_errors_from_selected_files(
        self,
    ):
        output = subprocess.CompletedProcess(
            [],
            0,
            stdout=(
                b"Files scanned: 1\nParse errors:\n  /root/bad.pm: error\n\n"
                b"Paths not scanned: 1\n  /root/subtree: Permission denied\n\n"
            ),
            stderr=b"",
        )
        observed, order, _, _ = RESEARCH.observe(output, Path("/root"))
        self.assertEqual(
            observed,
            {
                "selected_display": ["bad.pm"],
                "scanned": 1,
                "unscanned_display": ["subtree"],
                "exit": 0,
            },
        )
        self.assertEqual(order, ["bad.pm"])


if __name__ == "__main__":
    unittest.main()
