#!/usr/bin/env python3
"""Focused tests for scripts/ci/validate_review_surfaces.py.

Each test fixture is a minimal repository tree plus a minimal
policy/review-surfaces.toml. The issue-#11793 first falsifiers are encoded as
mutations of the compliant fixture; each must produce its typed error.
"""

from __future__ import annotations

import io
import shutil
import sys
import tempfile
import tomllib
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from typing import Any
from unittest import mock

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

import validate_review_surfaces as vrs  # noqa: E402

FIXTURE_DETECTOR_FILES = (
    "src/authority/catalog.rs",
    "src/authority/generated.md",
    "docs/policy/REVIEW_SURFACES.md",
)
FIXTURE_DETECTOR_DIRS = ("src/authority/nested",)

SELF_PATHS = (
    "policy/review-surfaces.toml",
    "scripts/ci/validate_review_surfaces.py",
    "scripts/ci/test_validate_review_surfaces.py",
    "docs/policy/REVIEW_SURFACES.md",
    ".github/CODEOWNERS",
)


def write_tree(root: Path, files: dict[str, str]) -> None:
    for rel, content in files.items():
        target = root / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")


def compliant_manifest_text() -> str:
    self_paths = ",\n  ".join(f'"{path}"' for path in SELF_PATHS)
    return f"""
schema_version = 1
policy = "review-surfaces"
owner = "EffortlessMetrics"
status = "advisory"
updated = "2026-08-24"
issue = "11793"
classification_rule = "One row per authority."
enforcement_boundary = "Advisory metadata only."
successor_consumption = "#11795 consumes the projection; #11796 consumes routing dispositions."
projection_doc = "docs/policy/REVIEW_SURFACES.md"
validator_script = "scripts/ci/validate_review_surfaces.py"
validator_test = "scripts/ci/test_validate_review_surfaces.py"

[families]
semantic_issue_completion = "Close contracts."
configuration_authority = "Configuration catalogs and adapters."
executable_policy_and_public_migration = "Executable policy surfaces."

[profile.semantic_close_authority]
fresh_direction = "Challenge false closes."
lenses = ["subject_evidence_identity", "lifecycle_currentness_concurrency", "spec_test_docs_consistency"]
required_roles = ["adversarial_challenger"]
packet_contract = "schemas/agent_review_packet.v1.schema.json"
handoff_authority = "#11701"

[profile.public_api_or_retirement_authority]
fresh_direction = "Challenge denominator movement."
lenses = ["release_external_boundary", "architecture_authority_duplication", "subject_evidence_identity"]
required_roles = ["adversarial_challenger"]
packet_contract = "schemas/agent_review_packet.v1.schema.json"
handoff_authority = "#10881"

[code_owner_identity.EffortlessSteven]
kind = "user"
status = "valid"
permission = "admin"
validation_method = "api"
evidence_date = "2026-08-24"

[surface.manifest_self]
family = "semantic_issue_completion"
authority = "This manifest."
controller = "#11793"
conflict_key = "authority_review.manifest"
risk_class = "live_repository_policy_control"
review_profile = "semantic_close_authority"
required_evidence = "checked_projection"
first_falsifier = "The manifest omits itself."
enforcement_successor = "#11796"
code_owner_route = {{ kind = "not_proven", resolution_owner = "#11796", note = "deferred" }}
paths = [
  {self_paths},
]

[surface.authority_catalog]
family = "configuration_authority"
authority = "Configuration catalog."
controller = "#10790"
conflict_key = "config.authority_catalog"
risk_class = "configuration_control"
review_profile = "semantic_close_authority"
required_evidence = "current_head_reviewer_packet"
first_falsifier = "An unregistered leaf slips through."
enforcement_successor = "#11796"
code_owner_route = {{ kind = "not_proven", resolution_owner = "#11796", note = "deferred" }}
paths = ["src/authority/**"]

[residue.deferred_store]
authority = "Accepted configuration store."
parent_issue = "#7057"
reason = "Not landed on current main."
resolution_owner = "#7057"
"""


