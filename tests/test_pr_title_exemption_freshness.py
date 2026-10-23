"""Exercise mutable exemption freshness in the actual title workflow (#15351).

Reuse the existing inline-script harness; no new policy or GitHub transport.
These cases establish the validation-time re-read, not ongoing invalidation of
an already published result or merge-group admission equivalence.
"""
from __future__ import annotations

import yaml

from test_pr_title_check_contract import (
    OTHER_SHA,
    WORKFLOW,
    _extract_script,
    _run_scenarios,
    pr_payload,
)


def test_skip_title_exemption_requires_current_label() -> None:
    pr = {
        **pr_payload(title="chore: maintainer exception"),
        "labels": [{"name": "skip-title-check"}],
    }
    cases = {
        "unchanged": (pr, True),
        "unrelated_label_added": (
            {**pr, "labels": pr["labels"] + [{"name": "area:ci"}]}, True
        ),
        "exemption_removed": ({**pr, "labels": []}, False),
        "labels_missing": ({k: v for k, v in pr.items() if k != "labels"}, False),
        "labels_malformed": ({**pr, "labels": None}, False),
        "head_moved": ({**pr, "head": {"sha": OTHER_SHA}}, False),
        "title_moved": ({**pr, "title": "chore: a different exception"}, False),
    }
    script = _extract_script(yaml.safe_load(WORKFLOW.read_text(encoding="utf-8")))
    results = _run_scenarios(script, [
        {
            "name": name,
            "event": "pull_request_target",
            "payload": {"pull_request": pr},
            "table": {"pr": pr, "reread": reread},
        }
        for name, (reread, _) in cases.items()
    ])
    assert set(results) == set(cases), "Every named case must execute"
    for name, (_, should_pass) in cases.items():
        result = results[name]
        assert result["thrown"] is None, (name, result)
        if should_pass:
            assert result["failed"] is None, (name, result)
            assert result["outputs"]["disposition"] == "skip-title-check-exempt"
            assert result["outputs"]["head_sha"] == pr["head"]["sha"]
        else:
            assert result["failed"], (name, "invalid exemption admitted", result)
            assert result["outputs"] == {}, (name, "failure emitted a success receipt")
    for name in ("exemption_removed", "labels_missing", "labels_malformed"):
        assert "skip-title-check" in results[name]["failed"], (name, results[name])
