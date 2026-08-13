from __future__ import annotations

import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Collection, Iterable, Mapping, Sequence

MAX_OUTPUT_CHARS = 64 * 1024


class CommandSurfaceError(ValueError):
    pass


@dataclass(frozen=True)
class CommandSpec:
    action: str
    caption: str
    command_id: str
    argument_kind: str
    result_kind: str


@dataclass(frozen=True)
class CommandInvocation:
    spec: CommandSpec
    arguments: list[Any]
    workspace_path: str | None


COMMAND_SPECS: dict[str, CommandSpec] = {
    "run_current_file": CommandSpec(
        "run_current_file",
        "Perl: Run Current File",
        "perl.runFile",
        "file",
        "output",
    ),
    "run_current_test": CommandSpec(
        "run_current_test",
        "Perl: Run Current Test File",
        "perl.runTestFile",
        "file",
        "output",
    ),
    "run_workspace_tests": CommandSpec(
        "run_workspace_tests",
        "Perl: Run Workspace Tests",
        "perl.runTests",
        "workspace",
        "output",
    ),
    "run_critic_compatibility": CommandSpec(
        "run_critic_compatibility",
        "Perl: Run Critic Command (Compatibility Surface)",
        "perl.runCritic",
        "file",
        "output",
    ),
    "go_to_test": CommandSpec(
        "go_to_test",
        "Perl: Go to Test",
        "perl.goToTest",
        "file",
        "navigation",
    ),
    "go_to_implementation": CommandSpec(
        "go_to_implementation",
        "Perl: Go to Implementation",
        "perl.goToImplementation",
        "file",
        "navigation",
    ),
    "workspace_trust_report": CommandSpec(
        "workspace_trust_report",
        "Perl: Workspace Trust Report",
        "perl.workspaceTrustReport",
        "none",
        "report",
    ),
    "preview_safe_delete": CommandSpec(
        "preview_safe_delete",
        "Perl: Preview Safe Delete",
        "perl.previewSafeDelete",
        "position",
        "preview",
    ),
}

# Applying safe delete remains intentionally absent. A palette command may expose
# preview, but it cannot manufacture the explicit preview/apply state token.
DESTRUCTIVE_COMMAND_IDS = {"perl.safeDeleteSymbol"}


def command_ids() -> set[str]:
    return {spec.command_id for spec in COMMAND_SPECS.values()}


def _normalized_file_path(file_path: str | None) -> str:
    if not file_path:
        raise CommandSurfaceError(
            "Save the active Perl buffer before running a filesystem-backed command."
        )
    return str(Path(file_path).expanduser().resolve())


def _path_is_inside(file_path: str, root_path: str) -> bool:
    try:
        file_normalized = os.path.normcase(os.path.abspath(file_path))
        root_normalized = os.path.normcase(os.path.abspath(root_path))
        return os.path.commonpath([file_normalized, root_normalized]) == root_normalized
    except ValueError:
        return False


def owning_workspace(file_path: str, workspace_folders: Iterable[str]) -> str:
    matches = [
        str(Path(folder).expanduser().resolve())
        for folder in workspace_folders
        if folder and _path_is_inside(file_path, folder)
    ]
    if not matches:
        raise CommandSurfaceError(
            "The active Perl file is not owned by a workspace folder in the active LSP-perllsp session."
        )
    return max(matches, key=len)


