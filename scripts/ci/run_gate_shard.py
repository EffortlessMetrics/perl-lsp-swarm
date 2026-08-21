#!/usr/bin/env python3
"""Run an ordered CI gate shard to terminality without masking later gates.

Each gate remains owned by ``cargo xtask gates --gate``. This adapter only
coordinates independent invocations, validates their receipts against the
canonical receipt contract, applies explicit policy-owned dependency edges,
preserves every completed result, and returns a non-zero shard verdict after
the full selected set has been classified.
"""

from __future__ import annotations

import argparse
import ast
import json
import os
import re
import shlex
import signal
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Sequence

try:
    import yaml
except ModuleNotFoundError:  # pragma: no cover - clean CI has the strict fallback
    yaml = None

SCHEMA_VERSION = "ci_gate_shard.v1"
EXECUTION_POLICY_SCHEMA_VERSION = 1
EXECUTION_POLICY_SOURCE = "gate-shard-execution"
GATE_ID = re.compile(r"^[A-Za-z0-9_.-]+$")
SEMVER = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
PASS_STATUSES = {"pass"}
SKIPPED_STATUSES = {"skip"}
TIMEOUT_STATUSES = {"timeout"}
NOT_PROVEN_STATUSES = {"error"}
DEPENDENCY_FAILURE_POLICIES = {"blocked_not_proven"}
COMMAND_INTERPRETERS = {
    "bash",
    "node",
    "perl",
    "python",
    "python2",
    "python3",
    "pwsh",
    "ruby",
    "sh",
}
COMMAND_PATH_SUFFIXES = {
    ".js",
    ".json",
    ".lock",
    ".md",
    ".pl",
    ".py",
    ".ps1",
    ".rs",
    ".sh",
    ".toml",
    ".txt",
    ".yaml",
    ".yml",
}
OUTPUT_PATH_FLAGS = {
    "--artifact-dir",
    "--json-out",
    "--log-path",
    "--output",
    "--output-path",
    "--receipt-dir",
    "--receipt-path",
    "--report",
    "--summary",
}
INPUT_PATH_FLAGS = {
    "--baseline",
    "--config",
    "--config-path",
    "--file",
    "--input",
    "--input-path",
    "--manifest",
    "--manifest-path",
    "--policy",
    "--receipt-schema",
    "--schema",
}
INTERPRETER_OPTIONS_WITH_ARGUMENT = {
    "--command",
    "--file",
    "-Command",
    "-W",
    "-X",
    "-c",
    "-m",
}
SHELL_OPERATORS = {"&&", ";", "||", "|", "&"}
SUPPORTED_SHELL_KEYWORDS = {"do", "done", "elif", "else", "fi", "for", "if", "in", "then"}
UNSUPPORTED_SHELL_CONSTRUCTS = {
    "!",
    "(",
    "[[",
    "]]",
    "case",
    "coproc",
    "esac",
    "function",
    "select",
    "until",
    "while",
    "{",
    "}",
    ")",
}
INTERPRETER_SIMPLE_OPTIONS = {
    "bash": {"-e", "-f", "-u", "-v", "-x", "--noprofile", "--norc"},
    "sh": {"-e", "-f", "-u", "-v", "-x"},
    "python": {"-B", "-E", "-I", "-O", "-OO", "-S", "-b", "-s", "-u"},
    "python2": {"-B", "-E", "-I", "-O", "-OO", "-S", "-b", "-s", "-u"},
    "python3": {"-B", "-E", "-I", "-O", "-OO", "-S", "-b", "-s", "-u"},
    "pwsh": {"-NoLogo", "-NoProfile", "-NonInteractive"},
}
INTERPRETER_SCRIPT_OPTIONS = {"-File"}
INTERPRETER_ALIASES = {
    **{name: name for name in COMMAND_INTERPRETERS},
    **{f"{name}.exe": name for name in COMMAND_INTERPRETERS},
    "powershell": "pwsh",
    "powershell.exe": "pwsh",
}
ATTACHED_INTERPRETER_ALIASES = {
    alias
    for alias, canonical in INTERPRETER_ALIASES.items()
    if canonical in {"bash", "python", "python2", "python3", "pwsh", "sh"}
}
UNSAFE_SHELL_COMMANDS = {"alias", "cd", "eval", "exec", "source"}
REDIRECTION = re.compile(
    r"^(?P<fd>[0-9]*)(?P<operator>&>>|&>|>>|\|>|>&|<&|<<|>|<)(?P<target>.*)$"
)
ENV_ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
MALFORMED_SHELL_PUNCTUATION = {";;", ";&", ";;&", "&&&", "|||"}
UNSUPPORTED_WRAPPER_COMMANDS = {
    "busybox",
    "call",
    "cmd",
    "cmd.exe",
    "chroot",
    "nice",
    "nohup",
    "parallel",
    "setsid",
    "stdbuf",
    "start",
    "sudo",
    "taskset",
    "time",
    "xargs",
}
SHELL_PUNCTUATION = set(";|&<>")
SUPPORTED_SHELL_PUNCTUATION = SHELL_OPERATORS | {"<", ">", "<<", ">>", ">&", "<&"}


@dataclass(frozen=True)
class DependencyRule:
    requires: tuple[str, ...]
    on_dependency_failure: str


@dataclass(frozen=True)
class ReceiptContract:
    top_required: frozenset[str]
    metadata_required: frozenset[str]
    gate_required: frozenset[str]
    summary_required: frozenset[str]
    gate_statuses: frozenset[str]
    gate_tiers: frozenset[str]
    summary_statuses: frozenset[str]


@dataclass
class GateObservation:
    gate_name: str
    command: list[str]
    reproduce: str
    receipt_path: str
    result: str
    receipt_status: str | None
    exit_code: int | None
    duration_ms: int
    blocked_by: list[str]
    message: str | None = None


def _yaml_scalar(value: str) -> str:
    """Decode the small scalar subset used by gate command declarations."""
    value = value.strip()
    if not value:
        return ""
    if value[0] == "'":
        if len(value) < 2 or value[-1] != "'":
            raise ValueError(f"gate command has invalid quoted scalar {value!r}")
        return value[1:-1].replace("''", "'")
    if value[0] == '"':
        try:
            decoded = ast.literal_eval(value)
        except (SyntaxError, ValueError) as error:
            raise ValueError(f"gate command has invalid quoted scalar {value!r}") from error
        if not isinstance(decoded, str):
            raise ValueError("gate command scalar must be a string")
        return decoded
    return value


def _yaml_inline_comment(value: str) -> str:
    quote: str | None = None
    escaped = False
    for index, character in enumerate(value):
        if quote is not None:
            if escaped:
                escaped = False
            elif character == "\\" and quote == '"':
                escaped = True
            elif character == quote:
                quote = None
            continue
        if character in {"'", '"'}:
            quote = character
        elif character == "#" and (index == 0 or value[index - 1].isspace()):
            return value[:index].rstrip()
    return value.strip()


