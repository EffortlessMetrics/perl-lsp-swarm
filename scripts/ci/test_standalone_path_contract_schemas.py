#!/usr/bin/env python3
"""Schema-only contract checks for the #11521 standalone PATH packet.

The POSIX and Windows installer adapters that will implement these mechanics are
not Rust, so the three published JSON Schemas must reject, without the Rust
example validator, the structural contradictions that validate_plan /
validate_persistence / validate_fresh_process reject. Committed positive
fixtures stay schema-valid; every encoded law is probed with a minimal negative
mutation constructed in-test (no runtime artifacts committed).
"""

import copy
import importlib
import json
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCHEMA_DIR = REPO_ROOT / "schemas"
FIXTURE_DIR = REPO_ROOT / "fixtures" / "experience" / "install_path_persistence"

PLAN_SCHEMA = SCHEMA_DIR / "standalone_path_plan.v1.schema.json"
PERSISTENCE_SCHEMA = SCHEMA_DIR / "standalone_path_persistence.v1.schema.json"
FRESH_PROCESS_SCHEMA = SCHEMA_DIR / "standalone_fresh_process.v1.schema.json"

PLAN_FIXTURES = [
    "plan_posix_installer_user_scope.json",
    "plan_windows_registry_user_path.json",
    "plan_package_manager_owned.json",
    "plan_manual_instruction_only.json",
    "plan_wsl_already_visible.json",
]
PERSISTENCE_FIXTURES = [
    "persistence_installer_persisted.json",
    "persistence_already_visible_no_change.json",
    "persistence_conflict_wrong_existing_entry.json",
    "persistence_permission_or_lock_failure.json",
    "persistence_instrument_failure.json",
    "persistence_package_manager_owned.json",
    "persistence_manual_action_required.json",
    "persistence_conflict_user_edited_entry.json",
]
FRESH_PROCESS_FIXTURES = [
    "fresh_visible_after_documented_new_session.json",
    "fresh_visible_immediately.json",
    "fresh_wrong_ambient_binary.json",
    "fresh_ambiguous_resolution.json",
    "fresh_not_found.json",
    "fresh_package_manager_owned_path.json",
]

# Committed invalid fixtures, and the exact law each one violates. The Rust
# battery proves each becomes valid once its one violation is repaired; here the
# schemas must independently refuse them.
INVALID_PLAN_FIXTURES = [
    "plan_invalid_unconfirmed_candidate.json",
    "plan_invalid_session_only_installer.json",
]
INVALID_PERSISTENCE_FIXTURES = [
    "persistence_invalid_session_only_visible.json",
    "persistence_invalid_persisted_without_mutation.json",
]
INVALID_FRESH_PROCESS_FIXTURES = [
    "fresh_invalid_current_process_prepend.json",
    "fresh_invalid_absolute_path_launch.json",
    "fresh_invalid_harness_prepared_path.json",
]


def _load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def _registry():
    jsonschema = importlib.import_module("jsonschema")
    referencing = importlib.import_module("referencing")
    registry = referencing.Registry()
    for path in (PLAN_SCHEMA, PERSISTENCE_SCHEMA, FRESH_PROCESS_SCHEMA):
        contents = _load(path)
        registry = registry.with_resource(
            contents["$id"], referencing.Resource.from_contents(contents)
        )
    return jsonschema, registry


def _validator(schema_path):
    jsonschema, registry = _registry()
    return jsonschema.Draft202012Validator(_load(schema_path), registry=registry)