def prepare_invocation(
    action: str,
    advertised_commands: Collection[str],
    *,
    file_path: str | None,
    workspace_folders: Sequence[str],
    line: int,
    character: int,
) -> CommandInvocation:
    spec = COMMAND_SPECS.get(action)
    if spec is None:
        raise CommandSurfaceError(f"Unknown Perl command-palette action: {action}")
    if spec.command_id in DESTRUCTIVE_COMMAND_IDS:
        raise CommandSurfaceError("Destructive commands cannot be bound directly to the palette.")
    if spec.command_id not in advertised_commands:
        raise CommandSurfaceError(
            f"The active perllsp server did not advertise {spec.command_id}."
        )

    workspace_path: str | None = None
    if spec.argument_kind == "none":
        arguments: list[Any] = []
    else:
        normalized_file = _normalized_file_path(file_path)
        if spec.argument_kind == "file":
            arguments = [normalized_file]
        elif spec.argument_kind == "workspace":
            workspace_path = owning_workspace(normalized_file, workspace_folders)
            arguments = [workspace_path]
        elif spec.argument_kind == "position":
            arguments = [
                {
                    "textDocument": {"uri": Path(normalized_file).as_uri()},
                    "position": {"line": max(0, line), "character": max(0, character)},
                }
            ]
        else:
            raise CommandSurfaceError(
                f"Unsupported argument contract for {spec.command_id}: {spec.argument_kind}"
            )

    return CommandInvocation(spec=spec, arguments=arguments, workspace_path=workspace_path)


def _humanize(key: str) -> str:
    separated = re.sub(r"(?<!^)(?=[A-Z])", " ", key).replace("_", " ").replace("-", " ")
    return separated[:1].upper() + separated[1:]


def _scalar(value: Any) -> str:
    if value is None:
        return "none"
    if isinstance(value, bool):
        return "yes" if value else "no"
    return str(value)


def _append_value(lines: list[str], label: str, value: Any, indent: int = 0) -> None:
    prefix = "  " * indent
    if isinstance(value, Mapping):
        lines.append(f"{prefix}{label}:")
        if not value:
            lines.append(f"{prefix}  (empty)")
            return
        for key in sorted(value, key=str):
            _append_value(lines, _humanize(str(key)), value[key], indent + 1)
        return
    if isinstance(value, (list, tuple)):
        lines.append(f"{prefix}{label}: {len(value)} item(s)")
        for index, item in enumerate(value[:50], start=1):
            _append_value(lines, f"{index}", item, indent + 1)
        if len(value) > 50:
            lines.append(f"{prefix}  … {len(value) - 50} additional item(s) omitted")
        return
    if isinstance(value, str) and "\n" in value:
        lines.append(f"{prefix}{label}:")
        lines.extend(f"{prefix}  {line}" for line in value.splitlines())
        return
    lines.append(f"{prefix}{label}: {_scalar(value)}")


def _bounded(text: str) -> str:
    if len(text) <= MAX_OUTPUT_CHARS:
        return text
    omitted = len(text) - MAX_OUTPUT_CHARS
    return text[:MAX_OUTPUT_CHARS] + f"\n\n… {omitted} character(s) omitted by LSP-perllsp.\n"


def format_result(caption: str, result: Any) -> str:
    lines = [caption, "=" * len(caption), ""]
    if result is None:
        lines.append("Completed. The server returned no additional details.")
    elif isinstance(result, str):
        lines.append(result)
    elif isinstance(result, Mapping):
        for key in sorted(result, key=str):
            _append_value(lines, _humanize(str(key)), result[key])
    elif isinstance(result, (list, tuple)):
        _append_value(lines, "Results", result)
    else:
        lines.append(_scalar(result))
    return _bounded("\n".join(lines).rstrip() + "\n")


def format_error(caption: str, command_id: str, error: Any) -> str:
    message = getattr(error, "message", None) or str(error)
    return _bounded(
        f"{caption}\n{'=' * len(caption)}\n\n"
        f"Server command: {command_id}\n"
        f"Status: failed\n"
        f"Error: {message}\n"
    )


def navigation_target(result: Any) -> str | None:
    if isinstance(result, str) and result:
        return result
    if not isinstance(result, Mapping):
        return None
    if result.get("found") is False:
        return None
    for key in ("uri", "targetUri", "path", "file", "filePath"):
        value = result.get(key)
        if isinstance(value, str) and value:
            return value
    candidates = result.get("candidates")
    if isinstance(candidates, list) and len(candidates) == 1:
        return navigation_target(candidates[0])
    return None
