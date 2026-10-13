#!/usr/bin/env python3
"""Tests for scripts/ci/ci_classify.py.

Covers the pure classify_one() function with offline fixtures.
No live GitHub API calls.  No external dependencies beyond stdlib.

Run with:
    python3 scripts/ci/test_ci_classify.py
    python3 -m unittest scripts.ci.test_ci_classify  (from repo root)

Exit code 0 on all-pass, non-zero on any failure.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

# ---------------------------------------------------------------------------
# Path setup: add scripts/ci to sys.path so we can import ci_classify directly.
# ---------------------------------------------------------------------------
_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

from ci_classify import (  # noqa: E402
    CLASS_COVERAGE_ARTIFACT,
    CLASS_EXPECTED_PATH_SKIP,
    CLASS_INFRA_ISSUE,
    CLASS_POLICY_MISMATCH,
    CLASS_PRODUCT_DEFECT,
    CLASS_REVIEW_GATE,
    CLASS_UNKNOWN,
    ROUTING,
    SCHEMA_VERSION,
    classification_record,
    classify_one,
    filter_failing,
    json_envelope,
    load_check_runs,
)
import ci_classify  # noqa: E402

SCRIPT = _HERE / "ci_classify.py"

# Fixtures directory (sibling to this test file).
FIXTURES_DIR = _HERE / "fixtures"


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _check(
    name: str,
    conclusion: str = "failure",
    *,
    required: bool = True,
    quarantine: bool = False,
    run_ci: bool = True,
    is_latest: bool = True,
) -> dict:
    return {
        "name": name,
        "conclusion": conclusion,
        "required": required,
        "quarantine": quarantine,
        "run_ci": run_ci,
        "is_latest": is_latest,
    }


def _cls(name: str, **kwargs: object) -> str:
    cls, _ = classify_one(_check(name, **kwargs))
    return cls


def _cli(
    args: list[str], *, input_text: str | None = None
) -> subprocess.CompletedProcess[str]:
    """Run the production ``ci_classify.py`` CLI in a child process."""
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        input=input_text,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )


def _cli_file(checks: object, *args: str) -> subprocess.CompletedProcess[str]:
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False, encoding="utf-8"
    ) as handle:
        json.dump(checks, handle)
        path = handle.name
    try:
        return _cli([*args, path])
    finally:
        os.unlink(path)


def _cli_json_file(checks: object) -> subprocess.CompletedProcess[str]:
    return _cli_file(checks, "--json")


# ---------------------------------------------------------------------------
# Core classification unit tests (4 required by spec)
# ---------------------------------------------------------------------------


class TestProductDefect(unittest.TestCase):
    """Spec requirement: product_defect — gate fails, quarantine=false, core gate."""

    def test_lsp_shard_failure(self) -> None:
        cls, rationale = classify_one(
            _check("CI Gate shard (lsp)", "failure", quarantine=False, required=True)
        )
        self.assertEqual(cls, CLASS_PRODUCT_DEFECT)
        self.assertIn("product gate", rationale)

    def test_merge_blocking_failure(self) -> None:
        cls, _ = classify_one(
            _check("CI Gate (Merge-Blocking)", "failure", quarantine=False)
        )
        self.assertEqual(cls, CLASS_PRODUCT_DEFECT)

    def test_ux_regression_tests(self) -> None:
        cls, _ = classify_one(
            _check("UX Regression Tests", "failure", quarantine=False)
        )
        self.assertEqual(cls, CLASS_PRODUCT_DEFECT)

    def test_lsp_memory_smoke(self) -> None:
        cls, _ = classify_one(
            _check("LSP Memory Smoke", "failure", quarantine=False)
        )
        self.assertEqual(cls, CLASS_PRODUCT_DEFECT)

    def test_pr_smoke(self) -> None:
        cls, _ = classify_one(_check("pr-smoke", "failure", quarantine=False))
        self.assertEqual(cls, CLASS_PRODUCT_DEFECT)

    def test_corpus_gate_failure(self) -> None:
        cls, _ = classify_one(
            _check("CI Gate shard (corpus)", "failure", quarantine=False)
        )
        self.assertEqual(cls, CLASS_PRODUCT_DEFECT)


class TestInfraIssue(unittest.TestCase):
    """Spec requirement: infra_issue — conclusion=cancelled or timed_out."""

    def test_cancelled_gate(self) -> None:
        cls, rationale = classify_one(
            _check("CI Gate shard (corpus)", "cancelled", quarantine=False)
        )
        self.assertEqual(cls, CLASS_INFRA_ISSUE)
        self.assertIn("cancelled", rationale)

    def test_timed_out_compile(self) -> None:
        cls, rationale = classify_one(
            _check("Compile All Targets", "timed_out", quarantine=False)
        )
        self.assertEqual(cls, CLASS_INFRA_ISSUE)
        self.assertIn("timed_out", rationale)

    def test_cancelled_takes_priority_over_product(self) -> None:
        """Infra cancellation should win over product_defect classification."""
        cls, _ = classify_one(
            _check("CI Gate shard (lsp)", "cancelled", quarantine=False, required=True)
        )
        self.assertEqual(cls, CLASS_INFRA_ISSUE)

    def test_quarantine_beats_timed_out_for_coverage_gate(self) -> None:
        """Quarantined coverage gate that times out → coverage_artifact, not infra_issue.

        A quarantined gate cannot meaningfully be 'retried' — the quarantine is the
        authoritative policy signal.  Routing as infra_issue would incorrectly suggest
        a retry is safe.
        """
        cls, _ = classify_one(
            _check("security_audit", "timed_out", quarantine=True)
        )
        self.assertEqual(cls, CLASS_COVERAGE_ARTIFACT)

    def test_quarantine_beats_cancelled_for_non_coverage_gate(self) -> None:
        """Quarantined non-coverage gate that is cancelled → expected_path_skip, not infra_issue."""
        cls, _ = classify_one(
            _check("some-quarantined-gate", "cancelled", quarantine=True)
        )
        self.assertEqual(cls, CLASS_EXPECTED_PATH_SKIP)


class TestPolicyMismatch(unittest.TestCase):
    """Spec requirement: policy_mismatch — mechanical-correctness gate, quarantine=false."""

    def test_fmt_gate(self) -> None:
        cls, rationale = classify_one(
            _check("fmt", "failure", quarantine=False)
        )
        self.assertEqual(cls, CLASS_POLICY_MISMATCH)
        self.assertIn("mechanical-correctness", rationale)

    def test_conflict_markers(self) -> None:
        cls, _ = classify_one(
            _check("check_conflict_markers", "failure", quarantine=False)
        )
        self.assertEqual(cls, CLASS_POLICY_MISMATCH)

    def test_conflict_markers_alt_name(self) -> None:
        """conflict-markers (GitHub job name variant) should also match."""
        cls, _ = classify_one(
            _check("conflict-markers", "failure", quarantine=False)
        )
        self.assertEqual(cls, CLASS_POLICY_MISMATCH)

    def test_publish_manifest_check(self) -> None:
        cls, _ = classify_one(
            _check("publish_manifest_check", "failure", quarantine=False)
        )
        self.assertEqual(cls, CLASS_POLICY_MISMATCH)

    def test_layer_check(self) -> None:
        cls, _ = classify_one(
            _check("layer_check", "failure", quarantine=False)
        )
        self.assertEqual(cls, CLASS_POLICY_MISMATCH)


class TestExpectedPathSkip(unittest.TestCase):
    """Spec requirement: expected_path_skip — quarantine=true or required=false for non-coverage gates."""

    def test_windows_required_false(self) -> None:
        """Windows Required with required=false is policy-sanctioned path skip."""
        cls, rationale = classify_one(
            _check("Windows Required", "failure", required=False, quarantine=False)
        )
        self.assertEqual(cls, CLASS_EXPECTED_PATH_SKIP)
        self.assertIn("required=false", rationale)

    def test_quarantined_non_coverage_gate(self) -> None:
        """A quarantined gate that is not a measurement/quality gate → expected_path_skip."""
        cls, rationale = classify_one(
            _check("some-experimental-check", "failure", quarantine=True)
        )
        self.assertEqual(cls, CLASS_EXPECTED_PATH_SKIP)
        self.assertIn("quarantine=true", rationale)

    def test_quarantined_product_gate_is_expected_path_skip(self) -> None:
        """A quarantined product gate must never reach product_defect — expected_path_skip wins."""
        cls, _ = classify_one(
            _check("CI Gate shard (lsp)", "failure", quarantine=True)
        )
        self.assertEqual(cls, CLASS_EXPECTED_PATH_SKIP)


class TestReviewGate(unittest.TestCase):
    """Spec requirement: review_gate — draft or superseded-SHA skip."""

    def test_draft_pr_check_run_ci_false(self) -> None:
        cls, rationale = classify_one(
            _check("draft-pr-check", "failure", run_ci=False)
        )
        self.assertEqual(cls, CLASS_REVIEW_GATE)
        self.assertIn("draft", rationale)

    def test_preflight_latest_not_latest(self) -> None:
        cls, rationale = classify_one(
            _check("preflight-latest-check", "failure", is_latest=False)
        )
        self.assertEqual(cls, CLASS_REVIEW_GATE)
        self.assertIn("superseded", rationale)

    def test_draft_pr_check_run_ci_true_falls_through(self) -> None:
        """draft-pr-check with run_ci=True should NOT classify as review_gate."""
        # If run_ci=true there's a real failure — falls through to product/unknown.
        cls, _ = classify_one(
            _check("draft-pr-check", "failure", run_ci=True, quarantine=False)
        )
        self.assertNotEqual(cls, CLASS_REVIEW_GATE)

    def test_preflight_latest_is_latest_falls_through(self) -> None:
        """preflight-latest-check with is_latest=True should NOT classify as review_gate."""
        cls, _ = classify_one(
            _check("preflight-latest-check", "failure", is_latest=True, quarantine=False)
        )
        self.assertNotEqual(cls, CLASS_REVIEW_GATE)


class TestCoverageArtifact(unittest.TestCase):
    """coverage_artifact — measurement/quality gate whose threshold or tooling drifted."""

    def test_coverage_skipped(self) -> None:
        cls, _ = classify_one(
            {"name": "coverage-baseline-check", "conclusion": "skipped"}
        )
        self.assertEqual(cls, CLASS_COVERAGE_ARTIFACT)

    def test_coverage_keyword_skipped(self) -> None:
        cls, _ = classify_one(
            {"name": "coverage-drift", "conclusion": "neutral"}
        )
        self.assertEqual(cls, CLASS_COVERAGE_ARTIFACT)

    def test_spec_acceptance_3_security_audit_quarantined(self) -> None:
        """Spec acceptance test 3: security_audit quarantine=true → coverage_artifact.

        Rationale per issue #907: security_audit is quarantined because cargo-audit
        breaks on CVSS 4.0 — environmental tooling issue, not a product regression.
        Routing: log & ignore; do not block merge.
        """
        cls, rationale = classify_one(
            _check("security_audit", "failure", quarantine=True)
        )
        self.assertEqual(cls, CLASS_COVERAGE_ARTIFACT)
        self.assertIn("measurement/quality gate", rationale)

    def test_spec_acceptance_8_parser_corpus_ratchet_required_false(self) -> None:
        """Spec acceptance test 8: parser_corpus_ratchet required=false → coverage_artifact.

        Rationale per issue #907: corpus ratchet with required=false indicates baseline
        drift, not a product regression.  Routing: log & ignore; do not block merge.
        """
        cls, rationale = classify_one(
            _check("parser_corpus_ratchet", "failure", required=False)
        )
        self.assertEqual(cls, CLASS_COVERAGE_ARTIFACT)
        self.assertIn("measurement/quality gate", rationale)

    def test_mutation_subset_required_false_is_coverage_artifact(self) -> None:
        """mutation-subset with required=false → coverage_artifact (measurement gate)."""
        cls, _ = classify_one(
            _check("mutation-subset", "failure", quarantine=False, required=False)
        )
        self.assertEqual(cls, CLASS_COVERAGE_ARTIFACT)

    def test_quarantined_mutation_is_coverage_artifact(self) -> None:
        """mutation quarantined → coverage_artifact, not expected_path_skip."""
        cls, _ = classify_one(
            _check("mutation", "failure", quarantine=True)
        )
        self.assertEqual(cls, CLASS_COVERAGE_ARTIFACT)

    def test_quarantined_fuzz_is_coverage_artifact(self) -> None:
        """fuzz quarantined → coverage_artifact, not expected_path_skip."""
        cls, _ = classify_one(
            _check("fuzz", "failure", quarantine=True)
        )
        self.assertEqual(cls, CLASS_COVERAGE_ARTIFACT)


class TestUnknown(unittest.TestCase):
    """unknown — no pattern matches."""

    def test_unknown_check(self) -> None:
        cls, rationale = classify_one(
            _check("some-new-experimental-gate", "failure", quarantine=False, required=True)
        )
        self.assertEqual(cls, CLASS_UNKNOWN)
        self.assertIn("no classification pattern matched", rationale)


class TestRoutingRecommendations(unittest.TestCase):
    """Routing is descriptive output, not a lifecycle-label command."""

    def test_actionable_routes_do_not_emit_lifecycle_labels(self) -> None:
        for classification in (CLASS_PRODUCT_DEFECT, CLASS_POLICY_MISMATCH):
            route = ROUTING[classification]
            self.assertNotIn("needs-builder-fix", route)
            self.assertNotIn("needs-ci-fix", route)
            self.assertNotIn("merge-ready", route)

    def test_empty_name(self) -> None:
        cls, _ = classify_one(
            {"name": "", "conclusion": "failure"}
        )
        self.assertEqual(cls, CLASS_UNKNOWN)


# ---------------------------------------------------------------------------
# Missing-field robustness tests
# ---------------------------------------------------------------------------


class TestMissingFieldGraceful(unittest.TestCase):
    """classify_one must not raise when optional fields are absent."""

    def test_minimal_input(self) -> None:
        """Only name + conclusion present — all defaults apply."""
        cls, rationale = classify_one({"name": "fmt", "conclusion": "failure"})
        # fmt with defaults (quarantine=false, required=true) → policy_mismatch
        self.assertEqual(cls, CLASS_POLICY_MISMATCH)
        self.assertIsInstance(rationale, str)
        self.assertTrue(rationale)

    def test_no_conclusion(self) -> None:
        """Missing conclusion — treat as empty string, no infra match."""
        cls, _ = classify_one({"name": "some-check"})
        # No conclusion, no quarantine, no pattern → unknown
        self.assertEqual(cls, CLASS_UNKNOWN)

    def test_no_name(self) -> None:
        """Missing name — should still return a class without raising."""
        cls, rationale = classify_one({"conclusion": "cancelled"})
        # conclusion=cancelled → infra_issue
        self.assertEqual(cls, CLASS_INFRA_ISSUE)

    def test_extra_unknown_fields_ignored(self) -> None:
        """Extra fields in the input must not cause errors."""
        cls, _ = classify_one(
            {
                "name": "fmt",
                "conclusion": "failure",
                "extra_field": "ignored",
                "another": 42,
            }
        )
        self.assertEqual(cls, CLASS_POLICY_MISMATCH)


# ---------------------------------------------------------------------------
# filter_failing tests
# ---------------------------------------------------------------------------


class TestFilterFailing(unittest.TestCase):
    def test_filters_success(self) -> None:
        checks = [
            {"name": "a", "conclusion": "success"},
            {"name": "b", "conclusion": "failure"},
        ]
        result = filter_failing(checks)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0]["name"], "b")

    def test_cancelled_included(self) -> None:
        checks = [{"name": "a", "conclusion": "cancelled"}]
        self.assertEqual(len(filter_failing(checks)), 1)

    def test_timed_out_included(self) -> None:
        checks = [{"name": "a", "conclusion": "timed_out"}]
        self.assertEqual(len(filter_failing(checks)), 1)

    def test_skipped_excluded(self) -> None:
        checks = [{"name": "a", "conclusion": "skipped"}]
        self.assertEqual(len(filter_failing(checks)), 0)

    def test_empty_list(self) -> None:
        self.assertEqual(filter_failing([]), [])


# ---------------------------------------------------------------------------
# Fixture-based integration tests
# ---------------------------------------------------------------------------


class TestFixtures(unittest.TestCase):
    """Load realistic fixture JSON files and verify expected classifications."""

    def _load_fixture(self, filename: str) -> list[dict]:
        path = FIXTURES_DIR / filename
        return json.loads(path.read_text(encoding="utf-8"))

    def test_product_defect_fixture(self) -> None:
        checks = self._load_fixture("product_defect.json")
        failing = filter_failing(checks)
        self.assertGreater(len(failing), 0)
        for check in failing:
            cls, _ = classify_one(check)
            self.assertEqual(
                cls,
                CLASS_PRODUCT_DEFECT,
                f"Expected product_defect for {check.get('name')!r}, got {cls!r}",
            )

    def test_infra_issue_fixture(self) -> None:
        checks = self._load_fixture("infra_issue.json")
        failing = filter_failing(checks)
        self.assertGreater(len(failing), 0)
        for check in failing:
            cls, _ = classify_one(check)
            self.assertEqual(
                cls,
                CLASS_INFRA_ISSUE,
                f"Expected infra_issue for {check.get('name')!r}, got {cls!r}",
            )

    def test_policy_mismatch_fixture(self) -> None:
        checks = self._load_fixture("policy_mismatch.json")
        failing = filter_failing(checks)
        self.assertGreater(len(failing), 0)
        for check in failing:
            cls, _ = classify_one(check)
            self.assertEqual(
                cls,
                CLASS_POLICY_MISMATCH,
                f"Expected policy_mismatch for {check.get('name')!r}, got {cls!r}",
            )

    def test_expected_path_skip_fixture(self) -> None:
        checks = self._load_fixture("expected_path_skip.json")
        failing = filter_failing(checks)
        self.assertGreater(len(failing), 0)
        for check in failing:
            cls, _ = classify_one(check)
            self.assertEqual(
                cls,
                CLASS_EXPECTED_PATH_SKIP,
                f"Expected expected_path_skip for {check.get('name')!r}, got {cls!r}",
            )

    def test_review_gate_fixture(self) -> None:
        checks = self._load_fixture("review_gate.json")
        # review_gate checks have conclusion=failure so they appear in filter_failing
        failing = filter_failing(checks)
        self.assertGreater(len(failing), 0)
        for check in failing:
            cls, _ = classify_one(check)
            self.assertEqual(
                cls,
                CLASS_REVIEW_GATE,
                f"Expected review_gate for {check.get('name')!r}, got {cls!r}",
            )

    def test_coverage_artifact_fixture(self) -> None:
        """Spec acceptance tests 3 and 8: coverage/quality gates are coverage_artifact."""
        checks = self._load_fixture("coverage_artifact.json")
        failing = filter_failing(checks)
        self.assertGreater(len(failing), 0)
        for check in failing:
            cls, _ = classify_one(check)
            self.assertEqual(
                cls,
                CLASS_COVERAGE_ARTIFACT,
                f"Expected coverage_artifact for {check.get('name')!r}, got {cls!r}",
            )

    def test_mixed_fixture_has_expected_classes(self) -> None:
        checks = self._load_fixture("mixed.json")
        failing = filter_failing(checks)
        classes = {check.get("name"): classify_one(check)[0] for check in failing}

        self.assertEqual(classes.get("fmt"), CLASS_POLICY_MISMATCH)
        self.assertEqual(classes.get("CI Gate shard (lsp)"), CLASS_PRODUCT_DEFECT)
        # cancelled → infra
        self.assertEqual(classes.get("CI Gate shard (corpus)"), CLASS_INFRA_ISSUE)
        # security_audit quarantine=true → coverage_artifact (measurement gate, not path skip)
        self.assertEqual(classes.get("security_audit"), CLASS_COVERAGE_ARTIFACT)
        # draft → review_gate
        self.assertEqual(classes.get("draft-pr-check"), CLASS_REVIEW_GATE)
        # unknown
        self.assertEqual(classes.get("some-new-experimental-check"), CLASS_UNKNOWN)


# ---------------------------------------------------------------------------
# load_check_runs tests
# ---------------------------------------------------------------------------


class TestLoadCheckRuns(unittest.TestCase):
    def test_load_from_file(self) -> None:
        data = [{"name": "fmt", "conclusion": "failure"}]
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".json", delete=False, encoding="utf-8"
        ) as f:
            json.dump(data, f)
            fname = f.name
        try:
            result = load_check_runs(fname)
            self.assertEqual(len(result), 1)
            self.assertEqual(result[0]["name"], "fmt")
        finally:
            os.unlink(fname)

    def test_load_github_envelope(self) -> None:
        """GitHub API envelopes with check_runs key are unwrapped."""
        data = {
            "check_runs": [
                {"name": "a", "conclusion": "failure"},
                {"name": "b", "conclusion": "success"},
            ]
        }
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".json", delete=False, encoding="utf-8"
        ) as f:
            json.dump(data, f)
            fname = f.name
        try:
            result = load_check_runs(fname)
            self.assertEqual(len(result), 2)
        finally:
            os.unlink(fname)


# ---------------------------------------------------------------------------
# --json envelope tests (issue #15285)
# ---------------------------------------------------------------------------


class TestJsonEnvelope(unittest.TestCase):
    """``json_envelope`` owns the versioned ``--json`` object.

    Schema: ``{"schema_version": <str>, "classifications": [<record>, ...]}``.

    Without an envelope a consumer that asserts ``data[0]["routing"] in {...}``
    cannot detect a routing-taxonomy bump; an envelope lets the consumer
    check ``schema_version`` first and refuse unknown shapes.
    """

    def test_envelope_keys_are_exactly_schema_version_and_classifications(self) -> None:
        envelope = json_envelope([])
        self.assertEqual(
            set(envelope.keys()), {"schema_version", "classifications"}
        )

    def test_schema_version_is_ci_classify_v1_string(self) -> None:
        envelope = json_envelope([])
        self.assertIsInstance(envelope["schema_version"], str)
        self.assertEqual(envelope["schema_version"], "ci_classify.v1")
        self.assertEqual(SCHEMA_VERSION, "ci_classify.v1")

    def test_empty_results_still_wrap_classifications_list(self) -> None:
        envelope = json_envelope([])
        self.assertIsInstance(envelope["classifications"], list)
        self.assertEqual(envelope["classifications"], [])

    def test_top_level_is_not_a_bare_list(self) -> None:
        """Regression: previous stdout JSON was a bare list of records."""
        envelope = json_envelope(
            [(_check("fmt"), CLASS_POLICY_MISMATCH, "mechanical")]
        )
        self.assertIsInstance(envelope, dict)
        self.assertNotIsInstance(envelope, list)
        self.assertNotIsInstance(envelope.get("classifications"), dict)

    def test_record_shape_and_policy_routing(self) -> None:
        check = _check("fmt")
        cls, rationale = classify_one(check)
        record = classification_record(check, cls, rationale)
        self.assertEqual(
            set(record.keys()),
            {"name", "conclusion", "class", "rationale", "routing"},
        )
        self.assertEqual(record["name"], "fmt")
        self.assertEqual(record["conclusion"], "failure")
        self.assertEqual(record["class"], CLASS_POLICY_MISMATCH)
        self.assertEqual(record["routing"], ROUTING[CLASS_POLICY_MISMATCH])
        self.assertEqual(record["rationale"], rationale)
        self.assertTrue(record["rationale"])

    def test_missing_name_and_conclusion_become_empty_strings(self) -> None:
        record = classification_record({}, CLASS_UNKNOWN, "no pattern")
        self.assertEqual(record["name"], "")
        self.assertEqual(record["conclusion"], "")
        self.assertIsInstance(record["name"], str)
        self.assertIsInstance(record["conclusion"], str)

    def test_null_name_and_conclusion_become_empty_strings(self) -> None:
        check = {"name": None, "conclusion": None}
        cls, rationale = classify_one(check)
        record = classification_record(check, cls, rationale)
        self.assertEqual(record["name"], "")
        self.assertEqual(record["conclusion"], "")
        self.assertNotIn("None", rationale)
        self.assertIn("''", rationale)
        envelope = json_envelope([(check, cls, rationale)])
        dumped = json.loads(json.dumps(envelope))
        self.assertEqual(dumped["classifications"][0]["name"], "")
        self.assertEqual(dumped["classifications"][0]["rationale"], rationale)
        self.assertNotIn(None, dumped["classifications"][0].values())

    def test_input_extra_keys_do_not_leak_into_records(self) -> None:
        record = classification_record(
            {
                "name": "fmt",
                "conclusion": "failure",
                "html_url": "https://example.invalid/run",
                "required": True,
            },
            CLASS_POLICY_MISMATCH,
            "mechanical",
        )
        self.assertEqual(
            set(record.keys()),
            {"name", "conclusion", "class", "rationale", "routing"},
        )
        self.assertNotIn("html_url", record)
        self.assertNotIn("required", record)

    def test_unknown_class_still_emits_known_routing(self) -> None:
        record = classification_record(
            {"name": "mystery-lane", "conclusion": "failure"},
            CLASS_UNKNOWN,
            "no classification pattern matched",
        )
        self.assertEqual(record["class"], CLASS_UNKNOWN)
        self.assertEqual(record["routing"], ROUTING[CLASS_UNKNOWN])

    def test_unmapped_class_routing_is_empty_string_not_missing(self) -> None:
        record = classification_record(
            {"name": "fmt", "conclusion": "failure"},
            "not_a_real_class",
            "bogus",
        )
        self.assertEqual(record["routing"], "")
        self.assertIn("routing", record)

    def test_run_json_delegates_to_json_envelope(self) -> None:
        """A duplicate inline serializer in ``run()`` must not pass.

        Equality against ``json_envelope()`` cannot catch a private copy that
        happens to match. A sentinel return from the helper can.
        """
        import argparse
        import contextlib
        import io

        sentinel = {
            "schema_version": "sentinel.v1",
            "classifications": [{"name": "sentinel"}],
        }
        checks = [{"name": "fmt", "conclusion": "failure"}]
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".json", delete=False, encoding="utf-8"
        ) as handle:
            json.dump(checks, handle)
            path = handle.name
        try:
            buf = io.StringIO()
            with mock.patch.object(
                ci_classify, "json_envelope", return_value=sentinel
            ) as patched:
                with contextlib.redirect_stdout(buf):
                    rc = ci_classify.run(
                        argparse.Namespace(input=path, pr=None, json=True)
                    )
            patched.assert_called_once()
            self.assertEqual(rc, 0)
            self.assertEqual(json.loads(buf.getvalue()), sentinel)
        finally:
            os.unlink(path)

    def test_json_envelope_delegates_to_classification_record(self) -> None:
        """Bypassing ``classification_record`` inside ``json_envelope`` must fail."""
        sentinel = {"name": "sentinel-record"}
        with mock.patch.object(
            ci_classify, "classification_record", return_value=sentinel
        ) as patched:
            envelope = ci_classify.json_envelope(
                [({"name": "fmt", "conclusion": "failure"}, "cls", "why")]
            )
        patched.assert_called_once()
        self.assertEqual(envelope["schema_version"], "ci_classify.v1")
        self.assertEqual(envelope["classifications"], [sentinel])


class TestJsonEnvelopeCli(unittest.TestCase):
    """Production ``python ci_classify.py --json`` stdout is the envelope.

    ``run()`` tests cannot catch an argparse wiring miss that drops ``--json``.
    """

    def test_cli_json_file_stdout_is_parseable_envelope(self) -> None:
        completed = _cli_json_file([{"name": "fmt", "conclusion": "failure"}])
        self.assertEqual(completed.returncode, 0, completed.stderr)
        envelope = json.loads(completed.stdout)
        self.assertEqual(envelope["schema_version"], "ci_classify.v1")
        self.assertEqual(len(envelope["classifications"]), 1)
        self.assertEqual(envelope["classifications"][0]["name"], "fmt")
        self.assertEqual(
            envelope["classifications"][0]["class"], CLASS_POLICY_MISMATCH
        )

    def test_cli_json_stdin_stdout_is_parseable_envelope(self) -> None:
        completed = _cli(
            ["--json"],
            input_text=json.dumps([{"name": "fmt", "conclusion": "failure"}]),
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        envelope = json.loads(completed.stdout)
        self.assertEqual(
            set(envelope.keys()), {"schema_version", "classifications"}
        )
        self.assertEqual(envelope["classifications"][0]["name"], "fmt")

    def test_cli_json_empty_array_still_emits_envelope(self) -> None:
        completed = _cli_json_file([])
        self.assertEqual(completed.returncode, 0, completed.stderr)
        envelope = json.loads(completed.stdout)
        self.assertEqual(envelope["schema_version"], "ci_classify.v1")
        self.assertEqual(envelope["classifications"], [])

    def test_cli_json_omits_non_failing_checks(self) -> None:
        completed = _cli_json_file(
            [
                {"name": "fmt", "conclusion": "success"},
                {"name": "fmt", "conclusion": "failure"},
                {
                    "name": "CI Gate shard (lsp)",
                    "conclusion": "cancelled",
                    "required": True,
                },
            ]
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        envelope = json.loads(completed.stdout)
        names = [row["name"] for row in envelope["classifications"]]
        self.assertEqual(names, ["fmt", "CI Gate shard (lsp)"])
        self.assertEqual(
            envelope["classifications"][1]["class"], CLASS_INFRA_ISSUE
        )

    def test_cli_json_stdout_is_only_the_envelope(self) -> None:
        completed = _cli_json_file([{"name": "fmt", "conclusion": "failure"}])
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertTrue(
            completed.stdout.lstrip().startswith("{"),
            f"expected object stdout, got {completed.stdout!r}",
        )
        envelope = json.loads(completed.stdout)
        self.assertIsInstance(envelope, dict)
        self.assertNotIsInstance(envelope, list)
        self.assertEqual(completed.stderr, "")

    def test_cli_json_preserves_unicode_name(self) -> None:
        completed = _cli_json_file(
            [{"name": "café — CI Gate shard", "conclusion": "failure"}]
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        envelope = json.loads(completed.stdout)
        self.assertEqual(
            envelope["classifications"][0]["name"], "café — CI Gate shard"
        )

    def test_cli_json_includes_action_required(self) -> None:
        completed = _cli_json_file(
            [{"name": "mystery-lane", "conclusion": "action_required"}]
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        envelope = json.loads(completed.stdout)
        self.assertEqual(len(envelope["classifications"]), 1)
        self.assertEqual(
            envelope["classifications"][0]["conclusion"], "action_required"
        )
        self.assertEqual(
            envelope["classifications"][0]["class"], CLASS_UNKNOWN
        )

    def test_cli_json_null_name_is_empty_string(self) -> None:
        completed = _cli_json_file([{"name": None, "conclusion": "failure"}])
        self.assertEqual(completed.returncode, 0, completed.stderr)
        envelope = json.loads(completed.stdout)
        record = envelope["classifications"][0]
        self.assertEqual(record["name"], "")
        self.assertIsInstance(record["name"], str)
        self.assertNotIn("None", record["rationale"])
        self.assertIn("''", record["rationale"])

    def test_cli_json_all_passing_still_emits_envelope(self) -> None:
        completed = _cli_json_file([{"name": "fmt", "conclusion": "success"}])
        self.assertEqual(completed.returncode, 0, completed.stderr)
        envelope = json.loads(completed.stdout)
        self.assertEqual(envelope["schema_version"], "ci_classify.v1")
        self.assertEqual(envelope["classifications"], [])

    def test_cli_without_json_is_not_an_envelope(self) -> None:
        """Opposite-direction control: prose mode must not look like JSON."""
        completed = _cli_file([{"name": "fmt", "conclusion": "failure"}])
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("CLASS", completed.stdout)
        with self.assertRaises(json.JSONDecodeError):
            json.loads(completed.stdout)

    def test_cli_prose_null_name_matches_empty_identity(self) -> None:
        completed = _cli_file([{"name": None, "conclusion": "failure"}])
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertNotIn("'None'", completed.stdout)


class TestPrEmptyFetchJsonEnvelope(unittest.TestCase):
    """``--pr N --json`` with an empty fetch must still emit the envelope.

    An empty ``gh pr checks`` result (no check-runs, gh unavailable, or a
    fresh PR) used to short-circuit with a prose line before any JSON, so a
    consumer piping ``--json`` output into ``json.load`` crashed on a
    versioned-contract violation. The prose summary is prose-mode only.
    """

    def _captured(self, *, json_mode: bool) -> tuple[int, str]:
        """Run ``run()`` against a stubbed empty ``--pr`` fetch."""
        import argparse
        import contextlib
        import io
        from unittest import mock

        import ci_classify
        from ci_classify import run

        args = argparse.Namespace(input=None, pr=42, json=json_mode)
        buf = io.StringIO()
        with mock.patch.object(
            ci_classify, "fetch_check_runs_via_gh", return_value=[]
        ):
            with contextlib.redirect_stdout(buf):
                rc = run(args)
        return rc, buf.getvalue()

    def test_empty_pr_fetch_json_still_emits_envelope(self) -> None:
        rc, stdout = self._captured(json_mode=True)
        self.assertEqual(rc, 0)
        envelope = json.loads(stdout)
        self.assertIsInstance(envelope, dict)
        self.assertEqual(envelope.get("schema_version"), SCHEMA_VERSION)
        self.assertEqual(envelope.get("classifications"), [])

    def test_empty_pr_fetch_prose_mode_keeps_summary(self) -> None:
        rc, stdout = self._captured(json_mode=False)
        self.assertEqual(rc, 0)
        self.assertIn(
            "No check-runs retrieved for PR; nothing to classify.",
            stdout,
        )


if __name__ == "__main__":
    unittest.main()
