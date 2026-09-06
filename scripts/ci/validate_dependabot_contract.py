#!/usr/bin/env python3
"""Stdlib-only Dependabot config/guidance source contract (#13478).

Proves `.github/dependabot.yml` stays aligned with the three canonical
contributor guides and that the book contributing stub remains a pointer.
Does not re-derive GitHub's `include: "scope"` composition rule; that
oracle is owned by the landed #13477 / #14180 xtask suites. This checker
only asserts the committed scalars those suites already proved.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

CONFIG_PATH = Path(".github/dependabot.yml")
MANAGEMENT_GUIDE = Path("docs/how-to/DEPENDENCY_MANAGEMENT.md")
QUICK_REFERENCE = Path("docs/how-to/DEPENDENCY_QUICK_REFERENCE.md")
CONTRIBUTING_GUIDE = Path("CONTRIBUTING.md")
BOOK_POINTER = Path("book/src/developer/contributing.md")
POPULATE_BOOK = Path("scripts/populate-book.sh")
POLICY_WORKFLOW = Path(".github/workflows/policy-validators.yml")

CANONICAL_GUIDES = (CONTRIBUTING_GUIDE, MANAGEMENT_GUIDE, QUICK_REFERENCE)
GOVERNED_SURFACES = (
    CONFIG_PATH,
    *CANONICAL_GUIDES,
    BOOK_POINTER,
    POPULATE_BOOK,
)

EXPECTED_ROWS = (
    ("cargo", "/"),
    ("github-actions", "/"),
    ("npm", "/vscode-extension"),
)
EXPECTED_SCHEDULE = {
    "interval": "weekly",
    "day": "monday",
    "time": "09:00",
    "timezone": "UTC",
}
COMMIT_PREFIX = "chore"
COMMIT_INCLUDE = "scope"
PLACEHOLDER_NAMES = frozenset(
    {"crate-name", "pattern*", "group-name", "my-group", "custom-label"}
)

KEY_RE = re.compile(r"^[A-Za-z0-9_-]+$")
DISCOVERY_RE = re.compile(
    r"""--author\s+(?:["']app/dependabot["']|app/dependabot)"""
)
LABEL_FILTER_RE = re.compile(
    r"""gh\s+pr\s+list[^\n]*--label\s+["']?dependencies["']?"""
    r"""|gh\s+pr\s+list[^\n]*label:dependencies"""
)
PIPE_MERGE_RE = re.compile(
    r"gh\s+pr\s+list[\s\S]{0,240}?\|[\s\S]{0,80}?gh\s+pr\s+merge"
)
XARGS_MERGE_RE = re.compile(r"xargs\s+gh\s+pr\s+merge")
SUBSHELL_MERGE_RE = re.compile(r"gh\s+pr\s+merge\s+\$\(\s*gh\s+pr\s+list")
MERGE_TARGET_RE = re.compile(r"gh\s+pr\s+merge\s+(\S+)")
VERSION_DELTA_RE = re.compile(
    r"version (?:delta|table)|highest semver|inspect(?:ing)? the (?:exact )?version",
    re.I,
)
DEPENDENCY_NAME_RE = re.compile(
    r"""dependency-name:\s*["']([^"']+)["']"""
)
CONTRIBUTING_POINTER_RE = re.compile(
    r"github\.com/EffortlessMetrics/perl-lsp-swarm/blob/main/CONTRIBUTING\.md"
)
OVERWRITE_BOOK_RE = re.compile(
    r"(?:copy_\w+|cp)\s+[^\n]*CONTRIBUTING\.md[^\n]*developer/contributing\.md",
    re.I,
)
CODE_FENCE_RE = re.compile(r"```(?:bash|sh|shell|zsh)?\n(.*?)```", re.S)

VALIDATOR_SCRIPT = Path("scripts/ci/validate_dependabot_contract.py")
VALIDATOR_TESTS = Path("scripts/ci/test_validate_dependabot_contract.py")
REQUIRED_WORKFLOW_PATHS = (
    CONFIG_PATH,
    *CANONICAL_GUIDES,
    BOOK_POINTER,
    POPULATE_BOOK,
    VALIDATOR_SCRIPT,
    VALIDATOR_TESTS,
)
REQUIRED_TEST_RUN = "python3 scripts/ci/test_validate_dependabot_contract.py"
REQUIRED_VALIDATOR_RUN = (
    "python3 scripts/ci/validate_dependabot_contract.py --repo-root ."
)


class ParseError(ValueError):
    """Bounded YAML subset rejected this document."""

    def __init__(self, message: str, line: int | None = None) -> None:
        self.line = line
        if line is None:
            super().__init__(message)
        else:
            super().__init__(f"line {line}: {message}")


class ValidationError(ValueError):
    """Instrument failure: a governed source cannot be read."""


@dataclass(frozen=True, order=True)
class Finding:
    finding_id: str
    path: str
    message: str

    def line(self) -> str:
        return f"{self.finding_id} {self.path}: {self.message}"


@dataclass(frozen=True)
class _PhysicalLine:
    no: int
    indent: int
    content: str


def _has_unquoted(content: str, needle: str) -> bool:
    in_quote: str | None = None
    i = 0
    while i < len(content):
        ch = content[i]
        if in_quote:
            if ch == "\\" and in_quote == '"':
                i += 2
                continue
            if ch == in_quote:
                in_quote = None
            i += 1
            continue
        if ch in "\"'":
            in_quote = ch
            i += 1
            continue
        if content.startswith(needle, i):
            return True
        i += 1
    return False


def _split_unquoted_colon(content: str) -> tuple[str, str] | None:
    in_quote: str | None = None
    for i, ch in enumerate(content):
        if in_quote:
            if ch == in_quote and (i == 0 or content[i - 1] != "\\"):
                in_quote = None
            continue
        if ch in "\"'":
            in_quote = ch
            continue
        if ch == ":":
            return content[:i], content[i + 1 :]
    return None


def _parse_flow_sequence(text: str, line: int) -> list[object]:
    raw = text.strip()
    if not (raw.startswith("[") and raw.endswith("]")):
        raise ParseError("flow sequence must be [ ... ]", line)
    inner = raw[1:-1].strip()
    if not inner:
        return []
    items: list[object] = []
    i = 0
    n = len(inner)
    while i < n:
        while i < n and inner[i] in " \t":
            i += 1
        if i >= n:
            break
        if inner[i] in "[{":
            raise ParseError("nested flow collections are unsupported", line)
        if inner[i] in "\"'":
            quote = inner[i]
            j = i + 1
            buf: list[str] = []
            while j < n:
                if inner[j] == "\\" and quote == '"':
                    j += 1
                    if j >= n:
                        raise ParseError("unterminated escape", line)
                    buf.append(inner[j])
                    j += 1
                    continue
                if inner[j] == quote:
                    break
                buf.append(inner[j])
                j += 1
            else:
                raise ParseError("unterminated quoted string", line)
            items.append("".join(buf))
            i = j + 1
        else:
            j = i
            while j < n and inner[j] != ",":
                j += 1
            token = inner[i:j].strip()
            items.append(_unquoted_scalar(token, line))
            i = j
        while i < n and inner[i] in " \t":
            i += 1
        if i >= n:
            break
        if inner[i] != ",":
            raise ParseError("expected comma in flow sequence", line)
        i += 1
    return items


def _unquoted_scalar(token: str, line: int) -> object:
    if not token:
        raise ParseError("empty unquoted scalar", line)
    if token[0] in "&*":
        raise ParseError("YAML anchors and aliases are unsupported", line)
    if re.fullmatch(r"-?\d+", token):
        return int(token)
    return token


def _parse_scalar(raw: str, line: int) -> object:
    text = raw.strip()
    if text.startswith("#"):
        raise ParseError("missing scalar value", line)
    if text.startswith("["):
        end = text.rfind("]")
        if end < 0:
            raise ParseError("unterminated flow sequence", line)
        seq = _parse_flow_sequence(text[: end + 1], line)
        leftover = text[end + 1 :].strip()
        if leftover and not leftover.startswith("#"):
            raise ParseError("trailing content after flow sequence", line)
        return seq
    if text[0] in "\"'":
        quote = text[0]
        j = 1
        buf: list[str] = []
        while j < len(text):
            ch = text[j]
            if ch == "\\" and quote == '"':
                j += 1
                if j >= len(text):
                    raise ParseError("unterminated escape", line)
                buf.append(text[j])
                j += 1
                continue
            if ch == quote:
                leftover = text[j + 1 :].strip()
                if leftover and not leftover.startswith("#"):
                    raise ParseError("trailing content after quoted scalar", line)
                return "".join(buf)
            buf.append(ch)
            j += 1
        raise ParseError("unterminated quoted string", line)
    if " #" in text:
        text = text.split(" #", 1)[0].rstrip()
    if text in {"|", ">", "|-", "|+", ">-", ">+"}:
        raise ParseError("multiline scalars are unsupported", line)
    return _unquoted_scalar(text, line)


def _split_key(content: str, line: int) -> tuple[str, str | None]:
    split = _split_unquoted_colon(content)
    if split is None:
        raise ParseError("mapping line is missing ':'", line)
    key_raw, rest_raw = split
    key = key_raw.strip()
    rest = rest_raw.strip()
    if not KEY_RE.match(key):
        raise ParseError(f"unsupported key {key!r}", line)
    if rest.startswith("#"):
        rest = ""
    return key, None if rest == "" else rest


def _physical_lines(text: str) -> list[_PhysicalLine]:
    if "\t" in text:
        raise ParseError("tabs are unsupported")
    lines: list[_PhysicalLine] = []
    for no, raw in enumerate(text.replace("\r\n", "\n").split("\n"), 1):
        if raw.strip() == "" or raw.strip().startswith("#"):
            continue
        if raw.strip() == "---":
            continue
        indent = len(raw) - len(raw.lstrip(" "))
        content = raw[indent:]
        if _has_unquoted(content, "{") or _has_unquoted(content, "<<") or _has_unquoted(
            content, "!!"
        ):
            raise ParseError("unsupported YAML construct", no)
        lines.append(_PhysicalLine(no, indent, content))
    return lines


class _Parser:
    def __init__(self, lines: list[_PhysicalLine]) -> None:
        self.lines = lines
        self.i = 0

    def peek(self) -> _PhysicalLine | None:
        if self.i >= len(self.lines):
            return None
        return self.lines[self.i]

    def parse_document(self) -> dict[str, object]:
        value = self.parse_mapping(0)
        leftover = self.peek()
        if leftover is not None:
            raise ParseError("trailing content after document mapping", leftover.no)
        if not isinstance(value, dict):
            raise ParseError("document must be a mapping")
        return value

    def parse_mapping(self, indent: int) -> dict[str, object]:
        result: dict[str, object] = {}
        while True:
            line = self.peek()
            if line is None or line.indent < indent:
                break
            if line.indent > indent:
                raise ParseError("unexpected indent", line.no)
            if line.content.startswith("- "):
                raise ParseError("sequence item where mapping key expected", line.no)
            self.i += 1
            key, rest = _split_key(line.content, line.no)
            if key in result:
                raise ParseError(f"duplicate key {key!r}", line.no)
            result[key] = self._parse_value(indent, rest, line.no)
        return result

    def _parse_value(
        self, parent_indent: int, rest: str | None, line_no: int
    ) -> object:
        if rest is not None:
            return _parse_scalar(rest, line_no)
        nxt = self.peek()
        if nxt is None or nxt.indent <= parent_indent:
            return None
        if nxt.content.startswith("- "):
            return self.parse_sequence(nxt.indent)
        return self.parse_mapping(nxt.indent)

    def parse_sequence(self, indent: int) -> list[object]:
        items: list[object] = []
        while True:
            line = self.peek()
            if line is None or line.indent < indent:
                break
            if line.indent > indent:
                raise ParseError("unexpected indent in sequence", line.no)
            if not line.content.startswith("- "):
                raise ParseError("expected sequence item", line.no)
            self.i += 1
            body = line.content[2:]
            body_indent = indent + 2
            if _looks_like_mapping_entry(body):
                key, rest = _split_key(body, line.no)
                mapping: dict[str, object] = {}
                mapping[key] = (
                    _parse_scalar(rest, line.no)
                    if rest is not None
                    else self._nested_or_null(body_indent)
                )
                extra = self.parse_mapping(body_indent)
                for extra_key, extra_value in extra.items():
                    if extra_key in mapping:
                        raise ParseError(f"duplicate key {extra_key!r}", line.no)
                    mapping[extra_key] = extra_value
                items.append(mapping)
            else:
                items.append(_parse_scalar(body, line.no))
        return items

    def _nested_or_null(self, indent: int) -> object:
        nxt = self.peek()
        if nxt is None or nxt.indent <= indent:
            return None
        if nxt.content.startswith("- "):
            return self.parse_sequence(nxt.indent)
        return self.parse_mapping(nxt.indent)


def _looks_like_mapping_entry(body: str) -> bool:
    split = _split_unquoted_colon(body)
    if split is None:
        return False
    key = split[0].strip()
    return bool(KEY_RE.match(key))


def parse_yaml_subset(text: str) -> dict[str, object]:
    """Parse the bounded Dependabot YAML subset, fail-closed otherwise."""
    parser = _Parser(_physical_lines(text))
    if parser.peek() is None:
        raise ParseError("document is empty")
    return parser.parse_document()


def _read_text(root: Path, rel: Path) -> str:
    path = root / rel
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        raise ValidationError(f"unreadable source: {rel.as_posix()}") from exc


def _string_field(mapping: dict[str, object], key: str) -> str | None:
    value = mapping.get(key)
    return value if isinstance(value, str) else None


def _mapping_field(mapping: dict[str, object], key: str) -> dict[str, object] | None:
    value = mapping.get(key)
    return value if isinstance(value, dict) else None


def _int_field(mapping: dict[str, object], key: str) -> int | None:
    value = mapping.get(key)
    return value if isinstance(value, int) and not isinstance(value, bool) else None


def _section(text: str, heading: str) -> str:
    start = text.find(heading)
    if start < 0:
        return ""
    rest = text[start + len(heading) :]
    nxt = re.search(r"\n### ", rest)
    return rest[: nxt.start()] if nxt else rest


def _backtick_bullets(section: str, marker: str) -> set[str]:
    idx = section.find(marker)
    if idx < 0:
        return set()
    rest = section[idx + len(marker) :]
    nxt = re.search(r"\n\*\*", rest)
    block = rest[: nxt.start()] if nxt else rest
    return set(re.findall(r"^- `([^`]+)`", block, re.MULTILINE))


def _code_fences(text: str) -> list[str]:
    return [match.group(1) for match in CODE_FENCE_RE.finditer(text)]


def _collapsed(text: str) -> str:
    return re.sub(r"\s+", " ", text)


def _has_forbidden_merge_pipe(text: str) -> bool:
    return bool(
        PIPE_MERGE_RE.search(text)
        or XARGS_MERGE_RE.search(text)
        or SUBSHELL_MERGE_RE.search(text)
    )


def _row_groups(row: dict[str, object]) -> set[str]:
    groups = _mapping_field(row, "groups")
    if groups is None:
        return set()
    return set(groups)


def _row_ignores(row: dict[str, object]) -> set[str]:
    ignore = row.get("ignore")
    if ignore is None:
        return set()
    if not isinstance(ignore, list):
        return set()
    names: set[str] = set()
    for item in ignore:
        if isinstance(item, dict):
            name = item.get("dependency-name")
            if isinstance(name, str):
                names.add(name)
    return names


def _inspect_config(doc: dict[str, object], path: str) -> list[Finding]:
    findings: list[Finding] = []
    if doc.get("version") != 2:
        findings.append(
            Finding("unsupported-shape", path, "Dependabot document version must be 2")
        )
    updates = doc.get("updates")
    if not isinstance(updates, list):
        findings.append(
            Finding(
                "unsupported-shape",
                path,
                "`updates` must be a sequence of ecosystem rows",
            )
        )
        return findings
    if not updates:
        findings.append(
            Finding("unsupported-shape", path, "`updates` must not be empty")
        )
        return findings

    seen: dict[tuple[str, str], int] = {}
    rows_by_key: dict[tuple[str, str], dict[str, object]] = {}
    for index, item in enumerate(updates):
        if not isinstance(item, dict):
            findings.append(
                Finding(
                    "unsupported-shape",
                    path,
                    f"updates[{index}] must be a mapping",
                )
            )
            continue
        ecosystem = _string_field(item, "package-ecosystem")
        directory = _string_field(item, "directory")
        if ecosystem is None or directory is None:
            findings.append(
                Finding(
                    "unsupported-shape",
                    path,
                    f"updates[{index}] must declare string package-ecosystem and directory",
                )
            )
            continue
        key = (ecosystem, directory)
        seen[key] = seen.get(key, 0) + 1
        rows_by_key.setdefault(key, item)

    for key, count in sorted(seen.items()):
        if count > 1:
            findings.append(
                Finding(
                    "duplicate-ecosystem-row",
                    path,
                    f"duplicate {key[0]} `{key[1]}` rows ({count}); uniqueness is required",
                )
            )
    for expected in EXPECTED_ROWS:
        if expected not in seen:
            findings.append(
                Finding(
                    "missing-ecosystem-row",
                    path,
                    f"missing unique {expected[0]} `{expected[1]}` row",
                )
            )
    for extra in sorted(set(seen) - set(EXPECTED_ROWS)):
        findings.append(
            Finding(
                "extra-ecosystem-row",
                path,
                f"unexpected {extra[0]} `{extra[1]}` row",
            )
        )

    for expected in EXPECTED_ROWS:
        row = rows_by_key.get(expected)
        if row is None:
            continue
        label = f"{expected[0]} `{expected[1]}`"
        if "labels" not in row:
            findings.append(
                Finding(
                    "labels-omitted",
                    path,
                    f"{label} omits `labels`; omission restores GitHub defaults, unlike `labels: []`",
                )
            )
        else:
            labels = row["labels"]
            if labels != []:
                findings.append(
                    Finding(
                        "custom-label-without-disposition",
                        path,
                        f"{label} `labels` must be the explicit empty list []; "
                        f"found {labels!r} with no checked consumer/disposition",
                    )
                )
        commit = _mapping_field(row, "commit-message")
        if commit is None:
            findings.append(
                Finding(
                    "commit-message-missing",
                    path,
                    f"{label} must declare commit-message prefix/include (#13477/#14180)",
                )
            )
        else:
            prefix = _string_field(commit, "prefix")
            include = _string_field(commit, "include")
            if prefix != COMMIT_PREFIX or include != COMMIT_INCLUDE:
                findings.append(
                    Finding(
                        "commit-message-scope",
                        path,
                        f"{label} must keep prefix {COMMIT_PREFIX!r} and include "
                        f"{COMMIT_INCLUDE!r} (landed #13477/#14180 scalars; this "
                        f"checker does not re-derive GitHub composition). "
                        f"found prefix={prefix!r} include={include!r}",
                    )
                )
        schedule = _mapping_field(row, "schedule")
        if schedule is None:
            findings.append(
                Finding("schedule-unparseable", path, f"{label} must declare schedule")
            )
        else:
            for field, expected_value in EXPECTED_SCHEDULE.items():
                if _string_field(schedule, field) != expected_value:
                    findings.append(
                        Finding(
                            "schedule-drift",
                            path,
                            f"{label} schedule.{field} must be {expected_value!r}",
                        )
                    )
        limit = _int_field(row, "open-pull-requests-limit")
        if limit is None or limit < 1:
            findings.append(
                Finding(
                    "open-pr-limit-unparseable",
                    path,
                    f"{label} open-pull-requests-limit must be a positive integer",
                )
            )
        groups = row.get("groups")
        if groups is not None and not isinstance(groups, dict):
            findings.append(
                Finding(
                    "unsupported-shape",
                    path,
                    f"{label} groups must be a mapping",
                )
            )
        ignore = row.get("ignore")
        if ignore is not None and not isinstance(ignore, list):
            findings.append(
                Finding(
                    "unsupported-shape",
                    path,
                    f"{label} ignore must be a sequence",
                )
            )
    return findings


def _guide_groups_and_ignores(text: str) -> dict[str, tuple[set[str], set[str]]]:
    sections = {
        "cargo": _section(text, "### Cargo Dependencies"),
        "github-actions": _section(text, "### GitHub Actions"),
        "npm": _section(text, "### npm Dependencies"),
    }
    parsed: dict[str, tuple[set[str], set[str]]] = {}
    for ecosystem, section in sections.items():
        groups = _backtick_bullets(section, "**Grouped Dependencies**:")
        ignores = _backtick_bullets(section, "**Major Version Exclusions**:")
        parsed[ecosystem] = (groups, ignores)
    return parsed


def _claimed_ignore_names(texts: list[str]) -> set[str]:
    names: set[str] = set()
    for text in texts:
        for name in DEPENDENCY_NAME_RE.findall(text):
            if name not in PLACEHOLDER_NAMES:
                names.add(name)
    return names


def _inspect_guides(
    guides: dict[Path, str],
    rows_by_key: dict[tuple[str, str], dict[str, object]],
) -> list[Finding]:
    findings: list[Finding] = []
    management = guides.get(MANAGEMENT_GUIDE, "")
    claimed = _guide_groups_and_ignores(management)
    yaml_ignore_claims = _claimed_ignore_names(list(guides.values()))

    for ecosystem, directory in EXPECTED_ROWS:
        row = rows_by_key.get((ecosystem, directory))
        if row is None:
            continue
        actual_groups = _row_groups(row)
        actual_ignores = _row_ignores(row)
        guide_groups, guide_ignores = claimed.get(ecosystem, (set(), set()))
        extra_group_claims = sorted(guide_groups - actual_groups)
        missing_group_claims = sorted(actual_groups - guide_groups)
        if extra_group_claims or missing_group_claims:
            findings.append(
                Finding(
                    "group-guide-mismatch",
                    MANAGEMENT_GUIDE.as_posix(),
                    f"{ecosystem} groups in the canonical guide {sorted(guide_groups)} "
                    f"must match config {sorted(actual_groups)}",
                )
            )
        if ecosystem == "cargo":
            extra_ignores = sorted(guide_ignores - actual_ignores)
            missing_ignores = sorted(actual_ignores - guide_ignores)
            if extra_ignores or missing_ignores:
                findings.append(
                    Finding(
                        "ignore-guide-mismatch",
                        MANAGEMENT_GUIDE.as_posix(),
                        f"Cargo ignores in the canonical guide {sorted(guide_ignores)} "
                        f"must match config {sorted(actual_ignores)}",
                    )
                )
        else:
            extra_ignores = sorted(guide_ignores - actual_ignores)
            if extra_ignores:
                findings.append(
                    Finding(
                        "ignore-guide-mismatch",
                        MANAGEMENT_GUIDE.as_posix(),
                        f"{ecosystem} guide ignore claims {extra_ignores} are absent from config",
                    )
                )

    all_config_groups: set[str] = set()
    all_config_ignores: set[str] = set()
    for row in rows_by_key.values():
        all_config_groups |= _row_groups(row)
        all_config_ignores |= _row_ignores(row)
    claimed_not_configured = sorted(
        name
        for name in yaml_ignore_claims
        if name not in all_config_ignores and name not in all_config_groups
    )
    if claimed_not_configured:
        findings.append(
            Finding(
                "ignore-guide-mismatch",
                MANAGEMENT_GUIDE.as_posix(),
                "guide dependency-name claims absent from config: "
                + ", ".join(claimed_not_configured),
            )
        )

    guide_blob = "\n".join(guides.values())
    if (
        rows_by_key.get(("npm", "/vscode-extension")) is None
        and "vscode-extension" in guide_blob
    ):
        findings.append(
            Finding(
                "directory-guide-drift",
                MANAGEMENT_GUIDE.as_posix(),
                "canonical guides still claim vscode-extension while config no longer owns that directory",
            )
        )

    for field, expected_value in EXPECTED_SCHEDULE.items():
        token = "Monday" if field == "day" else expected_value
        if token not in management and expected_value not in management:
            findings.append(
                Finding(
                    "schedule-guide-drift",
                    MANAGEMENT_GUIDE.as_posix(),
                    f"canonical guide must still claim schedule {field}={expected_value!r}",
                )
            )

    for rel, text in guides.items():
        posix = rel.as_posix()
        if "labels: []" not in text:
            findings.append(
                Finding(
                    "labels-suppression-undocumented",
                    posix,
                    "canonical guide must describe explicit `labels: []` default-label suppression",
                )
            )
        if DISCOVERY_RE.search(text) is None:
            findings.append(
                Finding(
                    "app-dependabot-discovery-missing",
                    posix,
                    "canonical guide must discover Dependabot PRs through app/dependabot",
                )
            )
        if LABEL_FILTER_RE.search(text):
            findings.append(
                Finding(
                    "retired-label-filter",
                    posix,
                    "retired `gh pr list --label dependencies` discovery is forbidden",
                )
            )
        if "highest semver" not in _collapsed(text).lower():
            findings.append(
                Finding(
                    "grouped-semver-undocumented",
                    posix,
                    "canonical guide must use the highest semver impact for grouped PRs",
                )
            )
        findings.extend(_inspect_merge_guidance(posix, text))
        findings.extend(_inspect_master_mentions(posix, text))
        findings.extend(_inspect_patch_query(posix, text))
    return findings


def _inspect_merge_guidance(path: str, text: str) -> list[Finding]:
    findings: list[Finding] = []
    fences = _code_fences(text)
    if _has_forbidden_merge_pipe(text):
        findings.append(
            Finding(
                "pipe-into-merge",
                path,
                "author/status query must not be piped, substituted, or looped into gh pr merge",
            )
        )
    for fence in fences:
        for match in MERGE_TARGET_RE.finditer(fence):
            target = match.group(1)
            if target.startswith("$(") or target.startswith("`"):
                findings.append(
                    Finding(
                        "pipe-into-merge",
                        path,
                        "gh pr merge target must not be a list substitution",
                    )
                )
            elif "<pr-number>" not in target and not re.match(
                r"^\$\{?[A-Za-z_][A-Za-z0-9_]*\}?$", target
            ):
                findings.append(
                    Finding(
                        "auto-merge-not-single-pr",
                        path,
                        "auto-merge examples must act on one explicitly reviewed PR",
                    )
                )
    return findings


def _inspect_patch_query(path: str, text: str) -> list[Finding]:
    findings: list[Finding] = []
    for match in re.finditer(r"status:success", text):
        window = text[max(0, match.start() - 250) : match.end() + 250]
        if re.search(r"patch updates", window, re.I) and VERSION_DELTA_RE.search(window) is None:
            findings.append(
                Finding(
                    "patch-query-misclassified",
                    path,
                    "author/status:success query must not be called patch updates without inspecting version deltas",
                )
            )
            break
    return findings


def _inspect_master_mentions(path: str, text: str) -> list[Finding]:
    findings: list[Finding] = []
    for match in re.finditer(r"`master`|\bmaster\b", text):
        window = text[max(0, match.start() - 180) : match.end() + 80]
        if re.search(r"EffortlessMetrics/perl-lsp(?!-swarm)", window):
            continue
        findings.append(
            Finding(
                "master-as-default-branch",
                path,
                "master is rejected where the instruction means this repository's default branch",
            )
        )
        break
    return findings


def _inspect_book_and_populate(book: str, populate: str) -> list[Finding]:
    findings: list[Finding] = []
    posix = BOOK_POINTER.as_posix()
    if CONTRIBUTING_POINTER_RE.search(book) is None:
        findings.append(
            Finding(
                "book-pointer-missing",
                posix,
                "book contributing stub must still select root CONTRIBUTING.md",
            )
        )
    if LABEL_FILTER_RE.search(book) or PIPE_MERGE_RE.search(book) or "gh pr merge" in book:
        findings.append(
            Finding(
                "book-policy-leak",
                posix,
                "book stub must not grow retired-label or bulk-merge recipes",
            )
        )
    if OVERWRITE_BOOK_RE.search(populate):
        findings.append(
            Finding(
                "populate-book-overwrite",
                POPULATE_BOOK.as_posix(),
                "populate-book.sh must not overwrite the contributing stub with CONTRIBUTING.md",
            )
        )
    return findings


def _active_workflow_text(text: str) -> str:
    kept: list[str] = []
    for line in text.splitlines():
        if line.lstrip().startswith("#"):
            continue
        kept.append(line)
    return "\n".join(kept)


def inspect_workflow_wiring(text: str) -> list[Finding]:
    """Require Policy Validators to execute this contract on governed edits."""
    findings: list[Finding] = []
    path = POLICY_WORKFLOW.as_posix()
    active = _active_workflow_text(text)
    for rel in REQUIRED_WORKFLOW_PATHS:
        needle = rel.as_posix()
        if needle not in active:
            findings.append(
                Finding(
                    "workflow-path-unwired",
                    path,
                    f"Policy Validators paths must include {needle}",
                )
            )
    if REQUIRED_TEST_RUN not in active:
        findings.append(
            Finding(
                "workflow-tests-unwired",
                path,
                "Policy Validators must run the focused Dependabot contract tests",
            )
        )
    if REQUIRED_VALIDATOR_RUN not in active:
        findings.append(
            Finding(
                "workflow-validator-unwired",
                path,
                "Policy Validators must run the Dependabot contract validator",
            )
        )
    return findings


def _rows_by_key(doc: dict[str, object]) -> dict[tuple[str, str], dict[str, object]]:
    rows: dict[tuple[str, str], dict[str, object]] = {}
    updates = doc.get("updates")
    if not isinstance(updates, list):
        return rows
    for item in updates:
        if not isinstance(item, dict):
            continue
        ecosystem = _string_field(item, "package-ecosystem")
        directory = _string_field(item, "directory")
        if ecosystem is None or directory is None:
            continue
        rows.setdefault((ecosystem, directory), item)
    return rows


def validate(
    root: Path,
    surfaces: list[Path] | None = None,
    *,
    check_wiring: bool = True,
) -> list[Finding]:
    """Return deterministic ordered findings for the selected surfaces."""
    selected = list(GOVERNED_SURFACES if surfaces is None else surfaces)
    if not selected:
        return [
            Finding(
                "empty-inventory",
                ".",
                "zero selected surfaces is failure",
            )
        ]

    findings: list[Finding] = []
    if BOOK_POINTER not in selected:
        findings.append(
            Finding(
                "book-surface-missing",
                BOOK_POINTER.as_posix(),
                "contributor-book surface dropped from inventory",
            )
        )

    texts: dict[Path, str] = {}
    for rel in selected:
        try:
            texts[rel] = _read_text(root, rel)
        except ValidationError as exc:
            findings.append(Finding("unreadable-source", rel.as_posix(), str(exc)))

    config_text = texts.get(CONFIG_PATH)
    doc: dict[str, object] | None = None
    if config_text is not None:
        try:
            parsed = parse_yaml_subset(config_text)
        except ParseError as exc:
            findings.append(
                Finding("malformed-yaml", CONFIG_PATH.as_posix(), str(exc))
            )
        else:
            doc = parsed
            findings.extend(_inspect_config(parsed, CONFIG_PATH.as_posix()))

    guides = {rel: texts[rel] for rel in CANONICAL_GUIDES if rel in texts}
    if doc is not None:
        findings.extend(_inspect_guides(guides, _rows_by_key(doc)))
    else:
        for rel, text in guides.items():
            findings.extend(_inspect_merge_guidance(rel.as_posix(), text))
            if LABEL_FILTER_RE.search(text):
                findings.append(
                    Finding(
                        "retired-label-filter",
                        rel.as_posix(),
                        "retired `gh pr list --label dependencies` discovery is forbidden",
                    )
                )

    book = texts.get(BOOK_POINTER)
    populate = texts.get(POPULATE_BOOK)
    if book is not None and populate is not None:
        findings.extend(_inspect_book_and_populate(book, populate))

    if check_wiring:
        try:
            workflow = _read_text(root, POLICY_WORKFLOW)
        except ValidationError as exc:
            findings.append(
                Finding("unreadable-source", POLICY_WORKFLOW.as_posix(), str(exc))
            )
        else:
            findings.extend(inspect_workflow_wiring(workflow))

    return sorted(set(findings))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="repository root to validate (defaults to this checkout)",
    )
    args = parser.parse_args(argv)
    try:
        findings = validate(args.repo_root.resolve())
    except ValidationError as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        return 2
    if findings:
        print("FAIL: dependabot source contract", file=sys.stderr)
        for finding in findings:
            print(f"  {finding.line()}", file=sys.stderr)
        return 1
    print("OK: dependabot source contract")
    return 0


if __name__ == "__main__":
    sys.exit(main())
