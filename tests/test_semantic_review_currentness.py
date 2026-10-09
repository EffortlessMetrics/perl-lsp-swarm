from __future__ import annotations

import ast
import contextlib
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

ACTIVE_REVIEW_SURFACES = [
    ROOT / ".agents/skills/review-pr/SKILL.md",
    ROOT / ".agents/skills/final-challenge/SKILL.md",
    ROOT / ".agents/skills/finish-pr/SKILL.md",
    ROOT / ".agents/skills/verify-live-ci/SKILL.md",
    ROOT / ".agents/skills/merge-reconcile/SKILL.md",
    ROOT / ".agents/skills/orchestrate-work/SKILL.md",
    ROOT / ".claude/skills/review-pr/SKILL.md",
    ROOT / ".claude/skills/final-challenge/SKILL.md",
    ROOT / ".claude/skills/finish-pr/SKILL.md",
    ROOT / ".claude/skills/verify-live-ci/SKILL.md",
    ROOT / ".claude/skills/merge-reconcile/SKILL.md",
    ROOT / ".claude/skills/orchestrate-work/SKILL.md",
    ROOT / "docs/how-to/SESSION_OPERATIONS.md",
    ROOT / "docs/swarm/modern-claude-operating-model.md",
    ROOT / "scripts/pre-merge-check.sh",
]

RETIRED_ACTIVE_COMMANDS = [
    re.compile(r"^\s*(?:\$\s*)?scripts/reviews/run\s+review-(?:start|done)\b"),
    re.compile(r"^\s*(?:\$\s*)?scripts/reviews/claim-digest\b"),
    re.compile(
        r"^\s*REVIEW_PROTOCOL_ENFORCE=1\s+scripts/ci/check-pr-review-convergence\b"
    ),
]

USEFUL_REVIEW_MARKERS = [
    "## Review scope",
    "## Evidence and falsifiers",
    "## Findings",
    "## No material findings",
    "## Prior finding dispositions",
    "## What this establishes",
    "## Residual risk / not proved",
    "## Next action",
]


def normalized(path: Path) -> str:
    return " ".join(path.read_text(encoding="utf-8").split())