class ValidateReviewSurfacesTests(unittest.TestCase):
    def make_root(self, manifest_text: str | None = None) -> Path:
        root = Path(tempfile.mkdtemp(prefix="review-surfaces-fixture-"))
        self.addCleanup(shutil.rmtree, root, True)
        files: dict[str, str] = {
            "src/authority/catalog.rs": "catalog",
            "src/authority/generated.md": "generated projection",
            "src/authority/nested/deep.rs": "nested",
            "docs/policy/REVIEW_SURFACES.md": "stale body\n",
            ".github/CODEOWNERS": "* @EffortlessSteven\n",
            "policy/review-surfaces.toml": "",
            "scripts/ci/validate_review_surfaces.py": "",
            "scripts/ci/test_validate_review_surfaces.py": "",
        }
        write_tree(root, files)
        manifest_path = root / "policy/review-surfaces.toml"
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_text(
            manifest_text if manifest_text is not None else compliant_manifest_text(),
            encoding="utf-8",
        )
        return root

    def run_main(self, root: Path, extra_args: list[str]) -> tuple[int, str]:
        old_argv = sys.argv
        try:
            sys.argv = ["validate_review_surfaces.py", "--root", str(root), *extra_args]
            stdout = io.StringIO()
            with redirect_stdout(stdout):
                status = vrs.main()
        finally:
            sys.argv = old_argv
        return status, stdout.getvalue()

    def fixture_detectors(self, files: tuple[str, ...] = FIXTURE_DETECTOR_FILES) -> Any:
        return (
            mock.patch.object(vrs, "DETECTOR_FILES", files),
            mock.patch.object(vrs, "DETECTOR_DIRS", FIXTURE_DETECTOR_DIRS),
        )

    def assert_strict_failure(self, root: Path, expected_error: str, extra_args: list[str] | None = None) -> str:
        patchers = self.fixture_detectors()
        for patcher in patchers:
            patcher.start()
        self.addCleanup(patchers[1].stop)
        self.addCleanup(patchers[0].stop)
        status, output = self.run_main(root, ["--strict"] + (extra_args or []))
        self.assertEqual(1, status, output)
        self.assertIn(expected_error, output)
        return output

    def load_manifest_document(self, text: str) -> dict[str, Any]:
        return tomllib.loads(text)

    # ------------------------------------------------------------------
    # Compliant denominator
    # ------------------------------------------------------------------

    def test_compliant_fixture_passes_strict(self) -> None:
        root = self.make_root()
        patchers = self.fixture_detectors()
        for patcher in patchers:
            patcher.start()
        self.addCleanup(patchers[1].stop)
        self.addCleanup(patchers[0].stop)
        status, output = self.run_main(root, ["--strict"])
        self.assertEqual(0, status, output)
        self.assertIn("Denominator contract valid.", output)

    def test_projection_rendering_is_deterministic(self) -> None:
        document = self.load_manifest_document(compliant_manifest_text())
        first = vrs.render_projection(document)
        second = vrs.render_projection(document)
        self.assertEqual(first, second)
        self.assertTrue(first.endswith("\n"))

    def test_moved_file_with_unchanged_component_needs_no_new_row(self) -> None:
        root = self.make_root()
        write_tree(root, {"src/authority/nested/moved_impl.rs": "moved"})
        patchers = self.fixture_detectors()
        for patcher in patchers:
            patcher.start()
        self.addCleanup(patchers[1].stop)
        self.addCleanup(patchers[0].stop)
        status, output = self.run_main(root, ["--strict"])
        self.assertEqual(0, status, output)
        self.assertNotIn("unclassified_sensitive_path", output)

    def test_detector_target_deleted_fails_closed(self) -> None:
        root = self.make_root()
        (root / "src/authority/catalog.rs").unlink()
        self.assert_strict_failure(root, "detector_target_missing")

    def test_non_strict_reports_issues_without_failing(self) -> None:
        root = self.make_root()
        (root / "src/authority/catalog.rs").unlink()
        patchers = self.fixture_detectors()
        for patcher in patchers:
            patcher.start()
        self.addCleanup(patchers[1].stop)
        self.addCleanup(patchers[0].stop)
        status, output = self.run_main(root, [])
        self.assertEqual(0, status, output)
        self.assertIn("Issues (", output)
        self.assertIn("detector_target_missing", output)

    # ------------------------------------------------------------------
    # Issue #11793 first falsifiers
    # ------------------------------------------------------------------

    def test_falsifier_1_new_close_proof_evaluator_outside_denominator(self) -> None:
        root = self.make_root()
        write_tree(root, {"src/orphan/new_evaluator.rs": "unclassified"})
        detectors = FIXTURE_DETECTOR_FILES + ("src/orphan/new_evaluator.rs",)
        patchers = self.fixture_detectors(detectors)
        for patcher in patchers:
            patcher.start()
        self.addCleanup(patchers[1].stop)
        self.addCleanup(patchers[0].stop)
        _status, output = self.run_main(root, ["--strict"])
        self.assertEqual(1, _status, output)
        self.assertIn("unclassified_sensitive_path", output)
        self.assertIn("src/orphan/new_evaluator.rs", output)

    def test_falsifier_2_accepted_store_matched_only_by_broad_crate_glob(self) -> None:
        manifest = compliant_manifest_text().replace(
            'paths = ["src/authority/**"]',
            'paths = ["crates/**"]',
            1,
        )
        root = self.make_root(manifest)
        output = self.assert_strict_failure(root, "broad_glob_binding")
        self.assertIn("crates/**", output)

    def test_falsifier_3_unvalidated_owner_claimed_valid(self) -> None:
        manifest = compliant_manifest_text().replace(
            'code_owner_route = { kind = "not_proven", resolution_owner = "#11796", note = "deferred" }\npaths = ["src/authority/**"]',
            'code_owner_route = { kind = "validated_pattern", identity = "EffortlessMetrics" }\npaths = ["src/authority/**"]',
            1,
        )
        root = self.make_root(manifest)
        (root / ".github/CODEOWNERS").write_text("* @EffortlessMetrics\n", encoding="utf-8")
        output = self.assert_strict_failure(root, "invalid_code_owner_claimed_valid")
        self.assertIn("unproven_code_owner", output)

    def test_falsifier_4_codeowners_or_labels_treated_as_review_evidence(self) -> None:
        manifest = compliant_manifest_text().replace(
            'required_evidence = "current_head_reviewer_packet"',
            'required_evidence = "codeowners_approval"',
            1,
        )
        root = self.make_root(manifest)
        output = self.assert_strict_failure(root, "invented_review_kind")
        self.assertIn("codeowners_approval", output)

    def test_falsifier_5_two_contradictory_review_authorities_on_one_path(self) -> None:
        extra_surface = """
[surface.rival_catalog]
family = "configuration_authority"
authority = "Rival catalog authority."
controller = "#9999"
conflict_key = "config.public_schema"
risk_class = "configuration_control"
review_profile = "public_api_or_retirement_authority"
required_evidence = "current_head_reviewer_packet"
first_falsifier = "Contradiction."
enforcement_successor = "#11796"
code_owner_route = { kind = "not_proven", resolution_owner = "#11796", note = "deferred" }
paths = ["src/authority/catalog.rs"]
"""
        root = self.make_root(compliant_manifest_text() + extra_surface)
        output = self.assert_strict_failure(root, "contradictory_path_ownership")
        self.assertIn("duplicate_path_binding", output)

    def test_falsifier_6_surface_without_known_review_profile(self) -> None:
        manifest = compliant_manifest_text().replace(
            'review_profile = "semantic_close_authority"',
            'review_profile = "vibes_based_authority"',
            1,
        )
        root = self.make_root(manifest)
        self.assert_strict_failure(root, "unknown_profile")

    def test_falsifier_7_manifest_omits_own_validator_paths(self) -> None:
        manifest = compliant_manifest_text().replace(
            '".github/CODEOWNERS"',
            '".github/unbound-file"',
            1,
        )
        root = self.make_root(manifest)
        output = self.assert_strict_failure(root, "self_surface_missing")
        self.assertIn(".github/CODEOWNERS", output)

    def test_falsifier_8_generated_inventory_edited_without_canonical_input(self) -> None:
        root = self.make_root()
        patchers = self.fixture_detectors()
        for patcher in patchers:
            patcher.start()
        self.addCleanup(patchers[1].stop)
        self.addCleanup(patchers[0].stop)
        status, _ = self.run_main(root, ["--write-projection"])
        self.assertEqual(0, status)
        projection = root / "docs/policy/REVIEW_SURFACES.md"
        projection.write_text("hand-edited lie\n", encoding="utf-8")
        status, output = self.run_main(root, ["--strict", "--check-projection"])
        self.assertEqual(1, status, output)
        self.assertIn("projection_stale", output)

    # ------------------------------------------------------------------
    # Structural hygiene
    # ------------------------------------------------------------------

    def test_missing_required_row_field_and_unknown_key_fail(self) -> None:
        manifest = compliant_manifest_text().replace('controller = "#10790"\n', "", 1)
        manifest += '\n[surface.sneaky]\nfamily = "configuration_authority"\nlabels = ["critical"]\n'
        root = self.make_root(manifest)
        output = self.assert_strict_failure(root, "missing_field")
        self.assertIn("unknown_field", output)
        self.assertIn("labels", output)

    def test_builder_self_review_alone_cannot_satisfy_a_profile(self) -> None:
        manifest = compliant_manifest_text().replace(
            'required_roles = ["adversarial_challenger"]\npacket_contract = "schemas/agent_review_packet.v1.schema.json"\nhandoff_authority = "#11701"',
            'required_roles = ["builder_self_review"]\npacket_contract = "schemas/agent_review_packet.v1.schema.json"\nhandoff_authority = "#11701"',
            1,
        )
        root = self.make_root(manifest)
        self.assert_strict_failure(root, "missing_independent_challenge")

    # ------------------------------------------------------------------
    # Review-thread regressions (#12272)
    # ------------------------------------------------------------------

    def test_validated_pattern_route_with_valid_identity_passes_strict(self) -> None:
        manifest = compliant_manifest_text().replace(
            'code_owner_route = { kind = "not_proven", resolution_owner = "#11796", note = "deferred" }\npaths = ["src/authority/**"]',
            'code_owner_route = { kind = "validated_pattern", identity = "EffortlessSteven" }\npaths = ["src/authority/**"]',
            1,
        )
        root = self.make_root(manifest)
        patchers = self.fixture_detectors()
        for patcher in patchers:
            patcher.start()
        self.addCleanup(patchers[1].stop)
        self.addCleanup(patchers[0].stop)
        status, output = self.run_main(root, ["--strict"])
        self.assertEqual(0, status, output)
        self.assertNotIn("unknown_field", output)

    def test_non_string_textual_field_fails_closed(self) -> None:
        manifest = compliant_manifest_text().replace(
            'controller = "#10790"',
            'controller = 10790',
            1,
        )
        root = self.make_root(manifest)
        output = self.assert_strict_failure(root, "wrong_type")
        self.assertIn("surface.authority_catalog.controller", output)

    def test_duplicate_binding_outside_detector_set_still_conflicts(self) -> None:
        extra_surface = """
[surface.peripheral_notes]
family = "configuration_authority"
authority = "Peripheral notes."
controller = "#9999"
conflict_key = "config.public_schema"
risk_class = "configuration_control"
review_profile = "public_api_or_retirement_authority"
required_evidence = "current_head_reviewer_packet"
first_falsifier = "Conflict outside the detector set."
enforcement_successor = "#11796"
code_owner_route = { kind = "not_proven", resolution_owner = "#11796", note = "deferred" }
paths = ["src/authority/extra.rs"]
"""
        root = self.make_root(compliant_manifest_text() + extra_surface)
        write_tree(root, {"src/authority/extra.rs": "extra"})
        output = self.assert_strict_failure(root, "duplicate_path_binding")
        self.assertIn("contradictory_path_ownership", output)
        self.assertNotIn("unclassified_sensitive_path", output)


if __name__ == "__main__":
    unittest.main()
