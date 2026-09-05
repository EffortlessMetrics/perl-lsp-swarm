#!/usr/bin/env python3
"""Focused tests for scripts/ci/authority_transfer_review.py (#11795).

The issue's first falsifiers are encoded as scenarios over a complete minimal
fixture root; each must produce its typed non-green result, never a
stale-valid-looking pass. Determinism of consecutive computations over
unchanged inputs is asserted byte-for-byte.
"""

from __future__ import annotations

import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from typing import Any

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

import authority_transfer_review as atr  # noqa: E402

REPOSITORY = "EffortlessMetrics/perl-lsp-swarm"
HEAD = "a" * 40
STALE_HEAD = "b" * 40
GOVERNED_CHANGED = ["src/authority/catalog.rs"]
UNRELATED_CHANGED = ["crates/other/src/lib.rs"]


def build_fixture_root(base: Path) -> Path:
    atr._write_fixture_root(base)
    packets = base / "packets"
    packets.mkdir(exist_ok=True)
    return base


def packet_body(
    profile: str,
    head_value: str,
    repo: str = REPOSITORY,
    authorities: list[dict[str, str]] | None = None,
) -> dict[str, Any]:
    # Single fixture source: the evaluator's self-test and these tests share it.
    return atr.fixture_packet_body(profile, head_value, repo, authorities)


class AuthorityTransferReviewTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="atr-tests-")
        self.addCleanup(self._tmp.cleanup)
        self.base = build_fixture_root(Path(self._tmp.name))
        self.packets_dir = self.base / "packets"

    def write_packet(self, name: str, body: dict[str, Any]) -> Path:
        target = self.packets_dir / name
        target.write_text(json.dumps(body), encoding="utf-8", newline="\n")
        return target

    def evaluate(
        self,
        changed: list[str],
        packets: list[Path],
        **overrides: Any,
    ) -> dict[str, Any]:
        inputs = {
            "root": overrides.get("root", self.base),
            "candidate_root": overrides.get("candidate_root"),
            "repository": overrides.get("repository", REPOSITORY),
            "pr_number": 1,
            "base_sha": "c" * 40,
            "head_sha": overrides.get("head_sha", HEAD),
            "merge_base_sha": overrides.get("merge_base_sha", ""),
            "head_tree_sha": overrides.get("head_tree_sha", ""),
            "changed_list": None,
            "changed_files": changed,
            "packets": [{"label": p.name, "path": p} for p in packets],
            "max_changed_files": overrides.get("max_changed_files", 100),
        }
        return atr.evaluate(inputs)

    # ------------------------------------------------------------------
    # Applicability and exact-head binding
    # ------------------------------------------------------------------

    def test_unrelated_pr_takes_the_cheap_not_applicable_route(self) -> None:
        receipt = self.evaluate(UNRELATED_CHANGED, [])
        self.assertEqual(atr.PASS_NOT_APPLICABLE, receipt["result"])
        self.assertEqual([], receipt["governed_rows"])
        self.assertEqual([], receipt["verdicts"])

    def test_governed_change_without_packet_is_typed_missing_and_head_bound(self) -> None:
        # Falsifier 1: candidate touches the configuration authority catalog with no packet.
        receipt = self.evaluate(GOVERNED_CHANGED, [])
        self.assertEqual(atr.FAIL_REVIEW_MISSING, receipt["result"])
        self.assertEqual(HEAD, receipt["evaluated_head_sha"])
        self.assertEqual(["authority_catalog"], [row["surface_id"] for row in receipt["governed_rows"]])
        self.assertEqual(
            atr.FAIL_REVIEW_MISSING, receipt["verdicts"][0]["result"]
        )

    def test_checked_projection_row_is_satisfiable_today(self) -> None:
        receipt = self.evaluate(["docs/policy/REVIEW_SURFACES.md"], [])
        self.assertEqual(atr.PASS_CURRENT_REVIEW, receipt["result"])
        self.assertEqual(
            ["checked_projection"],
            [row["required_evidence"] for row in receipt["governed_rows"]],
        )
        self.assertEqual(atr.PASS_CURRENT_REVIEW, receipt["verdicts"][0]["result"])

    def test_trusted_workflow_run_row_stays_not_proven_github(self) -> None:
        receipt = self.evaluate([".github/workflows/review-receipt-retirement.yml"], [])
        self.assertEqual(atr.NOT_PROVEN_GITHUB, receipt["result"])
        self.assertEqual(
            ["trusted_workflow_run"],
            [row["required_evidence"] for row in receipt["governed_rows"]],
        )
        self.assertEqual(atr.NOT_PROVEN_GITHUB, receipt["verdicts"][0]["result"])
        self.assertFalse(receipt["inputs"]["changed_files_truncated"])

    # ------------------------------------------------------------------
    # Exact current-head contract
    # ------------------------------------------------------------------

    def test_packet_bound_to_previous_head_is_stale_never_valid(self) -> None:
        # Falsifier 2: review binds the previous head.
        stale = self.write_packet(
            "stale.json", packet_body("semantic_close_authority", STALE_HEAD)
        )
        receipt = self.evaluate(GOVERNED_CHANGED, [stale])
        self.assertEqual(atr.FAIL_REVIEW_STALE_HEAD, receipt["result"])
        self.assertEqual("stale", receipt["packets"][0]["head_binding"])

    def test_packet_bound_to_another_base_is_stale_when_merge_base_is_trusted(self) -> None:
        # #11795: exact base identity, not only head. Packet base defaults to
        # "c" * 40, so binding to a different trusted merge base must fail.
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        receipt = self.evaluate(GOVERNED_CHANGED, [good], merge_base_sha="e" * 40)
        self.assertEqual(atr.FAIL_REVIEW_STALE_HEAD, receipt["result"])
        self.assertEqual("stale", receipt["packets"][0]["base_binding"])
        self.assertEqual("packet_base_differs_from_merge_base", receipt["packets"][0]["reason"])
        self.assertEqual("exact", receipt["identity_binding"]["base"])
        current = self.evaluate(GOVERNED_CHANGED, [good], merge_base_sha="c" * 40)
        self.assertEqual(atr.PASS_CURRENT_REVIEW, current["result"])
        self.assertEqual("current", current["packets"][0]["base_binding"])

    def test_packet_bound_to_another_tree_is_stale_when_head_tree_is_trusted(self) -> None:
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        receipt = self.evaluate(GOVERNED_CHANGED, [good], head_tree_sha="f" * 40)
        self.assertEqual(atr.FAIL_REVIEW_STALE_HEAD, receipt["result"])
        self.assertEqual("stale", receipt["packets"][0]["tree_binding"])
        self.assertEqual("packet_tree_differs_from_head_tree", receipt["packets"][0]["reason"])
        current = self.evaluate(GOVERNED_CHANGED, [good], head_tree_sha="d" * 40)
        self.assertEqual(atr.PASS_CURRENT_REVIEW, current["result"])
        self.assertEqual("current", current["packets"][0]["tree_binding"])

    def test_unbound_base_and_tree_are_recorded_not_assumed(self) -> None:
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        receipt = self.evaluate(GOVERNED_CHANGED, [good])
        self.assertEqual("unbound", receipt["identity_binding"]["base"])
        self.assertEqual("unbound", receipt["identity_binding"]["tree"])
        self.assertEqual("unbound", receipt["identity_binding"]["diff"])
        self.assertEqual("unbound", receipt["packets"][0]["base_binding"])

    def test_evidence_claiming_another_repository_exceeds_the_claim_ceiling(self) -> None:
        other = self.write_packet(
            "other.json",
            packet_body("semantic_close_authority", HEAD, repo="Elsewhere/other"),
        )
        receipt = self.evaluate(GOVERNED_CHANGED, [other])
        self.assertEqual(atr.FAIL_CLAIM_CEILING_EXCEEDED, receipt["result"])

    # ------------------------------------------------------------------
    # Profile, role, and negative-proof contracts
    # ------------------------------------------------------------------

    def test_profile_mismatch_between_packet_and_surface_fails(self) -> None:
        wrong = self.write_packet(
            "wrong.json", packet_body("live_repository_policy_authority", HEAD)
        )
        receipt = self.evaluate(GOVERNED_CHANGED, [wrong])
        self.assertEqual(atr.FAIL_REVIEW_PROFILE_MISMATCH, receipt["result"])

    def test_row_level_profile_divergence_fails_even_when_profile_exists(self) -> None:
        # A packet whose profile is valid in the manifest but differs from the
        # governed row's own review profile still fails the row.
        close_profile_packet = self.write_packet(
            "close-profile.json",
            packet_body(
                "semantic_close_authority",
                HEAD,
                authorities=[
                    {"ref": "config.public_schema", "subject": "schemas/perllsp-settings.schema.json"}
                ],
            ),
        )
        receipt = self.evaluate(["schemas/perllsp-settings.schema.json"], [close_profile_packet])
        self.assertEqual(atr.FAIL_REVIEW_PROFILE_MISMATCH, receipt["result"])
        self.assertEqual("settings_schema", receipt["verdicts"][0]["surface_id"])

    def test_codeowners_style_builder_only_review_is_not_review_evidence(self) -> None:
        # Falsifier 6: CODEOWNERS match or one approval treated as sufficient.
        builder_only = packet_body("semantic_close_authority", HEAD)
        builder_only["roles"] = [
            {"role": "builder_self_review", "required": True, "obligation": "Self-checked."}
        ]
        packet = self.write_packet("builder.json", builder_only)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_REVIEW_PROFILE_MISMATCH, receipt["result"])

    def test_missing_negative_controls_fail_the_first_falsifier_class(self) -> None:
        # Falsifier 4-style: review omits the first external-effect negative.
        body = packet_body("semantic_close_authority", HEAD)
        body["negative_controls"] = []
        packet = self.write_packet("no-controls.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_FIRST_FALSIFIER_MISSING, receipt["result"])

    def test_second_falsifier_without_its_own_control_fails_first_falsifier_class(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        body["challenge"]["falsifiers"].append(
            {"id": "F2", "stage": "review", "statement": "Second falsifier."}
        )
        packet = self.write_packet("uncontrolled-f2.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_FIRST_FALSIFIER_MISSING, receipt["result"])
        self.assertEqual(
            "falsifier_without_negative_control (F2)", receipt["packets"][0]["reason"]
        )

    def test_duplicate_negative_control_for_one_falsifier_is_incomplete(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        body["negative_controls"].append(dict(body["negative_controls"][0]))
        packet = self.write_packet("dup-control.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])
        self.assertEqual("duplicate_negative_control (F1)", receipt["packets"][0]["reason"])

    def test_no_test_mutation_obligation_is_zero_work_proof(self) -> None:
        # Falsifier 8: proof selected zero work but review reports current.
        body = packet_body("semantic_close_authority", HEAD)
        body["obligations"]["tests_mutations"] = []
        packet = self.write_packet("no-mutations.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_FIRST_FALSIFIER_MISSING, receipt["result"])

    def test_established_criterion_without_evidence_is_artifact_incomplete(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        body["negative_controls"][0]["checks"]["exists"] = {"status": "established"}
        packet = self.write_packet("unevidenced.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])
        self.assertTrue(
            receipt["packets"][0]["reason"].startswith("established_without_evidence ("),
            receipt["packets"][0]["reason"],
        )

    def test_not_established_negative_control_is_a_finding_never_a_pass(self) -> None:
        # Packet contract: every criterion must be established; a fully
        # not_established control must not validate into PASS_CURRENT_REVIEW.
        body = packet_body("semantic_close_authority", HEAD)
        for name in atr.NEGATIVE_CONTROL_CRITERIA:
            body["negative_controls"][0]["checks"][name] = {"status": "not_established"}
        packet = self.write_packet("unestablished.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])
        self.assertNotEqual(atr.PASS_CURRENT_REVIEW, receipt["result"])
        self.assertEqual(
            "negative_control_criterion_unestablished (exists)",
            receipt["packets"][0]["reason"],
        )

    def test_single_not_established_negative_control_criterion_fails(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        body["negative_controls"][0]["checks"]["exists"] = {"status": "not_established"}
        packet = self.write_packet("one-unestablished.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])
        self.assertEqual(
            "negative_control_criterion_unestablished (exists)",
            receipt["packets"][0]["reason"],
        )

    def test_malformed_packet_is_not_a_pass(self) -> None:
        malformed = self.packets_dir / "malformed.json"
        malformed.write_text("{not json", encoding="utf-8")
        receipt = self.evaluate(GOVERNED_CHANGED, [malformed])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])

    def test_invented_role_rejected_against_closed_vocabulary(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        body["roles"] = [{"role": "vibes_reviewer", "required": True, "obligation": "o"}]
        packet = self.write_packet("vibes.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])

    # ------------------------------------------------------------------
    # Predecessor disposition contract
    # ------------------------------------------------------------------

    def _manifest_with_predecessor_exit(self) -> Path:
        # No restore needed: setUp builds a fresh fixture tree for every test.
        manifest_path = self.base / atr.DEFAULT_MANIFEST
        variant = atr._fixture_manifest_text(
            catalog_predecessor_exit="Old catalog retired here."
        )
        manifest_path.write_text(variant, encoding="utf-8", newline="\n")
        import tomllib

        projection = self.base / atr.DEFAULT_PROJECTION
        rendered = atr.vrs.render_projection(tomllib.loads(variant))
        projection.write_text(rendered, encoding="utf-8", newline="\n")
        return manifest_path

    def test_predecessor_exit_without_disposition_fails(self) -> None:
        # Falsifier 5-style: new authority lands, independently mutable predecessor missed.
        self._manifest_with_predecessor_exit()
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        receipt = self.evaluate(GOVERNED_CHANGED, [good])
        self.assertEqual(atr.FAIL_PREDECESSOR_REVIEW_INCOMPLETE, receipt["result"])

    def test_unexpected_duplicate_disposition_is_controller_relation(self) -> None:
        # Falsifier 7: a controller-closing relation survives review.
        self._manifest_with_predecessor_exit()
        dup = packet_body("semantic_close_authority", HEAD)
        dup["subject"]["changed"]["migrated_seams"] = ["old catalog"]
        dup["old_paths"] = [{"seam": "old catalog", "disposition": "unexpected_duplicate"}]
        packet = self.write_packet("dup.json", dup)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_CONTROLLER_RELATION, receipt["result"])

    def test_typed_predecessor_acknowledgment_passes(self) -> None:
        self._manifest_with_predecessor_exit()
        ok = packet_body("semantic_close_authority", HEAD)
        ok["subject"]["changed"]["migrated_seams"] = ["old catalog"]
        ok["old_paths"] = [{"seam": "old catalog", "disposition": "removed"}]
        packet = self.write_packet("ok.json", ok)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.PASS_CURRENT_REVIEW, receipt["result"])

    # ------------------------------------------------------------------
    # Denominator and input bounds
    # ------------------------------------------------------------------

    def test_old_path_disposition_must_name_a_migrated_seam(self) -> None:
        # A disposition about a seam the subject never declared as migrated is
        # not predecessor review of this change.
        self._manifest_with_predecessor_exit()
        stray = packet_body("semantic_close_authority", HEAD)
        stray["old_paths"] = [{"seam": "unrelated seam", "disposition": "removed"}]
        packet = self.write_packet("stray.json", stray)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])
        self.assertEqual(
            "old_path_seam_not_migrated (unrelated seam)", receipt["packets"][0]["reason"]
        )

    def test_declared_migrated_seam_without_disposition_is_predecessor_incomplete(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        body["subject"]["changed"]["migrated_seams"] = ["old catalog"]
        packet = self.write_packet("undispositioned.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_PREDECESSOR_REVIEW_INCOMPLETE, receipt["result"])
        self.assertEqual(
            "migrated_seam_undispositioned (old catalog)", receipt["packets"][0]["reason"]
        )

    def test_broken_manifest_denominator_never_passes(self) -> None:
        manifest_path = self.base / atr.DEFAULT_MANIFEST
        saved = manifest_path.read_text(encoding="utf-8")

        def restore() -> None:
            manifest_path.write_text(saved, encoding="utf-8", newline="\n")

        self.addCleanup(restore)
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        manifest_path.write_text("schema_version = 99\n", encoding="utf-8", newline="\n")
        receipt = self.evaluate(GOVERNED_CHANGED, [good])
        self.assertEqual(atr.FAIL_DENOMINATOR_INCOMPLETE, receipt["result"])
        self.assertFalse(receipt["denominator"]["base_tree_strict_pass"])

    def test_emptied_manifest_denominator_never_passes(self) -> None:
        # An empty file parses to {} with zero rows; the evaluator must
        # report a denominator-incomplete issue instead of a clean pass.
        manifest_path = self.base / atr.DEFAULT_MANIFEST
        saved = manifest_path.read_text(encoding="utf-8")

        def restore() -> None:
            manifest_path.write_text(saved, encoding="utf-8", newline="\n")

        self.addCleanup(restore)
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        manifest_path.write_text("", encoding="utf-8", newline="\n")
        receipt = self.evaluate(GOVERNED_CHANGED, [good])
        self.assertEqual(atr.FAIL_DENOMINATOR_INCOMPLETE, receipt["result"])
        self.assertFalse(receipt["denominator"]["base_tree_strict_pass"])

    def test_projection_drift_fails_closed(self) -> None:
        projection = self.base / atr.DEFAULT_PROJECTION
        saved = projection.read_text(encoding="utf-8")

        def restore() -> None:
            projection.write_text(saved, encoding="utf-8", newline="\n")

        self.addCleanup(restore)
        projection.write_text("hand-edited lie\n", encoding="utf-8", newline="\n")
        receipt = self.evaluate(GOVERNED_CHANGED, [])
        self.assertEqual(atr.FAIL_DENOMINATOR_INCOMPLETE, receipt["result"])

    def test_candidate_denominator_breakage_is_detected_as_data(self) -> None:
        import tempfile as _tempfile

        with _tempfile.TemporaryDirectory(prefix="atr-candidate-") as cand_tmp:
            candidate = Path(cand_tmp)
            atr._write_fixture_root(candidate)
            manifest = candidate / atr.DEFAULT_MANIFEST
            manifest.write_text("schema_version = 99\n", encoding="utf-8", newline="\n")
            receipt = self.evaluate(
                UNRELATED_CHANGED, [], candidate_root=candidate
            )
            self.assertEqual(atr.FAIL_DENOMINATOR_INCOMPLETE, receipt["result"])
            self.assertTrue(receipt["denominator"]["candidate_tree_checked"])
            self.assertFalse(receipt["denominator"]["candidate_tree_strict_pass"])

    def test_bounded_changed_file_overflow_is_not_proven_github(self) -> None:
        # Falsifier 10 guard rail: bounds exceeded can never look like a clean pass.
        receipt = self.evaluate(
            GOVERNED_CHANGED + [f"filler/{i}.txt" for i in range(120)], []
        )
        self.assertEqual(atr.NOT_PROVEN_GITHUB, receipt["result"])

    # ------------------------------------------------------------------
    # Determinism and CLI contract
    # ------------------------------------------------------------------

    def test_non_utf8_changed_list_is_not_proven_never_ungoverned(self) -> None:
        listed = self.base / "changed.txt"
        listed.write_bytes(b"src/authority/catalog\xff.rs\n")
        inputs = {
            "root": self.base,
            "candidate_root": None,
            "repository": REPOSITORY,
            "pr_number": 1,
            "base_sha": "c" * 40,
            "head_sha": HEAD,
            "changed_list": listed,
            "changed_files": [],
            "packets": [],
            "max_changed_files": 100,
        }
        receipt = atr.evaluate(inputs)
        self.assertEqual(atr.NOT_PROVEN_GITHUB, receipt["result"])
        self.assertNotEqual(atr.PASS_NOT_APPLICABLE, receipt["result"])
        self.assertTrue(receipt["inputs"]["changed_list_error"].startswith("changed_list_not_utf8"))

    def test_consecutive_computations_over_unchanged_inputs_are_byte_identical(self) -> None:
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        first = atr.render_receipt(self.evaluate(GOVERNED_CHANGED, [good]))
        second = atr.render_receipt(self.evaluate(GOVERNED_CHANGED, [good]))
        self.assertEqual(first, second)

    def test_current_packet_passes_with_full_row_record(self) -> None:
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        receipt = self.evaluate(GOVERNED_CHANGED, [good])
        self.assertEqual(atr.PASS_CURRENT_REVIEW, receipt["result"])
        self.assertEqual(
            ["authority_catalog"],
            [row["surface_id"] for row in receipt["governed_rows"]],
        )
        self.assertEqual(64, len(receipt["denominator"]["manifest_sha256"]))

    def test_cli_exit_contract_receipt_and_summary(self) -> None:
        receipt_path = self.base / "out" / "receipt.json"
        summary_path = self.base / "out" / "summary.md"
        argv = [
            "--root",
            str(self.base),
            "--repository",
            REPOSITORY,
            "--pr-number",
            "1",
            "--base-sha",
            "c" * 40,
            "--head-sha",
            HEAD,
            "--changed-file",
            GOVERNED_CHANGED[0],
            "--receipt",
            str(receipt_path),
            "--summary",
            str(summary_path),
        ]
        # Governed change without packet must exit typed-failure (1).
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            status = atr.main(argv)
        self.assertEqual(atr.EXIT_TYPED_FAILURE, status)
        written = json.loads(receipt_path.read_text(encoding="utf-8"))
        self.assertEqual(atr.FAIL_REVIEW_MISSING, written["result"])
        summary = summary_path.read_text(encoding="utf-8")
        self.assertIn("FAIL_REVIEW_MISSING", summary)
        self.assertIn(HEAD, summary)

    def test_cli_not_proven_exit_code_is_distinct_from_typed_failure(self) -> None:
        receipt_path = self.base / "out" / "receipt.json"
        argv = [
            "--root",
            str(self.base),
            "--repository",
            REPOSITORY,
            "--pr-number",
            "1",
            "--base-sha",
            "c" * 40,
            "--head-sha",
            HEAD,
            "--changed-file",
            ".github/workflows/review-receipt-retirement.yml",
            "--receipt",
            str(receipt_path),
        ]
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            status = atr.main(argv)
        self.assertEqual(atr.EXIT_NOT_PROVEN, status)
        written = json.loads(receipt_path.read_text(encoding="utf-8"))
        self.assertEqual(atr.NOT_PROVEN_GITHUB, written["result"])

    def test_exit_code_classes_partition_the_result_vocabulary(self) -> None:
        # Membership, not a string prefix, decides the exit code.
        every = set(atr.PASS_RESULTS) | set(atr.TYPED_FAILURE_RESULTS) | set(atr.NOT_PROVEN_RESULTS)
        self.assertEqual(set(atr.SEVERITY_ORDER), every)
        self.assertFalse(set(atr.PASS_RESULTS) & set(atr.TYPED_FAILURE_RESULTS))
        self.assertFalse(set(atr.NOT_PROVEN_RESULTS) & set(atr.TYPED_FAILURE_RESULTS))

    def test_cli_self_test_flag_runs_green(self) -> None:
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            status = atr.main(["--self-test"])
        self.assertEqual(atr.EXIT_PASS, status)
        self.assertIn("self-test passed", buffer.getvalue())


if __name__ == "__main__":
    unittest.main()
