#!/usr/bin/env python3
"""Trusted-base ratchet for GitHub workflows and composite actions.

The scanner is stdlib-only and treats the candidate checkout as inert bytes. It
never imports or executes candidate code. The parser intentionally understands
only the GitHub Actions YAML structure needed by the registered rules; ambiguous
security-sensitive constructs fail closed instead of being interpreted as clean.
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import re
import stat
import sys
from collections import Counter
from pathlib import Path
from typing import Iterable, Sequence

SCHEMA_VERSION = 1
DEFAULT_MAX_FILES = 512
DEFAULT_MAX_FILE_BYTES = 2 * 1024 * 1024
DEFAULT_MAX_TOTAL_BYTES = 16 * 1024 * 1024
CONTROL_SOURCE_PATHS = (
    ".github/workflows/workflow-security-ratchet.yml",
    "scripts/ci/workflow_security_ratchet.py",
    "scripts/ci/test_workflow_security_ratchet.py",
)
FULL_SHA_RE = re.compile(r"^[0-9a-f]{40}$")
EXTERNAL_ACTION_RE = re.compile(r"^(?!\./)(?!docker://)([^/@\s]+/[^@\s]+)@([^\s#]+)$")
SECRET_RE = re.compile(r"\$\{\{\s*secrets\.")
CARGO_INSTALL_RE = re.compile(r"(?:^|[;&|]\s*|\s)cargo\s+install\s+([^\n]+)")
PERMISSION_KEYS = {
    "actions",
    "attestations",
    "checks",
    "contents",
    "deployments",
    "discussions",
    "id-token",
    "issues",
    "packages",
    "pages",
    "pull-requests",
    "repository-projects",
    "security-events",
    "statuses",
}
_KEY_LINE_RE = re.compile(
    r"^(?P<indent>\s*)(?P<list>-\s+)?(?:"
    r"(?P<quote>['\"])(?P<quoted>[^'\"]+)(?P=quote)|"
    r"(?P<plain>[A-Za-z0-9_-]+)"
    r")\s*:\s*(?P<value>.*)$"
)


@dataclasses.dataclass(frozen=True)
class ParsedKey:
    key: str
    value: str
    indent: int
    list_item: bool


@dataclasses.dataclass(frozen=True)
class RawFinding:
    rule: str
    path: str
    line: int
    evidence: str
    message: str


@dataclasses.dataclass(frozen=True)
class Finding:
    rule: str
    path: str
    line: int
    evidence: str
    message: str
    fingerprint: str

    def as_dict(self) -> dict[str, object]:
        return dataclasses.asdict(self)


def _parse_key_line(line: str) -> ParsedKey | None:
    match = _KEY_LINE_RE.match(line)
    if not match:
        return None
    key = match.group("quoted") or match.group("plain")
    list_marker = match.group("list") or ""
    return ParsedKey(
        key=key,
        value=match.group("value").strip(),
        indent=len(match.group("indent")) + len(list_marker),
        list_item=bool(list_marker),
    )


def _strip_scalar(value: str) -> str:
    value = value.split(" #", 1)[0].strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "'\"":
        return value[1:-1]
    return value


def _normalize_evidence(value: str) -> str:
    return re.sub(r"\s+", " ", value.strip())[:500]


def _fingerprint_findings(raw: Iterable[RawFinding]) -> list[Finding]:
    ordered = sorted(raw, key=lambda item: (item.path, item.rule, item.line, item.evidence))
    ordinals: Counter[tuple[str, str, str]] = Counter()
    findings: list[Finding] = []
    for item in ordered:
        normalized = _normalize_evidence(item.evidence)
        identity = (item.rule, item.path, normalized)
        ordinal = ordinals[identity]
        ordinals[identity] += 1
        digest = hashlib.sha256(
            f"{item.rule}\0{item.path}\0{normalized}\0{ordinal}".encode("utf-8")
        ).hexdigest()
        findings.append(
            Finding(
                rule=item.rule,
                path=item.path,
                line=item.line,
                evidence=normalized,
                message=item.message,
                fingerprint=digest,
            )
        )
    return findings


def _candidate_paths(root: Path) -> list[Path]:
    paths: list[Path] = []
    for relative in (Path(".github/workflows"), Path(".github/actions")):
        base = root / relative
        if not base.exists():
            continue
        for path in base.rglob("*"):
            if path.name.endswith((".yml", ".yaml")):
                paths.append(path)
    return sorted(paths, key=lambda path: path.relative_to(root).as_posix())


def _has_pr_trigger(lines: Sequence[str]) -> bool:
    if not lines:
        return False
    for index, line in enumerate(lines):
        parsed = _parse_key_line(line)
        if parsed is None or parsed.indent != 0 or parsed.key != "on":
            continue
        if "pull_request" in _strip_scalar(parsed.value):
            return True
        # Only a non-blank, non-comment line at column 0 ends the `on:` mapping.
        # Blank lines, comments, and block-sequence items ("- main" under
        # branches:) are not key lines, and treating them as terminators made
        # detection fail open on the common push-then-pull_request ordering.
        child_indent: int | None = None
        for candidate in lines[index + 1 :]:
            stripped = candidate.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if len(candidate) - len(candidate.lstrip()) == 0:
                break
            candidate_parsed = _parse_key_line(candidate)
            if candidate_parsed is None or candidate_parsed.list_item:
                continue
            if child_indent is None:
                child_indent = candidate_parsed.indent
            if candidate_parsed.indent != child_indent:
                continue
            if candidate_parsed.key in {"pull_request", "pull_request_target"}:
                return True
        return False
    return False


def _run_source_lines(lines: Sequence[str]) -> set[int]:
    run_lines: set[int] = set()
    index = 0
    while index < len(lines):
        parsed = _parse_key_line(lines[index])
        if parsed is None or parsed.key != "run":
            index += 1
            continue
        run_lines.add(index)
        if parsed.value.startswith(("|", ">")):
            cursor = index + 1
            while cursor < len(lines):
                candidate = lines[cursor]
                candidate_indent = len(candidate) - len(candidate.lstrip())
                if candidate.strip() and candidate_indent <= parsed.indent:
                    break
                run_lines.add(cursor)
                cursor += 1
            index = cursor
        else:
            index += 1
    return run_lines


def _checkout_persists(lines: Sequence[str], use_index: int, use_indent: int) -> bool:
    cursor = use_index + 1
    while cursor < len(lines):
        candidate = lines[cursor]
        parsed = _parse_key_line(candidate)
        # Stop at the next step in this list, and also at any dedent out of the
        # steps list (a sibling job or a new top-level block). Without the
        # dedent case the scan runs on into later jobs, and a persist-credentials
        # setting found there would clear this step's finding.
        if parsed and parsed.indent <= use_indent and (
            parsed.list_item or parsed.indent < use_indent
        ):
            break
        if (
            parsed
            and parsed.key == "persist-credentials"
            and _strip_scalar(parsed.value).lower() == "false"
        ):
            return False
        cursor += 1
    return True


def _cargo_install_pin_surface(args: str) -> str:
    """Keep pin checks on the cargo install argv, not later shell or comments."""
    without_comment = args.split("#", 1)[0]
    surface = without_comment
    for separator in ("&&", "||", ";", "|"):
        index = surface.find(separator)
        if index != -1:
            surface = surface[:index]
    return surface.strip()


def _cargo_install_is_pinned(command: str) -> bool:
    pin_surface = _cargo_install_pin_surface(command)
    return bool(
        re.search(r"(?:^|\s)--version(?:=|\s)\S+", pin_surface)
        or (
            re.search(r"(?:^|\s)--git(?:=|\s)\S+", pin_surface)
            and re.search(r"(?:^|\s)--rev(?:=|\s)\S+", pin_surface)
        )
        or re.search(r"(?:^|\s)--path(?:=|\s)\S+", pin_surface)
    )


def _run_line_with_continuations(
    lines: Sequence[str], index: int, run_lines: set[int]
) -> str:
    """Return one shell command with backslash continuations folded together."""
    command = lines[index]
    cursor = index
    while command.rstrip().endswith("\\"):
        next_index = cursor + 1
        if next_index not in run_lines:
            break
        command = command.rstrip()[:-1] + " " + lines[next_index].lstrip()
        cursor = next_index
    return command


def _security_sensitive_indirection(line: str) -> bool:
    stripped = line.strip()
    if stripped.startswith(("<<:", "- *")):
        return True
    if stripped.startswith(("{", "- {")) and re.search(
        r"(?:^|[,{])\s*(?:['\"])?(?:run|uses|permissions)(?:['\"])?\s*:",
        stripped,
    ):
        return True
    parsed = _parse_key_line(line)
    if parsed is None or parsed.key not in {
        "run",
        "uses",
        "permissions",
        *PERMISSION_KEYS,
    }:
        return False
    value = parsed.value.strip()
    return value.startswith(("*", "&", "{"))


def scan(
    root: Path,
    *,
    max_files: int = DEFAULT_MAX_FILES,
    max_file_bytes: int = DEFAULT_MAX_FILE_BYTES,
    max_total_bytes: int = DEFAULT_MAX_TOTAL_BYTES,
) -> list[Finding]:
    root = root.resolve()
    raw: list[RawFinding] = []
    paths = _candidate_paths(root)
    if len(paths) > max_files:
        raw.append(
            RawFinding(
                "candidate_file_count_exceeded",
                ".github",
                0,
                str(len(paths)),
                f"candidate contains {len(paths)} workflow/action files; limit is {max_files}",
            )
        )
        paths = paths[:max_files]

    total_bytes = 0
    for path in paths:
        relative = path.relative_to(root).as_posix()
        try:
            metadata = path.lstat()
        except OSError as error:
            raw.append(
                RawFinding(
                    "candidate_stat_failed",
                    relative,
                    0,
                    str(error),
                    "candidate file could not be inspected",
                )
            )
            continue
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
            raw.append(
                RawFinding(
                    "unsafe_candidate_file",
                    relative,
                    0,
                    oct(metadata.st_mode),
                    "candidate workflow/action must be a regular non-symlink file",
                )
            )
            continue
        if metadata.st_size > max_file_bytes:
            raw.append(
                RawFinding(
                    "candidate_file_oversized",
                    relative,
                    0,
                    str(metadata.st_size),
                    f"candidate file exceeds {max_file_bytes} bytes",
                )
            )
            continue
        total_bytes += metadata.st_size
        if total_bytes > max_total_bytes:
            raw.append(
                RawFinding(
                    "candidate_total_oversized",
                    relative,
                    0,
                    str(total_bytes),
                    f"candidate workflow/action bytes exceed {max_total_bytes}",
                )
            )
            break
        try:
            payload = path.read_bytes()
            text = payload.decode("utf-8", errors="strict")
        except UnicodeDecodeError as error:
            raw.append(
                RawFinding(
                    "malformed_utf8",
                    relative,
                    0,
                    str(error),
                    "candidate file is not valid UTF-8",
                )
            )
            continue
        except OSError as error:
            raw.append(
                RawFinding(
                    "candidate_read_failed",
                    relative,
                    0,
                    str(error),
                    "candidate file could not be read",
                )
            )
            continue
        if "\x00" in text:
            raw.append(
                RawFinding(
                    "malformed_nul",
                    relative,
                    0,
                    "NUL",
                    "candidate file contains a NUL byte",
                )
            )
            continue

        lines = text.splitlines()
        is_workflow = relative.startswith(".github/workflows/")
        pr_trigger = is_workflow and _has_pr_trigger(lines)
        run_lines = _run_source_lines(lines)
        parsed_lines = [_parse_key_line(line) for line in lines]
        write_all = any(
            parsed
            and parsed.key == "permissions"
            and _strip_scalar(parsed.value) == "write-all"
            for parsed in parsed_lines
        )
        write_surface = is_workflow and (
            write_all
            or any(
                parsed
                and parsed.key in PERMISSION_KEYS
                and _strip_scalar(parsed.value) == "write"
                for parsed in parsed_lines
            )
        )

        for index, line in enumerate(lines):
            line_number = index + 1
            parsed = parsed_lines[index]
            if parsed and parsed.key == "uses":
                action = _strip_scalar(parsed.value)
                external = EXTERNAL_ACTION_RE.match(action)
                if external and not FULL_SHA_RE.fullmatch(external.group(2)):
                    raw.append(
                        RawFinding(
                            "mutable_action_ref",
                            relative,
                            line_number,
                            action,
                            "external action ref is not a full commit SHA",
                        )
                    )
                if (
                    write_surface
                    and external
                    and external.group(1) == "actions/checkout"
                    and _checkout_persists(lines, index, parsed.indent)
                ):
                    raw.append(
                        RawFinding(
                            "checkout_persists_credentials_on_write_surface",
                            relative,
                            line_number,
                            action,
                            "write-capable workflow checkout must set persist-credentials: false",
                        )
                    )

            if _security_sensitive_indirection(line):
                raw.append(
                    RawFinding(
                        "unsupported_security_yaml_indirection",
                        relative,
                        line_number,
                        line,
                        "security-sensitive YAML indirection is not accepted by the ratchet",
                    )
                )

            if index in run_lines and "${{" in line:
                raw.append(
                    RawFinding(
                        "expression_in_run_source",
                        relative,
                        line_number,
                        line,
                        "GitHub expression is embedded in interpreter source; pass it through env as data",
                    )
                )

            if index in run_lines:
                install = CARGO_INSTALL_RE.search(
                    _run_line_with_continuations(lines, index, run_lines)
                )
                if install and not _cargo_install_is_pinned(install.group(1)):
                    raw.append(
                        RawFinding(
                            "floating_cargo_install",
                            relative,
                            line_number,
                            line,
                            "cargo install must use --version, --path, or --git with --rev",
                        )
                    )

            if pr_trigger and parsed:
                if (
                    parsed.key == "permissions"
                    and _strip_scalar(parsed.value) == "write-all"
                ):
                    raw.append(
                        RawFinding(
                            "pr_write_permission",
                            relative,
                            line_number,
                            line,
                            "PR-triggered workflow grants write-all authority",
                        )
                    )
                elif (
                    parsed.key in PERMISSION_KEYS
                    and _strip_scalar(parsed.value) == "write"
                ):
                    raw.append(
                        RawFinding(
                            "pr_write_permission",
                            relative,
                            line_number,
                            line,
                            "PR-triggered workflow grants write authority",
                        )
                    )
            if pr_trigger and SECRET_RE.search(line):
                raw.append(
                    RawFinding(
                        "pr_secret_reference",
                        relative,
                        line_number,
                        line,
                        "PR-triggered workflow references a secret",
                    )
                )

    return _fingerprint_findings(raw)


def _control_digests(root: Path) -> dict[str, str]:
    root = root.resolve()
    digests: dict[str, str] = {}
    for relative in CONTROL_SOURCE_PATHS:
        path = root / relative
        try:
            metadata = path.lstat()
        except OSError as error:
            raise ValueError(
                f"required workflow-security control is unavailable: {relative}: {error}"
            ) from error
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
            raise ValueError(
                f"required workflow-security control is not a regular file: {relative}"
            )
        digests[relative] = "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()
    return digests


def _baseline_payload(root: Path, findings: Sequence[Finding]) -> dict[str, object]:
    return {
        "schema_version": SCHEMA_VERSION,
        "control_files": _control_digests(root),
        "findings": [finding.as_dict() for finding in findings],
    }


def _load_baseline(path: Path, *, root: Path | None = None) -> dict[str, Finding]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if payload.get("schema_version") != SCHEMA_VERSION or not isinstance(
        payload.get("findings"), list
    ):
        raise ValueError(f"unsupported baseline schema in {path}")
    items = payload["findings"]
    raw: list[RawFinding] = []
    for item in items:
        if not isinstance(item, dict):
            raise ValueError(f"baseline finding is not an object in {path}")
        try:
            raw.append(
                RawFinding(
                    rule=str(item["rule"]),
                    path=str(item["path"]),
                    line=int(item["line"]),
                    evidence=str(item["evidence"]),
                    message=str(item["message"]),
                )
            )
        except (KeyError, TypeError, ValueError) as error:
            raise ValueError(f"invalid baseline finding in {path}: {error}") from error
    canonical = _fingerprint_findings(raw)
    canonical_items = [finding.as_dict() for finding in canonical]
    if items != canonical_items:
        raise ValueError(
            "baseline findings are not canonically ordered or contain invalid "
            f"fingerprints: {path}"
        )
    control_files = payload.get("control_files")
    if not isinstance(control_files, dict):
        raise ValueError(f"baseline control_files is missing or invalid in {path}")
    if root is not None and control_files != _control_digests(root):
        raise ValueError(f"baseline control digests do not match {root}")
    result: dict[str, Finding] = {}
    for finding in canonical:
        if finding.fingerprint in result:
            raise ValueError(
                f"duplicate baseline fingerprint in {path}: {finding.fingerprint}"
            )
        result[finding.fingerprint] = finding
    return result


def _write_json(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def write_baseline(root: Path, output: Path, args: argparse.Namespace) -> int:
    findings = scan(
        root,
        max_files=args.max_files,
        max_file_bytes=args.max_file_bytes,
        max_total_bytes=args.max_total_bytes,
    )
    _write_json(output, _baseline_payload(root, findings))
    print(f"workflow security baseline: {len(findings)} finding(s) -> {output}")
    return 0


def _comparison_payload(
    current: dict[str, Finding], baseline: dict[str, Finding], *, exact: bool
) -> tuple[dict[str, object], int]:
    new_ids = sorted(current.keys() - baseline.keys())
    existing_ids = sorted(current.keys() & baseline.keys())
    resolved_ids = sorted(baseline.keys() - current.keys())
    failed = bool(new_ids or (exact and resolved_ids))
    payload: dict[str, object] = {
        "schema_version": SCHEMA_VERSION,
        "mode": "exact" if exact else "ratchet",
        "status": "fail" if failed else "pass",
        "counts": {
            "current": len(current),
            "new": len(new_ids),
            "existing": len(existing_ids),
            "resolved": len(resolved_ids),
        },
        "new": [current[item].as_dict() for item in new_ids],
        "existing": [current[item].as_dict() for item in existing_ids],
        "resolved": [baseline[item].as_dict() for item in resolved_ids],
    }
    return payload, 1 if failed else 0


def _print_comparison(payload: dict[str, object]) -> None:
    counts = payload["counts"]
    assert isinstance(counts, dict)
    print(
        "workflow security "
        f"{payload['mode']}: current={counts['current']} new={counts['new']} "
        f"existing={counts['existing']} resolved={counts['resolved']}"
    )
    for item in payload["new"]:
        assert isinstance(item, dict)
        print(
            f"NEW {item['path']}:{item['line']} [{item['rule']}] "
            f"{item['message']}"
        )
        print(f"    {item['evidence']}")
    for item in payload["resolved"]:
        assert isinstance(item, dict)
        print(f"RESOLVED {item['path']} [{item['rule']}] {item['evidence']}")


def check(
    root: Path,
    baseline_path: Path,
    report_path: Path | None,
    args: argparse.Namespace,
    *,
    exact: bool,
) -> int:
    current = {
        finding.fingerprint: finding
        for finding in scan(
            root,
            max_files=args.max_files,
            max_file_bytes=args.max_file_bytes,
            max_total_bytes=args.max_total_bytes,
        )
    }
    baseline_root = args.baseline_root if args.baseline_root is not None else None
    baseline = _load_baseline(baseline_path, root=baseline_root)
    if exact:
        candidate_baseline = baseline_path.resolve()
        expected_baseline = (
            root.resolve() / ".ci/workflow-security-baseline.json"
        ).resolve()
        if candidate_baseline != expected_baseline:
            raise ValueError(
                f"exact baseline must be {expected_baseline}, got {candidate_baseline}"
            )
        _control_digests(root)
    payload, status = _comparison_payload(current, baseline, exact=exact)
    if report_path:
        _write_json(report_path, payload)
    _print_comparison(payload)
    return status


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--max-files", type=int, default=DEFAULT_MAX_FILES)
    parser.add_argument("--max-file-bytes", type=int, default=DEFAULT_MAX_FILE_BYTES)
    parser.add_argument("--max-total-bytes", type=int, default=DEFAULT_MAX_TOTAL_BYTES)
    subparsers = parser.add_subparsers(dest="command", required=True)

    baseline = subparsers.add_parser("baseline")
    baseline.add_argument("--root", type=Path, required=True)
    baseline.add_argument("--output", type=Path, required=True)

    for command in ("check", "validate-baseline"):
        check_parser = subparsers.add_parser(command)
        check_parser.add_argument("--root", type=Path, required=True)
        check_parser.add_argument("--baseline", type=Path, required=True)
        check_parser.add_argument("--baseline-root", type=Path)
        check_parser.add_argument("--report", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "baseline":
            return write_baseline(args.root, args.output, args)
        return check(
            args.root,
            args.baseline,
            args.report,
            args,
            exact=args.command == "validate-baseline",
        )
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"workflow security ratchet error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
