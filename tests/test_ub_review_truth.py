from __future__ import annotations

import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/ub-review.yml"
POLICY = ROOT / "policy/ub-review.toml"


def workflow_text() -> str:
    return WORKFLOW.read_text(encoding="utf-8")


def test_semantic_review_is_not_cancelled_or_restarted_on_every_push() -> None:
    text = workflow_text()

    assert "types: [opened, reopened, ready_for_review]" in text
    assert "types: [opened, synchronize, reopened, ready_for_review]" not in text
    assert "cancel-in-progress: false" in text
    assert "cancel-in-progress: ${{" not in text


def test_workflow_keeps_the_lane_advisory_but_post_failures_visible() -> None:
    text = workflow_text()

    assert "continue-on-error: true" in text
    assert "fail-on-post-error: 'true'" in text
    assert "fail-on-gate: 'true'" in text
    assert "UB Review Advisory on GitHub Hosted" in text
    assert "UB Review Advisory on CX53" not in text
    assert "UB Review Advisory on CX43" not in text


def test_consumer_receipt_separates_analysis_publication_and_gate() -> None:
    text = workflow_text()

    assert '"analysis_result": analysis_result' in text
    assert '"publication_result": publication_result' in text
    assert '"gate_result": gate_result' in text
    assert '"overall_result": overall_result' in text
    assert "terminal_state_sufficient_but_evidence_gaps_present" in text
    assert 'analysis_result = "limited"' in text
    assert 'overall_result = "not_proven"' in text
    assert "review_payload_prepared_without_post_receipt" in text
    assert "post_error_receipt_present" in text


def test_proof_profile_is_live_and_duplicate_ripr_is_disabled() -> None:
    policy = tomllib.loads(POLICY.read_text(encoding="utf-8"))

    assert policy["profile"] == "gh-runner-proof"
    assert policy["tools"]["ripr"]["enabled"] is False
    assert policy["profiles"]["gh-runner-proof"]["budgets"][
        "proof_max_focused_tests"
    ] == 1
    required = policy["proof"]["required"]
    assert len(required) == 1
    assert required[0]["command"] == (
        "cargo check --package perl-core-test-runner --locked"
    )


def test_truth_receipt_never_turns_action_success_into_clean_by_itself() -> None:
    text = workflow_text()

    assert 'if action_outcome != "success":' in text
    assert 'elif analysis_result in {"not_proven", "limited"}:' in text
    assert 'elif publication_result in {"failed", "not_proven"}:' in text
    assert 'elif gate_result in {"not_proven", "inconclusive"}:' in text
    assert "action_outcome == \"success\"" not in text
    assert "verdict=pass" not in text
