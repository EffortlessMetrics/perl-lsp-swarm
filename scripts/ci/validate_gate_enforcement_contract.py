#!/usr/bin/env python3
"""Validate checked-in CI policy roles against repository workflow emitters."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import stat
import subprocess
import sys
import tomllib
from pathlib import Path, PurePosixPath
from typing import Any, Iterable, NamedTuple, Optional


SUPPORTED_POLICY_VERSION = 2
SUPPORTED_POLICY_SOURCE = "github-enforcement-union"
CONTRACT_SCHEMA_VERSION = 2
ROLES = {"required", "advisory", "informational", "local"}
APPLICABILITY = {
    "always-or-scoped-noop",
    "conditional",
    "planned",
    "not-applicable",
}
PRODUCERS = {"repository-job", "external"}
REQUIRED_ENFORCEMENT = {
    "github-branch-protection",
    "github-ruleset",
    "github-branch-protection+ruleset",
}
CLASSIC_ENFORCEMENT = {
    "github-branch-protection",
    "github-branch-protection+ruleset",
}
RULESET_ENFORCEMENT = {
    "github-ruleset",
    "github-branch-protection+ruleset",
}
NON_REQUIRED_ENFORCEMENT = {"neither", "local", "not-proven"}
WORKFLOW_RESULTS = {"propagate", "continue"}
CHECK_ENTRY_FIELDS = frozenset(
    {
        "name",
        "producer",
        "workflow",
        "job",
        "workflow_result",
        "events",
        "required",
        "policy_role",
        "applicability",
        "enforcement",
        "classic_app_id",
        "ruleset_integration_id",
        "reason",
    }
)
EVENT_NAME = re.compile(r"^[A-Za-z0-9_.-]+$")
JOB_HEADER = re.compile(r"^  ([A-Za-z0-9_.-]+):\s*(?:#.*)?$")


class Finding(NamedTuple):
    code: str
    subject: str
    message: str


class Job(NamedTuple):
    job_id: str
    name: str | None
    name_static: bool
    continue_on_error: bool | None
    continue_static: bool
    condition: str | None
    condition_class: str


class Workflow(NamedTuple):
    path: str
    sha256: str
    events: frozenset[str]
    path_filtered_events: frozenset[str]
    jobs: dict[str, Job]


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _strip_inline_comment(value: str) -> str:
    quote: Optional[str] = None
    escaped = False
    for index, character in enumerate(value):
        if escaped:
            escaped = False
            continue
        if character == "\\" and quote == '"':
            escaped = True
            continue
        if quote:
            if character == quote:
                quote = None
            continue
        if character in {'"', "'"}:
            quote = character
            continue
        if character == "#" and (index == 0 or value[index - 1].isspace()):
            return value[:index].rstrip()
    return value.rstrip()


def _unquote(value: str) -> str:
    value = _strip_inline_comment(value).strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {'"', "'"}:
        return value[1:-1]
    return value


def _git(root: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", "-C", str(root), *args],
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def _repository_state(root: Path) -> tuple[str, bool]:
    sha = _git(root, "rev-parse", "HEAD")
    dirty = bool(_git(root, "status", "--porcelain", "--untracked-files=no"))
    return sha, dirty


def _relative_repo_path(root: Path, path: Path) -> str:
    root = root.resolve(strict=True)
    resolved = path.resolve(strict=True)
    try:
        return resolved.relative_to(root).as_posix()
    except ValueError as error:
        raise ValueError(f"path escapes repository root: {path}") from error


def _safe_repo_path(root: Path, raw: str, *, subject: str) -> tuple[Path, str]:
    if not isinstance(raw, str) or not raw:
        raise ValueError(f"{subject} path must be a non-empty repository-relative string")
    if "\\" in raw:
        raise ValueError(f"{subject} path must use repository POSIX separators: {raw!r}")
    pure = PurePosixPath(raw)
    if pure.is_absolute() or any(part in {"", ".", ".."} for part in pure.parts):
        raise ValueError(f"{subject} path is not a safe repository-relative path: {raw!r}")

    root_resolved = root.resolve(strict=True)
    candidate = root_resolved.joinpath(*pure.parts)
    cursor = root_resolved
    for part in pure.parts:
        cursor = cursor / part
        if cursor.is_symlink():
            raise ValueError(f"{subject} path traverses a symlink: {raw!r}")
    try:
        resolved = candidate.resolve(strict=True)
        resolved.relative_to(root_resolved)
    except (FileNotFoundError, ValueError) as error:
        raise ValueError(f"{subject} path is missing or outside the repository: {raw!r}") from error
    if not stat.S_ISREG(resolved.stat().st_mode):
        raise ValueError(f"{subject} path is not a regular file: {raw!r}")
    try:
        _git(root_resolved, "ls-files", "--error-unmatch", "--", pure.as_posix())
    except subprocess.CalledProcessError as error:
        raise ValueError(f"{subject} path is not tracked by git: {raw!r}") from error
    return resolved, pure.as_posix()


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _collect_block_value(lines: list[str], index: int, end: int, field_indent: int) -> tuple[str, int]:
    raw = _strip_inline_comment(lines[index].split(":", 1)[1]).strip()
    if raw not in {"|", "|-", "|+", ">", ">-", ">+"}:
        return raw, index + 1
    parts: list[str] = []
    cursor = index + 1
    while cursor < end:
        line = lines[cursor]
        if line.strip() and _indent(line) <= field_indent:
            break
        if line.strip():
            parts.append(line.strip())
        cursor += 1
    return " ".join(parts), cursor


def _split_top_level(expression: str, operator: str) -> list[str]:
    """Split on `operator` only outside parentheses and quoted strings."""
    parts: list[str] = []
    depth = 0
    quote: Optional[str] = None
    start = 0
    index = 0
    while index < len(expression):
        character = expression[index]
        if quote:
            if character == quote:
                quote = None
            index += 1
            continue
        if character in {'"', "'"}:
            quote = character
            index += 1
            continue
        if character == "(":
            depth += 1
        elif character == ")":
            depth = max(0, depth - 1)
        elif depth == 0 and expression.startswith(operator, index):
            parts.append(expression[start:index])
            index += len(operator)
            start = index
            continue
        index += 1
    parts.append(expression[start:])
    return parts


def _constant_truth(expression: str) -> bool | None:
    """Fold an expression to a constant when its literals alone decide it.

    Returns True/False when reachability is statically decided, or None when
    the expression depends on context this bounded parser does not evaluate.
    A `false` inside a quoted string is a string, not a boolean literal.
    """
    expression = expression.strip()
    while expression.startswith("(") and expression.endswith(")"):
        inner = expression[1:-1]
        if inner.count("(") == inner.count(")"):
            expression = inner.strip()
        else:
            break
    if expression in {"true", "always()"}:
        return True
    if expression == "false":
        return False

    disjuncts = _split_top_level(expression, "||")
    if len(disjuncts) > 1:
        values = [_constant_truth(part) for part in disjuncts]
        if any(value is True for value in values):
            return True
        if all(value is False for value in values):
            return False
        return None

    conjuncts = _split_top_level(expression, "&&")
    if len(conjuncts) > 1:
        values = [_constant_truth(part) for part in conjuncts]
        if any(value is False for value in values):
            return False
        if all(value is True for value in values):
            return True
        return None

    return None


def _condition_class(condition: str | None) -> str:
    if condition is None:
        return "always"
    normalized = condition.strip()
    if normalized.startswith("${{") and normalized.endswith("}}"):
        normalized = normalized[3:-2].strip()
    compact = re.sub(r"\s+", "", normalized).lower()
    if not compact:
        return "unknown"
    constant = _constant_truth(compact)
    if constant is True:
        return "always"
    if constant is False:
        return "never"
    return "conditional"


def _read_job(lines: list[str], start: int, end: int, job_id: str) -> Job:
    # GitHub uses the job ID as the check name when a direct `name:` field is absent.
    name: str | None = job_id
    name_static = True
    continue_on_error: bool | None = False
    continue_static = True
    condition: str | None = None

    index = start + 1
    while index < end:
        line = lines[index]
        if _indent(line) != 4:
            index += 1
            continue
        stripped = line.strip()
        key = _unquote(stripped.split(":", 1)[0].strip()) if ":" in stripped else ""
        if key == "name":
            raw, index = _collect_block_value(lines, index, end, 4)
            name_static = bool(raw) and "${{" not in raw
            name = _unquote(raw) if name_static else None
            continue
        if key == "continue-on-error":
            raw, index = _collect_block_value(lines, index, end, 4)
            value = _strip_inline_comment(raw).strip().lower()
            if value == "true":
                continue_on_error = True
            elif value == "false":
                continue_on_error = False
            else:
                continue_on_error = None
                continue_static = False
            continue
        if key == "if":
            raw, index = _collect_block_value(lines, index, end, 4)
            condition = _strip_inline_comment(raw).strip() or ""
            continue
        index += 1

    return Job(
        job_id=job_id,
        name=name,
        name_static=name_static,
        continue_on_error=continue_on_error,
        continue_static=continue_static,
        condition=condition,
        condition_class=_condition_class(condition),
    )


def _parse_events(lines: list[str]) -> tuple[frozenset[str], frozenset[str]]:
    on_index = next(
        (
            index
            for index, line in enumerate(lines)
            if _indent(line) == 0 and line.startswith("on:")
        ),
        None,
    )
    if on_index is None:
        return frozenset(), frozenset()

    inline = _strip_inline_comment(lines[on_index].split(":", 1)[1]).strip()
    if inline:
        if inline.startswith("[") and inline.endswith("]"):
            events = {
                item.strip().strip("'\"")
                for item in inline[1:-1].split(",")
                if item.strip()
            }
        else:
            events = {_unquote(inline)}
        return frozenset(events), frozenset()

    end = next(
        (
            index
            for index in range(on_index + 1, len(lines))
            if lines[index].strip() and _indent(lines[index]) == 0
        ),
        len(lines),
    )
    events: set[str] = set()
    filtered: set[str] = set()
    current_event: str | None = None
    for line in lines[on_index + 1 : end]:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = _indent(line)
        stripped = line.strip()
        if indent == 2 and ":" in stripped:
            key = stripped.split(":", 1)[0].strip().strip("'\"")
            if EVENT_NAME.fullmatch(key):
                current_event = key
                events.add(key)
            else:
                current_event = None
        elif indent >= 4 and current_event and stripped.split(":", 1)[0] in {
            "paths",
            "paths-ignore",
        }:
            filtered.add(current_event)
    return frozenset(events), frozenset(filtered)


def read_workflow(root: Path, raw_path: str) -> Workflow:
    path, relative = _safe_repo_path(root, raw_path, subject="workflow")
    lines = path.read_text(encoding="utf-8").splitlines()
    events, path_filtered_events = _parse_events(lines)
    jobs_index = next(
        (
            index
            for index, line in enumerate(lines)
            if _indent(line) == 0 and line == "jobs:"
        ),
        None,
    )
    jobs: dict[str, Job] = {}
    if jobs_index is not None:
        starts: list[tuple[int, str]] = []
        for index in range(jobs_index + 1, len(lines)):
            match = JOB_HEADER.match(lines[index])
            if match:
                starts.append((index, match.group(1)))
            elif lines[index].strip() and _indent(lines[index]) == 0:
                break
        for position, (start, job_id) in enumerate(starts):
            end = starts[position + 1][0] if position + 1 < len(starts) else len(lines)
            jobs[job_id] = _read_job(lines, start, end, job_id)
    return Workflow(
        path=relative,
        sha256=_sha256(path),
        events=events,
        path_filtered_events=path_filtered_events,
        jobs=jobs,
    )


def read_workflow_catalog(root: Path) -> dict[str, Workflow]:
    workflow_dir = root / ".github" / "workflows"
    if not workflow_dir.is_dir():
        raise ValueError("repository has no .github/workflows directory")
    catalog: dict[str, Workflow] = {}
    candidates = sorted([*workflow_dir.glob("*.yml"), *workflow_dir.glob("*.yaml")])
    for candidate in candidates:
        relative = candidate.relative_to(root).as_posix()
        workflow = read_workflow(root, relative)
        catalog[relative] = workflow
    return catalog


def build_producer_index(workflows: dict[str, Workflow]) -> dict[str, list[tuple[str, str]]]:
    index: dict[str, list[tuple[str, str]]] = {}
    for workflow in workflows.values():
        for job in workflow.jobs.values():
            if job.name_static and job.name:
                index.setdefault(job.name, []).append((workflow.path, job.job_id))
    for producers in index.values():
        producers.sort()
    return index


def _string_list(value: Any, *, field: str, subject: str) -> list[str]:
    if (
        not isinstance(value, list)
        or not value
        or any(not isinstance(item, str) or not item for item in value)
    ):
        raise ValueError(f"{subject} {field} must be a non-empty list of non-empty strings")
    return value


def _binding_findings(
    entry: dict[str, Any],
    *,
    field: str,
    allowed_enforcement: set[str],
    subject: str,
) -> list[Finding]:
    if field not in entry:
        return []
    value = entry[field]
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        return [
            Finding(
                f"invalid_{field}",
                subject,
                f"{field} must be a positive integer",
            )
        ]
    if entry.get("enforcement") not in allowed_enforcement:
        return [
            Finding(
                f"{field}_source_mismatch",
                subject,
                f"{field} is incompatible with enforcement {entry.get('enforcement')!r}",
            )
        ]
    return []


def validate_context(
    entry: dict[str, Any],
    workflows: dict[str, Workflow],
    producers: dict[str, list[tuple[str, str]]],
) -> tuple[list[Finding], bool]:
    findings: list[Finding] = []
    name = str(entry.get("name") or "<unnamed>")
    for field in sorted(set(entry) - CHECK_ENTRY_FIELDS):
        findings.append(
            Finding(
                "unknown_context_field",
                name,
                f"unsupported [[checks]] field {field!r}",
            )
        )
    role = entry.get("policy_role")
    required = entry.get("required")
    applicability = entry.get("applicability")
    enforcement = entry.get("enforcement")
    producer = entry.get("producer")

    if role not in ROLES:
        findings.append(Finding("invalid_policy_role", name, f"expected one of {sorted(ROLES)}"))
    if not isinstance(required, bool):
        findings.append(Finding("missing_required_boolean", name, "required must be explicit"))
    elif (role == "required") != required:
        findings.append(Finding("role_required_mismatch", name, "policy_role and required disagree"))
    if applicability not in APPLICABILITY:
        findings.append(Finding("invalid_applicability", name, f"expected one of {sorted(APPLICABILITY)}"))
    if producer not in PRODUCERS:
        findings.append(Finding("invalid_producer", name, f"expected one of {sorted(PRODUCERS)}"))
    if role == "required" and enforcement not in REQUIRED_ENFORCEMENT:
        findings.append(Finding("required_without_github_enforcement", name, "required context names no protected enforcement"))
    if role in {"advisory", "informational", "local"} and enforcement not in NON_REQUIRED_ENFORCEMENT:
        findings.append(Finding("nonrequired_claims_github_enforcement", name, f"non-required role claims {enforcement!r}"))

    findings.extend(
        _binding_findings(
            entry,
            field="classic_app_id",
            allowed_enforcement=CLASSIC_ENFORCEMENT,
            subject=name,
        )
    )
    findings.extend(
        _binding_findings(
            entry,
            field="ruleset_integration_id",
            allowed_enforcement=RULESET_ENFORCEMENT,
            subject=name,
        )
    )

    workflow = entry.get("workflow")
    job_id = entry.get("job")
    workflow_result = entry.get("workflow_result")

    if producer == "external":
        if isinstance(workflow, str) and workflow.startswith(".github/workflows/"):
            findings.append(Finding("external_producer_uses_repository_workflow", name, "external producer must not claim a repository workflow"))
        if job_id is not None or workflow_result is not None:
            findings.append(Finding("external_producer_has_job_contract", name, "external producer cannot declare repository job semantics"))
        return findings, False

    if producer != "repository-job":
        return findings, False

    if not isinstance(workflow, str) or workflow not in workflows:
        findings.append(Finding("workflow_missing", name, str(workflow)))
        return findings, False
    if not isinstance(job_id, str) or not job_id:
        findings.append(
            Finding(
                "required_context_unmapped" if role == "required" else "context_unmapped",
                name,
                "repository-owned context must name its emitting workflow job",
            )
        )
        return findings, False
    if workflow_result not in WORKFLOW_RESULTS:
        findings.append(Finding("invalid_workflow_result", name, f"expected one of {sorted(WORKFLOW_RESULTS)}"))

    declared_events: list[str] = []
    try:
        declared_events = _string_list(entry.get("events"), field="events", subject=name)
    except ValueError as error:
        findings.append(Finding("invalid_events", name, str(error)))

    workflow_data = workflows[workflow]
    job = workflow_data.jobs.get(job_id)
    if job is None:
        findings.append(Finding("workflow_job_missing", name, f"{workflow}:{job_id}"))
        return findings, True

    if not job.name_static or job.name is None:
        findings.append(Finding("job_name_not_static", name, f"{workflow}:{job_id} must emit a static direct name"))
    elif job.name != name:
        findings.append(Finding("context_name_mismatch", name, f"workflow emits {job.name!r}"))

    actual_producers = producers.get(name, [])
    expected_producer = (workflow, job_id)
    if expected_producer not in actual_producers:
        findings.append(Finding("mapped_producer_not_indexed", name, f"{workflow}:{job_id} is not the indexed static producer"))
    if len(actual_producers) != 1:
        findings.append(
            Finding(
                "duplicate_emitted_context",
                name,
                "expected one static producer, found "
                + (", ".join(f"{path}:{job}" for path, job in actual_producers) or "none"),
            )
        )

    if not job.continue_static:
        findings.append(Finding("job_continue_on_error_not_static", name, f"{workflow}:{job_id} has absent, commented, or dynamic direct continue-on-error"))
    elif workflow_result == "continue" and job.continue_on_error is not True:
        findings.append(Finding("workflow_result_mismatch", name, "policy says continue but job propagates failure"))
    elif workflow_result == "propagate" and job.continue_on_error is not False:
        findings.append(Finding("workflow_result_mismatch", name, "policy says propagate but job continues on error"))

    if job.condition_class == "never":
        findings.append(Finding("job_unreachable", name, f"{workflow}:{job_id} has if: false"))
    elif applicability == "always-or-scoped-noop" and job.condition_class != "always":
        findings.append(Finding("applicability_mismatch", name, f"policy says always-or-scoped-noop but job condition is {job.condition_class}"))
    elif applicability == "conditional" and job.condition_class != "conditional":
        findings.append(Finding("applicability_mismatch", name, f"policy says conditional but job condition is {job.condition_class}"))

    for event in declared_events:
        if event not in workflow_data.events:
            findings.append(Finding("workflow_event_missing", name, f"{workflow} does not trigger on {event}"))
        if role == "required" and event in workflow_data.path_filtered_events:
            findings.append(Finding("required_event_path_filtered", name, f"{workflow} filters paths for required event {event}"))
    return findings, True


def _canonical_context(entry: dict[str, Any]) -> dict[str, Any]:
    fields = (
        "name",
        "producer",
        "workflow",
        "job",
        "workflow_result",
        "required",
        "policy_role",
        "applicability",
        "enforcement",
        "classic_app_id",
        "ruleset_integration_id",
        "events",
    )
    return {field: entry[field] for field in fields if field in entry}


def validate(root: Path, policy_path: Path) -> dict[str, Any]:
    root = root.resolve(strict=True)
    repository_sha, repository_dirty = _repository_state(root)
    if repository_dirty:
        raise ValueError("repository has tracked modifications; exact subject is NOT_PROVEN")

    policy_relative = _relative_repo_path(root, policy_path)
    policy_file, policy_relative = _safe_repo_path(root, policy_relative, subject="policy")
    raw = tomllib.loads(policy_file.read_text(encoding="utf-8"))
    if raw.get("version") != SUPPORTED_POLICY_VERSION:
        raise ValueError(f"unsupported policy version {raw.get('version')!r}; expected {SUPPORTED_POLICY_VERSION}")
    if raw.get("source") != SUPPORTED_POLICY_SOURCE:
        raise ValueError(f"unsupported policy source {raw.get('source')!r}; expected {SUPPORTED_POLICY_SOURCE!r}")

    contexts = raw.get("checks")
    if not isinstance(contexts, list) or not contexts:
        raise ValueError("policy must contain at least one [[checks]] entry")

    workflows = read_workflow_catalog(root)
    producer_index = build_producer_index(workflows)

    findings: list[Finding] = []
    names: set[str] = set()
    mapped = 0
    canonical_contexts: list[dict[str, Any]] = []
    for entry in contexts:
        if not isinstance(entry, dict):
            findings.append(Finding("invalid_context_entry", policy_relative, "[[checks]] entry is not a table"))
            continue
        name = str(entry.get("name") or "<unnamed>")
        if name in names:
            findings.append(Finding("duplicate_context", name, "context names must be unique"))
        names.add(name)
        canonical_contexts.append(_canonical_context(entry))
        entry_findings, is_mapped = validate_context(entry, workflows, producer_index)
        findings.extend(entry_findings)
        mapped += int(is_mapped)

    workflow_subjects = [
        {"path": workflow.path, "sha256": workflow.sha256}
        for workflow in sorted(workflows.values(), key=lambda item: item.path)
    ]
    canonical_contexts.sort(key=lambda item: str(item.get("name", "")))
    policy_subject = {
        "path": policy_relative,
        "sha256": _sha256(policy_file),
        "version": SUPPORTED_POLICY_VERSION,
        "source": SUPPORTED_POLICY_SOURCE,
    }
    semantic_subject = {
        "schema_version": CONTRACT_SCHEMA_VERSION,
        "policy": {
            "path": policy_relative,
            "version": SUPPORTED_POLICY_VERSION,
            "source": SUPPORTED_POLICY_SOURCE,
        },
        "contexts": canonical_contexts,
    }
    exact_source_subject = {
        "repository_sha": repository_sha,
        "repository_dirty": repository_dirty,
        "policy": policy_subject,
        "workflow_catalog": workflow_subjects,
    }
    subjects = {
        **exact_source_subject,
        "contexts": canonical_contexts,
    }
    subject_json = json.dumps(
        semantic_subject,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")
    exact_source_json = json.dumps(
        exact_source_subject,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")
    subject_sha256 = hashlib.sha256(subject_json).hexdigest()
    exact_source_sha256 = hashlib.sha256(exact_source_json).hexdigest()

    return {
        "schema_version": CONTRACT_SCHEMA_VERSION,
        "status": "SUCCESS" if not findings else "BLOCKED",
        "policy_path": policy_relative,
        "policy_version": SUPPORTED_POLICY_VERSION,
        "policy_source": SUPPORTED_POLICY_SOURCE,
        "contexts": len(contexts),
        "mapped_jobs": mapped,
        "findings": [finding._asdict() for finding in findings],
        "subjects": subjects,
        "semantic_subject": semantic_subject,
        "subject_sha256": subject_sha256,
        "exact_source_sha256": exact_source_sha256,
        "live_enforcement_status": "NOT_PROVEN",
        "live_enforcement_reason": "static contract does not query classic protection and active rulesets",
    }


def _not_proven(policy: Path, error: Exception) -> dict[str, Any]:
    return {
        "schema_version": CONTRACT_SCHEMA_VERSION,
        "status": "NOT_PROVEN",
        "policy_path": str(policy),
        "policy_version": None,
        "policy_source": None,
        "contexts": 0,
        "mapped_jobs": 0,
        "findings": [Finding("instrument_failure", str(policy), str(error))._asdict()],
        "subjects": None,
        "semantic_subject": None,
        "subject_sha256": None,
        "exact_source_sha256": None,
        "live_enforcement_status": "NOT_PROVEN",
        "live_enforcement_reason": "static contract does not query classic protection and active rulesets",
    }


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--policy", type=Path, default=Path(".ci/policies/required-checks.toml"))
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(list(argv) if argv is not None else None)
    root = args.root.resolve()
    policy = args.policy if args.policy.is_absolute() else root / args.policy
    try:
        result = validate(root, policy)
    except (OSError, subprocess.CalledProcessError, tomllib.TOMLDecodeError, ValueError) as error:
        result = _not_proven(policy, error)

    if args.receipt:
        receipt = args.receipt if args.receipt.is_absolute() else root / args.receipt
        receipt.parent.mkdir(parents=True, exist_ok=True)
        receipt.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(f"Gate enforcement contract: {result['status']}")
        for finding in result["findings"]:
            print(f"- {finding['code']}: {finding['subject']}: {finding['message']}")
        if not result["findings"]:
            print("- checked-in roles, applicability, enforcement, producer identity, workflow result, events, and mapped job names agree")
            print(f"- exact static subject: {result['subject_sha256']} at repository {result['subjects']['repository_sha']}")
            print("- live GitHub enforcement remains a separate authenticated NOT_PROVEN input")
    return 0 if result["status"] == "SUCCESS" else 1


if __name__ == "__main__":
    sys.exit(main())
