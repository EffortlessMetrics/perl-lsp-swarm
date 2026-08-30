#!/usr/bin/env python3
"""Schema-only contract checks for the #11470 standalone installer packet.

The three published JSON Schemas must reject, without the Rust example
validator, the structural contradictions that validate_manifest /
validate_plan_against_current_manifest / validate_result reject. Committed
positive fixtures stay schema-valid; every encoded law is probed with a minimal
negative mutation constructed in-test (no runtime artifacts committed).
"""

import copy
import importlib
import json
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCHEMA_DIR = REPO_ROOT / "schemas"
FIXTURE_DIR = REPO_ROOT / "fixtures" / "experience" / "install_owned_state"

OWNED_STATE_SCHEMA = SCHEMA_DIR / "standalone_owned_state.v1.schema.json"
REMOVAL_PLAN_SCHEMA = SCHEMA_DIR / "standalone_removal_plan.v1.schema.json"
UNINSTALL_RESULT_SCHEMA = SCHEMA_DIR / "standalone_uninstall_result.v1.schema.json"

MANIFEST_FIXTURES = [
    "manifest_canonical_full_install.json",
    "manifest_running_current.json",
    "manifest_partial_deletion_retry.json",
    "manifest_instrument_failed.json",
    "manifest_symlink_substitution.json",
    "manifest_user_edited_path.json",
]
PLAN_FIXTURES = [
    "plan_full_removal.json",
    "plan_rollback_retained.json",
    "plan_blocked_running_all_preserve.json",
]
RESULT_FIXTURES = [
    "result_already_absent_complete_evidence.json",
    "result_partial_failure_retryable.json",
]

SHA256_A = "a" * 64


def _registry():
    jsonschema = importlib.import_module("jsonschema")
    referencing = importlib.import_module("referencing")
    registry = referencing.Registry()
    for path in (OWNED_STATE_SCHEMA, REMOVAL_PLAN_SCHEMA, UNINSTALL_RESULT_SCHEMA):
        contents = json.loads(path.read_text(encoding="utf-8"))
        registry = registry.with_resource(
            contents["$id"], referencing.Resource.from_contents(contents)
        )
    return jsonschema, registry


def _validator(schema_path):
    jsonschema, registry = _registry()
    return jsonschema.Draft202012Validator(_load(schema_path), registry=registry)


def _load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


