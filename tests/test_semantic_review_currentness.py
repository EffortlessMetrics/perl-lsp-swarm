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


def test_pre_merge_preserves_disposition_validation() -> None:
    text = (ROOT / "scripts/pre-merge-check.sh").read_text(encoding="utf-8")
    assert "resolved_without_disposition" in text
    assert "unresolved_total" in text
    assert "current_change_requests" in text
    assert "pending_reviewers" in text