def _yaml_command_scalar(value: str) -> str:
    """Accept only YAML strings for scalar command declarations."""
    stripped = value.strip()
    if not stripped:
        return ""
    candidate = _yaml_inline_comment(stripped)
    if candidate and candidate[0] in {"'", '"'}:
        return _yaml_scalar(candidate)
    if candidate.lower() in {"false", "null", "no", "off", "on", "true", "yes", "~"}:
        raise ValueError("gate command must be a YAML string")
    if re.fullmatch(
        r"(?:[-+]?(?:0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+)|"
        r"[-+]?(?:[0-9][0-9_]*(?:\.[0-9_]*)?|\.[0-9][0-9_]*)(?:[eE][-+]?[0-9_]+)?|"
        r"[-+]?\.(?:inf|nan)|[0-9]+(?::[0-9]+){1,2}|"
        r"[0-9]{4}-[0-9]{2}-[0-9]{2}(?:[Tt ].*)?)",
        candidate,
        flags=re.IGNORECASE,
    ):
        raise ValueError("gate command must be a YAML string")
    if candidate.startswith(("&", "*", "[", "{")):
        raise ValueError("gate command must be a YAML string")
    if candidate.startswith((">", "|")):
        raise ValueError("unsupported YAML block scalar indicator")
    return stripped


def _read_gate_command_specs(path: Path) -> dict[str, str]:
    """Read gate names and commands without requiring a third-party YAML module.

    The repository policy uses a deliberately small shape for these fields:
    ``gates:`` contains ``- name:`` records and each record has a scalar or
    folded ``command:`` value. Keeping this parser local makes the shard
    preflight runnable in the clean Python environment used by CI.
    """
    if not path.exists() or path.is_symlink() or not path.is_file():
        raise ValueError(f"gate policy is missing or not a regular file: {path}")
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise ValueError(f"gate policy is unreadable: {error}") from error
    if yaml is not None:
        return _read_gate_command_specs_with_yaml(text, path)
    return _read_gate_command_specs_fallback(text, path)


def _read_gate_command_specs_with_yaml(text: str, path: Path) -> dict[str, str]:
    """Read commands with the same folded/literal scalar semantics as YAML."""
    if yaml is None:
        raise AssertionError("YAML parser is unavailable")
    for raw in text.splitlines():
        if raw.startswith("    command:"):
            value = raw.split("command:", 1)[1].strip()
            candidate = _yaml_inline_comment(value)
            if candidate.startswith(("&", "*", "!")):
                raise ValueError("gate command must be a YAML string (unsupported anchor or tag)")
            if candidate.startswith((">", "|")) and not re.fullmatch(
                r"[>|][+-]?", candidate
            ):
                raise ValueError("unsupported YAML block scalar indicator")
    try:
        document = yaml.safe_load(text)
    except yaml.YAMLError as error:
        raise ValueError(f"gate policy has invalid YAML: {error}") from error
    if not isinstance(document, dict) or not isinstance(document.get("gates"), list):
        raise ValueError(f"gate policy has no gates sequence: {path}")
    specs: dict[str, str] = {}
    for index, row in enumerate(document["gates"]):
        if not isinstance(row, dict):
            raise ValueError(f"gate policy has invalid gate row at index {index}")
        name = row.get("name")
        if not isinstance(name, str) or not name:
            raise ValueError(f"gate policy has an empty gate name at index {index}")
        if name in specs:
            raise ValueError(f"gate policy repeats gate {name!r}")
        command = row.get("command", "")
        if "command" in row and command is None:
            raise ValueError(f"gate {name!r} command must be a YAML string")
        if not isinstance(command, str):
            raise ValueError(f"gate {name!r} command must be a YAML string")
        specs[name] = command
    return specs


def _read_gate_command_specs_fallback(text: str, path: Path) -> dict[str, str]:
    """Strict fallback for environments without the repository YAML module."""
    lines = text.splitlines()
    in_gates = False
    current: str | None = None
    specs: dict[str, str] = {}
    line_number = 0
    while line_number < len(lines):
        raw = lines[line_number]
        stripped = raw.strip()
        if not in_gates:
            if raw.rstrip() == "gates:":
                in_gates = True
            line_number += 1
            continue
        if stripped.startswith("#"):
            line_number += 1
            continue
        if stripped and not raw.startswith(" "):
            break
        if raw.startswith("  - name:"):
            name = _yaml_scalar(raw.split("name:", 1)[1])
            if not name:
                raise ValueError(f"gate policy has an empty gate name at line {line_number + 1}")
            if name in specs:
                raise ValueError(f"gate policy repeats gate {name!r}")
            current = name
            specs[current] = ""
            line_number += 1
            continue
        if current is None or not raw.startswith("    command:"):
            line_number += 1
            continue

        value = raw.split("command:", 1)[1].strip()
        block_header = re.fullmatch(r"(?P<header>[>|][+-]?)(?:\s+#.*)?", value)
        if block_header is not None:
            value = block_header.group("header")
        if value in {">", ">-", ">+", "|", "|-", "|+"}:
            folded = value.startswith(">")
            continuation: list[str] = []
            cursor = line_number + 1
            while cursor < len(lines):
                next_raw = lines[cursor]
                if next_raw.strip() and not next_raw.startswith("      "):
                    break
                continuation.append(next_raw.strip())
                cursor += 1
            if folded:
                command = " ".join(part for part in continuation if part)
            else:
                command = "\n".join(continuation)
            if value.endswith("-"):
                command = command.rstrip(" \n")
            specs[current] = command
            line_number = cursor
            continue

        specs[current] = _yaml_command_scalar(value)
        line_number += 1
    if not in_gates:
        raise ValueError(f"gate policy has no gates sequence: {path}")
    return specs


def _looks_like_command_path(token: str) -> bool:
    if not token or token.startswith("-") or token.startswith("{"):
        return False
    return (
        "/" in token
        or "\\" in token
        or Path(token).suffix.lower() in COMMAND_PATH_SUFFIXES
    )


def _contains_shell_expansion(token: str) -> bool:
    return (
        "$" in token
        or "`" in token
        or "*" in token
        or "?" in token
        or re.search(r"%[^%\r\n]+%", token) is not None
    )


def _contains_dynamic_shell_expansion(token: str) -> bool:
    return bool(
        "`" in token
        or "$(" in token
        or "<(" in token
        or ">(" in token
        or re.search(r"\$(?:[A-Za-z_][A-Za-z0-9_]*|\{[^}]+\}|[?*@#0-9])", token)
        or re.search(r"%[^%\r\n]+%", token) is not None
    )