class SemanticReviewCurrentnessPolicySurfaces(unittest.TestCase):
    """Policy-surface contracts for subject-bound semantic review currentness."""

    def test_active_review_surfaces_do_not_invoke_exact_head_receipts(self) -> None:
        violations: list[str] = []
        for path in ACTIVE_REVIEW_SURFACES:
            for line_number, line in enumerate(
                path.read_text(encoding="utf-8").splitlines(), start=1
            ):
                for pattern in RETIRED_ACTIVE_COMMANDS:
                    if pattern.search(line):
                        violations.append(
                            f"{path.relative_to(ROOT)}:{line_number}: {line.strip()}"
                        )

        assert not violations, "retired review receipt commands remain active:\n" + "\n".join(
            violations
        )

    def test_review_skills_require_useful_durable_records(self) -> None:
        for relative in (
            ".agents/skills/review-pr/SKILL.md",
            ".claude/skills/review-pr/SKILL.md",
        ):
            text = (ROOT / relative).read_text(encoding="utf-8")
            missing = [marker for marker in USEFUL_REVIEW_MARKERS if marker not in text]
            assert not missing, f"{relative} is missing useful review fields: {missing}"
            assert "Do not submit only `LGTM`" in text
            assert "head SHA" in text
            assert "claim digest" in text

    def test_roots_route_to_shared_semantic_currentness_contract(self) -> None:
        for relative in ("AGENTS.md", "CLAUDE.md"):
            text = normalized(ROOT / relative)
            assert "docs/agents/REVIEW_CURRENTNESS.md" in text
            assert "head SHA change alone → no review invalidation" in text
            assert "merge uses current head only as compare-and-swap protection" in text
            assert "Missing, partial, stale, contradictory, or instrument-failed evidence is `NOT_PROVEN`" in text

    def test_merge_skill_keeps_expected_head_race_protection(self) -> None:
        for relative in (
            ".agents/skills/merge-reconcile/SKILL.md",
            ".claude/skills/merge-reconcile/SKILL.md",
        ):
            text = normalized(ROOT / relative)
            assert "--match-head-commit" in text
            assert "compare-and-swap" in text
            assert "It does not make review currentness depend on the SHA." in text
            assert "Refresh only proof, review, and integration dimensions affected by the new commit." in text

    def test_orchestration_briefs_separate_stable_claim_from_volatile_observation(self) -> None:
        for relative in (
            ".agents/skills/orchestrate-work/SKILL.md",
            ".claude/skills/orchestrate-work/SKILL.md",
        ):
            text = normalized(ROOT / relative)
            assert "Separate the brief's stable part from its observed part." in text
            assert "Head SHAs, check results, mergeability, and counts" in text
            assert "Re-derive live protection, rulesets, contexts, and results before mutating." in text
            assert "claim digest" not in text.lower()

    def test_pre_merge_preserves_native_disposition_validation(self) -> None:
        text = (ROOT / "scripts/pre-merge-check.sh").read_text(encoding="utf-8")
        assert "check-pr-review-convergence" in text
        assert "native_review_facts_converged" in text
        assert "semantic_currentness_required" in text

    def test_pre_merge_requires_subject_bound_semantic_review(self) -> None:
        text = (ROOT / "scripts/pre-merge-check.sh").read_text(encoding="utf-8")
        assert "check-pr-semantic-review-currentness.py" in text
        assert 'SEMANTIC_CLASS" != "REVIEW_CURRENT"' in text
        assert "substantive review is" in text

    def test_legacy_review_writer_is_inert(self) -> None:
        text = (ROOT / "scripts/reviews/run").read_text(encoding="utf-8")
        assert "RETIRED:" in text
        assert "exit 2" in text
        assert "gh api" not in text
        assert "gh pr view" not in text
        assert "claim-digest" not in text
        assert "review-run:v1" not in text

    def test_native_convergence_is_fact_only(self) -> None:
        text = (ROOT / "scripts/ci/check-pr-review-convergence").read_text(
            encoding="utf-8"
        )
        assert "check-pr-claim-currentness" not in text
        assert "material_claim_receipt_required: false" in text
        assert "exact_head_review_required: false" in text
        assert 'review_currentness: "NOT_PROVEN"' in text
        assert "semantic_currentness_required: true" in text
        assert "semantic_changed_seam" not in text
        assert "pending_reviewers" in text
        assert "current_change_requests" in text
        assert "unresolved_total" in text
        assert "resolved_without_disposition" in text
        assert "SUBMITTED_REVIEW_PRESENT" in text
        assert "present_unclassified" in text

    def test_review_skills_publish_subject_bound_currentness_marker(self) -> None:
        for relative in (
            ".agents/skills/review-pr/SKILL.md",
            ".claude/skills/review-pr/SKILL.md",
        ):
            text = (ROOT / relative).read_text(encoding="utf-8")
            assert "check-pr-semantic-review-currentness.py" in text
            assert "--emit-marker" in text
            assert "semantic-review:v1" in text
            assert "COMMENTED` review is only a GitHub fact" in text

    def test_shared_contract_defines_durable_semantic_review_record(self) -> None:
        text = " ".join(
            (ROOT / "docs/agents/REVIEW_CURRENTNESS.md")
            .read_text(encoding="utf-8")
            .split()
        )
        assert "## Durable semantic review record" in text
        assert "git diff --binary --full-index" in text
        assert "whitespace-only edits in already-reviewed `.md`/`.txt` files" in text
        assert "indentation and spacing can be semantic" in text
        assert "fenced content is compared byte-for-byte" in text

    def test_subject_bound_checker_requires_durable_review_record(self) -> None:
        text = (
            ROOT / "scripts/ci/check-pr-semantic-review-currentness.py"
        ).read_text(encoding="utf-8")
        assert "semantic-review:v1" in text
        assert "REVIEW_CURRENT" in text
        assert "## Review scope" in text
        assert "## Evidence and falsifiers" in text
        assert "## What this establishes" in text
        assert "## Residual risk / not proved" in text
        assert "subject_sha256" in text
        assert "git" in text and "diff" in text and "--binary" in text

    def test_subject_bound_checker_versions_stdout_json_payload(self) -> None:
        """The stdout JSON payload must carry schema_version so a wire-shape
        bump is observable at the consumer side rather than silent. See
        #15284 — the marker envelope (`semantic-review:v1`) and the stdout
        JSON payload are two distinct wire surfaces and must be version-skew
        free at the producer side.

        Counting string occurrences of the field is not enough: a fourth
        `print(json.dumps(...))` or a `setdefault` that keeps a foreign
        version would still satisfy a source-text count. Pin the helper.
        """
        source = (
            ROOT / "scripts/ci/check-pr-semantic-review-currentness.py"
        ).read_text(encoding="utf-8")
        assert 'SCHEMA_VERSION = "semantic_review_currentness.v1"' in source
        assert 'setdefault("schema_version"' not in source
        assert "setdefault('schema_version'" not in source

        tree = ast.parse(source)
        helpers: set[str] = set()
        dumps_in: list[str | None] = []
        print_dumps_in: list[str | None] = []
        emit_calls_in_main = 0
        func_stack: list[str] = []

        def is_json_dumps(node: ast.AST) -> bool:
            return (
                isinstance(node, ast.Call)
                and isinstance(node.func, ast.Attribute)
                and isinstance(node.func.value, ast.Name)
                and node.func.value.id == "json"
                and node.func.attr == "dumps"
            )

        class Visitor(ast.NodeVisitor):
            def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
                func_stack.append(node.name)
                if node.name in {"stdout_payload", "emit_stdout_json"}:
                    helpers.add(node.name)
                self.generic_visit(node)
                func_stack.pop()

            def visit_Call(self, node: ast.Call) -> None:
                nonlocal emit_calls_in_main
                if is_json_dumps(node):
                    dumps_in.append(func_stack[-1] if func_stack else None)
                if isinstance(node.func, ast.Name) and node.func.id == "print":
                    for arg in node.args:
                        if is_json_dumps(arg):
                            print_dumps_in.append(func_stack[-1] if func_stack else None)
                if (
                    isinstance(node.func, ast.Name)
                    and node.func.id == "emit_stdout_json"
                    and func_stack
                    and func_stack[-1] == "main"
                ):
                    emit_calls_in_main += 1
                self.generic_visit(node)

        Visitor().visit(tree)
        assert helpers == {"stdout_payload", "emit_stdout_json"}
        assert dumps_in == ["emit_marker", "emit_stdout_json"]
        assert print_dumps_in == ["emit_stdout_json"]
        assert emit_calls_in_main == 3

    def test_semantic_carry_forward_is_narrow_and_not_code_whitespace(self) -> None:
        text = (
            ROOT / "scripts/ci/check-pr-semantic-review-currentness.py"
        ).read_text(encoding="utf-8")
        assert '{".md", ".txt"}' in text
        assert "whitespace-insensitive prose file" in text
        assert "--ignore-all-space" in text
        assert "--ignore-blank-lines" in text
        # The prose exemption stops at a fence. Without this the same file class that
        # publishes agent-executable commands would carry a review forward across a
        # respaced command, which `--ignore-all-space` alone cannot see.
        assert "def fenced_blocks(" in text
        assert "post-review change alters fenced code content" in text

    def test_convergence_sanitizes_numeric_collector_facts(self) -> None:
        text = (ROOT / "scripts/ci/check-pr-review-convergence").read_text(
            encoding="utf-8"
        )
        assert '[[ "$value" =~ ^[0-9]+$ ]]' in text
        assert 'not_proven "invalid_numeric_review_fact"' in text
        # The count is derived by subtraction and then clamped, so a provider
        # that reports more dismissals than human reviews cannot drive the
        # published value negative. Pinned by the honest name since #15035;
        # the clamp is asserted alongside it so renaming the variable cannot
        # quietly drop the guard this test exists to hold.
        assert "NON_DISMISSED_LATEST_NONBOT_REVIEW_COUNT=$(" in text
        assert 'if [[ "$NON_DISMISSED_LATEST_NONBOT_REVIEW_COUNT" -lt 0 ]]' in text
        assert "NON_DISMISSED_LATEST_NONBOT_REVIEW_COUNT=0" in text

    def test_state_projection_has_no_exact_head_lifecycle(self) -> None:
        text = (ROOT / "scripts/reviews/state").read_text(encoding="utf-8")
        assert "FIXED_HEAD" not in text
        assert "VERIFIED_HEAD" not in text
        assert "REVIEW_IN_FLIGHT" not in text
        assert "REVIEW_PROTOCOL_ENFORCE" not in text
        assert "exact_head_review_required: false" in text

    def test_state_projection_inherits_the_fact_only_contract(self) -> None:
        """The projection must not resurrect the verdict its parent stopped making.

        `scripts/reviews/state` reads the same native facts as
        `check-pr-review-convergence`. Leaving the retired constant here would
        reintroduce the tautology the wrapper was fixed to remove, one layer down.
        """
        text = (ROOT / "scripts/reviews/state").read_text(encoding="utf-8")
        assert "semantic_changed_seam" not in text
        assert 'review_currentness: "NOT_PROVEN"' in text
        assert "semantic_currentness_required: true" in text
        assert 'STATE="NATIVE_FACTS_CONVERGED"' in text
        assert "SUBMITTED_REVIEW_PRESENT | NO_SUBMITTED_HUMAN_REVIEW" in text
        # The helper reports converged native facts; it never reports REVIEWED.
        assert 'STATE="REVIEWED"' not in text

    def test_state_projection_turns_every_abnormal_child_exit_into_not_proven(self) -> None:
        text = (ROOT / "scripts/reviews/state").read_text(encoding="utf-8")
        assert '[[ "$CLOSE_EXIT" -ge 2 ]]' in text
        assert 'state: "NOT_PROVEN"' in text
        assert 'reason: "invalid_closeout_output"' in text
        assert "child_exit" in text


