from __future__ import annotations

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

ACTIVE_REVIEW_SURFACES = [
    ROOT / ".agents/skills/review-pr/SKILL.md",
    ROOT / ".agents/skills/final-challenge/SKILL.md",
    ROOT / ".agents/skills/finish-pr/SKILL.md",
    ROOT / ".agents/skills/verify-live-ci/SKILL.md",
    ROOT / ".agents/skills/merge-reconcile/SKILL.md",
    ROOT / ".claude/skills/review-pr/SKILL.md",
    ROOT / ".claude/skills/final-challenge/SKILL.md",
    ROOT / ".claude/skills/finish-pr/SKILL.md",
    ROOT / ".claude/skills/verify-live-ci/SKILL.md",
    ROOT / ".claude/skills/merge-reconcile/SKILL.md",
    ROOT / "scripts/pre-merge-check.sh",
]

RETIRED_ACTIVE_TOKENS = [
    "scripts/reviews/run review-start",
    "scripts/reviews/run review-done",
    "scripts/reviews/claim-digest",
    "REVIEW_PROTOCOL_ENFORCE=1",
]


def test_active_review_surfaces_do_not_require_exact_head_receipts() -> None:
    violations: list[str] = []
    for path in ACTIVE_REVIEW_SURFACES:
        text = path.read_text(encoding="utf-8")
        for token in RETIRED_ACTIVE_TOKENS:
            if token in text:
                violations.append(f"{path.relative_to(ROOT)}: {token}")

    assert not violations, "retired review receipt protocol remains active:\n" + "\n".join(
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
