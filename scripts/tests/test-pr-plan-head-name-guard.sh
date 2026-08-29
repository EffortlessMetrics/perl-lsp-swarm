#!/usr/bin/env bash
set -euo pipefail

# Self-test for the PR Plan Trigger Guard workflow (#6238).
#
# The guard runs on pull_request to surface GitHub's documented SHA-like
# head-branch suppression of pull_request_target events. This script proves,
# against the checked-in workflow text only:
#
# 1. the structural contract: unsuppressed pull_request trigger, per-PR
#    concurrency, least privilege, env-indirect payload access, no checkout /
#    actions / secrets / write scopes;
# 2. the classifier boundary matrix, using the exact regex extracted from the
#    shipped workflow so the test cannot drift from production logic;
# 3. documentation consistency: the inventory row and the pr-plan trigger-model
#    section stay attached to the guard.
#
# Live suppression of the published matcher itself remains a live-fire question
# tracked on #6238; this fixture proves the shipped artifact, not GitHub.

workflow='.github/workflows/pr-plan-head-name-guard.yml'
docs_trigger='docs/ci/pr-plan.md'
docs_inventory='docs/ci/inventory.md'
test -f "$workflow"
test -f "$docs_trigger"
test -f "$docs_inventory"

reject_re="$(python3 - "$workflow" <<'PY'
from __future__ import annotations

import re
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text(encoding="utf-8")


def die(message: str) -> None:
    raise AssertionError(message)


# --- structural contract ----------------------------------------------------


def trigger_events(text: str) -> set[str]:
    lines = text.splitlines()
    try:
        start = lines.index("on:")
    except ValueError as error:
        raise AssertionError("missing on: block") from error
    events = set()
    for line in lines[start + 1 :]:
        if not line.startswith("  "):
            break
        stripped = line.strip()
        if stripped.endswith(":") and not stripped.startswith("#"):
            events.add(stripped[:-1])
    return events


events = trigger_events(text)
if events != {"pull_request"}:
    die(f"guard must trigger solely via unsuppressed pull_request, got {sorted(events)}")
for needle in (
    "name: PR Plan Trigger Guard",
    "on:",
    "  pull_request:",
    "    branches: [master, main]",
    "    types: [opened, synchronize, reopened, ready_for_review]",
):
    if needle not in text:
        die(f"missing expected line: {needle!r}")

label_block = [
    line.strip()
    for line in text.splitlines()
    if line.strip().startswith("- ") and ("labeled" in line or "unlabeled" in line)
]
if label_block:
    die(f"label triggers must not appear (cancel semantics): {label_block}")

concurrency = "group: pr-plan-guard-${{ github.event.pull_request.number }}"
if concurrency not in text:
    die("missing per-PR concurrency group")
if "cancel-in-progress: true" not in text:
    die("missing cancel-in-progress for superseded synchronize events")

lines = text.splitlines()
for index, line in enumerate(lines):
    if line.strip() == "permissions:":
        scope_lines = []
        for follow in lines[index + 1 :]:
            if not follow.startswith((" ", "\t")):
                break
            if follow.strip():
                scope_lines.append(follow.strip())
        if scope_lines != ["contents: read"]:
            die(f"top-level permissions must be exactly contents: read, got {scope_lines}")
        break
else:
    die("missing top-level permissions block")

if "uses:" in text:
    die("guard must contain no actions (no checkout of any ref)")
if "secrets." in text:
    die("guard must not consume secrets")

job_line = next((l for l in lines if l.strip() == "head-name-guard:"), None)
if job_line is None:
    die("missing head-name-guard job")
needed = {
    "if: github.event.pull_request.draft != true",
    "runs-on: ubuntu-24.04",
}
for needle in needed:
    if not any(needle in line for line in lines):
        die(f"missing expected job declaration: {needle}")
timeouts = [line.strip() for line in lines if line.strip().startswith("timeout-minutes:")]
if len(timeouts) != 1 or timeouts[0] != "timeout-minutes: 2":
    die(f"expected exactly timeout-minutes: 2, got {timeouts}")


def run_block(lines: list[str]) -> list[str]:
    start = None
    for index, line in enumerate(lines):
        if line.strip() == "run: |":
            start = index + 1
            break
    if start is None:
        die("missing run block")
    block = []
    for line in lines[start:]:
        if line.strip() and not line.startswith(" " * 10):
            break
        block.append(line)
    if not any(line.strip() for line in block):
        die("empty run block")
    return block


script = "\n".join(run_block(lines))
if "github.event" in script:
    die("run script must reach the payload only through the HEAD_REF env var")
if '"$HEAD_REF"' not in script:
    die("run script must classify $HEAD_REF")
if "env:" not in text or "${{ github.event.pull_request.head.ref }}" not in text:
    die("payload must be bound through the env indirection")
if "Rename" not in script or "push" not in script:
    die("failure guidance must tell contributors to rename AND push (rename fires no event)")
if "::error::" not in script or "exit 1" not in script:
    die("verdict must fail closed loud via ::error:: and non-zero exit")

match = re.search(r"\[\[\s+\"\$HEAD_REF\"\s+=~\s+(\^.*?\$)\s+\]\]", script)
if match is None:
    die("could not extract the classification regex from the shipped run script")
print(match.group(1))
PY
)"

echo "extracted reject pattern: ${reject_re}"

# --- classifier boundary matrix ---------------------------------------------
# Semantics: matching names are the SHA-like class (guard fires, exit 1);
# non-matching names are allowed through silently.

assert_rejects() {
  local name="$1"
  if [[ ! "$name" =~ ${reject_re} ]]; then
    echo "FAIL: '$name' should be classified SHA-like but passes silently" >&2
    exit 1
  fi
}

assert_allows() {
  local name="$1"
  if [[ "$name" =~ ${reject_re} ]]; then
    echo "FAIL: '$name' should be allowed but the guard would fire" >&2
    exit 1
  fi
}

full40='a1b2c3d4e5f60718293a4b5c6d7e8f901234567d'

for name in \
  '0f1e2d3c4b5a' \
  '0f1e2d3' \
  "${full40}" \
  'ABCDEF0'; do
  assert_rejects "$name"
done

for name in \
  'agent/parser-fix' \
  'codex/6238-prplan-guard' \
  'master' \
  'main' \
  'release-notes-2.1' \
  'abcdefg' \
  'abc123' \
  "${full40}0" \
  'feature/x9'; do
  assert_allows "$name"
done

# Lower and upper bounds hold for the synthetic live-fixture name class.
assert_allows 'abc1234g'
assert_rejects 'abc1234'

# --- documentation consistency ----------------------------------------------

grep -q 'pr-plan-head-name-guard.yml' "$docs_inventory" ||
  { echo "FAIL: inventory.md missing the guard workflow row" >&2; exit 1; }
grep -q 'pr-plan-head-name-guard.yml' "$docs_trigger" ||
  { echo "FAIL: pr-plan.md does not attach the guard to the trigger model" >&2; exit 1; }
grep -q 'pr-plan.yml' "$docs_trigger" ||
  { echo "FAIL: pr-plan.md lost its own workflow attachment" >&2; exit 1; }

echo "pr-plan-head-name-guard self-test passed"
