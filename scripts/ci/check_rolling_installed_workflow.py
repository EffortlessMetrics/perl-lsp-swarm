#!/usr/bin/env python3
"""Structural recurrence checks for the rolling installed observation workflow.

The checks parse the workflow's YAML structure instead of matching strings
against raw bytes: a required control that has been commented out, a job-level
write permission, a drifted matrix tuple, or a flattened artifact download must
all fail even when the literal text still appears somewhere in the file.

The parser is stdlib-only and intentionally understands only the YAML subset
this workflow uses (block mappings/sequences, plain and quoted scalars, inline
flow lists, and literal block scalars). Anything else -- anchors, tags, flow
mappings, multi-line plain scalars -- fails closed.
"""

from __future__ import annotations

import pathlib
import re
import sys
from typing import Any

WORKFLOW = pathlib.Path(".github/workflows/rolling-installed-public-beta-observation.yml")
EXPECTED_MATRIX = (
    {
        "row_id": "linux-minimum",
        "os": "ubuntu-24.04",
        "platform": "linux",
        "architecture": "x64",
        "host_role": "minimum_supported",
    },
    {
        "row_id": "linux-current",
        "os": "ubuntu-24.04",
        "platform": "linux",
        "architecture": "x64",
        "host_role": "current_stable",
    },
    {
        "row_id": "windows-current",
        "os": "windows-2022",
        "platform": "windows",
        "architecture": "x64",
        "host_role": "current_stable",
    },
)
READ_ONLY_PERMISSION_VALUES = {"read", "none"}
FORBIDDEN_MUTATIONS = (
    "cargo publish",
    "npm publish",
    "vsce publish",
    "ovsx publish",
    "docker push",
    "gh release create",
    "gh release upload",
    "git push",
)
RELEASE_BUILD = (
    "cargo build --locked --release -p perllsp --bin perllsp -p perl-dap --bin perl-dap"
)
EXACT_CHECKOUT_TEST = 'test "$(git rev-parse HEAD)" = "$EXPECTED_SHA"'
ROW_ARTIFACT_NAME = "rolling-installed-row-${{ matrix.row_id }}"
ROW_DOWNLOAD_PATTERN = "rolling-installed-row-*"
FAN_IN_PACKET = "rolling_installed_public_beta_fan_in.json"
DISPATCH_ONLY = "github.event_name == 'workflow_dispatch'"
# The read-only mutation boundary extends to action identities: a publishing
# step introduced through `uses:` carries no forbidden run token.
ALLOWED_ACTIONS = {
    "./.github/actions/setup-vscode-toolchain",
    "Swatinem/rust-cache",
    "actions/checkout",
    "actions/download-artifact",
    "actions/upload-artifact",
    "dtolnay/rust-toolchain",
    "shogo82148/actions-setup-perl",
}


class WorkflowError(RuntimeError):
    """The workflow no longer preserves the exact rolling-evidence boundary."""


# ---------------------------------------------------------------------------
# Minimal YAML-subset parser (fail-closed)
# ---------------------------------------------------------------------------

_KEY_RE = re.compile(r"^(?P<key>[^\s:'\"][^:]*|'[^']*'|\"[^\"]*\"):\s*(?P<value>.*)$")


