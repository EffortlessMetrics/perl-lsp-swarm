#!/usr/bin/env bash
set -euo pipefail

# The control-plane contracts run in-session; this retained explicit diagnostic
# must never turn Draft/Ready or main publication into an Actions run. Execute
# the real publication contract and preserve its failure propagation.

workflow='.github/workflows/agent-flow-control-plane.yml'
test -f "$workflow"

python3 - "$workflow" <<'PY'
from __future__ import annotations

import sys
import re
from pathlib import Path


WORKFLOW = Path(sys.argv[1])
REQUIRED_COMMAND = (
    "cargo test -p xtask --test shift_left_publication_contract --locked"
)

def publication_runs(text: str) -> list[str]:
    """`run:` bodies of steps named for the publication contract.

    Line-based on purpose: this fixture bootstraps the Rust ratchet, so it must
    not assume the ratchet's own YAML parser is ever reached. Comment lines are
    skipped rather than accepted, so a command parked in a comment yields no
    run body at all.
    """
    lines = text.splitlines()
    marker = "- name: Check shift-left publication contract"
    bodies: list[str] = []
    for index, line in enumerate(lines):
        if line.strip() != marker:
            continue
        for follower in lines[index + 1 :]:
            stripped = follower.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if stripped.startswith("run:"):
                bodies.append(stripped[len("run:") :].strip())
            break
    return bodies


def validate(text: str) -> None:
    # Executable placement, not textual presence: counting raw occurrences
    # accepts the command inside a YAML comment and accepts a failure-swallowing
    # wrapper such as `... --locked || true`, either of which stops the Rust
    # ratchet running while leaving this bootstrap check green.
    runs = publication_runs(text)
    assert len(runs) == 1, (
        "agent-flow workflow must execute the shift-left publication contract "
        f"in exactly one named step, found {len(runs)}"
    )
    assert runs[0] == REQUIRED_COMMAND, (
        "the publication-contract step must run the ratchet exactly, found "
        f"{runs[0]!r}"
    )
    assert text.count(REQUIRED_COMMAND) == 1, (
        "agent-flow workflow must execute the shift-left publication contract exactly once"
    )
    on = re.search(r"(?m)^on:\n((?:[ \t].*\n|\n)*)", text)
    assert on, "explicit event mapping missing"
    events = set(re.findall(r"(?m)^  ([A-Za-z_]+):", on.group(1)))
    assert events == {"workflow_dispatch"}, f"ordinary control-plane Actions restored: {events}"



source = WORKFLOW.read_text(encoding="utf-8")
validate(source)

for event in ("pull_request", "pull_request_target", "pull_request_review",
              "pull_request_review_comment", "issue_comment", "push"):
    mutated = source.replace("on:\n", f"on:\n  {event}: {{}}\n", 1)
    assert mutated != source
    try:
        validate(mutated)
    except AssertionError:
        pass
    else:
        raise AssertionError(f"restoring ordinary {event} must fail the contract")

without_publication_check = source.replace(REQUIRED_COMMAND, "true", 1)
assert without_publication_check != source, "workflow-command mutation fixture must apply"
try:
    validate(without_publication_check)
except AssertionError:
    pass
else:
    raise AssertionError("removing the publication-contract command must fail the contract")

# The two bypasses a raw substring count accepts. Both keep the command present
# in the file while stopping it from enforcing anything.
commented_out = source.replace(
    f"run: {REQUIRED_COMMAND}", f"# run: {REQUIRED_COMMAND}", 1
)
assert commented_out != source, "comment-out mutation fixture must apply"
assert commented_out.count(REQUIRED_COMMAND) == 1, (
    "the comment-out fixture must keep the command textually present"
)
try:
    validate(commented_out)
except AssertionError:
    pass
else:
    raise AssertionError(
        "a publication-contract command parked in a comment must fail the contract"
    )

failure_swallowed = source.replace(
    f"run: {REQUIRED_COMMAND}", f"run: {REQUIRED_COMMAND} || true", 1
)
assert failure_swallowed != source, "failure-swallowing mutation fixture must apply"
assert failure_swallowed.count(REQUIRED_COMMAND) == 1, (
    "the failure-swallowing fixture must keep the command textually present"
)
try:
    validate(failure_swallowed)
except AssertionError:
    pass
else:
    raise AssertionError(
        "a publication-contract command wrapped to swallow failure must fail the contract"
    )

print("agent-flow control-plane trigger fixtures passed")
PY
