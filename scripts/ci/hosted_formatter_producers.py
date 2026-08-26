#!/usr/bin/env python3
"""Declared hosted formatter producer inventory (#9959).

One claim: after protected rustfmt parity, the advisory meta shard must not
re-execute `fmt`. Live merge blocking stays on required Rust Small (#9127);
the dedicated `Rust formatting` job stays the advisory receipt producer;
local `fmt` / `rustfmt_staged` stay shift-left; PR Smoke `pr-fast` overlap
stays #9166.
"""

from __future__ import annotations

import re
from typing import Any

CARGO_FMT_RE = re.compile(r"cargo\s+fmt\b")
XTASK_FMT_RE = re.compile(r"(?:cargo\s+xtask|just)\s+fmt\b")
RUSTFMT_CHECK_RE = re.compile(r"scripts/ci/rustfmt_check\.py")
DEDICATED_JOB_ID = "rust-formatting"
DEDICATED_CONTEXT_NAME = "Rust formatting"
META_SHARD_NAME = "meta"
FMT_GATE = "fmt"
STAGED_GATE = "rustfmt_staged"
PR_FAST_TIER = "pr_fast"
PARITY_REASON_NEEDLES = (
    "advisory receipt-producing dedicated formatter",
    "perl lsp rust small result",
)


def job_bodies(workflow_text: str) -> dict[str, str]:
    """Return indent-2 GitHub Actions job bodies keyed by job id."""
    bodies: dict[str, list[str]] = {}
    current: str | None = None
    in_jobs = False
    for line in workflow_text.splitlines():
        if line == "jobs:":
            in_jobs = True
            current = None
            continue
        if in_jobs and line and not line.startswith((" ", "\t")):
            in_jobs = False
            current = None
            continue
        if not in_jobs:
            continue
        if (
            line.startswith("  ")
            and not line.startswith("   ")
            and line.rstrip().endswith(":")
            and not line.lstrip().startswith("-")
        ):
            current = line.strip()[:-1]
            bodies[current] = [line]
        elif current is not None:
            bodies[current].append(line)
    return {job_id: "\n".join(lines) for job_id, lines in bodies.items()}


def active_code_lines(text: str) -> list[str]:
    lines: list[str] = []
    for raw in text.splitlines():
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        lines.append(raw)
    return lines


def merge_gate_shard_lanes(workflow_text: str) -> dict[str, list[str]]:
    marker = "  merge-gate-shards:\n"
    if marker not in workflow_text:
        raise AssertionError("merge-gate-shards job is missing")
    start = workflow_text.index(marker)
    end_marker = "    permissions:\n"
    if end_marker not in workflow_text[start:]:
        raise AssertionError("merge-gate-shards matrix terminator is missing")
    end = workflow_text.index(end_marker, start)
    lane: str | None = None
    lanes: dict[str, list[str]] = {}
    for line in workflow_text[start:end].splitlines():
        stripped = line.strip()
        if stripped.startswith("- name: "):
            lane = stripped.removeprefix("- name: ")
        elif lane is not None and stripped.startswith("gates: "):
            lanes[lane] = stripped.removeprefix("gates: ").split()
    if not lanes:
        raise AssertionError("merge-gate-shards matrix has no gate lists")
    return lanes


def _gate_policy_row(policy_text: str, name: str) -> str:
    marker = f"  - name: {name}\n"
    start = policy_text.find(marker)
    if start < 0:
        raise AssertionError(f"gate policy is missing {name}")
    rest = policy_text[start + len(marker) :]
    next_row = rest.find("\n  - name:")
    return rest if next_row < 0 else rest[:next_row]


def ci_gate_mapped_gates(policy_text: str) -> list[str]:
    marker = "    ci-gate:\n"
    start = policy_text.find(marker)
    if start < 0:
        raise AssertionError("workflow_integration job_mapping is missing ci-gate")
    rest = policy_text[start + len(marker) :]
    next_job = rest.find("\n    release-gate:")
    block = rest if next_job < 0 else rest[:next_job]
    names: list[str] = []
    in_gates = False
    for line in block.splitlines():
        stripped = line.strip()
        if stripped == "gates:":
            in_gates = True
            continue
        if in_gates and stripped.startswith("- "):
            names.append(stripped[2:].strip())
            continue
        if in_gates and stripped and not stripped.startswith("- "):
            break
    if not names:
        raise AssertionError("ci-gate job_mapping has no gates")
    return names


def dedicated_job_active(workflow_text: str) -> str:
    jobs = job_bodies(workflow_text)
    body = jobs.get(DEDICATED_JOB_ID)
    if not isinstance(body, str):
        raise AssertionError("dedicated rust-formatting job is missing")
    return "\n".join(active_code_lines(body))