def _contains_windows_path_syntax(command: str) -> bool:
    return bool(
        re.search(r"(?:^|[\s=\"'(])[A-Za-z]:[\\/]", command)
        or re.search(
            r"(?:^|[\s=])(?:[A-Za-z0-9_.-]+\\)+[A-Za-z0-9_.-]+",
            command,
        )
        or "\\\\" in command
    )


def _reject_ambiguous_unquoted_hash(value: str) -> None:
    """Reject plain YAML hashes that could hide shell syntax after parsing."""
    quote: str | None = None
    escaped = False
    for index, character in enumerate(value):
        if quote is not None:
            if escaped:
                escaped = False
            elif character == "\\" and quote == '"':
                escaped = True
            elif character == quote:
                quote = None
            continue
        if character in {"'", '"'}:
            quote = character
        elif character == "#" and index > 0 and not value[index - 1].isspace():
            raise ValueError("ambiguous unquoted YAML # in gate command")


def _is_determinism_output_path(tokens: Sequence[str], index: int) -> bool:
    """Allow only the checked-in determinism gate's loop-generated log path."""
    suffix = (
        ";",
        "done",
        ";",
        "diff",
        "-q",
        "run_1.log",
        "run_2.log",
        "&&",
        "diff",
        "-q",
        "run_1.log",
        "run_3.log",
    )
    return (
        index > 0
        and tokens[index] == "run_${i}.log"
        and tokens[index - 1] == ">"
        and tuple(tokens[:8]) == ("for", "i", "in", "1", "2", "3", ";", "do")
        and tuple(tokens[index + 1 :]) == suffix
    )


def _is_determinism_filter(tokens: Sequence[str], index: int) -> bool:
    return (
        index + 2 < len(tokens)
        and tokens[index] == r"s/^Finished in .*//; s/^\s*Time:\s.*$//"
        and tokens[index + 1] == ">"
        and _is_determinism_output_path(tokens, index + 2)
    )


def _is_nested_lock_grouping(tokens: Sequence[str], index: int) -> bool:
    return (
        tokens[index] in {"(", ")"}
        and list(tokens[:2]) == ["if", "find"]
        and (
            (
                tokens[index] == "("
                and index >= 2
                and tuple(tokens[index - 2 : index + 1]) == ("find", ".", "(")
            )
            or (
                tokens[index] == ")"
                and index + 1 < len(tokens)
                and tokens[index + 1] == "-prune"
            )
        )
    )


def _split_gate_command(command: str) -> list[str]:
    lexer = shlex.shlex(command, posix=True, punctuation_chars=";&|<>")
    lexer.whitespace_split = True
    lexer.commenters = "#"
    return list(lexer)


def _interpreter_name(token: str) -> str | None:
    token_name = token.replace("\\", "/").rsplit("/", 1)[-1].lower()
    return INTERPRETER_ALIASES.get(token_name)


def _is_env_assignment(token: str) -> bool:
    return ENV_ASSIGNMENT.match(token) is not None


def _repository_relative_path(
    path: str,
    root: Path,
    *,
    subject: str,
    require_file: bool = False,
    allow_determinism_output: bool = False,
) -> None:
    """Reject path syntax that can escape or dynamically leave the checkout."""
    path = path.rstrip(";")
    if path in {"&0", "&1", "&2"} or path == "/dev/null":
        return
    if "\\" in path or re.match(r"^[A-Za-z]:", path):
        raise ValueError(f"unsupported Windows path spelling in {subject}: {path}")
    if path.startswith("~"):
        raise ValueError(f"unsupported tilde expansion in {subject}: {path}")
    if _contains_shell_expansion(path) and not (
        allow_determinism_output and path == "run_${i}.log"
    ):
        raise ValueError(f"unsupported shell expansion in {subject}: {path}")
    candidate = Path(path)
    if candidate.is_absolute() or re.match(r"^[A-Za-z]:[\\/]", path):
        resolved = candidate.resolve()
    else:
        if ".." in Path(path.replace("\\", "/")).parts:
            raise ValueError(f"path escapes checked-out tree in {subject}: {path}")
        resolved = (root / candidate).resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError as error:
        raise ValueError(f"path escapes checked-out tree in {subject}: {path}") from error
    if require_file and (not resolved.exists() or resolved.is_symlink() or not resolved.is_file()):
        raise ValueError(f"missing input path in {subject}: {path}")


def _validate_shell_constructs(tokens: Sequence[str], root: Path) -> None:
    """Reject shell forms whose nested paths cannot be proven statically."""
    for index, token in enumerate(tokens):
        token_name = token.replace("\\", "/").rsplit("/", 1)[-1].lower()
        if _is_env_assignment(token) and (
            index == 0 or tokens[index - 1] in SHELL_OPERATORS
        ):
            raise ValueError(f"unsupported shell prefix assignment in gate policy: {token}")
        if (
            ("{" in token or "}" in token)
            and token != "{package_args}"
            and not _is_determinism_output_path(tokens, index)
        ):
            raise ValueError(f"unsupported shell brace expansion in gate policy: {token}")
        if (
            ("*" in token or "?" in token)
            and not _is_determinism_filter(tokens, index)
        ):
            raise ValueError(f"unsupported shell glob expansion in gate policy: {token}")
        if token in UNSAFE_SHELL_COMMANDS:
            raise ValueError(f"unsupported shell command in gate policy: {token}")
        if token in UNSUPPORTED_SHELL_CONSTRUCTS and not _is_nested_lock_grouping(
            tokens, index
        ):
            raise ValueError(f"unsupported shell construct in gate policy: {token}")
        if token in MALFORMED_SHELL_PUNCTUATION:
            raise ValueError(f"malformed shell punctuation in gate policy: {token}")
        if token in {"&>", "&>>", ">|"}:
            raise ValueError(f"unsupported Bash redirection in gate policy: {token}")
        if (
            token
            and set(token) <= SHELL_PUNCTUATION
            and token not in SUPPORTED_SHELL_PUNCTUATION
        ):
            raise ValueError(f"malformed shell punctuation in gate policy: {token}")
        if token.startswith("~"):
            raise ValueError(f"unsupported tilde expansion in gate policy: {token}")
        if (
            not _is_determinism_output_path(tokens, index)
            and _contains_dynamic_shell_expansion(token)
        ):
            raise ValueError(f"unsupported dynamic shell expansion: {token}")
        if "$(" in token or "<(" in token or ">(" in token or "`" in token:
            raise ValueError(f"unsupported nested shell expression: {token}")
        if token_name == "env" and index + 1 < len(tokens):
            next_token = tokens[index + 1]
            if next_token == "-S" or next_token.startswith("-S"):
                raise ValueError("unsupported env -S shell-string execution")
        if token_name in UNSUPPORTED_WRAPPER_COMMANDS:
            raise ValueError(f"unsupported nested-shell wrapper in gate policy: {token}")
        if token_name == "find" and any(
            candidate in {"-exec", "-execdir"} for candidate in tokens[index + 1 :]
        ):
            raise ValueError("unsupported find -exec nested-shell wrapper in gate policy")
        if token_name == "command" and index + 1 < len(tokens):
            if _interpreter_name(tokens[index + 1]) is not None:
                raise ValueError("unsupported command nested-shell wrapper in gate policy")
        if token_name == "timeout" and any(
            _interpreter_name(candidate) is not None for candidate in tokens[index + 1 :]
        ):
            raise ValueError("unsupported timeout nested-shell wrapper in gate policy")

        match = REDIRECTION.match(token)
        if match is None:
            continue
        operator = match.group("operator")
        target = match.group("target")
        if operator == "<<":
            raise ValueError("unsupported here-document redirection")
        if not target:
            if index + 1 >= len(tokens):
                raise ValueError("redirection is missing its target")
            target = tokens[index + 1]
        if target.startswith("&") or operator in {">&", "<&"}:
            if target not in {"&0", "&1", "&2", "0", "1", "2"}:
                raise ValueError(f"unsupported file-descriptor redirection: {target}")
            continue
        if operator in {">&", "<&"} and target in {"0", "1", "2"}:
            continue
        _repository_relative_path(
            target,
            root,
            subject="redirection",
            require_file=operator == "<",
            allow_determinism_output=_is_determinism_output_path(tokens, index + 1)
            if not match.group("target")
            else _is_determinism_output_path(tokens, index),
        )


