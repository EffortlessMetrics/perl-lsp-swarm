"""Contract tests for the exact-subject PR title check (#15351).

These tests execute the real inline github-script body from
``.github/workflows/pr-title-check.yml`` under Node with stubbed
``context``/``github``/``core`` objects, then assert the fail-closed
exact-subject binding and the enforced issue-link policy.

Covered discriminating cases (from issue #15351's test list):

- pull_request_target validates the exact payload number/head/title.
- workflow_dispatch requires explicit ``pr_number`` and ``expected_head_sha``.
- Zero matches, closed PR, moved head, and edited title fail distinctly.
- A no-PR dispatch cannot produce a successful required context, and
  branch-name discovery (``pulls.list``) is never consulted for authority.
- Missing same-repository non-zero issue references fail rather than warn.
- Placeholder ``(#0000)`` label-add failure is non-success.
- Receipt outputs bind pr number, head SHA, and title digest on success.
"""

from __future__ import annotations

import json
import hashlib
import re
import subprocess
import tempfile
from pathlib import Path

import yaml

WORKFLOW = (
    Path(__file__).resolve().parents[1] / ".github" / "workflows" / "pr-title-check.yml"
)

RUNNER_TEMPLATE = r"""
const fs = require('fs');
const script = fs.readFileSync(process.argv[2], 'utf8');
const scenarios = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
const results = {};

function normSha(sha) { return String(sha || '').toLowerCase(); }

async function runScenario(sc) {
  const calls = [];
  const state = { failed: null, outputs: {} };
  const core = {
    info: () => {},
    notice: () => {},
    warning: (m) => calls.push('warning:' + m),
    setFailed: (m) => { state.failed = m; calls.push('setFailed:' + m); },
    setOutput: (k, v) => { state.outputs[k] = String(v); },
  };

  let pullsGetCalls = 0;
  const prHandler = (table) => (params) => {
    calls.push('pulls.get:' + JSON.stringify(params));
    pullsGetCalls += 1;
    const expectedNumber = table.expectedNumber || (table.pr && table.pr.number);
    if (params.pull_number !== expectedNumber) {
      throw Object.assign(new Error('unexpected pull number'), { status: 400 });
    }
    if (pullsGetCalls === 2 && table.reread) { return { data: table.reread }; }
    return { data: table.pr };
  };
  const issueHandler = (table) => (params) => {
    calls.push('issues.get:' + JSON.stringify(params));
    if (table.issueMissing) {
      throw Object.assign(new Error('Not Found'), { status: 404 });
    }
    return { data: { number: params.issue_number } };
  };
  const addLabelsHandler = (table) => (params) => {
    calls.push('addLabels:' + JSON.stringify(params));
    if (table.addLabelsFails) {
      throw Object.assign(new Error('Resource not accessible by integration'), { status: 403 });
    }
    return { data: [] };
  };
  const removeLabelHandler = (table) => (params) => {
    calls.push('removeLabel:' + JSON.stringify(params));
    if (table.removeLabelFails) {
      throw Object.assign(new Error('Resource not accessible by integration'), { status: 403 });
    }
    return { data: [] };
  };

  const table = sc.table || {};
  const github = {
    rest: {
      pulls: {
        get: prHandler(table),
        list: async (params) => {
          calls.push('pulls.list:' + JSON.stringify(params));
          return { data: [] };
        },
      },
      issues: {
        get: issueHandler(table),
        addLabels: addLabelsHandler(table),
        removeLabel: removeLabelHandler(table),
        listComments: async () => ({ data: [] }),
        createComment: async () => ({ data: {} }),
        updateComment: async () => ({ data: {} }),
      },
    },
  };

  const context = {
    eventName: sc.event,
    actor: sc.actor || 'someone',
    repo: { owner: 'o', repo: 'r' },
    ref: sc.ref || 'refs/heads/some-branch',
    payload: sc.payload || {},
  };

  const prevEnv = process.env.PR_NUMBER_INPUT;
  const prevSha = process.env.EXPECTED_HEAD_SHA_INPUT;
  process.env.PR_NUMBER_INPUT = (sc.env && sc.env.pr_number) || '';
  process.env.EXPECTED_HEAD_SHA_INPUT = (sc.env && sc.env.expected_head_sha) || '';

  const fn = new Function(
    'require', 'context', 'github', 'core',
    'return (async () => {\n' + script + '\n})()'
  );
  try {
    await fn(require, context, github, core);
  } catch (e) {
    state.thrown = String(e && e.message);
  } finally {
    process.env.PR_NUMBER_INPUT = prevEnv;
    process.env.EXPECTED_HEAD_SHA_INPUT = prevSha;
  }

  return {
    failed: state.failed,
    thrown: state.thrown || null,
    outputs: state.outputs,
    pullsListCalled: calls.some((c) => c.startsWith('pulls.list:')),
    calls,
  };
}

(async () => {
  for (const sc of scenarios) {
    results[sc.name] = await runScenario(sc);
  }
  fs.writeFileSync(process.argv[4], JSON.stringify(results, null, 2));
})();
"""


