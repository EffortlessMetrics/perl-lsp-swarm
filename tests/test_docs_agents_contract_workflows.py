"""Independent existence gate for docs/agents contract workflows (#14628).

Four docs/agents contract workflows are path-filtered on their own definition
and assert their own existence from a test that only that workflow runs.
Deleting the workflow file matches the filter, but no definition remains to
schedule, so the assertion never runs.

This module is the always-running required check. It reads a TOML registry
rather than a hardcoded per-workflow list, fails when a registered path is
missing, and requires registry membership for any workflow that matches the
docs/agents contract class — so a fifth cannot silently opt out.

What this does not prove: those workflows' own contract oracles still hold;
#13788/#13789 worked-lane ledger content; generated-artifact drift (#14161).
"""

from __future__ import annotations

import re
import tomllib
import unittest
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "policy" / "docs-agents-contract-workflows.toml"
ALLOWLIST = ROOT / "policy" / "non-rust-allowlist.toml"
WORKFLOWS_DIR = ROOT / ".github" / "workflows"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
GATE_POLICY = ROOT / ".ci" / "gate-policy.yaml"
SELF_TEST = "tests/test_docs_agents_contract_workflows.py"
GATE_NAME = "docs_agents_contract_workflows"
CONTROL_PLANE_KIND = "control_plane_contract_test"
WORKFLOW_PREFIX = ".github/workflows/"
WORKFLOW_SUFFIXES = (".yml", ".yaml")
WORKFLOW_GLOBS = ("*.yml", "*.yaml")

# Claim-boundary pin: #14628 named these four. They live in the registry's
# `named_class` as data; this frozenset only detects dropping one of them
# from that data. It is not the existence oracle.
ISSUE_CLASS_BASENAMES = frozenset(
    {
        "agent-authority-status.yml",
        "active-authority-contract.yml",
        "legacy-authority-banners.yml",
        "worked-lane-corpus.yml",
    }
)
UNITTEST_TOKEN = re.compile(r"tests(?:/|\.)[A-Za-z0-9_./-]+")


def load_toml(path: Path) -> dict[str, Any]:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def load_registry(path: Path = REGISTRY) -> dict[str, Any]:
    return load_toml(path)


def posix(path: str) -> str:
    return path.replace("\\", "/")


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def top_level_block(text: str, key: str) -> str:
    """Return the body of a top-level YAML mapping key, excluding the key line."""
    lines = text.splitlines()
    marker = f"{key}:"
    try:
        start = next(index for index, line in enumerate(lines) if line == marker)
    except StopIteration as error:
        raise AssertionError(f"missing top-level `{key}:` block") from error
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if lines[index] and not lines[index].startswith(" ") and not lines[index].startswith("#"):
            end = index
            break
    return "\n".join(lines[start:end])


@dataclass(frozen=True)
class EventFilters:
    """Trigger filters for one `on.<event>` mapping.

    Distinguishes a missing event from an unfiltered event. `paths` and
    `paths-ignore` are parsed from block lists and flow-style sequences.
    """

    present: bool
    paths: frozenset[str] = frozenset()
    paths_ignore: frozenset[str] = frozenset()
    has_paths_key: bool = False
    has_paths_ignore_key: bool = False

    @property
    def unfiltered(self) -> bool:
        return self.present and not self.has_paths_key and not self.has_paths_ignore_key


