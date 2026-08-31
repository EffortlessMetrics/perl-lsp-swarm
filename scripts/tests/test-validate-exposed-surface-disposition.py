#!/usr/bin/env python3
"""Focused tests for the release exposed-surface disposition validator."""

from __future__ import annotations

import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).parents[2]
SPEC = importlib.util.spec_from_file_location(
    "validate_exposed_surface_disposition",
    ROOT / "scripts/validate-exposed-surface-disposition.py",
)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ExposedSurfaceDispositionValidationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.schema = json.loads(
            (ROOT / "docs/releases/exposed-surface-disposition.v1.schema.json").read_text(encoding="utf-8")
        )
        cls.projection = json.loads(
            (ROOT / "scripts/tests/fixtures/exposed-surface-disposition/valid.json").read_text(encoding="utf-8")
        )

    def assert_invalid(self, projection: dict, message: str) -> None:
        with self.assertRaisesRegex(ValueError, message):
            MODULE.validate_projection(self.schema, projection)

    def test_valid_ready_projection_passes_and_cli_requires_canonical_bytes(self) -> None:
        MODULE.validate_projection(self.schema, self.projection)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            schema_path = root / "schema.json"
            projection_path = root / "projection.json"
            schema_path.write_text(json.dumps(self.schema), encoding="utf-8")
            projection_path.write_text(MODULE.canonical_bytes(self.projection), encoding="utf-8")
            self.assertEqual(MODULE.main(["--schema", str(schema_path), "--projection", str(projection_path)]), 0)

    def test_rejects_unknown_disposition(self) -> None:
        projection = copy.deepcopy(self.projection)
        projection["rows"][0]["disposition"] = "SOMEDAY"
        self.assert_invalid(projection, "disposition is invalid")

    def test_rejects_duplicate_canonical_authority_row(self) -> None:
        projection = copy.deepcopy(self.projection)
        projection["rows"].append(copy.deepcopy(projection["rows"][0]))
        self.assert_invalid(projection, "duplicate canonical authority row")

    def test_rejects_missing_or_stale_canonical_authority_identity(self) -> None:
        projection = copy.deepcopy(self.projection)
        projection["rows"][0]["surface_ref"].pop("digest")
        self.assert_invalid(projection, "surface_ref.*missing keys")

        projection = copy.deepcopy(self.projection)
        projection["rows"][0]["surface_ref"]["digest"] = "stale"
        self.assert_invalid(projection, "surface_ref.digest")

    def test_rejects_cross_subject_or_cross_profile_evidence(self) -> None:
        projection = copy.deepcopy(self.projection)
        projection["rows"][0]["evidence_subjects"][0]["repository_sha"] = "d" * 40
        self.assert_invalid(projection, "cross-subject evidence")

        projection = copy.deepcopy(self.projection)
        projection["rows"][0]["evidence_subjects"][0]["artifact_profile"] = "perllsp_stdio"
        self.assert_invalid(projection, "not declared by the row")

    def test_rejects_ready_without_installed_evidence(self) -> None:
        projection = copy.deepcopy(self.projection)
        projection["rows"][0]["evidence_subjects"] = []
        self.assert_invalid(projection, "READY requires exact installed evidence")

    def test_rejects_disabled_while_default_reachable_or_without_absence_proof(self) -> None:
        projection = copy.deepcopy(self.projection)
        row = projection["rows"][0]
        row["disposition"] = "DISABLED"
        row["claim_effect"] = "remove_or_withhold"
        self.assert_invalid(projection, "cannot remain default reachable")

        projection = copy.deepcopy(self.projection)
        row = projection["rows"][0]
        row["disposition"] = "DISABLED"
        row["claim_effect"] = "remove_or_withhold"
        row["default_reachable"] = False
        self.assert_invalid(projection, "requires artifact-absence evidence")

    def test_rejects_bounded_preview_without_refusal_boundary(self) -> None:
        projection = copy.deepcopy(self.projection)
        row = projection["rows"][0]
        row["disposition"] = "BOUNDED_PREVIEW"
        row["claim_effect"] = "limit"
        self.assert_invalid(projection, "requires refusal-boundary evidence")

    def test_rejects_blocked_or_not_proven_without_owner_or_with_retained_claim(self) -> None:
        projection = copy.deepcopy(self.projection)
        row = projection["rows"][0]
        row["disposition"] = "BLOCKED"
        row["claim_effect"] = "remove_or_withhold"
        self.assert_invalid(projection, "BLOCKED requires an owning issue")

        projection = copy.deepcopy(self.projection)
        row = projection["rows"][0]
        row["disposition"] = "NOT_PROVEN"
        row["owner_issue"] = "https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14412"
        self.assert_invalid(projection, "NOT_PROVEN must remove or withhold")

    def test_rejects_noncanonical_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            projection_path = Path(directory) / "projection.json"
            projection_path.write_text(json.dumps(self.projection), encoding="utf-8")
            self.assertEqual(MODULE.main(["--projection", str(projection_path)]), 1)


if __name__ == "__main__":
    unittest.main()
