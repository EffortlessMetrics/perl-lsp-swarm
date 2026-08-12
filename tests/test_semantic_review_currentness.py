from __future__ import annotations

import re
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


def test_active_review_surfaces_do_not_invoke_exact_head_receipts() -> None:
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


def test_review_skills_require_useful_durable_records() -> None:
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


def test_roots_define_semantic_review_currentness() -> None:
    for relative in ("AGENTS.md", "CLAUDE.md"):
        text = (ROOT / relative).read_text(encoding="utf-8")
        assert "A later commit does not invalidate review" in text
        assert "merely because the SHA changed" in text
        assert "Do not post `Review pass (...) at" in text


def test_merge_skill_keeps_expected_head_race_protection() -> None:
    for relative in (
        ".agents/skills/merge-reconcile/SKILL.md",
        ".claude/skills/merge-reconcile/SKILL.md",
    ):
        text = (ROOT / relative).read_text(encoding="utf-8")
        assert "--match-head-commit" in text
        assert "compare-and-swap" in text
        assert "does not make review validity depend on the SHA" in text


def test_orchestration_briefs_are_semantic_not_claim_hashed() -> None:
    for relative in (
        ".agents/skills/orchestrate-work/SKILL.md",
        ".claude/skills/orchestrate-work/SKILL.md",
    ):
        text = (ROOT / relative).read_text(encoding="utf-8")
        assert "reviewed semantic seams" in text
        assert "Do not include a claim digest" in text


def test_pre_merge_preserves_native_disposition_validation() -> None:
    text = (ROOT / "scripts/pre-merge-check.sh").read_text(encoding="utf-8")
    assert "check-pr-review-convergence" in text
    assert "native_review_facts_converged" in text
    assert "semantic_currentness_required" in text


def test_pre_merge_requires_subject_bound_semantic_review() -> None:
    text = (ROOT / "scripts/pre-merge-check.sh").read_text(encoding="utf-8")
    assert "check-pr-semantic-review-currentness.py" in text
    assert 'SEMANTIC_CLASS" != "REVIEW_CURRENT"' in text
    assert "substantive review is" in text


def test_legacy_review_writer_is_inert() -> None:
    text = (ROOT / "scripts/reviews/run").read_text(encoding="utf-8")
    assert "RETIRED:" in text
    assert "exit 2" in text
    assert "gh api" not in text
    assert "gh pr view" not in text
    assert "claim-digest" not in text
    assert "review-run:v1" not in text


def test_native_convergence_is_fact_only() -> None:
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


def test_subject_bound_checker_requires_durable_review_record() -> None:
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


def test_semantic_carry_forward_is_narrow_and_not_code_whitespace() -> None:
    text = (
        ROOT / "scripts/ci/check-pr-semantic-review-currentness.py"
    ).read_text(encoding="utf-8")
    assert '{".md", ".txt"}' in text
    assert "whitespace-insensitive prose file" in text
    assert "--ignore-all-space" in text
    assert "--ignore-blank-lines" in text


def test_convergence_sanitizes_numeric_collector_facts() -> None:
    text = (ROOT / "scripts/ci/check-pr-review-convergence").read_text(
        encoding="utf-8"
    )
    assert '[[ "$value" =~ ^[0-9]+$ ]]' in text
    assert 'not_proven "invalid_numeric_review_fact"' in text
    assert "SUBMITTED_HUMAN_REVIEW_COUNT=$(" in text


def test_state_projection_has_no_exact_head_lifecycle() -> None:
    text = (ROOT / "scripts/reviews/state").read_text(encoding="utf-8")
    assert "FIXED_HEAD" not in text
    assert "VERIFIED_HEAD" not in text
    assert "REVIEW_IN_FLIGHT" not in text
    assert "REVIEW_PROTOCOL_ENFORCE" not in text
    assert 'review_currentness: "semantic_changed_seam"' in text
    assert "exact_head_review_required: false" in text


def test_state_projection_turns_every_abnormal_child_exit_into_not_proven() -> None:
    text = (ROOT / "scripts/reviews/state").read_text(encoding="utf-8")
    assert '[[ "$CLOSE_EXIT" -ge 2 ]]' in text
    assert 'state: "NOT_PROVEN"' in text
    assert 'reason: "invalid_closeout_output"' in text
    assert "child_exit" in text