def _referenced_command_paths(tokens: Sequence[str], root: Path) -> list[str]:
    """Return input script and option paths while validating output destinations.

    The shard accepts only paths that can be resolved against the checked-out
    tree. Shell expansions and globbed paths are rejected rather than guessed.
    Output destinations remain outside the existence proof: they are confined
    to the checkout and checked for values, but are not required to pre-exist.
    """
    references: list[str] = []
    skip_next_output = False
    skip_next_interpreter_option = False
    expect_input_path = False
    expect_interpreter_script = False
    interpreter_pending = False
    interpreter_name: str | None = None
    env_pending = False
    env_command_pending = False
    env_skip_option_value = False
    command_position = True
    redirection_target_pending = False
    separator_requires_command = False
    for index, token in enumerate(tokens):
        token_name = token.replace("\\", "/").rsplit("/", 1)[-1].lower()
        interpreter_token_name = _interpreter_name(token)
        if skip_next_output:
            skip_next_output = False
            if token.startswith("-") or token in SHELL_OPERATORS or REDIRECTION.match(token):
                raise ValueError(f"output path option is missing its value: {token}")
            _repository_relative_path(token, root, subject="output path")
            continue
        if token in OUTPUT_PATH_FLAGS:
            skip_next_output = True
            continue
        if token == "--receipt":
            # The xtask corpus gates use `--receipt` as a boolean, while
            # several standalone gates use `--receipt PATH`. Treat a
            # non-option successor as the optional output destination.
            if index + 1 < len(tokens):
                successor = tokens[index + 1]
                if (
                    not successor.startswith("-")
                    and successor not in SHELL_OPERATORS
                    and REDIRECTION.match(successor) is None
                ):
                    skip_next_output = True
            continue
        if token.startswith("--receipt="):
            value = token.split("=", 1)[1]
            if not value:
                raise ValueError(f"output path option is missing its value: {token}")
            _repository_relative_path(value, root, subject="output path")
            continue
        if any(token.startswith(f"{flag}=") for flag in OUTPUT_PATH_FLAGS):
            value = token.split("=", 1)[1]
            if not value:
                raise ValueError(f"output path option is missing its value: {token}")
            _repository_relative_path(value, root, subject="output path")
            continue

        if redirection_target_pending:
            redirection_target_pending = False
            continue
        redirection = REDIRECTION.match(token)
        if redirection is not None:
            if not redirection.group("target"):
                redirection_target_pending = True
            continue

        if expect_input_path:
            if token.startswith("-"):
                raise ValueError(f"input path option is missing its value: {token}")
            if _contains_shell_expansion(token):
                raise ValueError(f"unsupported shell expansion in input path: {token}")
            references.append(token)
            expect_input_path = False
            continue

        if token in INPUT_PATH_FLAGS:
            expect_input_path = True
            continue
        if any(token.startswith(f"{flag}=") for flag in INPUT_PATH_FLAGS):
            value = token.split("=", 1)[1]
            if not value or _contains_shell_expansion(value):
                raise ValueError(f"unsupported shell expansion in input path: {token}")
            references.append(value)
            continue

        if token in SHELL_OPERATORS:
            command_position = True
            separator_requires_command = token in {"&&", "||", "|", "&"}
            interpreter_pending = False
            interpreter_name = None
            skip_next_interpreter_option = False
            expect_interpreter_script = False
            env_pending = False
            env_command_pending = False
            env_skip_option_value = False
            continue

        if env_command_pending:
            if _is_env_assignment(token):
                continue
            if token.startswith("-"):
                raise ValueError(f"env -- command is missing or invalid: {token}")
            env_command_pending = False
            if interpreter_token_name is not None:
                interpreter_pending = True
                interpreter_name = interpreter_token_name
                command_position = False
                separator_requires_command = False
                continue
            if _looks_like_command_path(token):
                references.append(token)
                command_position = False
                separator_requires_command = False
                continue
            raise ValueError(f"env -- command is not a supported executable path: {token}")

        if env_pending:
            if env_skip_option_value:
                env_skip_option_value = False
                continue
            if token == "-S" or token.startswith("-S"):
                raise ValueError("unsupported env -S shell-string execution")
            if token in {"-u", "--unset"}:
                env_skip_option_value = True
                continue
            if token in {"-i", "--ignore-environment", "-0", "--null"}:
                continue
            if token == "--":
                env_pending = False
                env_command_pending = True
                continue
            if _is_env_assignment(token):
                continue
            if token.startswith("-"):
                raise ValueError(f"unsupported env option: {token}")
            env_pending = False
            if interpreter_token_name is not None:
                interpreter_pending = True
                interpreter_name = interpreter_token_name
                command_position = False
                separator_requires_command = False
                continue
            if _looks_like_command_path(token):
                references.append(token)
                command_position = False
                separator_requires_command = False
            continue

        if command_position and token_name == "env":
            env_pending = True
            command_position = False
            separator_requires_command = False
            continue

        if skip_next_interpreter_option:
            skip_next_interpreter_option = False
            if token.startswith("-") or token in SHELL_OPERATORS or REDIRECTION.match(token):
                raise ValueError(f"interpreter option is missing its value: {token}")
            continue
        if interpreter_pending:
            if expect_interpreter_script:
                if token.startswith("-"):
                    raise ValueError(f"interpreter script option is missing its path: {token}")
                if _contains_shell_expansion(token):
                    raise ValueError(f"unsupported shell expansion in command path: {token}")
                references.append(token)
                expect_interpreter_script = False
                interpreter_pending = False
                interpreter_name = None
                command_position = False
                separator_requires_command = False
                continue
            if token in INTERPRETER_SCRIPT_OPTIONS:
                if interpreter_name != "pwsh":
                    raise ValueError(f"unsupported interpreter script option: {token}")
                expect_interpreter_script = True
                continue
            if token in INTERPRETER_OPTIONS_WITH_ARGUMENT:
                if token in {"-c", "--command", "-Command", "-m"}:
                    raise ValueError(f"unsupported nested interpreter command: {token}")
                skip_next_interpreter_option = True
                continue
            if token.startswith("-"):
                allowed = INTERPRETER_SIMPLE_OPTIONS.get(interpreter_name or "", set())
                if token not in allowed:
                    raise ValueError(f"unsupported or attached interpreter option: {token}")
                continue
            if _contains_shell_expansion(token):
                raise ValueError(f"unsupported shell expansion in command path: {token}")
            if token in SUPPORTED_SHELL_KEYWORDS:
                raise ValueError(f"interpreter script path is missing before shell keyword: {token}")
            references.append(token)
            interpreter_pending = False
            interpreter_name = None
            command_position = False
            continue

        if command_position and token in SUPPORTED_SHELL_KEYWORDS:
            separator_requires_command = False
            continue
        if command_position and interpreter_token_name is not None:
            separator_requires_command = False
            interpreter_pending = True
            interpreter_name = interpreter_token_name
            command_position = False
            continue
        if any(
            token_name.startswith(f"{interpreter}-")
            for interpreter in ATTACHED_INTERPRETER_ALIASES
        ):
            raise ValueError(f"unsupported attached interpreter option: {token}")
        if command_position and _looks_like_command_path(token):
            references.append(token)
        if command_position:
            command_position = False
            separator_requires_command = False
    if expect_input_path:
        raise ValueError("input path option is missing its value")
    if expect_interpreter_script:
        raise ValueError("interpreter script option is missing its path")
    if skip_next_output:
        raise ValueError("output path option is missing its value")
    if skip_next_interpreter_option:
        raise ValueError("interpreter option is missing its value")
    if interpreter_pending:
        raise ValueError("interpreter command is missing its script path")
    if env_command_pending:
        raise ValueError("env -- wrapper is missing its command")
    if env_pending:
        raise ValueError("env wrapper is missing its command")
    if redirection_target_pending:
        raise ValueError("redirection is missing its target")
    if separator_requires_command:
        raise ValueError("shell separator is missing its command")
    return references


