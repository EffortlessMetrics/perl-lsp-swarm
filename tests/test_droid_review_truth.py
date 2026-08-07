from __future__ import annotations

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/droid-review.yml"
STACKED_DROID_SHA = "b324e7b416fddb19581831ad3b043c0ee953d526"


def workflow_text() -> str:
    return WORKFLOW.read_text(encoding="utf-8")


def test_semantic_review_is_not_cancelled_or_restarted_on_every_push() -> None:
    text = workflow_text()

    assert "types: [opened, reopened, ready_for_review]" in text
    assert "types: [opened, synchronize, reopened, ready_for_review]" not in text
    assert "cancel-in-progress: false" in text
    assert "cancel-in-progress: true" not in text
    assert "review_depth: deep" in text
    assert "review_depth: shallow" not in text


def test_upstream_review_and_outcome_actions_are_pinned_together() -> None:
    text = workflow_text()

    assert (
        f"uses: EffortlessMetrics/droid-action-safe@{STACKED_DROID_SHA}" in text
    )
    assert (
        "uses: EffortlessMetrics/droid-action-safe/review-outcome@"
        f"{STACKED_DROID_SHA}" in text
    )
    assert "expected-head: ${{ github.event.pull_request.head.sha }}" in text
    assert "validated-path: ${{ runner.temp }}/droid-prompts/review_validated.json" in text


def test_action_success_is_not_review_success() -> None:
    text = workflow_text()

    assert '"action_outcome": os.environ.get("DROID_REVIEW_OUTCOME", "unknown")' in text
    assert '"review_result": os.environ.get("REVIEW_RESULT") or "not_proven"' in text
    assert (
        '"publication_result": os.environ.get("PUBLICATION_RESULT") or "not_proven"'
        in text
    )
    assert '"verdict": "warn"' in text
    assert "review_posted=true" not in text
    assert "verdict=pass" not in text


def test_receipt_uses_verified_publication_and_semantic_counts() -> None:
    text = workflow_text()

    assert '"candidate_count": as_int("CANDIDATE_COUNT")' in text
    assert '"validated_count": as_int("VALIDATED_COUNT")' in text
    assert '"approved_inline_count": as_int("APPROVED_INLINE_COUNT")' in text
    assert (
        '"independent_finding_count": as_int("INDEPENDENT_FINDING_COUNT")'
        in text
    )
    assert '"review_body_submitted": as_bool("REVIEW_BODY_SUBMITTED")' in text
    assert '"publication_verified": as_bool("REVIEW_BODY_SUBMITTED")' in text
    assert '"not_proven_reason": os.environ.get("NOT_PROVEN_REASON") or None' in text


def test_receipt_preserves_non_churn_contract() -> None:
    text = workflow_text()

    assert '"automatic_full_review_on_synchronize": False' in text
    assert '"semantic_review_cancel_in_progress": False' in text
    assert "review running" not in text.lower()
    assert "review done" not in text.lower()