def _indent_of(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _strip_comment(text: str) -> str:
    quote: str | None = None
    index = 0
    while index < len(text):
        char = text[index]
        if quote is not None:
            if char == quote:
                quote = None
        elif char in {"'", '"'}:
            quote = char
        elif char == "#" and index > 0 and text[index - 1].isspace():
            return text[:index].rstrip()
        index += 1
    return text.strip()


def _parse_scalar(raw: str) -> Any:
    value = _strip_comment(raw)
    if not value:
        return ""
    if value[0] in "&!{":
        raise WorkflowError(f"unsupported YAML indirection in scalar: {value!r}")
    if value[0] == "*":
        raise WorkflowError(f"unsupported YAML alias in scalar: {value!r}")
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
        inner = value[1:-1]
        return inner.replace("''", "'") if value[0] == "'" else inner
    if value[0] == "[":
        if not value.endswith("]"):
            raise WorkflowError(f"unsupported flow scalar: {value!r}")
        body = value[1:-1].strip()
        if not body:
            return []
        return [_parse_scalar(part) for part in body.split(",")]
    if value in {"true", "false"}:
        return value == "true"
    if re.fullmatch(r"-?[0-9]+", value):
        return int(value)
    return value


def _skip_ignored(lines: list[str], index: int) -> int:
    while index < len(lines):
        stripped = lines[index].strip()
        if stripped and not stripped.startswith("#"):
            break
        index += 1
    return index


def _parse_literal(lines: list[str], index: int, parent_indent: int) -> tuple[str, int]:
    block: list[str] = []
    while index < len(lines):
        line = lines[index]
        if line.strip() and _indent_of(line) <= parent_indent:
            break
        block.append(line)
        index += 1
    nonempty = [line for line in block if line.strip()]
    base = min((_indent_of(line) for line in nonempty), default=parent_indent + 2)
    return "\n".join(
        line[base:] if line.strip() else "" for line in block
    ).rstrip("\n") + "\n", index


def _parse_key_value(content: str) -> tuple[str, str]:
    match = _KEY_RE.match(content)
    if match is None:
        raise WorkflowError(f"unsupported YAML line: {content!r}")
    key = match.group("key")
    if len(key) >= 2 and key[0] in "'\"" and key[-1] == key[0]:
        key = key[1:-1]
    return key, match.group("value")


def _parse_block(lines: list[str], index: int, indent: int) -> tuple[Any, int]:
    index = _skip_ignored(lines, index)
    if index >= len(lines) or _indent_of(lines[index]) < indent:
        return None, index
    indent = _indent_of(lines[index])
    stripped = lines[index].strip()
    if stripped == "-" or stripped.startswith("- "):
        return _parse_sequence(lines, index, indent)
    return _parse_mapping(lines, index, indent)


def _parse_mapping(lines: list[str], index: int, indent: int) -> tuple[dict, int]:
    result: dict[str, Any] = {}
    while True:
        index = _skip_ignored(lines, index)
        if index >= len(lines):
            break
        line = lines[index]
        current = _indent_of(line)
        if current < indent or line.strip().startswith("-"):
            break
        if current > indent:
            raise WorkflowError(f"unexpected indentation at line {index + 1}: {line!r}")
        key, raw_value = _parse_key_value(line.strip())
        if key in result:
            raise WorkflowError(f"duplicate YAML key {key!r} at line {index + 1}")
        value = _strip_comment(raw_value)
        if value.startswith(("|", ">")):
            if not re.fullmatch(r"[>|][+-]?", value):
                raise WorkflowError(
                    f"unsupported block scalar header {value!r} at line {index + 1}"
                )
            result[key], index = _parse_literal(lines, index + 1, indent)
            continue
        if value:
            result[key] = _parse_scalar(raw_value)
            index += 1
            continue
        child, index = _parse_block(lines, index + 1, indent + 1)
        result[key] = child
    return result, index


def _parse_sequence(lines: list[str], index: int, indent: int) -> tuple[list, int]:
    result: list[Any] = []
    while True:
        index = _skip_ignored(lines, index)
        if index >= len(lines):
            break
        line = lines[index]
        current = _indent_of(line)
        if current != indent or not (
            line.strip() == "-" or line.strip().startswith("- ")
        ):
            break
        content = line.strip()[1:].strip()
        if not content:
            child, index = _parse_block(lines, index + 1, indent + 1)
            result.append(child)
            continue
        if _KEY_RE.match(content):
            # Inline mapping start: "- key: value" with continuation keys
            # indented past the dash.
            virtual = " " * (indent + 2) + content
            item, index = _parse_mapping([*lines[:index], virtual, *lines[index + 1 :]], index, indent + 2)
            result.append(item)
            continue
        result.append(_parse_scalar(content))
        index += 1
    return result, index


def parse_workflow(text: str) -> dict[str, Any]:
    document, index = _parse_block(text.splitlines(), 0, 0)
    if not isinstance(document, dict):
        raise WorkflowError("workflow is not a YAML mapping")
    if _skip_ignored(text.splitlines(), index) < len(text.splitlines()):
        raise WorkflowError("trailing unparsed YAML content")
    return document


# ---------------------------------------------------------------------------
# Structural checks
# ---------------------------------------------------------------------------


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise WorkflowError(message)


def _needs(job: Any) -> list[str]:
    """Normalize a scalar or flow-list `needs` to a list for element checks."""
    value = job.get("needs") if isinstance(job, dict) else None
    if isinstance(value, str):
        return [value]
    return [item for item in value or [] if isinstance(item, str)]


def _executable_lines(run: Any) -> list[str]:
    """Shell-executable lines of a run block: comments and blanks removed."""
    if not isinstance(run, str):
        return []
    return [
        line
        for line in run.splitlines()
        if line.strip() and not line.strip().startswith("#")
    ]


def _steps(job: Any, name: str) -> list[dict]:
    _require(isinstance(job, dict), f"job {name} is missing or not a mapping")
    steps = job.get("steps")
    _require(isinstance(steps, list), f"job {name} has no steps")
    return [step for step in steps if isinstance(step, dict)]


def _step_named(job: Any, job_name: str, step_name: str) -> dict:
    for step in _steps(job, job_name):
        if step.get("name") == step_name:
            return step
    raise WorkflowError(f"job {job_name} misses step {step_name!r}")


def _run_texts(job: Any, name: str) -> list[str]:
    return [
        "\n".join(_executable_lines(step.get("run")))
        for step in _steps(job, name)
        if "run" in step
    ]


def _require_run(run_texts: list[str], token: str, message: str) -> None:
    _require(
        any(token in text for text in run_texts),
        message,
    )


def _check_permissions_block(value: Any, scope: str) -> None:
    _require(
        isinstance(value, dict),
        f"{scope} permissions must be an explicit mapping (no write-all or scalar)",
    )
    for key, level in value.items():
        _require(
            isinstance(key, str) and level in READ_ONLY_PERMISSION_VALUES,
            f"{scope} permission {key!r} grants a write scope: {level!r}",
        )


def validate(document: dict[str, Any]) -> None:
    # Read-only boundary: the workflow token is read-only at the top level and
    # no job may widen it to any write scope.
    permissions = document.get("permissions")
    _check_permissions_block(permissions, "workflow")
    _require(
        permissions == {"contents": "read"},
        "workflow permissions must be exactly contents: read",
    )

    concurrency = document.get("concurrency")
    _require(
        isinstance(concurrency, dict)
        and concurrency.get("cancel-in-progress") is False,
        "rolling runs must not cancel in-progress evidence",
    )

    triggers = document.get("on")
    _require(isinstance(triggers, dict), "workflow triggers must be a mapping")
    _require(
        "workflow_dispatch" in triggers,
        "rolling observation must remain manually dispatched",
    )
    _require(
        set(triggers) <= {"workflow_dispatch", "pull_request"},
        f"unexpected workflow triggers: {sorted(triggers)}",
    )

    jobs = document.get("jobs")
    _require(isinstance(jobs, dict), "workflow has no jobs")
    for name in ("contract", "subject", "installed-row", "fan-in"):
        _require(name in jobs, f"workflow misses job {name}")
    for name, job in jobs.items():
        if isinstance(job, dict) and "permissions" in job:
            _check_permissions_block(job["permissions"], f"job {name}")

    contract = jobs["contract"]
    contract_runs = _run_texts(contract, "contract")
    _require_run(
        contract_runs,
        "python3 scripts/ci/test_rolling_installed_observation.py",
        "contract job must run the receipt and fan-in falsifiers",
    )
    _require_run(
        contract_runs,
        "python3 scripts/ci/check_rolling_installed_workflow.py",
        "contract job must run this structural recurrence check",
    )

    subject = jobs["subject"]
    _require(
        subject.get("if") == DISPATCH_ONLY,
        "subject pinning must remain manual-only",
    )
    _require(
        "contract" in _needs(subject),
        "subject pinning must remain downstream of contract tests",
    )
    _step_named(subject, "subject", "Require current default-branch tip and resolve identities")
    _require_run(
        _run_texts(subject, "subject"),
        '"$DISPATCH_BRANCH" != "$DEFAULT_BRANCH"',
        "subject job must refuse non-default-branch dispatch",
    )
    _require_run(
        _run_texts(subject, "subject"),
        '"$SOURCE_SHA" != "$DEFAULT_SHA"',
        "subject job must refuse a stale dispatch subject",
    )

    row = jobs["installed-row"]
    _require(
        row.get("if") == DISPATCH_ONLY,
        "platform rows must remain manual-only",
    )
    _require(
        "subject" in _needs(row),
        "platform rows must remain downstream of the pinned subject",
    )
    matrix = row.get("strategy", {}).get("matrix", {}).get("include")
    _require(isinstance(matrix, list), "installed-row matrix must be an include list")
    _require(
        sorted((sorted(entry.items()) for entry in matrix if isinstance(entry, dict)))
        == sorted(sorted(entry.items()) for entry in EXPECTED_MATRIX),
        "installed-row matrix must keep exactly the canonical row tuples "
        "(linux-minimum, linux-current, windows-current with their exact "
        "platform/architecture/host-role bindings)",
    )

    verify = _step_named(row, "installed-row", "Verify checkout identity")
    _require(
        EXACT_CHECKOUT_TEST in "\n".join(_executable_lines(verify.get("run"))),
        "platform rows must verify their exact checkout SHA",
    )
    row_runs = _run_texts(row, "installed-row")
    _require_run(
        row_runs,
        RELEASE_BUILD,
        "workflow must build release-profile server and DAP subjects",
    )
    _require_run(
        row_runs,
        "rolling_installed_observation.py package",
        "workflow must create the release-shaped product-unit artifact",
    )
    _require_run(
        row_runs,
        "npm run test:published:local",
        "workflow must execute the existing packaged VSIX journey",
    )
    _require_run(
        row_runs,
        "PERL_LSP_FIRST_HOUR_SERVER_PATH",
        "packaged journey must receive the exact built server path",
    )
    _require_run(
        row_runs,
        "PERL_LSP_SERVER_SOURCE_SHA",
        "packaged journey must bind the server to exact source identity",
    )
    assemble = _step_named(
        row, "installed-row", "Assemble exact row without cross-surface inference"
    )
    _require(
        assemble.get("if") == "always()",
        "row assembly must run after failed smoke execution",
    )
    _require(
        "rolling_installed_observation.py row"
        in "\n".join(_executable_lines(assemble.get("run"))),
        "each platform must produce one typed row",
    )
    upload = _step_named(
        row, "installed-row", "Upload row, exact artifacts, and child receipts"
    )
    upload_with = upload.get("with") if isinstance(upload.get("with"), dict) else {}
    _require(
        upload_with.get("name") == ROW_ARTIFACT_NAME,
        "each row must upload under its own row-id artifact name",
    )

    fan_in = jobs["fan-in"]
    _require(
        isinstance(fan_in.get("if"), str) and "workflow_dispatch" in fan_in["if"],
        "fan-in must remain manual-only",
    )
    download = _step_named(fan_in, "fan-in", "Download all available platform rows")
    download_with = (
        download.get("with") if isinstance(download.get("with"), dict) else {}
    )
    _require(
        download_with.get("pattern") == ROW_DOWNLOAD_PATTERN,
        "fan-in must download exactly the per-row artifacts",
    )
    _require(
        not download_with.get("merge-multiple", False),
        "row artifacts must keep per-row directories; merge-multiple flattening "
        "collapses identical rolling-installed-row.json roots and loses rows",
    )
    fan_in_runs = _run_texts(fan_in, "fan-in")
    _require_run(
        fan_in_runs,
        "rolling_installed_observation.py fan-in",
        "workflow must produce the rolling fan-in packet",
    )
    _require_run(
        fan_in_runs,
        FAN_IN_PACKET,
        "workflow must retain the named rolling fan-in packet",
    )

    # Public-mutation boundary over executable run content only; commented-out
    # text is not an executable command.
    executable = "\n".join(
        line
        for name in jobs
        for text in _run_texts(jobs[name], name)
        for line in text.splitlines()
    ).lower()
    for forbidden in FORBIDDEN_MUTATIONS:
        if forbidden in executable:
            raise WorkflowError(
                f"rolling observation gained public mutation command: {forbidden}"
            )

    # A publishing step introduced through `uses:` contains no forbidden run
    # token, so the mutation boundary must also pin every action identity.
    for job_name in jobs:
        for step in _steps(jobs[job_name], job_name):
            uses = step.get("uses")
            if uses is None:
                continue
            _require(
                isinstance(uses, str),
                f"job {job_name} step uses must be a string: {uses!r}",
            )
            identity = uses.split("@", 1)[0]
            _require(
                identity in ALLOWED_ACTIONS,
                f"job {job_name} uses unapproved action {uses!r}",
            )


# ---------------------------------------------------------------------------
# Negative controls (text-level mutations that must all fail validation)
# ---------------------------------------------------------------------------


def replace_once(text: str, old: str, new: str, mutation: str) -> str:
    count = text.count(old)
    if count != 1:
        raise WorkflowError(
            f"negative-control setup {mutation} expected one {old!r}, found {count}"
        )
    return text.replace(old, new, 1)


def expect_failure(text: str, mutation: str) -> None:
    if mutation == "drop_windows":
        mutated = replace_once(text, "row_id: windows-current", "row_id: windows-dropped", mutation)
    elif mutation == "matrix_tuple_drift":
        mutated = replace_once(text, "os: windows-2022", "os: ubuntu-24.04", mutation)
    elif mutation == "drop_exact_checkout":
        mutated = replace_once(text, EXACT_CHECKOUT_TEST, "echo unchecked", mutation)
    elif mutation == "workspace_build":
        mutated = replace_once(
            text,
            RELEASE_BUILD,
            "cargo build -p perllsp --bin perllsp",
            mutation,
        )
    elif mutation == "comment_out_build":
        mutated = replace_once(text, RELEASE_BUILD, f"# {RELEASE_BUILD}", mutation)
    elif mutation == "drop_server_identity":
        count = text.count("PERL_LSP_SERVER_SOURCE_SHA")
        if count < 1:
            raise WorkflowError(
                "negative-control setup drop_server_identity found no source-identity binding"
            )
        mutated = text.replace(
            "PERL_LSP_SERVER_SOURCE_SHA",
            "REMOVED_SERVER_SOURCE_SHA",
        )
    elif mutation == "needs_contract_drift":
        mutated = replace_once(text, "needs: contract", "needs: contract-lite", mutation)
    elif mutation == "needs_subject_drift":
        mutated = replace_once(text, "needs: subject\n", "needs: subject-preview\n", mutation)
    elif mutation == "publishing_action":
        mutated = replace_once(
            text,
            "      - name: Run receipt and fan-in falsifiers",
            "      - name: Publish release\n"
            "        uses: softprops/action-gh-release@9d7c94cfd0d1f3ed45544c887983e9fa900fd056\n"
            "      - name: Run receipt and fan-in falsifiers",
            mutation,
        )
    elif mutation == "add_publish":
        mutated = replace_once(
            text,
            "python3 scripts/ci/check_rolling_installed_workflow.py",
            "python3 scripts/ci/check_rolling_installed_workflow.py\n          cargo publish",
            mutation,
        )
    elif mutation == "job_write_scope":
        mutated = replace_once(
            text,
            "  contract:\n    name: Validate rolling observation contract",
            "  contract:\n    permissions:\n      contents: write\n"
            "    name: Validate rolling observation contract",
            mutation,
        )
    elif mutation == "flatten_rows":
        mutated = replace_once(
            text,
            f"pattern: {ROW_DOWNLOAD_PATTERN}",
            f"pattern: {ROW_DOWNLOAD_PATTERN}\n          merge-multiple: true",
            mutation,
        )
    elif mutation == "cancel_running":
        mutated = replace_once(text, "cancel-in-progress: false", "cancel-in-progress: true", mutation)
    else:
        raise WorkflowError(f"unknown negative-control mutation {mutation}")

    try:
        validate(parse_workflow(mutated))
    except WorkflowError:
        return
    raise WorkflowError(f"negative control did not fail: {mutation}")


def main() -> int:
    try:
        text = WORKFLOW.read_text(encoding="utf-8")
        validate(parse_workflow(text))
        for mutation in (
            "drop_windows",
            "matrix_tuple_drift",
            "drop_exact_checkout",
            "workspace_build",
            "comment_out_build",
            "drop_server_identity",
            "needs_contract_drift",
            "needs_subject_drift",
            "publishing_action",
            "add_publish",
            "job_write_scope",
            "flatten_rows",
            "cancel_running",
        ):
            expect_failure(text, mutation)
    except (OSError, WorkflowError) as error:
        print(f"rolling installed workflow check failed: {error}", file=sys.stderr)
        return 1

    print(
        "rolling installed workflow: exact main, canonical row tuples, read-only "
        "permissions, per-row artifact identity, packaged VSIX execution, durable "
        "fan-in, and no-public-mutation boundary verified"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