def load_gate_commands(
    path: Path,
    selected_gates: Sequence[str],
    *,
    root: Path | None = None,
) -> dict[str, str]:
    """Validate selected gates against the exact checked-out policy.

    This is intentionally a pre-execution boundary: no selected gate may be
    spawned until its policy row, command string, and any referenced input
    script path are present in the checked-out tree.
    """
    specs = _read_gate_command_specs(path)
    policy_root = path.parent.parent if path.parent.name == ".ci" else path.parent
    repository_root = (root or policy_root).resolve()
    commands: dict[str, str] = {}
    for gate in selected_gates:
        if gate not in specs:
            raise ValueError(f"selected gate {gate!r} has no gate-policy row")
        command = specs[gate].strip()
        if not command:
            raise ValueError(f"selected gate {gate!r} has no executable command")
        _reject_ambiguous_unquoted_hash(command)
        if "`" in command:
            raise ValueError(f"selected gate {gate!r} uses unsupported backtick substitution")
        if _contains_windows_path_syntax(command):
            raise ValueError(f"selected gate {gate!r} uses unsupported Windows path spelling")
        try:
            tokens = _split_gate_command(command)
        except ValueError as error:
            raise ValueError(f"selected gate {gate!r} has an invalid command: {error}") from error
        if not tokens or not any(token.strip() for token in tokens):
            raise ValueError(f"selected gate {gate!r} has no executable command")
        _validate_shell_constructs(tokens, repository_root)
        for reference in _referenced_command_paths(tokens, repository_root):
            referenced = Path(reference)
            if not referenced.is_absolute():
                referenced = repository_root / referenced
            try:
                referenced.resolve().relative_to(repository_root)
            except ValueError as error:
                raise ValueError(
                    f"selected gate {gate!r} references path outside checked-out tree: {reference}"
                ) from error
            if not referenced.exists() or referenced.is_symlink() or not referenced.is_file():
                raise ValueError(
                    f"selected gate {gate!r} references missing command path: {reference}"
                )
        commands[gate] = command
    return commands


def _regular_json_object(path: Path, *, subject: str) -> dict[str, Any]:
    if not path.exists() or path.is_symlink() or not path.is_file():
        raise ValueError(f"{subject} is missing or not a regular file: {path}")
    try:
        value: Any = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ValueError(f"{subject} is unreadable: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{subject} root must be an object")
    return value


def _required_set(value: Any, *, subject: str) -> frozenset[str]:
    if not isinstance(value, list) or not value or any(
        not isinstance(item, str) or not item for item in value
    ):
        raise ValueError(f"{subject} must be a non-empty string list")
    if len(value) != len(set(value)):
        raise ValueError(f"{subject} contains duplicates")
    return frozenset(value)


def _enum_set(value: Any, *, subject: str) -> frozenset[str]:
    if not isinstance(value, list) or not value or any(
        not isinstance(item, str) or not item for item in value
    ):
        raise ValueError(f"{subject} must be a non-empty string enum")
    return frozenset(value)


def load_receipt_contract(path: Path) -> ReceiptContract:
    schema = _regular_json_object(path, subject="gate receipt schema")
    definitions = schema.get("$defs")
    if not isinstance(definitions, dict):
        raise ValueError("gate receipt schema has no $defs object")
    metadata = definitions.get("metadata")
    gate_result = definitions.get("gate_result")
    summary = definitions.get("summary")
    if not all(isinstance(item, dict) for item in (metadata, gate_result, summary)):
        raise ValueError(
            "gate receipt schema is missing metadata/gate_result/summary definitions"
        )

    gate_properties = gate_result.get("properties")
    summary_properties = summary.get("properties")
    if not isinstance(gate_properties, dict) or not isinstance(summary_properties, dict):
        raise ValueError("gate receipt schema has invalid properties")
    status_property = gate_properties.get("status")
    tier_property = gate_properties.get("tier")
    overall_property = summary_properties.get("overall_status")
    if not all(
        isinstance(item, dict)
        for item in (status_property, tier_property, overall_property)
    ):
        raise ValueError(
            "gate receipt schema is missing status/tier/overall_status properties"
        )

    return ReceiptContract(
        top_required=_required_set(schema.get("required"), subject="receipt required"),
        metadata_required=_required_set(
            metadata.get("required"), subject="receipt metadata required"
        ),
        gate_required=_required_set(
            gate_result.get("required"), subject="receipt gate required"
        ),
        summary_required=_required_set(
            summary.get("required"), subject="receipt summary required"
        ),
        gate_statuses=_enum_set(
            status_property.get("enum"), subject="receipt gate statuses"
        ),
        gate_tiers=_enum_set(
            tier_property.get("enum"), subject="receipt gate tiers"
        ),
        summary_statuses=_enum_set(
            overall_property.get("enum"), subject="receipt summary statuses"
        ),
    )