def undeclared_ci_yml_formatter_commands(workflow_text: str) -> list[str]:
    """Return undeclared cargo-fmt / rustfmt_check sites in ci.yml jobs.

    Declared in this workflow: dedicated `rust-formatting` (receipt producer)
    and advisory `pr-smoke` (`xtask gates --tier pr-fast`, #9166 overlap).
    Meta-shard `fmt` is not a declared producer after #9959.
    """
    undeclared: list[str] = []
    for job_id, body in job_bodies(workflow_text).items():
        if job_id in {DEDICATED_JOB_ID, "pr-smoke"}:
            continue
        active = "\n".join(active_code_lines(body))
        if CARGO_FMT_RE.search(active) or RUSTFMT_CHECK_RE.search(active) or XTASK_FMT_RE.search(active):
            undeclared.append(job_id)
    return undeclared


def validate_hosted_formatter_inventory(
    *,
    ci_workflow: str,
    execution_policy: dict[str, Any],
    gate_policy: str,
    required_checks: dict[str, Any],
) -> None:
    lanes = merge_gate_shard_lanes(ci_workflow)
    if META_SHARD_NAME not in lanes:
        raise AssertionError("meta shard is missing from merge-gate-shards")
    if FMT_GATE in lanes[META_SHARD_NAME]:
        raise AssertionError(
            "meta shard must not re-execute fmt after protected rustfmt parity (#9959)"
        )
    for name, gates in lanes.items():
        if FMT_GATE in gates:
            raise AssertionError(
                f"{name} shard must not host a duplicate fmt producer (#9959)"
            )

    execution_gates = execution_policy.get("gates")
    if not isinstance(execution_gates, dict):
        raise AssertionError("gate-shard-execution policy is missing gates")
    if FMT_GATE in execution_gates:
        raise AssertionError(
            "gate-shard-execution must not retain a retired meta fmt row (#9959)"
        )

    if FMT_GATE in ci_gate_mapped_gates(gate_policy):
        raise AssertionError(
            "ci-gate job_mapping must not claim matrix-executed fmt after #9959"
        )

    fmt_row = _gate_policy_row(gate_policy, FMT_GATE)
    if f"tier: {PR_FAST_TIER}" not in fmt_row:
        raise AssertionError("local pr-fast fmt gate must remain for shift-left")
    if "cargo xtask fmt --check" not in fmt_row:
        raise AssertionError("local fmt gate must keep cargo xtask fmt --check")

    staged_row = _gate_policy_row(gate_policy, STAGED_GATE)
    if "tier: commit" not in staged_row:
        raise AssertionError("rustfmt_staged must remain a commit-tier shift-left gate")
    if "cargo xtask commit-check rustfmt_staged" not in staged_row:
        raise AssertionError("rustfmt_staged command drifted")

    dedicated_active = dedicated_job_active(ci_workflow)
    if "scripts/ci/rustfmt_check.py" not in dedicated_active:
        raise AssertionError("dedicated rust-formatting producer must keep rustfmt_check.py")
    if "scripts/ci/verify_rustfmt_receipt.py" not in dedicated_active:
        raise AssertionError("dedicated rust-formatting producer must keep receipt verification")

    undeclared = undeclared_ci_yml_formatter_commands(ci_workflow)
    if undeclared:
        raise AssertionError(
            "undeclared hosted formatter producer in ci.yml jobs: "
            + ", ".join(undeclared)
        )

    if "Keep the existing advisory `fmt` meta-shard during the parity window" in ci_workflow:
        raise AssertionError(
            "parity-window comment must not keep retired meta fmt as current authority"
        )

    entries = [
        item
        for item in required_checks.get("checks", [])
        if isinstance(item, dict) and item.get("name") == DEDICATED_CONTEXT_NAME
    ]
    if len(entries) != 1:
        raise AssertionError("policy must contain exactly one dedicated formatter context")
    entry = entries[0]
    if (
        entry.get("required") is not False
        or entry.get("policy_role") != "advisory"
        or entry.get("enforcement") != "neither"
        or entry.get("job") != DEDICATED_JOB_ID
    ):
        raise AssertionError(
            "dedicated formatter context must remain advisory; live blocking stays on Rust Small"
        )
    reason = str(entry.get("reason", "")).lower()
    missing = [needle for needle in PARITY_REASON_NEEDLES if needle not in reason]
    if missing:
        raise AssertionError(
            "dedicated formatter policy must declare the current parity relationship; "
            f"missing {missing!r}"
        )
    if "synthesize" in reason and "pass" in reason:
        raise AssertionError("policy reason must not describe synthesizing a formatter pass")
