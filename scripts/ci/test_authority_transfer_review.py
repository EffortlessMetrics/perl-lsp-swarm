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

    def packet_for_other_surface(self) -> Path:
        """Build a current packet whose authority claim targets `close.contract`
        (a different surface than `authority_catalog`); this proves the row
        under review has at least one packet supplied but none that actually
        cover it, which is the FAIL_REVIEW_MISSING case after #16100."""
        body = packet_body("semantic_close_authority", HEAD)
        body["subject"]["changed"]["authorities"] = [
            {"ref": "close.contract", "subject": "docs/agents/CLOSE_PROOF_POLICY.md"}
        ]
        return self.write_packet("off-surface.json", body)

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

    def test_emitted_receipt_pins_self_describing_schema_token(self) -> None:
        # D1 Python variant (#15651): the receipt's schema_version must be a
        # self-describing string token, never a bare integer literal.
        receipt = self.evaluate(UNRELATED_CHANGED, [])
        self.assertEqual("authority-transfer-review.v1", receipt["schema_version"])
        self.assertEqual(receipt["schema_version"], atr.SCHEMA)

    def test_governed_change_without_packet_is_not_proven_and_head_bound(self) -> None:
        # Falsifier 1: candidate touches the configuration authority catalog with no
        # packet supplied. With zero packets the workflow has no packet source
        # configured at this invocation, so the verdict is NOT_PROVEN_GITHUB —
        # evidence could not be established. A red that says "review missing"
        # would lie about the proposition it claims to have checked; see #16100.
        receipt = self.evaluate(GOVERNED_CHANGED, [])
        self.assertEqual(atr.NOT_PROVEN_GITHUB, receipt["result"])
        self.assertEqual(HEAD, receipt["evaluated_head_sha"])
        self.assertEqual(["authority_catalog"], [row["surface_id"] for row in receipt["governed_rows"]])
        self.assertEqual(
            atr.NOT_PROVEN_GITHUB, receipt["verdicts"][0]["result"]
        )

    def test_not_proven_github_exits_neutral_never_reds_the_advisory_job(self) -> None:
        # #16150 Shape 3: until a packet channel exists, every governed change
        # yields NOT_PROVEN_GITHUB. That evidence boundary must not exit
        # non-zero — a red job every governed PR earns identically carries no
        # information and trains readers to ignore red. The typed verdict
        # stays in the summary; the instrument-failure and typed-failure
        # classes stay loud.
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            code = atr.main(
                [
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
                ]
            )
        self.assertEqual(atr.EXIT_PASS, code)
        self.assertIn("Result: `NOT_PROVEN_GITHUB`", buffer.getvalue())
        self.assertEqual(
            atr.EXIT_PASS, atr.exit_code_for_result(atr.NOT_PROVEN_SUBJECT)
        )
        self.assertEqual(
            atr.EXIT_NOT_PROVEN, atr.exit_code_for_result(atr.INSTRUMENT_FAILURE)
        )
        self.assertEqual(
            atr.EXIT_TYPED_FAILURE, atr.exit_code_for_result(atr.FAIL_REVIEW_MISSING)
        )

    def test_governed_change_with_off_surface_packets_is_typed_missing(self) -> None:
        # Regression coverage for #16100: packets supplied but none cover the
        # governed row is a real review-absence (FAIL_REVIEW_MISSING), distinct
        # from "no packets supplied" (NOT_PROVEN_GITHUB above). The verdict
        # distinguishes the wiring gap from genuine missing review.
        receipt = self.evaluate(GOVERNED_CHANGED, [self.packet_for_other_surface()])
        self.assertEqual(atr.FAIL_REVIEW_MISSING, receipt["result"])
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
        # A JSON parse failure happens before any ref can be read from the
        # packet, so the evaluator cannot know which surface (if any) it
        # claimed; it is treated as unrelated (see
        # test_unrelated_malformed_packet_does_not_invalidate_a_valid_row)
        # rather than aggregated onto the PR-wide result, and the uncovered
        # governed row still reports its own typed non-pass.
        malformed = self.packets_dir / "malformed.json"
        malformed.write_text("{not json", encoding="utf-8")
        receipt = self.evaluate(GOVERNED_CHANGED, [malformed])
        self.assertEqual(atr.FAIL_REVIEW_MISSING, receipt["result"])
        self.assertEqual(
            atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["packets"][0]["verdict"]
        )

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
        # Governed change with no packet supplied keeps its typed NOT_PROVEN_GITHUB
        # verdict in the receipt and summary, but exits neutral (0) under #16150
        # Shape 3: the advisory context must not red the job on a wiring/evidence
        # boundary every governed PR earns identically. A typed-failure exit
        # would lie about what was checked; a non-zero exit trains readers to
        # ignore red.
        buffer = io.StringIO()
        with redirect_stdout(buffer):
            status = atr.main(argv)
        self.assertEqual(atr.EXIT_PASS, status)
        written = json.loads(receipt_path.read_text(encoding="utf-8"))
        self.assertEqual(atr.NOT_PROVEN_GITHUB, written["result"])
        summary = summary_path.read_text(encoding="utf-8")
        self.assertIn("NOT_PROVEN_GITHUB", summary)
        self.assertIn(HEAD, summary)

    def test_cli_not_proven_verdict_is_advisory_neutral_distinct_from_typed_failure(self) -> None:
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
        # #16150 Shape 3: the trusted_workflow_run row's NOT_PROVEN_GITHUB
        # verdict is advisory-neutral at the job surface; the typed verdict
        # stays in the receipt.
        self.assertEqual(atr.EXIT_PASS, status)
        written = json.loads(receipt_path.read_text(encoding="utf-8"))
        self.assertEqual(atr.NOT_PROVEN_GITHUB, written["result"])

    def test_changed_list_nul_delimited_paths_with_special_characters_match(self) -> None:
        # `git diff --name-only -z` emits raw NUL-terminated bytes with no
        # C-quoting; a tab, newline, quote, or backslash inside a governed
        # path must still match its surface binding after that split.
        # vrs.normalize canonicalizes backslashes to forward slashes; assert
        # against the normalized form so this test targets the NUL-split
        # behavior under test, not normalization semantics.
        weird = "src/authority/tab\tnewline\nquote\"back/slash.rs"
        listed = self.base / "changed.nul"
        listed.write_bytes(weird.encode("utf-8") + b"\x00")
        inputs = {
            "root": self.base,
            "candidate_root": None,
            "repository": REPOSITORY,
            "pr_number": 1,
            "base_sha": "c" * 40,
            "head_sha": HEAD,
            "merge_base_sha": "",
            "head_tree_sha": "",
            "changed_list": listed,
            "changed_files": [],
            "packets": [],
            "max_changed_files": 100,
        }
        receipt = atr.evaluate(inputs)
        self.assertEqual(["authority_catalog"], [row["surface_id"] for row in receipt["governed_rows"]])
        self.assertEqual([weird], receipt["governed_rows"][0]["matched_paths"])

    def test_changed_list_nul_separated_undecodable_entry_fails_closed(self) -> None:
        listed = self.base / "changed.nul"
        listed.write_bytes(b"src/authority/catalog.rs\x00src/authority/bad\xff.rs\x00")
        inputs = {
            "root": self.base,
            "candidate_root": None,
            "repository": REPOSITORY,
            "pr_number": 1,
            "base_sha": "c" * 40,
            "head_sha": HEAD,
            "merge_base_sha": "",
            "head_tree_sha": "",
            "changed_list": listed,
            "changed_files": [],
            "packets": [],
            "max_changed_files": 100,
        }
        receipt = atr.evaluate(inputs)
        self.assertEqual(atr.NOT_PROVEN_GITHUB, receipt["result"])
        self.assertIn("changed_list_not_utf8", receipt["inputs"]["changed_list_error"])

    # ------------------------------------------------------------------
    # Candidate-tree extraction safety (fail closed before any bytes land)
    # ------------------------------------------------------------------

    def test_tree_entry_validator_accepts_ordinary_paths(self) -> None:
        entries = atr.parse_ls_tree_entries(
            b"100644 blob abc\tsrc/authority/catalog.rs\x00"
            b"040000 tree def\tsrc/authority\x00"
        )
        self.assertEqual([], atr.validate_tree_entries(entries))

    def test_tree_entry_validator_rejects_absolute_path(self) -> None:
        entries = [("100644", "/etc/passwd")]
        self.assertEqual(["unsafe_path ('/etc/passwd')"], atr.validate_tree_entries(entries))

    def test_tree_entry_validator_rejects_parent_traversal(self) -> None:
        entries = [("100644", "../../outside/evil.txt")]
        violations = atr.validate_tree_entries(entries)
        self.assertEqual(1, len(violations))
        self.assertIn("unsafe_path", violations[0])

    def test_tree_entry_validator_accepts_symlink_mode_for_post_extraction_removal(self) -> None:
        # Removal, not rejection: the repository tracks
        # crates/tree-sitter-perl/test/corpus as mode 120000, so rejecting
        # symlinks pre-extraction fails every run before the evaluator
        # starts. The workflow deletes every link after extraction, before
        # anything reads the tree.
        entries = [("120000", "src/authority/link")]
        self.assertEqual([], atr.validate_tree_entries(entries))

    def test_tree_entry_validator_rejects_submodule_gitlink_mode(self) -> None:
        entries = [("160000", "vendor/evil")]
        self.assertEqual(
            ["unsafe_mode (160000:vendor/evil)"], atr.validate_tree_entries(entries)
        )

    def test_cli_validate_tree_entries_flag_exits_not_proven_on_unsafe_entry(self) -> None:
        import io as _io
        from unittest import mock

        raw = b"160000 commit abc\tvendor/evil\x00"
        with mock.patch.object(sys, "stdin") as stdin_mock:
            stdin_mock.buffer = _io.BytesIO(raw)
            status = atr.main(["--validate-tree-entries"])
        self.assertEqual(atr.EXIT_NOT_PROVEN, status)

    def test_cli_validate_tree_entries_flag_exits_pass_on_safe_tree(self) -> None:
        import io as _io
        from unittest import mock

        raw = b"100644 blob abc\tsrc/authority/catalog.rs\x00"
        with mock.patch.object(sys, "stdin") as stdin_mock:
            stdin_mock.buffer = _io.BytesIO(raw)
            status = atr.main(["--validate-tree-entries"])
        self.assertEqual(atr.EXIT_PASS, status)

    # ------------------------------------------------------------------
    # Structural packet validation matches the closed contract, not a subset
    # ------------------------------------------------------------------

    def test_lens_omitted_from_the_closed_set_is_artifact_incomplete(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        # Drop one of the nine closed base lenses: a row's own required-lens
        # subset could still be satisfied, but the packet is no longer
        # structurally valid against the closed contract.
        body["lenses"] = [row for row in body["lenses"] if row["lens"] != atr.vrs.LENSES[-1]]
        packet = self.write_packet("lens-omitted.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])

    def test_duplicate_lens_entry_is_artifact_incomplete(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        body["lenses"].append(dict(body["lenses"][0]))
        packet = self.write_packet("lens-dup.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])

    def test_not_applicable_lens_without_reason_is_artifact_incomplete(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        body["lenses"][0]["applicability"] = "not_applicable"
        packet = self.write_packet("lens-no-reason.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])

    def test_missing_primary_proposition_is_artifact_incomplete(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        del body["challenge"]["primary_proposition"]
        packet = self.write_packet("no-proposition.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])

    def test_unknown_negative_control_criterion_is_artifact_incomplete(self) -> None:
        body = packet_body("semantic_close_authority", HEAD)
        body["negative_controls"][0]["checks"]["invented_criterion"] = {
            "status": "established",
            "evidence": "e",
        }
        packet = self.write_packet("nc-unknown.json", body)
        receipt = self.evaluate(GOVERNED_CHANGED, [packet])
        self.assertEqual(atr.FAIL_ARTIFACT_REVIEW_INCOMPLETE, receipt["result"])

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

    # ------------------------------------------------------------------
    # Unrelated-packet aggregation and subject/path coverage binding
    # ------------------------------------------------------------------

    def test_unrelated_malformed_packet_does_not_invalidate_a_valid_row(self) -> None:
        # A genuinely valid packet covers the only governed row...
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        # ...and an unrelated malformed document (e.g. a stray leftover from
        # another review) is also supplied. Its parse failure happens before
        # any ref is known, so it can never claim (and therefore can never
        # be shown to cover) any governed row here; it must not drag the
        # otherwise-current review down to a failure.
        malformed = self.packets_dir / "unrelated-malformed.json"
        malformed.write_text("{not json", encoding="utf-8")
        receipt = self.evaluate(GOVERNED_CHANGED, [good, malformed])
        self.assertEqual(atr.PASS_CURRENT_REVIEW, receipt["result"])

    def test_unrelated_wrong_surface_packet_does_not_invalidate_a_valid_row(self) -> None:
        # A valid packet covers the changed governed row...
        good = self.write_packet("good.json", packet_body("semantic_close_authority", HEAD))
        # ...and a second packet is structurally broken (missing roles) but
        # claims a ref for a surface nothing in this PR touched. It never
        # matches a governed row and must not poison the aggregate.
        unrelated_body = packet_body(
            "semantic_close_authority",
            HEAD,
            authorities=[{"ref": "manifest_self", "subject": "policy/review-surfaces.toml"}],
        )
        del unrelated_body["roles"]
        unrelated = self.write_packet("unrelated-broken.json", unrelated_body)
        receipt = self.evaluate(GOVERNED_CHANGED, [good, unrelated])
        self.assertEqual(atr.PASS_CURRENT_REVIEW, receipt["result"])

    def test_packet_covering_only_some_changed_paths_in_a_row_is_missing(self) -> None:
        # Two files change inside the same governed surface (src/authority/**)
        # but the packet's authority subject names only one of them. Binding
        # coverage to the ref alone would let this pass despite the reviewer
        # never having named the second file as reviewed.
        changed = ["src/authority/catalog.rs", "src/authority/other.rs"]
        partial = self.write_packet(
            "partial.json",
            packet_body(
                "semantic_close_authority",
                HEAD,
                authorities=[
                    {"ref": "config.authority_catalog", "subject": "src/authority/catalog.rs"}
                ],
            ),
        )
        receipt = self.evaluate(changed, [partial])
        self.assertEqual(atr.FAIL_REVIEW_MISSING, receipt["result"])

    def test_packet_covering_every_changed_path_in_a_row_passes(self) -> None:
        changed = ["src/authority/catalog.rs", "src/authority/other.rs"]
        full = self.write_packet(
            "full.json",
            packet_body(
                "semantic_close_authority",
                HEAD,
                authorities=[
                    {"ref": "config.authority_catalog", "subject": "src/authority/catalog.rs"},
                    {"ref": "config.authority_catalog", "subject": "src/authority/other.rs"},
                ],
            ),
        )
        receipt = self.evaluate(changed, [full])
        self.assertEqual(atr.PASS_CURRENT_REVIEW, receipt["result"])

    # ------------------------------------------------------------------
    # Candidate-union applicability (base ∪ candidate, never base alone)
    # ------------------------------------------------------------------

    def test_merge_governed_rows_unites_candidate_added_surface(self) -> None:
        base_rows = [
            {"surface_id": "a", "matched_paths": ["x.rs"]},
        ]
        candidate_rows = [
            {"surface_id": "b", "matched_paths": ["y.rs"]},
        ]
        merged = atr.merge_governed_rows(base_rows, candidate_rows)
        self.assertEqual(["a", "b"], [row["surface_id"] for row in merged])

    def test_merge_governed_rows_keeps_base_metadata_and_widens_paths(self) -> None:
        base_rows = [
            {
                "surface_id": "a",
                "family": "base-family",
                "matched_paths": ["x.rs"],
            },
        ]
        candidate_rows = [
            {
                "surface_id": "a",
                "family": "candidate-family",
                "matched_paths": ["y.rs"],
            },
        ]
        merged = atr.merge_governed_rows(base_rows, candidate_rows)
        self.assertEqual(1, len(merged))
        # Base metadata wins: the candidate only contributes applicability.
        self.assertEqual("base-family", merged[0]["family"])
        self.assertEqual(["x.rs", "y.rs"], merged[0]["matched_paths"])

    def test_candidate_added_surface_is_governed_not_not_applicable(self) -> None:
        import shutil

        candidate_root = Path(self._tmp.name) / "candidate"
        shutil.copytree(self.base, candidate_root, ignore=shutil.ignore_patterns("packets"))
        manifest_path = candidate_root / atr.DEFAULT_MANIFEST
        extra = """
[surface.candidate_added]
family = "semantic_issue_completion"
authority = "Candidate-added surface."
controller = "#11795"
conflict_key = "candidate.added"
risk_class = "semantic_control"
review_profile = "semantic_close_authority"
required_evidence = "current_head_reviewer_packet"
first_falsifier = "The candidate-added path is ungoverned."
enforcement_successor = "#11796"
code_owner_route = { kind = "not_proven", resolution_owner = "#11796", note = "deferred" }
paths = [
  "src/candidate-added/**",
]
"""
        manifest_path.write_text(
            manifest_path.read_text(encoding="utf-8") + extra,
            encoding="utf-8",
            newline="\n",
        )
        added = candidate_root / "src/candidate-added/new.rs"
        added.parent.mkdir(parents=True, exist_ok=True)
        added.write_text("candidate", encoding="utf-8")
        doc = atr.tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        projection_path = candidate_root / atr.DEFAULT_PROJECTION
        projection_path.write_text(
            atr.vrs.render_projection(doc), encoding="utf-8", newline="\n"
        )
        changed = ["src/candidate-added/new.rs"]
        # Base alone sees no governed row for this path.
        base_only = self.evaluate(changed, [])
        self.assertEqual(atr.PASS_NOT_APPLICABLE, base_only["result"])
        # The union sees the candidate-added surface. With no packets supplied,
        # #16100 makes this NOT_PROVEN_GITHUB (wiring gap, not a typed
        # missing-review), so the verdict distinguishes the candidate-extension
        # case from a real review-absence.
        receipt = self.evaluate(changed, [], candidate_root=candidate_root)
        self.assertEqual(atr.NOT_PROVEN_GITHUB, receipt["result"])
        self.assertEqual(
            ["candidate_added"], [row["surface_id"] for row in receipt["governed_rows"]]
        )

    # ------------------------------------------------------------------
    # Workflow shape pins (rename detection, symlink removal)
    # ------------------------------------------------------------------

    def test_workflow_changed_file_diff_disables_rename_detection(self) -> None:
        workflow = _HERE / "../../.github/workflows/authority-transfer-review.yml"
        text = workflow.resolve().read_text(encoding="utf-8")
        self.assertIn("--no-renames", text)

    def test_workflow_neutralizes_symlinks_by_removal(self) -> None:
        workflow = _HERE / "../../.github/workflows/authority-transfer-review.yml"
        text = workflow.resolve().read_text(encoding="utf-8")
        self.assertIn("print -delete", text)


if __name__ == "__main__":
    unittest.main()