class StandaloneContractSchemaTests(unittest.TestCase):
    def setUp(self):
        self.manifest_validator = _validator(OWNED_STATE_SCHEMA)
        self.plan_validator = _validator(REMOVAL_PLAN_SCHEMA)
        self.result_validator = _validator(UNINSTALL_RESULT_SCHEMA)

    # ── positive fixtures stay valid ─────────────────────────────────────────

    def test_committed_manifest_fixtures_stay_schema_valid(self):
        for name in MANIFEST_FIXTURES:
            with self.subTest(fixture=name):
                document = _load(FIXTURE_DIR / name)
                self._expect_accepted(
                    self.manifest_validator,
                    document,
                    f"fixture {name} must stay schema-valid",
                )

    def test_committed_plan_fixtures_stay_schema_valid(self):
        for name in PLAN_FIXTURES:
            with self.subTest(fixture=name):
                document = _load(FIXTURE_DIR / name)
                self._expect_accepted(
                    self.plan_validator,
                    document,
                    f"fixture {name} must stay schema-valid",
                )

    def test_committed_result_fixtures_stay_schema_valid(self):
        for name in RESULT_FIXTURES:
            with self.subTest(fixture=name):
                document = _load(FIXTURE_DIR / name)
                self._expect_accepted(
                    self.result_validator,
                    document,
                    f"fixture {name} must stay schema-valid",
                )

    # ── owned_state: identity kind binds digest presence ──────────────────────

    def _mutated(self, validator, fixture_name, mutation, description):
        document = _load(FIXTURE_DIR / fixture_name)
        mutation(document)
        errors = list(validator.iter_errors(document))
        self.assertTrue(errors, f"{description}: expected rejection, got none")
        return errors

    @staticmethod
    def _first_entry_with_kind(document, kinds):
        for entry in document["entries"]:
            if entry["identity"]["kind"] in kinds:
                return entry
        raise AssertionError(f"fixture has no entry with kind in {kinds}")

    # ── owned_state: absolute roots are one canonical representation ─────────

    NON_CANONICAL_ROOTS = [
        ("/home/alice/.local/share/../etc/perllsp", "posix parent-segment traversal"),
        ("//home/alice/.local/share/perllsp", "double leading separator"),
        ("/home/alice/.local/share/perllsp/", "trailing separator"),
        ("/home/alice/.local/share//perllsp", "empty inner segment"),
        ("C:\\perllsp\\..\\Windows", "drive parent-segment traversal"),
        ("C:/perllsp/bin", "drive form with non-canonical separator"),
        ("/home/alice/.local/share/perllsp\\x", "posix form with backslash"),
        ("\\\\?\\C:\\perllsp", "dos device path escape"),
    ]

    CANONICAL_ROOTS = [
        "/opt/perllsp",
        "/srv/.perllsp-hidden/x",
        "C:\\Users\\alice\\.perllsp",
        "\\\\file.corp\\share\\perllsp",
    ]

    def test_absolute_root_must_be_one_canonical_representation(self):
        for root, description in self.NON_CANONICAL_ROOTS:
            with self.subTest(root=root):

                def set_root(doc, value=root):
                    doc["install_root"]["absolute_path"] = value

                self._mutated(
                    self.manifest_validator,
                    "manifest_canonical_full_install.json",
                    set_root,
                    f"{description} must be schema-invalid",
                )

    def test_canonical_root_forms_stay_schema_valid(self):
        for root in self.CANONICAL_ROOTS:
            with self.subTest(root=root):
                document = _load(FIXTURE_DIR / "manifest_canonical_full_install.json")
                document["install_root"]["absolute_path"] = root
                self._expect_accepted(
                    self.manifest_validator,
                    document,
                    f"canonical root {root} must stay valid",
                )

    def test_digest_backed_identity_requires_sha256(self):
        for kind in ("sha256_content", "directory_tree_digest"):
            with self.subTest(kind=kind):
                self._mutated(
                    self.manifest_validator,
                    "manifest_canonical_full_install.json",
                    lambda doc: self._first_entry_with_kind(doc, {kind})
                    ["identity"].pop("sha256"),
                    f"{kind} without sha256 must be schema-invalid",
                )

    def test_unavailable_identity_forbids_sha256(self):
        self._mutated(
            self.manifest_validator,
            "manifest_instrument_failed.json",
            lambda doc: self._first_entry_with_kind(doc, {"unavailable"})
            ["identity"].update({"sha256": SHA256_A}),
            "unavailable identity carrying sha256 must be schema-invalid",
        )

    # ── removal_plan: destructive rows require verified identity ─────────────

    def test_destructive_action_requires_verified_identity_sha256(self):
        for action_kind in ("remove_exact", "remove_marker"):
            with self.subTest(action=action_kind):

                def strip_digest(doc, kind=action_kind):
                    for action in doc["actions"]:
                        if action["action"] == kind:
                            del action["verified_identity_sha256"]
                            return
                    raise AssertionError(f"fixture has no {kind} action")

                self._mutated(
                    self.plan_validator,
                    "plan_full_removal.json",
                    strip_digest,
                    f"{action_kind} without verified_identity_sha256 must be "
                    "schema-invalid",
                )

    def test_non_destructive_action_forbids_verified_identity_sha256(self):
        for action_kind in ("preserve", "revalidate"):
            with self.subTest(action=action_kind):

                def add_digest(doc, kind=action_kind):
                    for action in doc["actions"]:
                        if action["action"] == kind:
                            action["verified_identity_sha256"] = SHA256_A
                            return
                    raise AssertionError(f"fixture has no {kind} action")

                self._mutated(
                    self.plan_validator,
                    "plan_blocked_running_all_preserve.json",
                    add_digest,
                    f"{action_kind} carrying verified_identity_sha256 must be "
                    "schema-invalid",
                )

    def test_plan_postcondition_lists_are_exact_sets(self):
        duplicate_preserved = (
            lambda doc: doc["postconditions"]["verify_preserved"].append(
                "notes.txt"
            )
        )
        self._mutated(
            self.plan_validator,
            "plan_full_removal.json",
            duplicate_preserved,
            "duplicate verify_preserved entry must be schema-invalid",
        )

        duplicate_absent = (
            lambda doc: doc["postconditions"]["verify_entries_absent"].append("current")
        )
        self._mutated(
            self.plan_validator,
            "plan_full_removal.json",
            duplicate_absent,
            "duplicate verify_entries_absent entry must be schema-invalid",
        )

        duplicate_cleanup = lambda doc: doc["path_cleanup"]["entries"].append(
            ".perllsp-path-marker"
        )
        self._mutated(
            self.plan_validator,
            "plan_full_removal.json",
            duplicate_cleanup,
            "duplicate path_cleanup entry must be schema-invalid",
        )

    # ── uninstall_result: terminal coherence laws ────────────────────────────

    def _result_case(self, **overrides):
        document = {
            "schema_version": "standalone_uninstall_result.v1",
            "result": "removed",
            "claim_boundary": "schema-only coherence probe; reports nothing.",
            "plan_id": "probe-plan",
            "bound_manifest_sha256": SHA256_A,
            "removed_entries": ["current"],
            "preserved_entries": [],
            "failed_entries": [],
            "complete_evidence": True,
            "activation_state": "conditional_activation_not_selected",
            "retryable": False,
            "limitations": [],
        }
        document.update(copy.deepcopy(overrides))
        return document

    def _expect_rejected(self, document, description):
        errors = list(self.result_validator.iter_errors(document))
        self.assertTrue(errors, f"{description}: expected rejection, got none")

    def _expect_accepted(self, validator, document, description):
        errors = list(validator.iter_errors(document))
        self.assertEqual(
            [], errors, f"{description}: expected acceptance, got {errors}"
        )

    def test_removed_requires_entries_evidence_and_finality(self):
        base = self._result_case()
        self._expect_accepted(self.result_validator, base, "canonical removed result")

        empty_removal = self._result_case(removed_entries=[])
        self._expect_rejected(empty_removal, "removed with no removed_entries")

        incomplete = self._result_case(complete_evidence=False)
        self._expect_rejected(incomplete, "removed without complete_evidence")

        retryable = self._result_case(retryable=True)
        self._expect_rejected(retryable, "removed marked retryable")

        with_failures = self._result_case(
            failed_entries=[
                {
                    "relative_path": "current",
                    "stage": "remove",
                    "detail": "probe failure entry",
                }
            ]
        )
        self._expect_rejected(with_failures, "removed carrying failed_entries")

    def test_already_absent_requires_complete_evidence_and_removes_nothing(self):
        incomplete = self._result_case(
            result="already_absent_owned_state", complete_evidence=False
        )
        self._expect_rejected(incomplete, "already_absent without complete_evidence")

        coherent_absence = self._result_case(
            result="already_absent_owned_state",
            complete_evidence=True,
            removed_entries=[],
        )
        self._expect_accepted(
            self.result_validator, coherent_absence, "coherent already_absent result"
        )

        absent_with_failures = self._result_case(
            result="already_absent_owned_state",
            failed_entries=[
                {
                    "relative_path": "current",
                    "stage": "verify",
                    "detail": "probe failure entry",
                }
            ],
        )
        self._expect_rejected(absent_with_failures, "already_absent with failures")

    def test_nothing_executed_outcomes_claim_no_preserved_rows(self):
        absent_with_preserved = self._result_case(
            result="already_absent_owned_state",
            complete_evidence=True,
            removed_entries=[],
            preserved_entries=["notes.txt"],
        )
        self._expect_rejected(absent_with_preserved, "already_absent claiming preserved")

        blocked_with_preserved = self._result_case(
            result="blocked_running",
            preserved_entries=["notes.txt"],
        )
        self._expect_rejected(blocked_with_preserved, "blocked_running claiming preserved")

        not_applicable_with_preserved = self._result_case(
            result="not_applicable",
            activation_state="conditional_activation_selected",
            removed_entries=[],
            preserved_entries=["notes.txt"],
        )
        self._expect_rejected(
            not_applicable_with_preserved, "not_applicable claiming preserved"
        )

    def test_reported_entry_populations_are_exact_sets(self):
        duplicate_removed = self._result_case(
            removed_entries=["current", "current"]
        )
        self._expect_rejected(duplicate_removed, "duplicate removed_entries")

        duplicate_preserved = self._result_case(
            preserved_entries=["notes.txt", "notes.txt"]
        )
        self._expect_rejected(duplicate_preserved, "duplicate preserved_entries")

    def test_partial_failure_stays_explicit(self):
        silent = self._result_case(result="partial_failure", failed_entries=[])
        self._expect_rejected(silent, "partial_failure without failed_entries")

    def test_path_cleanup_failed_requires_failures_and_prior_removal(self):
        no_failures = self._result_case(
            result="path_cleanup_failed", failed_entries=[]
        )
        self._expect_rejected(no_failures, "path_cleanup_failed without failures")

        nothing_removed = self._result_case(
            result="path_cleanup_failed",
            removed_entries=[],
            failed_entries=[
                {
                    "relative_path": "bin/perllsp",
                    "stage": "marker_cleanup",
                    "detail": "probe marker cleanup failure",
                }
            ],
        )
        self._expect_rejected(nothing_removed, "path_cleanup_failed with no removals")

    def test_not_applicable_requires_conditional_activation_selection(self):
        unselected = self._result_case(
            result="not_applicable",
            activation_state="conditional_activation_not_selected",
        )
        self._expect_rejected(unselected, "not_applicable without activation gate")

        selected = self._result_case(
            result="not_applicable",
            activation_state="conditional_activation_selected",
        )
        self._expect_accepted(
            self.result_validator, selected, "gated not_applicable result"
        )

    def test_blocked_and_unproven_outcomes_delete_nothing(self):
        for outcome in (
            "blocked_running",
            "blocked_unknown_or_foreign",
            "root_or_manifest_mismatch",
            "cancelled",
            "instrument_failure",
            "not_proven",
        ):
            with self.subTest(result=outcome):
                removed_anyway = self._result_case(
                    result=outcome, removed_entries=["current"]
                )
                self._expect_rejected(
                    removed_anyway, f"{outcome} reporting removed entries"
                )
                failed_anyway = self._result_case(
                    result=outcome,
                    failed_entries=[
                        {
                            "relative_path": "current",
                            "stage": "verify",
                            "detail": "probe failure entry",
                        }
                    ],
                )
                self._expect_rejected(
                    failed_anyway, f"{outcome} carrying failed_entries"
                )


if __name__ == "__main__":
    unittest.main()
