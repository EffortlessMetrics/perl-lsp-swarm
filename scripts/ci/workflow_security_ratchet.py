#!/usr/bin/env python3
"""Trusted-base ratchet for GitHub workflows and composite actions.

The scanner is intentionally stdlib-only and treats the candidate checkout as
inert bytes. It never imports or executes candidate code.
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import os
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
ACTION_RE = re.compile(r"\buses:\s*([^\s#]+)")
EXTERNAL_ACTION_RE = re.compile(r"^(?!\./)(?!docker://)([^/@\s]+/[^@\s]+)@([^\s#]+)$")
FULL_SHA_RE = re.compile(r"^[0-9a-f]{40}$")
WRITE_PERMISSION_RE = re.compile(
    r"^\s*(actions|attestations|checks|contents|deployments|discussions|id-token|issues|packages|pages|pull-requests|repository-projects|security-events|statuses):\s*write\s*(?:#.*)?$"
)
SECRET_RE = re.compile(r"\$\{\{\s*secrets\.")
CARGO_INSTALL_RE = re.compile(r"(?:^|[;&|]\s*|\s)cargo\s+install\s+([^\n]+)")


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
        stripped = line.strip()
        indent = len(line) - len(line.lstrip())
        if indent == 0 and stripped.startswith("on:"):
            inline = stripped[3:].strip()
            if "pull_request" in inline:
                return True
            for candidate in lines[index + 1 :]:
                candidate_stripped = candidate.strip()
                candidate_indent = len(candidate) - len(candidate.lstrip())
                if candidate_stripped and candidate_indent == 0:
                    break
                if re.match(r"^pull_request(?:_target)?:", candidate_stripped):
                    return True
            return False
    return False


def _run_source_lines(lines: Sequence[str]) -> set[int]:
    run_lines: set[int] = set()
    index = 0
    while index < len(lines):
        line = lines[index]
        match = re.match(r"^(\s*)run:\s*(.*)$", line)
        if not match:
            index += 1
            continue
        base_indent = len(match.group(1))
        value = match.group(2).strip()
        run_lines.add(index)
        if value.startswith(("|", ">")):
            cursor = index + 1
            while cursor < len(lines):
                candidate = lines[cursor]
                candidate_indent = len(candidate) - len(candidate.lstrip())
                if candidate.strip() and candidate_indent <= base_indent:
                    break
                run_lines.add(cursor)
                cursor += 1
            index = cursor
        else:
            index += 1
    return run_lines


def _checkout_persists(lines: Sequence[str], use_index: int) -> bool:
    use_indent = len(lines[use_index]) - len(lines[use_index].lstrip())
    cursor = use_index + 1
    while cursor < len(lines):
        candidate = lines[cursor]
        stripped = candidate.strip()
        indent = len(candidate) - len(candidate.lstrip())
        if stripped and indent <= use_indent and stripped.startswith("-"):
            break
        if re.match(r"^persist-credentials:\s*false\s*(?:#.*)?$", stripped):
            return False
        cursor += 1
    return True


def _cargo_install_is_pinned(command: str) -> bool:
    return bool(
        re.search(r"(?:^|\s)--version(?:=|\s)\S+", command)
        or (re.search(r"(?:^|\s)--git(?:=|\s)\S+", command) and re.search(r"(?:^|\s)--rev(?:=|\s)\S+", command))
        or re.search(r"(?:^|\s)--path(?:=|\s)\S+", command)
    )


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
            raw.append(RawFinding("candidate_stat_failed", relative, 0, str(error), "candidate file could not be inspected"))
            continue
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
            raw.append(RawFinding("unsafe_candidate_file", relative, 0, oct(metadata.st_mode), "candidate workflow/action must be a regular non-symlink file"))
            continue
        if metadata.st_size > max_file_bytes:
            raw.append(RawFinding("candidate_file_oversized", relative, 0, str(metadata.st_size), f"candidate file exceeds {max_file_bytes} bytes"))
            continue
        total_bytes += metadata.st_size
        if total_bytes > max_total_bytes:
            raw.append(RawFinding("candidate_total_oversized", relative, 0, str(total_bytes), f"candidate workflow/action bytes exceed {max_total_bytes}"))
            break
        try:
            payload = path.read_bytes()
            text = payload.decode("utf-8", errors="strict")
        except UnicodeDecodeError as error:
            raw.append(RawFinding("malformed_utf8", relative, 0, str(error), "candidate file is not valid UTF-8"))
            continue
        except OSError as error:
            raw.append(RawFinding("candidate_read_failed", relative, 0, str(error), "candidate file could not be read"))
            continue
        if "\x00" in text:
            raw.append(RawFinding("malformed_nul", relative, 0, "NUL", "candidate file contains a NUL byte"))
            continue

        lines = text.splitlines()
        is_workflow = relative.startswith(".github/workflows/")
        pr_trigger = is_workflow and _has_pr_trigger(lines)
        run_lines = _run_source_lines(lines)
        write_surface = is_workflow and any(WRITE_PERMISSION_RE.match(line) for line in lines)

        for index, line in enumerate(lines):
            line_number = index + 1
            action_match = ACTION_RE.search(line)
            if action_match:
                action = action_match.group(1).strip("'\"")
                external = EXTERNAL_ACTION_RE.match(action)
                if external and not FULL_SHA_RE.fullmatch(external.group(2)):
                    raw.append(RawFinding("mutable_action_ref", relative, line_number, action, "external action ref is not a full commit SHA"))
                if write_surface and external and external.group(1) == "actions/checkout" and _checkout_persists(lines, index):
                    raw.append(RawFinding("checkout_persists_credentials_on_write_surface", relative, line_number, action, "write-capable workflow checkout must set persist-credentials: false"))

            if index in run_lines and "${{" in line:
                raw.append(RawFinding("expression_in_run_source", relative, line_number, line, "GitHub expression is embedded in interpreter source; pass it through env as data"))

            if index in run_lines:
                install = CARGO_INSTALL_RE.search(line)
                if install and not _cargo_install_is_pinned(install.group(1)):
                    raw.append(RawFinding("floating_cargo_install", relative, line_number, line, "cargo install must use --version, --path, or --git with --rev"))

            if pr_trigger and WRITE_PERMISSION_RE.match(line):
                raw.append(RawFinding("pr_write_permission", relative, line_number, line, "PR-triggered workflow grants write authority"))
            if pr_trigger and SECRET_RE.search(line):
                raw.append(RawFinding("pr_secret_reference", relative, line_number, line, "PR-triggered workflow references a secret"))

    return _fingerprint_findings(raw)


def _load_baseline(path: Path) -> dict[str, Finding]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if payload.get("schema_version") != SCHEMA_VERSION or not isinstance(payload.get("findings"), list):
        raise ValueError(f"unsupported baseline schema in {path}")
    result: dict[str, Finding] = {}
    for item in payload["findings"]:
        finding = Finding(**item)
        result[finding.fingerprint] = finding
    return result


def _write_json(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_baseline(root: Path, output: Path, args: argparse.Namespace) -> int:
    findings = scan(root, max_files=args.max_files, max_file_bytes=args.max_file_bytes, max_total_bytes=args.max_total_bytes)
    _write_json(output, {"schema_version": SCHEMA_VERSION, "findings": [finding.as_dict() for finding in findings]})
    print(f"workflow security baseline: {len(findings)} finding(s) -> {output}")
    return 0


def check(root: Path, baseline_path: Path, report_path: Path | None, args: argparse.Namespace) -> int:
    current = {finding.fingerprint: finding for finding in scan(root, max_files=args.max_files, max_file_bytes=args.max_file_bytes, max_total_bytes=args.max_total_bytes)}
    baseline = _load_baseline(baseline_path)
    new_ids = sorted(current.keys() - baseline.keys())
    existing_ids = sorted(current.keys() & baseline.keys())
    resolved_ids = sorted(baseline.keys() - current.keys())
    payload = {
        "schema_version": SCHEMA_VERSION,
        "status": "fail" if new_ids else "pass",
        "counts": {"current": len(current), "new": len(new_ids), "existing": len(existing_ids), "resolved": len(resolved_ids)},
        "new": [current[item].as_dict() for item in new_ids],
        "existing": [current[item].as_dict() for item in existing_ids],
        "resolved": [baseline[item].as_dict() for item in resolved_ids],
    }
    if report_path:
        _write_json(report_path, payload)
    print(f"workflow security ratchet: current={len(current)} new={len(new_ids)} existing={len(existing_ids)} resolved={len(resolved_ids)}")
    for item in payload["new"]:
        print(f"NEW {item['path']}:{item['line']} [{item['rule']}] {item['message']}")
        print(f"    {item['evidence']}")
    for item in payload["resolved"]:
        print(f"RESOLVED {item['path']} [{item['rule']}] {item['evidence']}")
    return 1 if new_ids else 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--max-files", type=int, default=DEFAULT_MAX_FILES)
    parser.add_argument("--max-file-bytes", type=int, default=DEFAULT_MAX_FILE_BYTES)
    parser.add_argument("--max-total-bytes", type=int, default=DEFAULT_MAX_TOTAL_BYTES)
    subparsers = parser.add_subparsers(dest="command", required=True)
    baseline = subparsers.add_parser("baseline")
    baseline.add_argument("--root", type=Path, required=True)
    baseline.add_argument("--output", type=Path, required=True)
    check_parser = subparsers.add_parser("check")
    check_parser.add_argument("--root", type=Path, required=True)
    check_parser.add_argument("--baseline", type=Path, required=True)
    check_parser.add_argument("--report", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "baseline":
            return write_baseline(args.root, args.output, args)
        return check(args.root, args.baseline, args.report, args)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"workflow security ratchet error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