def _extract_script(workflow: dict) -> str:
    step = workflow["jobs"]["validate-title"]["steps"][0]
    assert step["uses"].startswith("actions/github-script@"), step.get("uses")
    return step["with"]["script"]


def _run_scenarios(script: str, scenarios: list[dict]) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        runner = tmpdir / "runner.js"
        payload = tmpdir / "scenarios.json"
        out = tmpdir / "results.json"
        runner.write_text(RUNNER_TEMPLATE, encoding="utf-8")
        payload.write_text(json.dumps(scenarios), encoding="utf-8")
        script_file = tmpdir / "script.js"
        script_file.write_text(script, encoding="utf-8")
        proc = subprocess.run(
            ["node", str(runner), str(script_file), str(payload), str(out)],
            capture_output=True,
            text=True,
            timeout=120,
            cwd=tmp,
        )
        if proc.returncode != 0:
            raise AssertionError(
                f"node runner failed: rc={proc.returncode}\nstdout={proc.stdout}\nstderr={proc.stderr}"
            )
        return json.loads(out.read_text(encoding="utf-8"))


# Shared fixture data ---------------------------------------------------------

SHA = "a" * 40
OTHER_SHA = "b" * 40


def pr_payload(number: int = 1200, sha: str = SHA, title: str | None = None) -> dict:
    return {
        "number": number,
        "state": "open",
        "title": title if title is not None else "fix: thing (#1200)",
        "user": {"login": "someone"},
        "labels": [],
        "head": {"sha": sha},
    }


def base_scenarios() -> list[dict]:
    return [
        {
            "name": "pr_event_exact_payload_passes",
            "event": "pull_request_target",
            "payload": {"pull_request": pr_payload()},
            "table": {"pr": pr_payload(), "expectedNumber": 1200},
        },
        {
            "name": "dispatch_exact_pair_passes",
            "event": "workflow_dispatch",
            "env": {"pr_number": "1200", "expected_head_sha": SHA},
            "table": {"pr": pr_payload()},
        },
        {
            "name": "dispatch_missing_pr_number_fails",
            "event": "workflow_dispatch",
            "env": {"pr_number": "", "expected_head_sha": SHA},
            "table": {"pr": pr_payload()},
        },
        {
            "name": "dispatch_missing_head_sha_fails",
            "event": "workflow_dispatch",
            "env": {"pr_number": "1200", "expected_head_sha": ""},
            "table": {"pr": pr_payload()},
        },
        {
            "name": "dispatch_head_mismatch_fails",
            "event": "workflow_dispatch",
            "env": {"pr_number": "1200", "expected_head_sha": OTHER_SHA},
            "table": {"pr": pr_payload()},
        },
        {
            "name": "closed_pr_fails",
            "event": "pull_request_target",
            "payload": {"pull_request": pr_payload()},
            "table": {"pr": {**pr_payload(), "state": "closed"}},
        },
        {
            "name": "missing_issue_reference_fails",
            "event": "pull_request_target",
            "payload": {"pull_request": pr_payload(title="fix: no reference at all")},
            "table": {"pr": pr_payload(title="fix: no reference at all")},
        },
        {
            "name": "nonexistent_nonzero_reference_fails",
            "event": "pull_request_target",
            "payload": {"pull_request": pr_payload(title="fix: thing (#999999999)")},
            "table": {
                "pr": pr_payload(title="fix: thing (#999999999)"),
                "issueMissing": True,
            },
        },
        {
            "name": "placeholder_label_add_failure_fails",
            "event": "pull_request_target",
            "payload": {"pull_request": pr_payload(title="fix: thing (#0000)")},
            "table": {
                "pr": pr_payload(title="fix: thing (#0000)"),
                "addLabelsFails": True,
            },
        },
        {
            "name": "placeholder_records_obligation_and_passes",
            "event": "pull_request_target",
            "payload": {"pull_request": pr_payload(title="fix: thing (#0000)")},
            "table": {"pr": pr_payload(title="fix: thing (#0000)")},
        },
        {
            "name": "stale_needs_issue_link_cleared_on_real_reference",
            "event": "pull_request_target",
            "payload": {"pull_request": {**pr_payload(), "labels": [{"name": "needs-issue-link"}]}},
            "table": {"pr": {**pr_payload(), "labels": [{"name": "needs-issue-link"}]}},
        },
        {
            "name": "label_removal_failure_fails",
            "event": "pull_request_target",
            "payload": {"pull_request": {**pr_payload(), "labels": [{"name": "needs-issue-link"}]}},
            "table": {
                "pr": {**pr_payload(), "labels": [{"name": "needs-issue-link"}]},
                "removeLabelFails": True,
            },
        },
        {
            "name": "title_edited_during_validation_fails",
            "event": "pull_request_target",
            "payload": {"pull_request": pr_payload()},
            "table": {
                "pr": pr_payload(),
                "reread": {**pr_payload(), "title": "fix: thing, retitled (#1200)"},
            },
        },
        {
            "name": "head_moved_during_validation_fails",
            "event": "pull_request_target",
            "payload": {"pull_request": pr_payload()},
            "table": {
                "pr": pr_payload(),
                "reread": {**pr_payload(), "head": {"sha": OTHER_SHA}},
            },
        },
    ]