def load_execution_policy(
    path: Path, selected_gates: Sequence[str]
) -> dict[str, DependencyRule]:
    payload = _regular_json_object(path, subject="shard execution policy")
    if payload.get("schema_version") != EXECUTION_POLICY_SCHEMA_VERSION:
        raise ValueError(
            "unsupported shard execution policy schema_version "
            f"{payload.get('schema_version')!r}"
        )
    if payload.get("source") != EXECUTION_POLICY_SOURCE:
        raise ValueError(
            f"unsupported shard execution policy source {payload.get('source')!r}"
        )
    unknown_top_level = set(payload) - {
        "schema_version",
        "source",
        "owner_issue",
        "migration_owner_issue",
        "gates",
    }
    if unknown_top_level:
        raise ValueError(
            "shard execution policy has unsupported top-level fields: "
            + ", ".join(sorted(unknown_top_level))
        )
    for field in ("owner_issue", "migration_owner_issue"):
        if not isinstance(payload.get(field), int) or payload[field] <= 0:
            raise ValueError(
                f"shard execution policy {field} must be a positive issue number"
            )
    raw_gates = payload.get("gates")
    if not isinstance(raw_gates, dict):
        raise ValueError("shard execution policy must contain a gates object")

    selected = set(selected_gates)
    rules: dict[str, DependencyRule] = {}
    for gate in selected_gates:
        raw_rule = raw_gates.get(gate)
        if not isinstance(raw_rule, dict):
            raise ValueError(f"selected gate {gate!r} has no execution-policy row")
        unknown = set(raw_rule) - {"requires", "on_dependency_failure"}
        if unknown:
            raise ValueError(
                f"selected gate {gate!r} has unsupported execution-policy fields: "
                + ", ".join(sorted(unknown))
            )
        raw_requires = raw_rule.get("requires", [])
        if not isinstance(raw_requires, list) or any(
            not isinstance(item, str) or not GATE_ID.fullmatch(item)
            for item in raw_requires
        ):
            raise ValueError(f"selected gate {gate!r} has an invalid requires list")
        requires = tuple(raw_requires)
        if len(requires) != len(set(requires)):
            raise ValueError(f"selected gate {gate!r} repeats a dependency")
        if gate in requires:
            raise ValueError(f"selected gate {gate!r} depends on itself")
        missing = [dependency for dependency in requires if dependency not in selected]
        if missing:
            raise ValueError(
                f"selected gate {gate!r} requires unselected gate(s): "
                + ", ".join(missing)
            )
        failure_policy = raw_rule.get(
            "on_dependency_failure", "blocked_not_proven"
        )
        if failure_policy not in DEPENDENCY_FAILURE_POLICIES:
            raise ValueError(
                f"selected gate {gate!r} has unsupported on_dependency_failure "
                f"{failure_policy!r}"
            )
        rules[gate] = DependencyRule(
            requires=requires,
            on_dependency_failure=failure_policy,
        )

    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(gate: str) -> None:
        if gate in visiting:
            raise ValueError(f"shard execution dependency cycle includes {gate!r}")
        if gate in visited:
            return
        visiting.add(gate)
        for dependency in rules[gate].requires:
            visit(dependency)
        visiting.remove(gate)
        visited.add(gate)

    for gate in selected_gates:
        visit(gate)
    return rules


def _missing_required(value: dict[str, Any], required: frozenset[str]) -> list[str]:
    return sorted(required - set(value))