class StandalonePathContractSchemaTests(unittest.TestCase):
    def setUp(self):
        self.plan_validator = _validator(PLAN_SCHEMA)
        self.persistence_validator = _validator(PERSISTENCE_SCHEMA)
        self.fresh_validator = _validator(FRESH_PROCESS_SCHEMA)

    # ── helpers ─────────────────────────────────────────────────────────────

    def _expect_accepted(self, validator, document, message):
        errors = sorted(validator.iter_errors(document), key=lambda error: error.path)
        self.assertEqual(
            [], [error.message for error in errors], f"{message}: {errors}"
        )

    def _expect_rejected(self, validator, document, message):
        self.assertTrue(
            any(True for _ in validator.iter_errors(document)),
            f"{message}: the schema accepted a document it must refuse",
        )

    def _plan(self, name="plan_posix_installer_user_scope.json"):
        return copy.deepcopy(_load(FIXTURE_DIR / name))

    def _persistence(self, name="persistence_installer_persisted.json"):
        return copy.deepcopy(_load(FIXTURE_DIR / name))

    def _fresh(self, name="fresh_visible_immediately.json"):
        return copy.deepcopy(_load(FIXTURE_DIR / name))

    # ── positive fixtures stay valid ────────────────────────────────────────

    def test_committed_plan_fixtures_stay_schema_valid(self):
        for name in PLAN_FIXTURES:
            with self.subTest(fixture=name):
                self._expect_accepted(
                    self.plan_validator,
                    _load(FIXTURE_DIR / name),
                    f"fixture {name} must stay schema-valid",
                )

    def test_committed_persistence_fixtures_stay_schema_valid(self):
        for name in PERSISTENCE_FIXTURES:
            with self.subTest(fixture=name):
                self._expect_accepted(
                    self.persistence_validator,
                    _load(FIXTURE_DIR / name),
                    f"fixture {name} must stay schema-valid",
                )

    def test_committed_fresh_process_fixtures_stay_schema_valid(self):
        for name in FRESH_PROCESS_FIXTURES:
            with self.subTest(fixture=name):
                self._expect_accepted(
                    self.fresh_validator,
                    _load(FIXTURE_DIR / name),
                    f"fixture {name} must stay schema-valid",
                )

    def test_committed_invalid_fixtures_are_refused_without_the_rust_validator(self):
        cases = (
            (self.plan_validator, INVALID_PLAN_FIXTURES),
            (self.persistence_validator, INVALID_PERSISTENCE_FIXTURES),
            (self.fresh_validator, INVALID_FRESH_PROCESS_FIXTURES),
        )
        for validator, names in cases:
            for name in names:
                with self.subTest(fixture=name):
                    self._expect_rejected(
                        validator,
                        _load(FIXTURE_DIR / name),
                        f"fixture {name} encodes a violated law",
                    )

    # ── plan laws ───────────────────────────────────────────────────────────

    def test_installer_mutation_requires_the_confirmed_current_selection(self):
        for state in ("unconfirmed", "superseded", "not_proven"):
            with self.subTest(confirmation_state=state):
                document = self._plan()
                document["subject"]["confirmation_state"] = state
                self._expect_rejected(
                    self.plan_validator,
                    document,
                    "an unconfirmed candidate must not reach installer-owned mutation",
                )

    def test_only_the_installer_owns_an_entry(self):
        document = self._plan("plan_package_manager_owned.json")
        document["path_policy"]["owned_entry"] = {
            "entry_kind": "profile_line",
            "entry_location": "/home/operator/.profile",
            "entry_value": "/opt/homebrew/Cellar/perl-lsp/bin",
            "marker": "perl-lsp-installer-owned-path-entry",
        }
        self._expect_rejected(
            self.plan_validator, document, "ownership must not be laundered"
        )

    def test_an_installer_owned_policy_must_name_an_entry(self):
        document = self._plan()
        document["path_policy"]["owned_entry"] = {"entry_kind": "none"}
        self._expect_rejected(
            self.plan_validator,
            document,
            "installer-owned persistence must name exactly one canonical entry",
        )

    def test_an_owned_entry_must_be_completely_specified(self):
        for dropped in ("entry_location", "entry_value", "marker"):
            with self.subTest(dropped=dropped):
                document = self._plan()
                del document["path_policy"]["owned_entry"][dropped]
                self._expect_rejected(
                    self.plan_validator,
                    document,
                    f"an owned entry without {dropped} is not exactly removable",
                )

    def test_policy_and_mutation_owner_cannot_disagree(self):
        document = self._plan()
        document["path_policy"]["mutation_owner"] = "package_manager"
        self._expect_rejected(
            self.plan_validator, document, "each policy has exactly one owner"
        )

    def test_a_session_only_scope_is_never_installer_owned(self):
        document = self._plan()
        document["path_policy"]["scope"] = "session_only"
        self._expect_rejected(
            self.plan_validator,
            document,
            "a current-process change is not durable persistence",
        )

    def test_a_system_elevated_scope_requires_a_system_install_root(self):
        document = self._plan()
        document["path_policy"]["scope"] = "system_elevated"
        self._expect_rejected(
            self.plan_validator, document, "user scope is never silently widened"
        )

    def test_a_shell_specific_scope_names_the_exact_shell(self):
        document = self._plan("plan_manual_instruction_only.json")
        document["environment"]["shell_family"] = None
        self._expect_rejected(
            self.plan_validator,
            document,
            "one shell's evidence never satisfies all shells",
        )

    def test_persistence_mechanisms_do_not_cross_platforms(self):
        document = self._plan()
        document["path_policy"]["owned_entry"]["entry_kind"] = "registry_user_path"
        self._expect_rejected(
            self.plan_validator, document, "the registry is a Windows mechanism"
        )

        windows = self._plan("plan_windows_registry_user_path.json")
        windows["path_policy"]["owned_entry"]["entry_kind"] = "profile_line"
        self._expect_rejected(
            self.plan_validator,
            windows,
            "profile lines are POSIX and WSL mechanisms",
        )

    def test_no_path_change_requires_no_new_session(self):
        document = self._plan("plan_package_manager_owned.json")
        document["path_policy"]["policy"] = "no_path_change"
        document["path_policy"]["mutation_owner"] = "none"
        document["path_policy"]["new_session_requirement"] = "new_shell"
        self._expect_rejected(
            self.plan_validator,
            document,
            "a policy that changes nothing cannot require a new session",
        )

    def test_a_declared_prior_state_digest_must_be_present_and_exact(self):
        document = self._plan()
        document["prior_state"]["sha256"] = None
        self._expect_rejected(
            self.plan_validator,
            document,
            "a sha256_content identity must carry the digest it claims",
        )

        unavailable = self._plan("plan_windows_registry_user_path.json")
        unavailable["prior_state"]["sha256"] = "a" * 64
        self._expect_rejected(
            self.plan_validator,
            unavailable,
            "ambiguous prior state stays ambiguous",
        )

    # ── redaction and injection laws ────────────────────────────────────────

    def test_a_complete_path_value_cannot_enter_a_durable_document(self):
        document = self._plan()
        document["path_policy"]["owned_entry"]["entry_value"] = (
            "/home/operator/.local/share/perl-lsp/bin:/usr/bin:/bin"
        )
        self._expect_rejected(
            self.plan_validator,
            document,
            "durable receipts carry the exact owned entry, never the complete PATH",
        )

    def test_expansion_syntax_cannot_reach_a_profile_or_the_registry(self):
        for injected in (
            "/home/operator/$HOME/bin",
            "/home/operator/`id -u`/bin",
            "/home/operator/bin;rm -rf /",
            "%USERPROFILE%\\bin",
            "~/bin",
        ):
            with self.subTest(injected=injected):
                document = self._plan()
                document["path_policy"]["owned_entry"]["entry_value"] = injected
                self._expect_rejected(
                    self.plan_validator,
                    document,
                    "path text is a literal exact identity and is never expanded",
                )

    def test_a_command_name_with_a_separator_is_not_a_path_lookup(self):
        for name in ("bin/perllsp", "bin\\perllsp", "./perllsp"):
            with self.subTest(command_name=name):
                document = self._plan()
                document["subject"]["command_name"] = name
                self._expect_rejected(
                    self.plan_validator,
                    document,
                    "a command name with a separator is a path invocation",
                )

    def test_a_conflict_reason_stays_a_typed_field(self):
        document = self._persistence("persistence_conflict_wrong_existing_entry.json")
        document["conflicting_entry"]["reason"] = "PATH=/usr/bin:/bin had an old entry"
        self._expect_rejected(
            self.persistence_validator,
            document,
            "free text in a durable receipt defeats bounded-field redaction",
        )

    # ── persistence laws ────────────────────────────────────────────────────

    def test_outcomes_that_wrote_nothing_cannot_have_written_anything(self):
        for result in (
            "already_visible_no_change",
            "manual_action_required",
            "permission_or_lock_failure",
            "cancelled",
            "not_proven",
        ):
            with self.subTest(result=result):
                document = self._persistence()
                document["result"] = result
                document["mutation_performed"] = True
                self._expect_rejected(
                    self.persistence_validator,
                    document,
                    f"{result} asserts that no durable state was written",
                )

    def test_installer_persisted_requires_a_durable_write_that_added_the_entry(self):
        document = self._persistence()
        document["mutation_performed"] = False
        self._expect_rejected(
            self.persistence_validator,
            document,
            "installer_persisted must have written durable state",
        )

        absent = self._persistence()
        absent["entry_state"] = "absent"
        self._expect_rejected(
            self.persistence_validator,
            absent,
            "installer_persisted must have added the owned entry",
        )

    def test_an_added_entry_requires_a_durable_write(self):
        document = self._persistence()
        document["result"] = "new_session_required"
        document["new_session_required"] = True
        document["mutation_performed"] = False
        self._expect_rejected(
            self.persistence_validator,
            document,
            "an entry cannot appear without a durable write",
        )

    def test_a_conflict_must_name_the_state_it_conflicts_with(self):
        document = self._persistence("persistence_conflict_wrong_existing_entry.json")
        document["conflicting_entry"] = None
        self._expect_rejected(
            self.persistence_validator,
            document,
            "a conflict must name the exact conflicting state",
        )

    def test_only_a_conflict_may_report_a_conflicting_entry(self):
        document = self._persistence()
        document["conflicting_entry"] = {
            "entry_kind": "profile_line",
            "entry_location": "/home/operator/.bash_profile",
            "reason": "duplicate_entry",
        }
        self._expect_rejected(
            self.persistence_validator,
            document,
            "an observed conflict cannot be reported under another outcome",
        )

    def test_a_session_only_scope_cannot_claim_a_visible_entry(self):
        for result in ("already_visible_no_change", "installer_persisted"):
            with self.subTest(result=result):
                document = self._persistence()
                document["result"] = result
                if result == "already_visible_no_change":
                    document["mutation_performed"] = False
                    document["entry_state"] = "already_present"
                document["observed_scope"] = "session_only"
                self._expect_rejected(
                    self.persistence_validator,
                    document,
                    "a current-process change is not durable persistence",
                )

    def test_exactly_one_canonical_owned_entry_or_none(self):
        for count in (0, 2, 3):
            with self.subTest(owned_entries_observed=count):
                document = self._persistence()
                document["owned_entries_observed"] = count
                self._expect_rejected(
                    self.persistence_validator,
                    document,
                    "duplicate, case, and alias spellings are not idempotent",
                )

        absent = self._persistence("persistence_permission_or_lock_failure.json")
        absent["owned_entries_observed"] = 1
        self._expect_rejected(
            self.persistence_validator,
            absent,
            "an absent entry cannot be observed",
        )

    def test_an_incomplete_instrument_concludes_nothing(self):
        document = self._persistence("persistence_instrument_failure.json")
        document["result"] = "already_visible_no_change"
        document["entry_state"] = "already_present"
        document["owned_entries_observed"] = 1
        self._expect_rejected(
            self.persistence_validator,
            document,
            "an incomplete instrument cannot conclude a real outcome",
        )

    def test_a_complete_instrument_cannot_report_instrument_failure(self):
        document = self._persistence("persistence_instrument_failure.json")
        document["instrument_complete"] = True
        self._expect_rejected(
            self.persistence_validator,
            document,
            "instrument_failure cannot be reported by a complete instrument",
        )

    # ── fresh-process laws ──────────────────────────────────────────────────

    def test_a_manufactured_environment_cannot_prove_discoverability(self):
        cases = (
            ("origin", "current_process"),
            ("origin", "installer_child_process"),
            ("environment_source", "installer_mutated_current_process"),
            ("environment_source", "harness_injected_path"),
            ("path_prepared_by_harness", True),
        )
        for field, value in cases:
            with self.subTest(field=field, value=value):
                document = self._fresh()
                document["session"][field] = value
                self._expect_rejected(
                    self.fresh_validator,
                    document,
                    "a visibility claim requires a genuinely new ambient session",
                )

    def test_an_absolute_path_invocation_cannot_prove_discoverability(self):
        document = self._fresh()
        document["lookup"]["method"] = "absolute_path_invocation"
        self._expect_rejected(
            self.fresh_validator,
            document,
            "invoking a known path proves nothing about command discoverability",
        )

    def test_a_wrong_ambient_binary_cannot_be_reported_as_success(self):
        for result in ("path_visible_immediately", "not_found", "manual_path_action_required"):
            with self.subTest(result=result):
                document = self._fresh("fresh_wrong_ambient_binary.json")
                document["result"] = result
                self._expect_rejected(
                    self.fresh_validator,
                    document,
                    "a resolved non-candidate is wrong_ambient_binary",
                )

    def test_wrong_ambient_binary_must_have_resolved_a_non_candidate(self):
        document = self._fresh("fresh_wrong_ambient_binary.json")
        document["lookup"]["resolved"] = None
        self._expect_rejected(
            self.fresh_validator,
            document,
            "wrong_ambient_binary must observe the exact binary that won",
        )

    def test_an_ambiguous_lookup_is_reported_as_ambiguous_and_nothing_else(self):
        document = self._fresh("fresh_ambiguous_resolution.json")
        document["result"] = "not_found"
        self._expect_rejected(
            self.fresh_validator,
            document,
            "ambiguity stays visible",
        )

    def test_ambiguity_requires_at_least_two_observed_candidates(self):
        document = self._fresh("fresh_ambiguous_resolution.json")
        document["lookup"]["competing_candidates"] = [
            {"path": "/usr/local/bin/perllsp", "sha256": None}
        ]
        self._expect_rejected(
            self.fresh_validator,
            document,
            "ambiguity requires competing candidates",
        )

    def test_not_found_cannot_hide_candidates_it_observed(self):
        document = self._fresh("fresh_not_found.json")
        document["lookup"]["competing_candidates"] = [
            {"path": "/usr/local/bin/perllsp", "sha256": None}
        ]
        self._expect_rejected(
            self.fresh_validator,
            document,
            "not_found cannot have observed candidates",
        )

    def test_the_observed_candidate_set_is_exact(self):
        document = self._fresh("fresh_ambiguous_resolution.json")
        duplicate = document["lookup"]["competing_candidates"][0]
        document["lookup"]["competing_candidates"] = [duplicate, copy.deepcopy(duplicate)]
        self._expect_rejected(
            self.fresh_validator,
            document,
            "the observed candidate set carries no duplicates",
        )

    def test_a_lookup_never_attempted_concludes_nothing_about_discoverability(self):
        document = self._fresh()
        document["lookup"]["method"] = "not_attempted"
        self._expect_rejected(
            self.fresh_validator,
            document,
            "a result that was never looked up is not an observation",
        )

    def test_a_failed_startup_is_not_a_clean_visibility_claim(self):
        document = self._fresh()
        document["startup_disposition"] = "failed"
        self._expect_rejected(
            self.fresh_validator,
            document,
            "a failed start is not a clean visibility claim",
        )

    def test_an_incomplete_instrument_concludes_nothing_about_a_lookup(self):
        document = self._fresh()
        document["instrument_complete"] = False
        self._expect_rejected(
            self.fresh_validator,
            document,
            "an incomplete instrument cannot conclude a real outcome",
        )

    # ── review findings (PR #16014, Codex) ──────────────────────────────────

    def test_a_current_process_edit_cannot_certify_visibility(self):
        """A schema-only consumer must refuse manufactured persistence evidence."""
        for result, entry_state in (
            ("already_visible_no_change", "already_present"),
            ("installer_persisted", "added"),
        ):
            with self.subTest(result=result):
                document = self._persistence()
                document["result"] = result
                document["entry_state"] = entry_state
                document["mutation_performed"] = False
                document["current_process_env_mutated"] = True
                self._expect_rejected(
                    self.persistence_validator,
                    document,
                    "a current-process edit with no durable write is not persistence",
                )

    def test_install_root_containment_is_a_declared_consumer_obligation(self):
        """SPP-C06 cannot be expressed in JSON Schema, so it is declared, not silent.

        `plan_invalid_entry_outside_root.json` is deliberately absent from
        INVALID_PLAN_FIXTURES: the schema accepts it and only the Rust validator
        rejects it. This test pins that gap so it stays a documented consumer
        obligation rather than becoming an accidental hole — if a future schema
        can express the containment law, this test fails and should be replaced
        by adding the fixture to INVALID_PLAN_FIXTURES.
        """
        document = _load(FIXTURE_DIR / "plan_invalid_entry_outside_root.json")
        self._expect_accepted(
            self.plan_validator,
            document,
            "the schema cannot relate entry_value to install_root",
        )
        entry_value = _load(PLAN_SCHEMA)["$defs"]["owned_entry"]["properties"]["entry_value"]
        description = entry_value["oneOf"][0]["description"]
        self.assertIn(
            "NOT SCHEMA-ENFORCED",
            description,
            "the unenforceable law must be declared to schema-only consumers",
        )
        self.assertIn("install_root", description)

    def test_unmodelled_fields_and_words_are_refused(self):
        document = self._plan()
        document["surprise"] = "unmodelled"
        self._expect_rejected(
            self.plan_validator, document, "unmodelled fields are refused"
        )

        receipt = self._persistence()
        receipt["result"] = "probably_fine"
        self._expect_rejected(
            self.persistence_validator, receipt, "unmodelled result words are refused"
        )


if __name__ == "__main__":
    unittest.main()