def test_workflow_dispatch_inputs_are_required() -> None:
    workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
    inputs = workflow[True]["workflow_dispatch"]["inputs"]
    assert inputs["pr_number"]["required"] is True
    assert inputs["expected_head_sha"]["required"] is True


def test_script_never_uses_branch_discovery_for_authority() -> None:
    script = _extract_script(yaml.safe_load(WORKFLOW.read_text(encoding="utf-8")))
    assert "pulls.list" not in script
    assert "pulls.get" in script
    assert "expected_head_sha" in script


def test_inline_script_behavior() -> None:
    workflow = yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))
    script = _extract_script(workflow)
    results = _run_scenarios(script, base_scenarios())

    def expect_pass(name: str) -> None:
        r = results[name]
        assert not r["failed"], f"{name} unexpectedly failed: {r['failed']}"

    def expect_fail(name: str, needle: str) -> None:
        r = results[name]
        assert r["failed"], f"{name} unexpectedly passed (outputs={r['outputs']})"
        assert needle in r["failed"], f"{name}: message lacks {needle!r}: {r['failed']}"

    expect_pass("pr_event_exact_payload_passes")
    expect_pass("dispatch_exact_pair_passes")
    expect_fail(
        "dispatch_missing_pr_number_fails", "requires an explicit pr_number input"
    )
    expect_fail(
        "dispatch_missing_head_sha_fails", "requires an explicit expected_head_sha input"
    )
    expect_fail("dispatch_head_mismatch_fails", "Head mismatch")
    expect_fail("closed_pr_fails", "only an open PR can be title-validated")
    expect_fail("missing_issue_reference_fails", "must reference an issue")
    expect_fail("nonexistent_nonzero_reference_fails", "#999999999 does not resolve")
    expect_fail(
        "placeholder_label_add_failure_fails",
        "needs-issue-link obligation",
    )
    expect_pass("placeholder_records_obligation_and_passes")
    expect_pass("stale_needs_issue_link_cleared_on_real_reference")
    expect_fail("label_removal_failure_fails", "needs-issue-link obligation")
    expect_fail("title_edited_during_validation_fails", "title changed during validation")
    expect_fail("head_moved_during_validation_fails", "head moved during validation")

    # No scenario may consult branch discovery; the dispatch that would find
    # zero PRs can never emit success.
    for name, r in results.items():
        assert not r["pullsListCalled"], f"{name} consulted pulls.list"
        assert not r["thrown"], f"{name} escaped via exception: {r['thrown']}"

    # Success receipts bind the exact subject: pr number, head SHA, title digest.
    ok = results["dispatch_exact_pair_passes"]
    assert ok["outputs"]["pr_number"] == "1200"
    assert ok["outputs"]["head_sha"] == SHA
    expected_title_digest = hashlib.sha256(
        pr_payload()["title"].encode("utf-8")
    ).hexdigest()
    assert ok["outputs"]["title_sha256"] == expected_title_digest
    assert ok["outputs"]["disposition"] == "real-issue-reference"

    # The placeholder transaction recorded its obligation before passing.
    placeholder = results["placeholder_records_obligation_and_passes"]
    assert placeholder["outputs"]["disposition"] == "placeholder-needs-issue-link"
    assert any(
        c.startswith("addLabels:") and "needs-issue-link" in c for c in placeholder["calls"]
    )

    # Stale backstop label is actively reconciled on real reference.
    cleared = results["stale_needs_issue_link_cleared_on_real_reference"]
    assert any(c.startswith("removeLabel:") for c in cleared["calls"])

    # Exact-subject reads go through pulls.get by number.
    read = results["dispatch_exact_pair_passes"]["calls"]
    assert any("pull_number" in c and "1200" in c for c in read)