def _nonnegative_integer(value: Any, *, subject: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{subject} must be a non-negative integer")
    return value


def _validate_receipt(
    payload: dict[str, Any],
    *,
    contract: ReceiptContract,
    gate: str,
    subject_sha: str,
) -> str:
    missing = _missing_required(payload, contract.top_required)
    if missing:
        raise ValueError(
            "receipt is missing required field(s): " + ", ".join(missing)
        )

    version = payload.get("schema_version")
    if not (
        (isinstance(version, int) and not isinstance(version, bool) and version >= 1)
        or (isinstance(version, str) and SEMVER.fullmatch(version))
    ):
        raise ValueError(f"receipt has unsupported schema_version {version!r}")

    metadata = payload.get("metadata")
    if not isinstance(metadata, dict):
        raise ValueError("receipt metadata must be an object")
    missing = _missing_required(metadata, contract.metadata_required)
    if missing:
        raise ValueError(
            "receipt metadata is missing required field(s): " + ", ".join(missing)
        )
    if subject_sha and metadata.get("git_sha") != subject_sha:
        raise ValueError(
            f"receipt subject {metadata.get('git_sha')!r} does not match "
            f"{subject_sha!r}"
        )
    if not isinstance(metadata.get("timestamp"), str) or not metadata["timestamp"]:
        raise ValueError("receipt metadata timestamp must be a non-empty string")
    if not isinstance(metadata.get("git_branch"), str) or not metadata["git_branch"]:
        raise ValueError("receipt metadata git_branch must be a non-empty string")
    for field in ("toolchain", "platform", "environment"):
        if not isinstance(metadata.get(field), dict):
            raise ValueError(f"receipt metadata {field} must be an object")

    rows = payload.get("gates")
    if not isinstance(rows, list) or len(rows) != 1 or not isinstance(rows[0], dict):
        raise ValueError("single-gate invocation must emit exactly one gate row")
    row = rows[0]
    missing = _missing_required(row, contract.gate_required)
    if missing:
        raise ValueError(
            "receipt gate row is missing required field(s): " + ", ".join(missing)
        )
    observed_name = row.get("gate_name")
    if observed_name != gate:
        raise ValueError(f"receipt names gate {observed_name!r}, expected {gate!r}")
    tier = row.get("tier")
    if tier not in contract.gate_tiers:
        raise ValueError(f"receipt gate row has unsupported tier {tier!r}")
    status = row.get("status")
    if status not in contract.gate_statuses:
        raise ValueError(f"receipt gate row has unsupported status {status!r}")
    _nonnegative_integer(
        row.get("duration_ms"), subject="receipt gate duration_ms"
    )
    if not isinstance(row.get("command"), str) or not row["command"]:
        raise ValueError("receipt gate command must be a non-empty string")
    exit_code = row.get("exit_code")
    if exit_code is not None and (
        isinstance(exit_code, bool) or not isinstance(exit_code, int)
    ):
        raise ValueError("receipt gate exit_code must be an integer or null")

    summary = payload.get("summary")
    if not isinstance(summary, dict):
        raise ValueError("receipt summary must be an object")
    missing = _missing_required(summary, contract.summary_required)
    if missing:
        raise ValueError(
            "receipt summary is missing required field(s): " + ", ".join(missing)
        )
    counts = {
        key: _nonnegative_integer(
            summary.get(key, 0), subject=f"receipt summary {key}"
        )
        for key in ("passed", "failed", "skipped", "timeout", "error")
    }
    total_gates = _nonnegative_integer(
        summary.get("total_gates"), subject="receipt summary total_gates"
    )
    _nonnegative_integer(
        summary.get("total_duration_ms"),
        subject="receipt summary total_duration_ms",
    )
    overall_status = summary.get("overall_status")
    if overall_status not in contract.summary_statuses:
        raise ValueError(
            f"receipt summary has unsupported overall_status {overall_status!r}"
        )
    if total_gates != 1 or sum(counts.values()) != 1:
        raise ValueError(
            "single-gate receipt summary must reconcile to exactly one gate result"
        )
    count_key = {
        "pass": "passed",
        "fail": "failed",
        "skip": "skipped",
        "timeout": "timeout",
        "error": "error",
    }[status]
    if counts[count_key] != 1:
        raise ValueError(
            f"receipt summary does not reconcile with gate status {status!r}"
        )
    return status


def _process_group_options(platform_name: str) -> dict[str, Any]:
    if platform_name == "nt":
        creation_flag = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
        return {"creationflags": creation_flag} if creation_flag else {}
    return {"start_new_session": True}


def _terminate_process_group(
    process: subprocess.Popen[bytes], signum: int, *, platform_name: str
) -> None:
    if process.poll() is not None:
        return
    if platform_name != "nt" and hasattr(os, "killpg"):
        hard_kill = getattr(signal, "SIGKILL", signal.SIGTERM)
        for child_signal in (signum, hard_kill):
            try:
                os.killpg(process.pid, child_signal)
            except OSError:
                break
        return

    control_break = getattr(signal, "CTRL_BREAK_EVENT", None)
    if control_break is not None:
        try:
            process.send_signal(control_break)
        except OSError:
            pass
    for action in (process.terminate, process.kill):
        try:
            action()
        except OSError:
            break


class ShardRunner:
    def __init__(
        self,
        *,
        xtask: Path,
        gate_policy: Path,
        receipt_dir: Path,
        summary_path: Path,
        subject_sha: str,
        gates: Sequence[str],
        dependency_rules: dict[str, DependencyRule],
        receipt_contract: ReceiptContract,
    ) -> None:
        self.xtask = xtask
        self.gate_policy = gate_policy
        self.receipt_dir = receipt_dir
        self.summary_path = summary_path
        self.subject_sha = subject_sha
        self.gates = list(gates)
        self.dependency_rules = dependency_rules
        self.receipt_contract = receipt_contract
        self.observations: dict[str, GateObservation] = {}
        self.current_gate: str | None = None
        self.current_process: subprocess.Popen[bytes] | None = None
        self.interrupted_signal: int | None = None

    def _command(self, gate: str) -> list[str]:
        return [
            str(self.xtask),
            "gates",
            "--gate",
            gate,
            "--gate-policy",
            str(self.gate_policy),
            "--receipt",
            "--receipt-path",
            str(self._receipt_path(gate)),
        ]

    def _reproduce(self, gate: str) -> str:
        return " ".join(self._command(gate))

    def _receipt_path(self, gate: str) -> Path:
        return self.receipt_dir / f"{gate}.json"

    def _unstarted_observation(self, gate: str, *, message: str) -> GateObservation:
        return GateObservation(
            gate_name=gate,
            command=self._command(gate),
            reproduce=self._reproduce(gate),
            receipt_path=str(self._receipt_path(gate)),
            result="not_proven",
            receipt_status=None,
            exit_code=None,
            duration_ms=0,
            blocked_by=[],
            message=message,
        )

    def _write_summary(self) -> None:
        rows: list[GateObservation] = []
        for gate in self.gates:
            observed = self.observations.get(gate)
            if observed is not None:
                rows.append(observed)
            elif gate == self.current_gate:
                rows.append(
                    GateObservation(
                        gate_name=gate,
                        command=self._command(gate),
                        reproduce=self._reproduce(gate),
                        receipt_path=str(self._receipt_path(gate)),
                        result="cancelled",
                        receipt_status=None,
                        exit_code=None,
                        duration_ms=0,
                        blocked_by=[],
                        message=(
                            f"shard interrupted by signal {self.interrupted_signal}"
                            if self.interrupted_signal is not None
                            else "gate is still running"
                        ),
                    )
                )
            else:
                waiting_on = [
                    dependency
                    for dependency in self.dependency_rules[gate].requires
                    if dependency not in self.observations
                ]
                message = (
                    "gate did not start before the shard stopped"
                    if not waiting_on
                    else "gate was waiting for dependency result(s): "
                    + ", ".join(waiting_on)
                )
                rows.append(self._unstarted_observation(gate, message=message))

        non_success = [item.gate_name for item in rows if item.result != "success"]
        payload = {
            "schema_version": SCHEMA_VERSION,
            "subject_sha": self.subject_sha,
            "selected_gates": self.gates,
            "gates": [asdict(item) for item in rows],
            "summary": {
                "total": len(rows),
                "success": len(rows) - len(non_success),
                "non_success": len(non_success),
                "non_success_gates": non_success,
                "overall_status": "passed" if not non_success else "failed",
            },
        }
        self.summary_path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self.summary_path.with_name(f".{self.summary_path.name}.tmp")
        temporary.write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        os.replace(temporary, self.summary_path)

    def _prepare_receipt_path(self, path: Path) -> None:
        if path.is_symlink():
            raise ValueError(f"refusing symlink receipt path: {path}")
        if path.exists():
            if not path.is_file():
                raise ValueError(f"receipt path is not a regular file: {path}")
            path.unlink()

    def _read_receipt(self, gate: str, path: Path) -> str:
        payload = _regular_json_object(path, subject="gate receipt")
        return _validate_receipt(
            payload,
            contract=self.receipt_contract,
            gate=gate,
            subject_sha=self.subject_sha,
        )

    @staticmethod
    def _classify(returncode: int, receipt_status: str) -> tuple[str, str | None]:
        if receipt_status in SKIPPED_STATUSES:
            return "not_proven", "selected gate was skipped"
        if receipt_status in PASS_STATUSES:
            if returncode == 0:
                return "success", None
            return "instrument_failure", (
                f"gate receipt says {receipt_status!r} but process exited {returncode}"
            )
        if returncode == 0:
            return "instrument_failure", (
                f"gate process exited zero but receipt says {receipt_status!r}"
            )
        if receipt_status in TIMEOUT_STATUSES:
            return "timeout", None
        if receipt_status in NOT_PROVEN_STATUSES:
            return "not_proven", None
        return "failure", None

    def _terminate_current_process(self, signum: int) -> None:
        process = self.current_process
        if process is None:
            return
        # A Python signal handler can interrupt Popen.wait(). Calling wait()
        # again from the handler can deadlock on Popen's internal wait lock.
        _terminate_process_group(process, signum, platform_name=os.name)

    def handle_signal(self, signum: int, _frame: object) -> None:
        self.interrupted_signal = signum
        self._terminate_current_process(signum)
        self._write_summary()
        raise SystemExit(128 + signum)

    def _run_gate(self, gate: str) -> GateObservation:
        receipt_path = self._receipt_path(gate)
        command = self._command(gate)
        started = time.monotonic()
        print(f"::group::gate {gate}", flush=True)
        try:
            self._prepare_receipt_path(receipt_path)
            process = subprocess.Popen(command, **_process_group_options(os.name))
            self.current_process = process
            returncode = process.wait()
            self.current_process = None
            try:
                receipt_status = self._read_receipt(gate, receipt_path)
                result, message = self._classify(returncode, receipt_status)
            except ValueError as error:
                receipt_status = None
                result = "instrument_failure"
                message = str(error)
        except (OSError, ValueError) as error:
            self.current_process = None
            returncode = None
            receipt_status = None
            result = "instrument_failure"
            message = str(error)
        finally:
            duration_ms = max(0, int((time.monotonic() - started) * 1000))
            print("::endgroup::", flush=True)
        return GateObservation(
            gate_name=gate,
            command=command,
            reproduce=self._reproduce(gate),
            receipt_path=str(receipt_path),
            result=result,
            receipt_status=receipt_status,
            exit_code=returncode,
            duration_ms=duration_ms,
            blocked_by=[],
            message=message,
        )

    def _blocked_observation(
        self, gate: str, blocked_by: list[str]
    ) -> GateObservation:
        return GateObservation(
            gate_name=gate,
            command=self._command(gate),
            reproduce=self._reproduce(gate),
            receipt_path=str(self._receipt_path(gate)),
            result="blocked_not_proven",
            receipt_status=None,
            exit_code=None,
            duration_ms=0,
            blocked_by=blocked_by,
            message="dependency non-success: " + ", ".join(blocked_by),
        )

    def run(self) -> int:
        self.receipt_dir.mkdir(parents=True, exist_ok=True)
        pending = set(self.gates)
        while pending:
            progressed = False
            for gate in self.gates:
                if gate not in pending:
                    continue
                rule = self.dependency_rules[gate]
                if any(dependency in pending for dependency in rule.requires):
                    continue
                blocked_by = [
                    dependency
                    for dependency in rule.requires
                    if self.observations[dependency].result != "success"
                ]
                if blocked_by:
                    self.observations[gate] = self._blocked_observation(
                        gate, blocked_by
                    )
                    pending.remove(gate)
                    progressed = True
                    self._write_summary()
                    continue

                self.current_gate = gate
                self._write_summary()
                self.observations[gate] = self._run_gate(gate)
                self.current_gate = None
                pending.remove(gate)
                progressed = True
                self._write_summary()

            if not progressed:
                # load_execution_policy rejects cycles. Reaching this path means
                # the execution instrument has lost its own invariant.
                for gate in self.gates:
                    if gate in pending:
                        self.observations[gate] = self._unstarted_observation(
                            gate,
                            message="dependency scheduler made no progress",
                        )
                pending.clear()
                self._write_summary()

        self._write_summary()
        # Fail the shard on any hard gate failure, or when nothing executed
        # successfully (an all-skip shard is not proof). A shard whose executed
        # gates all passed with some policy-selected skips is a pass: skip is
        # not_proven for that gate, not for the shard.
        results = [self.observations[gate].result for gate in self.gates]
        hard_failures = {"failure", "timeout", "instrument_failure"}
        if set(results) & hard_failures:
            return 1
        return 0 if "success" in results else 1


def validate_args(args: argparse.Namespace) -> None:
    if not args.gates:
        raise ValueError("at least one gate is required")
    if len(args.gates) != len(set(args.gates)):
        raise ValueError("gate list contains duplicates")
    for gate in args.gates:
        if not GATE_ID.fullmatch(gate):
            raise ValueError(f"invalid gate ID: {gate!r}")
    if os.environ.get("GITHUB_ACTIONS") == "true" and not re.fullmatch(
        r"[0-9a-f]{40}", args.subject_sha
    ):
        raise ValueError("hosted shard requires an exact 40-character subject SHA")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xtask", type=Path, required=True)
    parser.add_argument("--receipt-dir", type=Path, required=True)
    parser.add_argument("--summary", dest="summary_path", type=Path, required=True)
    parser.add_argument(
        "--execution-policy",
        type=Path,
        default=Path(".ci/gate-shard-execution.json"),
    )
    parser.add_argument(
        "--receipt-schema",
        type=Path,
        default=Path(".ci/receipt.schema.json"),
    )
    parser.add_argument(
        "--gate-policy",
        type=Path,
        default=Path(".ci/gate-policy.yaml"),
    )
    parser.add_argument(
        "--subject-sha", default=os.environ.get("GITHUB_SHA", "")
    )
    parser.add_argument("gates", nargs="+")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        validate_args(args)
        load_gate_commands(args.gate_policy, args.gates, root=Path.cwd())
        dependency_rules = load_execution_policy(args.execution_policy, args.gates)
        receipt_contract = load_receipt_contract(args.receipt_schema)
    except ValueError as error:
        parser.error(str(error))
    runner = ShardRunner(
        xtask=args.xtask,
        gate_policy=args.gate_policy,
        receipt_dir=args.receipt_dir,
        summary_path=args.summary_path,
        subject_sha=args.subject_sha,
        gates=args.gates,
        dependency_rules=dependency_rules,
        receipt_contract=receipt_contract,
    )
    signal.signal(signal.SIGTERM, runner.handle_signal)
    signal.signal(signal.SIGINT, runner.handle_signal)
    return runner.run()


if __name__ == "__main__":
    sys.exit(main())
