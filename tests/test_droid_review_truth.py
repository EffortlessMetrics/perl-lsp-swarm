from __future__ import annotations

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/droid-review.yml"


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


def test_action_success_is_not_review_success() -> None:
    text = workflow_text()

    assert '"action_outcome": action_outcome' in text
    assert '"semantic_result": semantic_result' in text
    assert '"review_posted": None' in text
    assert '"publication_verified": False' in text
    assert '"verdict": "warn"' in text
    assert "review_posted=true" not in text
    assert "verdict=pass" not in text


def test_empty_legacy_candidate_set_is_not_clean_evidence() -> None:
    text = workflow_text()

    assert "legacy_validator_cannot_prove_independent_clean_review" in text
    assert 'declared_result in {"clean", "findings", "not_proven", "stale"}' in text
    assert 'semantic_result = "findings"' in text
    assert 'semantic_result = "not_proven"' in text
    assert '"candidate_count": len(results)' in text
    assert '"approved_candidate_count": approved_candidates' in text
    assert '"independent_finding_count": independent_findings' in text


def test_receipt_distinguishes_analysis_from_publication() -> None:
    text = workflow_text()

    assert '"review_body_declared": bool(review_body.strip())' in text
    assert '"validated_artifact_present": artifact is not None' in text
    assert '"not_proven_reason": not_proven_reason' in text
    assert '"automatic_full_review_on_synchronize": False' in text
    assert '"semantic_review_cancel_in_progress": False' in text
