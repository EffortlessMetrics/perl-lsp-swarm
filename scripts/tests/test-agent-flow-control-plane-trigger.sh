#!/usr/bin/env bash
set -euo pipefail

# This is a fixture for the declarative GitHub path filter. It does not claim
# to simulate GitHub event delivery. It proves that both pull-request and push
# filters keep the control-plane allowlist explicit and exclude product paths.

workflow='.github/workflows/agent-flow-control-plane.yml'
test -f "$workflow"

python3 - "$workflow" <<'PY'
from __future__ import annotations

import sys
from pathlib import Path


WORKFLOW = Path(sys.argv[1])
REQUIRED_PATHS = {
    "AGENTS.md",
    "CLAUDE.md",
    "docs/agents/**",
    "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md",
    "docs/specs/README.md",
    "docs/INDEX.md",
    ".agents/skills/**",
    ".claude/skills/**",
    ".claude/settings.json",
    ".codex/**",
    "xtask/src/main.rs",
    "xtask/src/tasks/agent_flow.rs",
    "xtask/src/tasks/mod.rs",
    "xtask/tests/agent_merge_review_backstop.rs",
    "xtask/tests/pr_convergence_contract.rs",
    "scripts/tests/test-agent-flow-control-plane-trigger.sh",
    ".github/workflows/agent-flow-control-plane.yml",
}
TARGET = "docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md"
EVENTS = ("pull_request", "push")


def indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def event_bounds(lines: list[str], event: str) -> tuple[int, int]:
    marker = f"  {event}:"
    try:
        start = lines.index(marker)
    except ValueError as error:
        raise AssertionError(f"missing on.{event} event block") from error

    end = len(lines)
    for index in range(start + 1, len(lines)):
        line = lines[index]
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if indent(line) <= 2 and stripped.endswith(":"):
            end = index
            break
    return start, end


def event_paths(text: str, event: str) -> set[str]:
    lines = text.splitlines()
    start, end = event_bounds(lines, event)
    block = lines[start:end]
    try:
        paths_index = block.index("    paths:")
    except ValueError as error:
        raise AssertionError(f"missing on.{event}.paths") from error

    paths: set[str] = set()
    for line in block[paths_index + 1 :]:
        stripped = line.strip()
        if not stripped:
            continue
        if indent(line) <= 4:
            break
        prefix = "- '"
        if stripped.startswith(prefix) and stripped.endswith("'"):
            paths.add(stripped[len(prefix) : -1])
    return paths


def validate(text: str) -> None:
    for event in EVENTS:
        paths = event_paths(text, event)
        missing = REQUIRED_PATHS - paths
        assert not missing, f"on.{event}.paths missing {sorted(missing)!r}"
        assert "**" not in paths, f"on.{event}.paths must not contain a repository-wide glob"
        assert "crates/**" not in paths, f"on.{event}.paths must not contain product crates"
        assert not any(
            path.startswith("crates/perl-parser") for path in paths
        ), f"on.{event}.paths must not include parser product paths"


def remove_scoped_path(text: str, event: str, path: str) -> str:
    lines = text.splitlines()
    start, end = event_bounds(lines, event)
    needle = f"      - '{path}'"
    for index in range(start, end):
        if lines[index] == needle:
            del lines[index]
            return "\n".join(lines) + "\n"
    raise AssertionError(f"mutation fixture could not remove {path!r} from {event}")


source = WORKFLOW.read_text(encoding="utf-8")
validate(source)

for event in EVENTS:
    mutated = remove_scoped_path(source, event, TARGET)
    try:
        validate(mutated)
    except AssertionError:
        pass
    else:
        raise AssertionError(
            f"removing {TARGET!r} only from on.{event}.paths must fail the contract"
        )

print("agent-flow control-plane trigger fixtures passed")
PY