def _unquote_yaml_scalar(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and (
        (value.startswith("'") and value.endswith("'"))
        or (value.startswith('"') and value.endswith('"'))
    ):
        return value[1:-1]
    return value


def _flow_sequence(value: str) -> tuple[str, ...]:
    text = value.strip()
    if not (text.startswith("[") and text.endswith("]")):
        return ()
    inner = text[1:-1].strip()
    if not inner:
        return ()
    return tuple(
        _unquote_yaml_scalar(part) for part in inner.split(",") if part.strip()
    )


def _filter_key_kind(stripped: str, key: str) -> str | None:
    """Return `block`, `flow`, or None. `paths` must not match `paths-ignore`."""
    prefix = f"{key}:"
    if stripped == prefix:
        return "block"
    if stripped.startswith(prefix + "[") or stripped.startswith(prefix + " ["):
        return "flow"
    if stripped.startswith(prefix + " ") and not stripped.startswith(prefix + " -"):
        remainder = stripped[len(prefix) :].lstrip()
        if remainder.startswith("["):
            return "flow"
    return None


def _parse_filter_list(block: list[str], key: str) -> tuple[bool, frozenset[str]]:
    for index, line in enumerate(block):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        kind = _filter_key_kind(stripped, key)
        if kind is None:
            continue
        if kind == "flow":
            remainder = stripped.split(":", 1)[1]
            return True, frozenset(_flow_sequence(remainder))
        items: set[str] = set()
        key_indent = _indent(line)
        for child in block[index + 1 :]:
            child_stripped = child.strip()
            if not child_stripped or child_stripped.startswith("#"):
                continue
            if _indent(child) <= key_indent:
                break
            if not child_stripped.startswith("- "):
                continue
            items.add(_unquote_yaml_scalar(child_stripped[2:]))
        return True, frozenset(items)
    return False, frozenset()


def _filters_from_flow_mapping(text: str) -> EventFilters:
    """Parse `pull_request: {paths: [...], paths-ignore: [...]}` on one line."""
    ignore_match = _FLOW_IGNORE_KEY.search(text)
    paths_match = _FLOW_PATHS_KEY.search(text)
    return EventFilters(
        present=True,
        paths=frozenset(_flow_sequence(paths_match.group(1))) if paths_match else frozenset(),
        paths_ignore=frozenset(_flow_sequence(ignore_match.group(1)))
        if ignore_match
        else frozenset(),
        has_paths_key=paths_match is not None,
        has_paths_ignore_key=ignore_match is not None,
    )


def parse_event_filters(workflow_text: str, event: str) -> EventFilters:
    """Parse `on.<event>` path filters without a YAML library."""
    on_block = top_level_block(workflow_text, "on")
    lines = on_block.splitlines()
    marker = f"  {event}:"
    try:
        start = next(index for index, line in enumerate(lines) if line.startswith(marker))
    except StopIteration:
        return EventFilters(present=False)

    remainder = lines[start][len(marker) :].strip()
    if remainder.startswith("{"):
        return _filters_from_flow_mapping(remainder)

    event_indent = _indent(lines[start])
    end = len(lines)
    for index in range(start + 1, len(lines)):
        stripped = lines[index].strip()
        if not stripped or stripped.startswith("#"):
            continue
        if _indent(lines[index]) <= event_indent and stripped.endswith(":"):
            end = index
            break

    block = lines[start:end]
    has_paths, paths = _parse_filter_list(block, "paths")
    has_ignore, ignored = _parse_filter_list(block, "paths-ignore")
    return EventFilters(
        present=True,
        paths=paths,
        paths_ignore=ignored,
        has_paths_key=has_paths,
        has_paths_ignore_key=has_ignore,
    )


def event_paths(workflow_text: str, event: str) -> set[str]:
    """Collect `on.<event>.paths` entries (block or flow-style)."""
    return set(parse_event_filters(workflow_text, event).paths)


def run_commands(workflow_text: str) -> tuple[str, ...]:
    """Collect `run:` command text, including folded `|` / `>` blocks."""
    lines = workflow_text.splitlines()
    commands: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        active = line.strip().removeprefix("- ").lstrip()
        if line.lstrip().startswith("#") or not active.startswith("run:"):
            index += 1
            continue

        indent = _indent(line)
        value = active.removeprefix("run:").strip()
        if value not in {"|", "|-", ">", ">-"}:
            commands.append(" ".join(value.split()))
            index += 1
            continue

        block: list[str] = []
        index += 1
        pending = ""
        while index < len(lines):
            candidate = lines[index]
            stripped = candidate.strip()
            if stripped and _indent(candidate) <= indent:
                break
            if stripped and not stripped.startswith("#"):
                continued = stripped.endswith("\\")
                piece = stripped.rstrip("\\").strip()
                pending = f"{pending} {piece}".strip() if pending else piece
                if not continued:
                    block.append(pending)
                    pending = ""
            index += 1
        if pending:
            block.append(pending)
        # Literal blocks keep line boundaries; folded blocks collapse to spaces.
        joiner = "\n" if value in {"|", "|-"} else " "
        commands.append(joiner.join(part for part in block if part))
    return tuple(commands)


_FLOW_PATHS_KEY = re.compile(r"(?<![\w-])paths:\s*(\[[^\]]*\])")
_FLOW_IGNORE_KEY = re.compile(r"paths-ignore:\s*(\[[^\]]*\])")
_PYTHON_VALUE_FLAGS = frozenset({"-W", "-X", "--check-hash-based-pycs"})


def _split_unquoted_operators(line: str) -> list[str]:
    """Split on `&&`, `||`, `;`, and `|` that are not inside quotes.

    This is quote-aware word-boundary splitting, not a shell parser: no
    expansions, escapes, or heredocs.
    """
    parts: list[str] = []
    buf: list[str] = []
    quote: str | None = None
    index = 0
    while index < len(line):
        char = line[index]
        if quote is not None:
            buf.append(char)
            if char == quote:
                quote = None
            index += 1
            continue
        if char in {'"', "'"}:
            quote = char
            buf.append(char)
            index += 1
            continue
        if line.startswith("&&", index) or line.startswith("||", index):
            parts.append("".join(buf))
            buf = []
            index += 2
            continue
        if char in {";", "|"}:
            parts.append("".join(buf))
            buf = []
            index += 1
            continue
        buf.append(char)
        index += 1
    parts.append("".join(buf))
    return parts


def _shell_words(text: str) -> list[str]:
    """Split on unquoted whitespace and drop matching quotes from words."""
    words: list[str] = []
    buf: list[str] = []
    quote: str | None = None
    for char in text:
        if quote is not None:
            if char == quote:
                quote = None
            else:
                buf.append(char)
            continue
        if char in {'"', "'"}:
            quote = char
            continue
        if char.isspace():
            if buf:
                words.append("".join(buf))
                buf = []
            continue
        buf.append(char)
    if buf:
        words.append("".join(buf))
    return words


def _command_segments(command: str) -> tuple[str, ...]:
    parts: list[str] = []
    for line in command.splitlines():
        parts.extend(_split_unquoted_operators(line))
    return tuple(part.strip() for part in parts if part.strip())


def _python_script_operand(tokens: list[str]) -> str | None:
    """Return `tests/*.py` after interpreter flags (`python3 -u tests/foo.py`)."""
    if not tokens or tokens[0] not in {"python3", "python"}:
        return None
    index = 1
    while index < len(tokens):
        token = tokens[index]
        if token == "--":
            index += 1
            break
        if token.startswith("-") and token not in {"-m", "-c"}:
            index += 1
            name = token.split("=", 1)[0]
            if (
                name in _PYTHON_VALUE_FLAGS
                and "=" not in token
                and index < len(tokens)
                and not tokens[index].startswith("-")
            ):
                index += 1
            continue
        break
    if index >= len(tokens):
        return None
    operand = tokens[index].rstrip(",")
    if operand.startswith("tests/") and operand.endswith(".py"):
        return posix(operand)
    return None


def _unittest_module_targets(segment: str) -> set[str]:
    targets: set[str] = set()
    for token in UNITTEST_TOKEN.findall(segment):
        token = token.rstrip(",")
        if token.endswith(".py"):
            targets.add(posix(token))
            continue
        module = token.split(".", 1)[1] if token.startswith("tests.") else token
        file_stem = module.split(".", 1)[0]
        if file_stem.startswith("test_") or file_stem.startswith("test-"):
            targets.add(f"tests/{file_stem}.py")
        elif token.startswith("tests/"):
            targets.add(posix(token))
    return targets


def _segment_test_paths(segment: str) -> set[str]:
    tokens = _shell_words(segment)
    if not tokens or tokens[0] not in {"python3", "python"}:
        return set()
    joined = " ".join(tokens)
    if "-m unittest" in joined:
        return _unittest_module_targets(joined)
    operand = _python_script_operand(tokens)
    return {operand} if operand else set()


def unittest_targets(commands: Iterable[str]) -> set[str]:
    """Map python unittest/script invocations onto `tests/*.py` paths.

    The hosted shard runner rejects `python3 -m unittest` (`-m` is an unsupported
    nested interpreter command), so this gate's own production command is
    `python3 tests/test_docs_agents_contract_workflows.py`. A fifth that copies
    that form, including interpreter flags such as `-u` and quoted script or
    unittest operands, must still be discovered. Test paths that appear only
    in a later non-python command (`echo tests/foo.py`) are not operands.
    """
    targets: set[str] = set()
    for command in commands:
        for segment in _command_segments(command):
            targets.update(_segment_test_paths(segment))
    return targets


def control_plane_test_paths(allowlist: dict[str, Any]) -> set[str]:
    paths: set[str] = set()
    for entry in allowlist.get("allow", []):
        if not isinstance(entry, dict):
            continue
        if entry.get("kind") != CONTROL_PLANE_KIND:
            continue
        path = entry.get("path")
        if isinstance(path, str) and path:
            paths.add(posix(path))
    return paths


def allowlist_covered_workflows(allowlist: dict[str, Any]) -> set[str]:
    workflows: set[str] = set()
    for entry in allowlist.get("allow", []):
        if not isinstance(entry, dict) or entry.get("kind") != CONTROL_PLANE_KIND:
            continue
        covered = entry.get("covered_by", [])
        if not isinstance(covered, list):
            continue
        for item in covered:
            if not isinstance(item, str):
                continue
            path = posix(item)
            if path.startswith(WORKFLOW_PREFIX) and path.endswith(WORKFLOW_SUFFIXES):
                workflows.add(path)
    return workflows


def _github_glob_regex(pattern: str) -> str:
    """Translate a GitHub Actions path-filter glob to a fullmatch regex.

    Actions uses picomatch: `**` crosses `/`; `*` and `?` do not. This is that
    subset, not a glob library: no character classes, extglobs, or negation.
    """
    parts: list[str] = []
    index = 0
    while index < len(pattern):
        if pattern.startswith("**", index):
            rest = index + 2
            if rest < len(pattern) and pattern[rest] == "/":
                parts.append("(?:.*/)?")
                index = rest + 1
                continue
            parts.append(".*")
            index = rest
            continue
        char = pattern[index]
        if char == "*":
            parts.append("[^/]*")
        elif char == "?":
            parts.append("[^/]")
        else:
            parts.append(re.escape(char))
        index += 1
    return r"\A" + "".join(parts) + r"\Z"


def path_filter_matches(path: str, pattern: str) -> bool:
    """True when `pattern` would include `path` in a GitHub `paths:` filter."""
    path = posix(path)
    pattern = posix(pattern)
    if path == pattern:
        return True
    if not any(char in pattern for char in "*?"):
        return False
    return re.fullmatch(_github_glob_regex(pattern), path) is not None


def self_path_filtered(relative_path: str, workflow_text: str) -> bool:
    """True when any `on.pull_request.paths` entry matches this workflow file."""
    filters = parse_event_filters(workflow_text, "pull_request")
    if not filters.has_paths_key:
        return False
    own = posix(relative_path)
    return any(path_filter_matches(own, pattern) for pattern in filters.paths)


def is_contract_workflow(
    relative_path: str,
    workflow_text: str,
    contract_tests: set[str],
) -> bool:
    """A docs/agents contract workflow is self-path-filtered and runs a
    `control_plane_contract_test` unittest or `python3 tests/*.py` script.

    Self-path-filtered means GitHub would schedule the workflow when its own
    file changes: an exact `paths:` entry or a glob that matches that file.
    `paths-ignore` alone is not this class; GitHub forbids combining `paths`
    and `paths-ignore` on one event.
    """
    if not self_path_filtered(relative_path, workflow_text):
        return False
    return bool(unittest_targets(run_commands(workflow_text)) & contract_tests)


def discover_contract_workflows(
    workflow_files: dict[str, str],
    allowlist: dict[str, Any],
) -> set[str]:
    contract_tests = control_plane_test_paths(allowlist)
    discovered = set(allowlist_covered_workflows(allowlist))
    for relative_path, text in workflow_files.items():
        if is_contract_workflow(relative_path, text, contract_tests):
            discovered.add(posix(relative_path))
    return discovered


def registered_paths(registry: dict[str, Any]) -> list[str]:
    rows = registry.get("workflows", [])
    if not isinstance(rows, list):
        return []
    paths: list[str] = []
    for row in rows:
        if isinstance(row, dict) and isinstance(row.get("path"), str):
            paths.append(row["path"])
    return paths


def named_class(registry: dict[str, Any]) -> list[str]:
    values = registry.get("named_class", [])
    if not isinstance(values, list):
        return []
    return [posix(value) for value in values if isinstance(value, str)]


def registry_shape_errors(registry: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if registry.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if not isinstance(registry.get("owner"), str) or not str(registry.get("owner")).strip():
        errors.append("owner must be a non-empty string")
    if registry.get("tracking_issue") != 14628:
        errors.append("tracking_issue must be 14628")
    if registry.get("gate") != GATE_NAME:
        errors.append(f"gate must be {GATE_NAME}")

    names = named_class(registry)
    if not names:
        errors.append("named_class must be a non-empty list")
    seen_names: set[str] = set()
    for name in names:
        if "/" in name or name != Path(name).name:
            errors.append(f"named_class entry must be a workflow basename: {name!r}")
        if not name.endswith(WORKFLOW_SUFFIXES):
            errors.append(f"named_class entry must end in .yml or .yaml: {name!r}")
        if name in seen_names:
            errors.append(f"named_class duplicates {name!r}")
        seen_names.add(name)

    rows = registry.get("workflows", [])
    if not isinstance(rows, list) or not rows:
        errors.append("workflows must contain at least one path")
        return errors
    seen_paths: set[str] = set()
    for row in rows:
        if not isinstance(row, dict):
            errors.append("workflows rows must be tables")
            continue
        path = row.get("path")
        if not isinstance(path, str) or not path:
            errors.append("workflow path must be a non-empty string")
            continue
        if path in seen_paths:
            errors.append(f"duplicate workflow path: {path}")
        seen_paths.add(path)
        errors.extend(_path_shape_errors(path))
        contract = row.get("contract")
        if not isinstance(contract, str) or not contract.strip():
            errors.append(f"{path}: contract must be a non-empty string")
    return errors


def _path_shape_errors(path: str) -> list[str]:
    errors: list[str] = []
    if path.startswith("/") or path.startswith("\\") or ":/" in path or ":\\" in path:
        errors.append(f"path must be repository-relative: {path}")
    if posix(path) != path:
        errors.append(f"path must use POSIX separators: {path}")
    if ".." in Path(path).parts:
        errors.append(f"path must not escape with ..: {path}")
    if not path.startswith(WORKFLOW_PREFIX):
        errors.append(f"path must start with {WORKFLOW_PREFIX}: {path}")
    if not path.endswith(WORKFLOW_SUFFIXES):
        errors.append(f"path must end with .yml or .yaml: {path}")
    rest = path[len(WORKFLOW_PREFIX) :]
    if not rest or "/" in rest:
        errors.append(f"path must be a workflow basename under {WORKFLOW_PREFIX}: {path}")
    return errors


def existence_errors(
    paths: Iterable[str], exists: Callable[[str], bool]
) -> list[str]:
    errors: list[str] = []
    for path in paths:
        if not exists(path):
            errors.append(f"registered contract workflow is missing: {path}")
    return errors


def membership_errors(discovered: Iterable[str], registered: Iterable[str]) -> list[str]:
    missing = sorted(set(discovered) - set(registered))
    return [
        f"docs/agents contract workflow is not in the registry: {path}"
        for path in missing
    ]


def named_class_errors(
    names: Iterable[str],
    present: Iterable[str],
    registered: Iterable[str],
) -> list[str]:
    """A named-class basename that exists on disk must have a registry row.

    A named file that is absent (today: worked-lane-corpus.yml on #13789) is
    not a failure. Presence without registration is the silent opt-out.
    """
    present_basenames = {Path(path).name for path in present}
    registered_basenames = {Path(path).name for path in registered}
    errors: list[str] = []
    for name in names:
        if name in present_basenames and name not in registered_basenames:
            errors.append(
                f"named docs/agents contract workflow is present but unregistered: {name}"
            )
    return errors


def host_gate_errors(
    ci_workflow_text: str,
    gate_policy_text: str,
    *,
    self_test: str = SELF_TEST,
    gate_name: str = GATE_NAME,
) -> list[str]:
    errors: list[str] = []
    try:
        pull_request = parse_event_filters(ci_workflow_text, "pull_request")
    except AssertionError as error:
        return [str(error)]
    if not pull_request.present:
        errors.append(
            "host workflow `.github/workflows/ci.yml` must declare on.pull_request"
        )
    elif not pull_request.unfiltered:
        found: list[str] = []
        if pull_request.has_paths_key:
            found.append(f"paths={sorted(pull_request.paths)!r}")
        if pull_request.has_paths_ignore_key:
            found.append(f"paths-ignore={sorted(pull_request.paths_ignore)!r}")
        errors.append(
            "host workflow `.github/workflows/ci.yml` on.pull_request must have "
            f"no path filter; found {', '.join(found)}"
        )

    shard_gates = policy_shard_gates(ci_workflow_text)
    if gate_name not in shard_gates:
        errors.append(
            f"policy shard gates must include {gate_name}; found {shard_gates!r}"
        )

    block = gate_block(gate_policy_text, gate_name)
    if block is None:
        errors.append(f"gate-policy.yaml must define {gate_name}")
        return errors
    if not re.search(r"^    required: true\s*$", block, re.M):
        errors.append(f"{gate_name} must be required: true")
    command_match = re.search(r"^    command: (.+)$", block, re.M)
    command = command_match.group(1).strip() if command_match else ""
    expected = f"python3 {self_test}"
    if command != expected:
        errors.append(
            f"{gate_name} command must be {expected!r}; found {command!r}"
        )
    return errors


def policy_shard_gates(ci_workflow_text: str) -> list[str]:
    lines = ci_workflow_text.splitlines()
    for index, line in enumerate(lines):
        if line.strip() != "- name: policy":
            continue
        for candidate in lines[index + 1 : index + 8]:
            stripped = candidate.strip()
            if stripped.startswith("gates:"):
                return stripped.split(":", 1)[1].split()
    return []


def gate_block(policy_text: str, gate: str) -> str | None:
    match = re.search(rf"^  - name: {re.escape(gate)}\s*$", policy_text, re.M)
    if match is None:
        return None
    start = match.start()
    rest = policy_text[match.end() :]
    next_gate = re.search(r"\n  - name: ", rest)
    end = match.end() + (next_gate.start() if next_gate else len(rest))
    return policy_text[start:end]


def live_workflow_files() -> dict[str, str]:
    files: dict[str, str] = {}
    for pattern in WORKFLOW_GLOBS:
        for path in sorted(WORKFLOWS_DIR.glob(pattern)):
            relative = posix(str(path.relative_to(ROOT)))
            files[relative] = path.read_text(encoding="utf-8")
    return files


def live_exists(path: str) -> bool:
    return (ROOT / path).is_file()


def validate_live() -> list[str]:
    registry = load_registry()
    allowlist = load_toml(ALLOWLIST)
    errors = registry_shape_errors(registry)
    registered = [posix(path) for path in registered_paths(registry)]
    errors.extend(existence_errors(registered, live_exists))
    discovered = discover_contract_workflows(live_workflow_files(), allowlist)
    errors.extend(membership_errors(discovered, registered))
    present = list(live_workflow_files())
    errors.extend(named_class_errors(named_class(registry), present, registered))
    errors.extend(
        host_gate_errors(
            CI_WORKFLOW.read_text(encoding="utf-8"),
            GATE_POLICY.read_text(encoding="utf-8"),
        )
    )
    return errors


class RegistryShapeTests(unittest.TestCase):
    def test_live_registry_shape_is_valid(self) -> None:
        self.assertEqual(registry_shape_errors(load_registry()), [])

    def test_named_class_covers_the_four_issue_members(self) -> None:
        self.assertEqual(set(named_class(load_registry())), ISSUE_CLASS_BASENAMES)

    def test_empty_registry_is_rejected(self) -> None:
        document = load_registry()
        document["workflows"] = []
        errors = registry_shape_errors(document)
        self.assertTrue(any("at least one path" in error for error in errors), errors)

    def test_empty_named_class_is_rejected(self) -> None:
        document = load_registry()
        document["named_class"] = []
        errors = registry_shape_errors(document)
        self.assertTrue(any("named_class must be a non-empty list" in error for error in errors), errors)

    def test_duplicate_paths_are_rejected(self) -> None:
        document = load_registry()
        path = registered_paths(document)[0]
        document["workflows"].append({"path": path, "contract": "dup"})
        errors = registry_shape_errors(document)
        self.assertTrue(any("duplicate workflow path" in error for error in errors), errors)

    def test_path_escape_is_rejected(self) -> None:
        document = load_registry()
        document["workflows"] = [
            {"path": ".github/workflows/../secret.yml", "contract": "escape"}
        ]
        errors = registry_shape_errors(document)
        self.assertTrue(any("must not escape" in error for error in errors), errors)

    def test_absolute_path_is_rejected(self) -> None:
        document = load_registry()
        document["workflows"] = [
            {"path": "/etc/passwd.yml", "contract": "absolute"}
        ]
        errors = registry_shape_errors(document)
        self.assertTrue(any("repository-relative" in error for error in errors), errors)

    def test_backslash_path_is_rejected(self) -> None:
        document = load_registry()
        document["workflows"] = [
            {"path": ".github\\workflows\\foo.yml", "contract": "windows"}
        ]
        errors = registry_shape_errors(document)
        self.assertTrue(any("POSIX separators" in error for error in errors), errors)

    def test_nested_workflow_path_is_rejected(self) -> None:
        document = load_registry()
        document["workflows"] = [
            {
                "path": ".github/workflows/nested/agent-authority-status.yml",
                "contract": "nested",
            }
        ]
        errors = registry_shape_errors(document)
        self.assertTrue(any("workflow basename" in error for error in errors), errors)

    def test_missing_contract_field_is_rejected(self) -> None:
        document = load_registry()
        document["workflows"][0]["contract"] = ""
        errors = registry_shape_errors(document)
        self.assertTrue(any("contract must be a non-empty string" in error for error in errors), errors)

    def test_wrong_tracking_issue_is_rejected(self) -> None:
        document = load_registry()
        document["tracking_issue"] = 13788
        errors = registry_shape_errors(document)
        self.assertTrue(any("tracking_issue must be 14628" in error for error in errors), errors)


class ExistenceTests(unittest.TestCase):
    def test_deleting_a_registered_workflow_is_caught(self) -> None:
        """The defect: the file is gone and the self-scheduled test never runs."""
        registered = (
            ".github/workflows/agent-authority-status.yml",
            ".github/workflows/active-authority-contract.yml",
            ".github/workflows/legacy-authority-banners.yml",
        )
        deleted = registered[0]
        remaining = set(registered) - {deleted}
        errors = existence_errors(registered, remaining.__contains__)
        self.assertEqual(
            [f"registered contract workflow is missing: {deleted}"],
            errors,
        )

    def test_existing_registered_paths_pass(self) -> None:
        present = {
            ".github/workflows/agent-authority-status.yml",
            ".github/workflows/active-authority-contract.yml",
        }
        self.assertEqual(existence_errors(present, present.__contains__), [])

    def test_live_registered_paths_exist(self) -> None:
        self.assertEqual(
            existence_errors(registered_paths(load_registry()), live_exists), []
        )


class MembershipTests(unittest.TestCase):
    def test_unregistered_fifth_is_caught(self) -> None:
        registered = {
            ".github/workflows/agent-authority-status.yml",
            ".github/workflows/active-authority-contract.yml",
            ".github/workflows/legacy-authority-banners.yml",
        }
        discovered = registered | {".github/workflows/new-docs-contract.yml"}
        errors = membership_errors(discovered, registered)
        self.assertEqual(
            [
                "docs/agents contract workflow is not in the registry: "
                ".github/workflows/new-docs-contract.yml"
            ],
            errors,
        )

    def test_registered_superset_is_not_a_membership_failure(self) -> None:
        """Retirement is a registry edit; extra registered rows fail existence, not membership."""
        registered = {
            ".github/workflows/agent-authority-status.yml",
            ".github/workflows/legacy-authority-banners.yml",
        }
        discovered = {".github/workflows/agent-authority-status.yml"}
        self.assertEqual(membership_errors(discovered, registered), [])

    def test_named_class_member_present_but_unregistered_is_caught(self) -> None:
        errors = named_class_errors(
            ISSUE_CLASS_BASENAMES,
            [".github/workflows/worked-lane-corpus.yml"],
            [
                ".github/workflows/agent-authority-status.yml",
                ".github/workflows/active-authority-contract.yml",
                ".github/workflows/legacy-authority-banners.yml",
            ],
        )
        self.assertEqual(
            [
                "named docs/agents contract workflow is present but unregistered: "
                "worked-lane-corpus.yml"
            ],
            errors,
        )

    def test_named_class_member_absent_is_not_a_failure(self) -> None:
        """worked-lane-corpus.yml is named by #14628 and is not on origin/main."""
        self.assertEqual(
            named_class_errors(
                ISSUE_CLASS_BASENAMES,
                [
                    ".github/workflows/agent-authority-status.yml",
                    ".github/workflows/active-authority-contract.yml",
                    ".github/workflows/legacy-authority-banners.yml",
                ],
                [
                    ".github/workflows/agent-authority-status.yml",
                    ".github/workflows/active-authority-contract.yml",
                    ".github/workflows/legacy-authority-banners.yml",
                ],
            ),
            [],
        )


class DiscoveryTests(unittest.TestCase):
    def test_self_filtered_control_plane_workflow_is_discovered(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - '.github/workflows/new-docs-contract.yml'
      - 'docs/agents/README.md'
jobs:
  check:
    steps:
      - run: python3 -m unittest tests/test_new_docs_contract.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_new_docs_contract.py",
                    "covered_by": [".github/workflows/new-docs-contract.yml"],
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/new-docs-contract.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, {".github/workflows/new-docs-contract.yml"})

    def test_workflow_without_self_path_filter_is_not_structural_match(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - 'docs/agents/README.md'
jobs:
  check:
    steps:
      - run: python3 -m unittest tests/test_new_docs_contract.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_new_docs_contract.py",
                    "covered_by": ["python3 -m unittest tests/test_new_docs_contract.py"],
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/new-docs-contract.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, set())

    def test_wildcard_self_path_filter_is_discovered(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - '.github/workflows/**'
jobs:
  check:
    steps:
      - run: python3 tests/test_glob_docs_contract.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_glob_docs_contract.py",
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/glob-docs-contract.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, {".github/workflows/glob-docs-contract.yml"})
        self.assertTrue(
            path_filter_matches(
                ".github/workflows/glob-docs-contract.yml",
                ".github/workflows/**",
            )
        )
        self.assertTrue(
            path_filter_matches(
                ".github/workflows/glob-docs-contract.yml",
                ".github/workflows/*.yml",
            )
        )
        self.assertFalse(
            path_filter_matches(
                ".github/workflows/glob-docs-contract.yml",
                ".github/*",
            )
        )
        self.assertFalse(
            path_filter_matches(
                ".github/workflows/nested/glob-docs-contract.yml",
                ".github/workflows/*.yml",
            )
        )

    def test_paths_ignore_alone_is_not_self_filtered(self) -> None:
        workflow = """\
on:
  pull_request:
    paths-ignore:
      - 'docs/**'
jobs:
  check:
    steps:
      - run: python3 tests/test_glob_docs_contract.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_glob_docs_contract.py",
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/glob-docs-contract.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, set())

    def test_self_filtered_non_contract_unittest_is_not_discovered(self) -> None:
        """review-receipt-retirement.yml has this shape with a different kind."""
        workflow = """\
on:
  pull_request:
    paths:
      - '.github/workflows/review-receipt-retirement.yml'
      - 'tests/test_retired_review_receipt_commands.py'
jobs:
  check:
    steps:
      - run: python3 -m unittest tests/test_retired_review_receipt_commands.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": "retired_command_test",
                    "path": "tests/test_retired_review_receipt_commands.py",
                    "covered_by": [".github/workflows/review-receipt-retirement.yml"],
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/review-receipt-retirement.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, set())

    def test_allowlist_covered_by_registers_even_if_structure_is_odd(self) -> None:
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_new_docs_contract.py",
                    "covered_by": [".github/workflows/odd-docs-contract.yml"],
                }
            ]
        }
        discovered = discover_contract_workflows({}, allowlist)
        self.assertEqual(discovered, {".github/workflows/odd-docs-contract.yml"})

    def test_folded_unittest_invocation_is_discovered(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - ".github/workflows/folded-docs-contract.yml"
jobs:
  check:
    steps:
      - name: Check
        run: |
          python3 -m unittest \\
            tests/test_folded_docs_contract.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_folded_docs_contract.py",
                }
            ]
        }
        self.assertTrue(
            is_contract_workflow(
                ".github/workflows/folded-docs-contract.yml",
                workflow,
                {"tests/test_folded_docs_contract.py"},
            )
        )
        discovered = discover_contract_workflows(
            {".github/workflows/folded-docs-contract.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, {".github/workflows/folded-docs-contract.yml"})

    def test_double_quoted_self_path_counts(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - ".github/workflows/quoted.yml"
jobs:
  check:
    steps:
      - run: python3 -m unittest tests/test_quoted.py
"""
        self.assertTrue(
            is_contract_workflow(
                ".github/workflows/quoted.yml",
                workflow,
                {"tests/test_quoted.py"},
            )
        )

    def test_comment_cannot_satisfy_self_path_or_unittest(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - 'docs/agents/README.md'
      # - '.github/workflows/commented.yml'
jobs:
  check:
    steps:
      # run: python3 -m unittest tests/test_commented.py
      - run: echo ok
"""
        self.assertFalse(
            is_contract_workflow(
                ".github/workflows/commented.yml",
                workflow,
                {"tests/test_commented.py"},
            )
        )

    def test_module_form_unittest_maps_to_test_path(self) -> None:
        self.assertEqual(
            unittest_targets(
                ["python3 -m unittest tests.test_worked_lane_corpus"]
            ),
            {"tests/test_worked_lane_corpus.py"},
        )

    def test_script_form_control_plane_workflow_is_discovered(self) -> None:
        """Copying this gate's `python3 tests/*.py` invocation must not opt out."""
        workflow = """\
on:
  pull_request:
    paths:
      - '.github/workflows/script-docs-contract.yml'
jobs:
  check:
    steps:
      - run: python3 tests/test_script_docs_contract.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_script_docs_contract.py",
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/script-docs-contract.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, {".github/workflows/script-docs-contract.yml"})

    def test_script_form_maps_to_test_path(self) -> None:
        self.assertEqual(
            unittest_targets(
                ["python3 tests/test_docs_agents_contract_workflows.py"]
            ),
            {"tests/test_docs_agents_contract_workflows.py"},
        )

    def test_interpreter_flag_before_script_is_discovered(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - '.github/workflows/flag-docs-contract.yml'
jobs:
  check:
    steps:
      - run: python3 -u tests/test_script_docs_contract.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_script_docs_contract.py",
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/flag-docs-contract.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, {".github/workflows/flag-docs-contract.yml"})
        self.assertEqual(
            unittest_targets(["python3 -u tests/test_script_docs_contract.py"]),
            {"tests/test_script_docs_contract.py"},
        )
        self.assertEqual(
            unittest_targets(["python3 -W error tests/test_script_docs_contract.py"]),
            {"tests/test_script_docs_contract.py"},
        )

    def test_quoted_script_operand_is_discovered(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - '.github/workflows/quoted-docs-contract.yml'
jobs:
  check:
    steps:
      - run: python3 "tests/test_script_docs_contract.py"
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_script_docs_contract.py",
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/quoted-docs-contract.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, {".github/workflows/quoted-docs-contract.yml"})
        self.assertEqual(
            unittest_targets(['python3 "tests/test_script_docs_contract.py"']),
            {"tests/test_script_docs_contract.py"},
        )
        self.assertEqual(
            unittest_targets(["python3 'tests/test_script_docs_contract.py'"]),
            {"tests/test_script_docs_contract.py"},
        )

    def test_quoted_unittest_target_is_discovered(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - '.github/workflows/quoted-unittest-docs-contract.yml'
jobs:
  check:
    steps:
      - run: python3 -m unittest "tests/test_quoted_docs_contract.py"
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_quoted_docs_contract.py",
                }
            ]
        }
        discovered = discover_contract_workflows(
            {
                ".github/workflows/quoted-unittest-docs-contract.yml": workflow
            },
            allowlist,
        )
        self.assertEqual(
            discovered,
            {".github/workflows/quoted-unittest-docs-contract.yml"},
        )
        self.assertEqual(
            unittest_targets(
                ['python3 -m unittest "tests/test_quoted_docs_contract.py"']
            ),
            {"tests/test_quoted_docs_contract.py"},
        )
        self.assertEqual(
            unittest_targets(
                ["python3 -m unittest 'tests.test_quoted_docs_contract'"]
            ),
            {"tests/test_quoted_docs_contract.py"},
        )

    def test_echo_after_unrelated_python_is_not_a_unittest_target(self) -> None:
        self.assertEqual(
            unittest_targets(
                [
                    "python3 tests/unrelated.py && echo tests/test_script_docs_contract.py"
                ]
            ),
            {"tests/unrelated.py"},
        )

    def test_non_python_mention_of_tests_is_not_a_unittest_target(self) -> None:
        self.assertEqual(
            unittest_targets(["echo tests/test_script_docs_contract.py"]),
            set(),
        )
        self.assertEqual(
            unittest_targets(['echo "tests/test_script_docs_contract.py"']),
            set(),
        )
        self.assertEqual(
            unittest_targets(
                ['echo "python3 tests/test_script_docs_contract.py && true"']
            ),
            set(),
        )

    def test_live_discovery_matches_registered_present_class(self) -> None:
        registry = load_registry()
        registered = {posix(path) for path in registered_paths(registry)}
        discovered = discover_contract_workflows(
            live_workflow_files(), load_toml(ALLOWLIST)
        )
        present_named = {
            f"{WORKFLOW_PREFIX}{name}"
            for name in named_class(registry)
            if live_exists(f"{WORKFLOW_PREFIX}{name}")
        }
        self.assertTrue(present_named, "named class on disk must not be empty")
        self.assertEqual(discovered, present_named)
        self.assertEqual(membership_errors(discovered, registered), [])
        self.assertTrue(discovered <= registered)

    def test_flow_style_self_path_counts(self) -> None:
        workflow = """\
on:
  pull_request:
    paths: ['.github/workflows/flow.yml', 'docs/agents/README.md']
jobs:
  check:
    steps:
      - run: python3 -m unittest tests/test_flow.py
"""
        self.assertTrue(
            is_contract_workflow(
                ".github/workflows/flow.yml",
                workflow,
                {"tests/test_flow.py"},
            )
        )

    def test_inline_mapping_self_path_counts(self) -> None:
        workflow = """\
on:
  pull_request: {paths: ['.github/workflows/map.yml']}
jobs:
  check:
    steps:
      - run: python3 -m unittest tests/test_map.py
"""
        self.assertTrue(
            is_contract_workflow(
                ".github/workflows/map.yml",
                workflow,
                {"tests/test_map.py"},
            )
        )

    def test_yaml_suffix_self_filtered_workflow_is_discovered(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - '.github/workflows/extra.yaml'
jobs:
  check:
    steps:
      - run: python3 -m unittest tests/test_extra.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_extra.py",
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/extra.yaml": workflow}, allowlist
        )
        self.assertEqual(discovered, {".github/workflows/extra.yaml"})
        self.assertEqual(_path_shape_errors(".github/workflows/extra.yaml"), [])

    def test_literal_block_with_setup_then_python_is_discovered(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - '.github/workflows/setup-docs-contract.yml'
jobs:
  check:
    steps:
      - name: Check
        run: |
          echo setup
          python3 tests/test_script_docs_contract.py
"""
        allowlist = {
            "allow": [
                {
                    "kind": CONTROL_PLANE_KIND,
                    "path": "tests/test_script_docs_contract.py",
                }
            ]
        }
        discovered = discover_contract_workflows(
            {".github/workflows/setup-docs-contract.yml": workflow}, allowlist
        )
        self.assertEqual(discovered, {".github/workflows/setup-docs-contract.yml"})

    def test_unquoted_self_path_counts(self) -> None:
        workflow = """\
on:
  pull_request:
    paths:
      - .github/workflows/bare.yml
jobs:
  check:
    steps:
      - run: python3 -m unittest tests/test_bare.py
"""
        self.assertTrue(
            is_contract_workflow(
                ".github/workflows/bare.yml",
                workflow,
                {"tests/test_bare.py"},
            )
        )


class HostGateTests(unittest.TestCase):
    def test_missing_pull_request_event_on_host_is_caught(self) -> None:
        ci = """\
on:
  push:
    branches: [main]
jobs:
  merge-gate-shards:
    strategy:
      matrix:
        include:
          - name: policy
            gates: docs_agents_contract_workflows
"""
        policy = """\
  - name: docs_agents_contract_workflows
    required: true
    command: python3 tests/test_docs_agents_contract_workflows.py
"""
        errors = host_gate_errors(ci, policy)
        self.assertTrue(any("must declare on.pull_request" in error for error in errors), errors)

    def test_paths_ignore_on_host_is_caught(self) -> None:
        ci = """\
on:
  pull_request:
    branches: [main]
    paths-ignore:
      - '.github/workflows/**'
jobs:
  merge-gate-shards:
    strategy:
      matrix:
        include:
          - name: policy
            gates: docs_agents_contract_workflows
"""
        policy = """\
  - name: docs_agents_contract_workflows
    required: true
    command: python3 tests/test_docs_agents_contract_workflows.py
"""
        errors = host_gate_errors(ci, policy)
        self.assertTrue(any("paths-ignore" in error for error in errors), errors)

    def test_inline_mapping_path_filter_on_host_is_caught(self) -> None:
        ci = """\
on:
  pull_request: {paths: ['.github/workflows/ci.yml']}
jobs:
  merge-gate-shards:
    strategy:
      matrix:
        include:
          - name: policy
            gates: docs_agents_contract_workflows
"""
        policy = """\
  - name: docs_agents_contract_workflows
    required: true
    command: python3 tests/test_docs_agents_contract_workflows.py
"""
        errors = host_gate_errors(ci, policy)
        self.assertTrue(any("no path filter" in error for error in errors), errors)

    def test_path_filter_on_host_is_caught(self) -> None:
        ci = """\
on:
  pull_request:
    branches: [main]
    paths:
      - '.github/workflows/ci.yml'
jobs:
  merge-gate-shards:
    strategy:
      matrix:
        include:
          - name: policy
            gates: docs_agents_contract_workflows
"""
        policy = """\
  - name: docs_agents_contract_workflows
    required: true
    command: python3 tests/test_docs_agents_contract_workflows.py
"""
        errors = host_gate_errors(ci, policy)
        self.assertTrue(any("no path filter" in error for error in errors), errors)

    def test_advisory_gate_is_caught(self) -> None:
        ci = """\
on:
  pull_request:
    branches: [main]
jobs:
  merge-gate-shards:
    strategy:
      matrix:
        include:
          - name: policy
            gates: docs_agents_contract_workflows
"""
        policy = """\
  - name: docs_agents_contract_workflows
    required: false
    command: python3 tests/test_docs_agents_contract_workflows.py
"""
        errors = host_gate_errors(ci, policy)
        self.assertTrue(any("required: true" in error for error in errors), errors)

    def test_missing_shard_membership_is_caught(self) -> None:
        ci = """\
on:
  pull_request:
    branches: [main]
jobs:
  merge-gate-shards:
    strategy:
      matrix:
        include:
          - name: policy
            gates: non_rust_inventory_check
"""
        policy = """\
  - name: docs_agents_contract_workflows
    required: true
    command: python3 tests/test_docs_agents_contract_workflows.py
"""
        errors = host_gate_errors(ci, policy)
        self.assertTrue(any("policy shard gates must include" in error for error in errors), errors)

    def test_wrong_command_is_caught(self) -> None:
        ci = """\
on:
  pull_request:
    branches: [main]
jobs:
  merge-gate-shards:
    strategy:
      matrix:
        include:
          - name: policy
            gates: docs_agents_contract_workflows
"""
        policy = """\
  - name: docs_agents_contract_workflows
    required: true
    command: python3 -m unittest tests/test_agent_authority_status.py
"""
        errors = host_gate_errors(ci, policy)
        self.assertTrue(any("command must be" in error for error in errors), errors)

    def test_well_formed_host_passes(self) -> None:
        ci = """\
on:
  pull_request:
    branches: [main]
jobs:
  merge-gate-shards:
    strategy:
      matrix:
        include:
          - name: policy
            gates: non_rust_inventory_check docs_agents_contract_workflows
"""
        policy = """\
  - name: docs_agents_contract_workflows
    required: true
    command: python3 tests/test_docs_agents_contract_workflows.py
"""
        self.assertEqual(host_gate_errors(ci, policy), [])


class LiveContractTests(unittest.TestCase):
    def test_live_tree_satisfies_the_contract(self) -> None:
        self.assertEqual(validate_live(), [])

    def test_dropping_named_class_member_from_registry_data_is_caught(self) -> None:
        document = load_registry()
        document["named_class"] = [
            name
            for name in named_class(document)
            if name != "worked-lane-corpus.yml"
        ]
        self.assertNotEqual(set(named_class(document)), ISSUE_CLASS_BASENAMES)


if __name__ == "__main__":
    unittest.main()