class LaterCumulativeReviewTests(unittest.TestCase):
    """Exercise the real reducer and Git subject digest, not a duplicate model (#15010)."""

    @classmethod
    def setUpClass(cls) -> None:
        path = ROOT / "scripts/ci/check-pr-semantic-review-currentness.py"
        spec = importlib.util.spec_from_file_location("review_supersession_subject", path)
        assert spec and spec.loader
        cls.checker = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = cls.checker
        spec.loader.exec_module(cls.checker)

    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.git("init", "-q")
        self.git("config", "user.name", "Review fixture")
        self.git("config", "user.email", "review@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        (self.root / "source.rs").write_text("fn value() -> u8 { 1 }\n", encoding="utf-8")
        self.base = self.commit("base")
        (self.root / "source.rs").write_text("fn value() -> u8 { 2 }\n", encoding="utf-8")
        self.head = self.commit("candidate")

    def git(self, *args: str) -> str:
        return subprocess.run(
            ["git", *args], cwd=self.root, check=True, capture_output=True,
            text=True, encoding="utf-8", timeout=15,
        ).stdout.strip()

    def commit(self, message: str) -> str:
        self.git("add", ".")
        self.git("commit", "-q", "-m", message)
        return self.git("rev-parse", "HEAD")

    def record(self, result="REVIEW_CURRENT", second=1, **overrides):
        head = overrides.pop("commit_oid", self.head)
        sections = "\n\n".join(
            f"{heading}\n- {'no open findings' if heading == '## No material findings' else 'fixture evidence'}"
            for heading in (
                "## Review scope", "## Evidence and falsifiers",
                "## No material findings" if result == "REVIEW_CURRENT" else "## Findings",
                "## What this establishes", "## Residual risk / not proved",
            )
        )
        body = f"{sections}\n\n## Substantive review result\n- {result}\n"
        if result == "REVIEW_CURRENT":
            marker = {
                "head": head, "merge_base": self.base, "pr": 15010,
                "result": result,
                "subject_sha256": self.checker.subject_digest(self.root, self.base, head),
            }
            body += "\n<!-- semantic-review:v1 " + json.dumps(marker) + " -->\n"
        values = dict(
            login="reviewer", user_type="User", state="COMMENTED", body=body,
            commit_oid=head, submitted_at=f"2026-10-09T01:00:{second:02d}Z",
        )
        values.update(overrides)
        return self.checker.Review(**values)

    def verdict(self, *records, head=None):
        return self.checker.evaluate(
            self.root, pr=15010, current_head=head or self.head, reviews=iter(records)
        )

    def test_same_head_later_commented_changes_required_blocks(self):
        value = self.verdict(self.record(), self.record("CHANGES_REQUIRED", 2))
        self.assertEqual("NOT_PROVEN", value["classification"])
        self.assertEqual("later_cumulative_review_changes_required", value["reason"])

    def test_all_adverse_cumulative_outcomes_block(self):
        for result in ("NOT_PROVEN", "BLOCKED_BY_PREREQUISITE", "SUPERSEDED_OR_CLOSE"):
            with self.subTest(result=result):
                value = self.verdict(self.record(), self.record(result, 2))
                self.assertEqual("NOT_PROVEN", value["classification"])
                self.assertEqual("later_cumulative_review_" + result.lower(), value["reason"])

    def test_adverse_same_timestamp_wins_over_api_order(self):
        clean, adverse = self.record(), self.record("CHANGES_REQUIRED")
        for records in ((clean, adverse), (adverse, clean)):
            with self.subTest(order=records):
                self.assertEqual("NOT_PROVEN", self.verdict(*records)["classification"])

    def test_genuine_later_cumulative_rereview_can_restore_currentness(self):
        records = (self.record(), self.record("CHANGES_REQUIRED", 2), self.record(second=3))
        self.assertEqual("REVIEW_CURRENT", self.verdict(*reversed(records))["classification"])

    def test_repaired_head_requires_its_own_valid_positive_marker(self):
        clean, adverse = self.record(), self.record("CHANGES_REQUIRED", 2)
        (self.root / "source.rs").write_text("fn value() -> u8 { 3 }\n", encoding="utf-8")
        repaired = self.commit("repair")
        self.assertEqual("NOT_PROVEN", self.verdict(clean, adverse, head=repaired)["classification"])
        rereview = self.record(second=3, commit_oid=repaired)
        self.assertEqual("REVIEW_CURRENT", self.verdict(clean, adverse, rereview, head=repaired)["classification"])

    def test_unmarked_later_clean_claim_does_not_erase_adverse(self):
        clean, adverse = self.record(), self.record("CHANGES_REQUIRED", 2)
        unmarked = self.record(second=3)
        unmarked = unmarked._replace(body=unmarked.body.split("<!--")[0])
        self.assertEqual("NOT_PROVEN", self.verdict(clean, adverse, unmarked)["classification"])

    def test_bot_dismissed_and_pending_reviews_do_not_become_cumulative_verdicts(self):
        for override in ({"user_type": "Bot"}, {"state": "DISMISSED"}, {"state": "PENDING"}):
            with self.subTest(override=override):
                value = self.verdict(self.record(), self.record("CHANGES_REQUIRED", 2, **override))
                self.assertEqual("REVIEW_CURRENT", value["classification"])

    def test_ordinary_comment_is_not_an_adverse_cumulative_review(self):
        comment = self.record("CHANGES_REQUIRED", 2, body="Could this need CHANGES_REQUIRED later?")
        self.assertEqual("REVIEW_CURRENT", self.verdict(self.record(), comment)["classification"])

    def test_quoted_complete_review_is_not_a_live_declaration(self):
        quoted = self.record("CHANGES_REQUIRED", 2)
        quoted = quoted._replace(body="```markdown\n" + quoted.body + "```\n")
        self.assertEqual("REVIEW_CURRENT", self.verdict(self.record(), quoted)["classification"])

    def test_fenced_required_sections_cannot_pose_as_a_cumulative_review(self):
        quoted = self.record("CHANGES_REQUIRED", 2)
        prefix = quoted.body.split("## Substantive review result")[0]
        quoted = quoted._replace(body="```markdown\n" + prefix + "```\n## Substantive review result\n- CHANGES_REQUIRED\n")
        self.assertEqual("REVIEW_CURRENT", self.verdict(self.record(), quoted)["classification"])

    def test_conflicting_result_declaration_does_not_reuse_older_green(self):
        ambiguous = self.record("CHANGES_REQUIRED", 2)
        ambiguous = ambiguous._replace(body=ambiguous.body + "- REVIEW_CURRENT\n")
        value = self.verdict(self.record(), ambiguous)
        self.assertEqual("NOT_PROVEN", value["classification"])
        self.assertEqual("later_cumulative_review_ambiguous", value["reason"])

    def test_distinct_clean_reviews_and_replayed_publications_remain_valid(self):
        clean = self.record()
        self.assertEqual("REVIEW_CURRENT", self.verdict(clean, clean, self.record(second=2))["classification"])

    def test_timestamp_comparison_uses_instants_not_strings(self):
        # 20:00:02 -05:00 is later than 01:00:01Z on the following date.
        adverse = self.record("CHANGES_REQUIRED", submitted_at="2026-10-08T20:00:02-05:00")
        self.assertEqual("NOT_PROVEN", self.verdict(self.record(), adverse)["classification"])
        rereview = self.record(submitted_at="2026-10-08T20:00:03-05:00")
        self.assertEqual("REVIEW_CURRENT", self.verdict(self.record(), adverse, rereview)["classification"])

    def test_missing_time_or_identity_in_cumulative_review_is_instrument_failure(self):
        for override in ({"submitted_at": ""}, {"submitted_at": "2026-10-09T01:00:02"}, {"user_type": ""}, {"login": ""}, {"commit_oid": "not-a-sha"}):
            with self.subTest(override=override):
                with self.assertRaises(self.checker.CurrentnessError):
                    self.verdict(self.record(), self.record("CHANGES_REQUIRED", 2, **override))

    def test_cli_returns_nonzero_for_later_body_only_adverse(self):
        fixture = self.root / "reviews.json"
        fixture.write_text(json.dumps({"head": self.head, "reviews": [r._asdict() for r in (self.record(), self.record("CHANGES_REQUIRED", 2))]}), encoding="utf-8")
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = self.checker.main(["15010", "fixture/repo", "--root", str(self.root), "--fixture", str(fixture)])
        self.assertEqual(1, code)
        value = json.loads(output.getvalue())
        self.assertEqual("semantic_review_currentness.v1", value["schema_version"])
        self.assertEqual("NOT_PROVEN", value["classification"])

    def test_pending_positive_cannot_override_published_adverse(self):
        pending = self.record(second=3, state="PENDING")
        value = self.verdict(self.record(), self.record("CHANGES_REQUIRED", 2), pending)
        self.assertEqual("NOT_PROVEN", value["classification"])

    def test_quoted_positive_sections_cannot_override_published_adverse(self):
        forged = self.record(second=3)
        prefix, rest = forged.body.split("## Substantive review result", 1)
        forged = forged._replace(body="```markdown\n" + prefix + "```\n## Substantive review result" + rest)
        value = self.verdict(self.record(), self.record("CHANGES_REQUIRED", 2), forged)
        self.assertEqual("NOT_PROVEN", value["classification"])

    def test_proven_older_invalid_identity_does_not_block_later_valid_review(self):
        for override in ({"login": ""}, {"user_type": ""}, {"user_type": "Ghost"},
                         {"commit_oid": "not-a-sha"}, {"state": "UNKNOWN"}):
            with self.subTest(override=override):
                historical = self.record("CHANGES_REQUIRED", 1, **override)
                current = self.record(second=2)
                self.assertEqual("REVIEW_CURRENT", self.verdict(historical, current)["classification"])

    def test_equal_time_invalid_identity_still_fails_closed(self):
        for override in ({"login": ""}, {"user_type": "Ghost"}, {"commit_oid": "bad"}):
            with self.subTest(override=override):
                with self.assertRaises(self.checker.CurrentnessError):
                    self.verdict(self.record(), self.record("CHANGES_REQUIRED", **override))

    def test_unorderable_historical_claim_is_not_proven_older(self):
        # A timestamp failure cannot establish the old-record exemption.
        historical = self.record("CHANGES_REQUIRED", submitted_at="not-a-timestamp")
        with self.assertRaises(self.checker.CurrentnessError):
            self.verdict(historical, self.record(second=2))

    def test_no_review_stays_not_proven(self):
        self.assertEqual("NOT_PROVEN", self.verdict()["classification"])


if __name__ == "__main__":
    unittest.main()
