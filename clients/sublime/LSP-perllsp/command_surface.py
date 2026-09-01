from __future__ import annotations

import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Collection, Iterable, Mapping, Sequence

MAX_OUTPUT_CHARS = 64 * 1024

# Individual bulk fields are budgeted so no single payload can consume the
# global display budget and erase control/result fields rendered after it.
# Control fields (success/error/reason/nextAction) additionally render ahead
# of alphabetical order so failure semantics never depend on key sorting.
MAX_FIELD_CHARS = 4 * 1024

# Result payloads come from an external server and may contain arbitrarily
# nested JSON-like values. Refuse to descend indefinitely so hostile input
# produces a bounded diagnostic instead of a recursion error.
MAX_RENDER_DEPTH = 64

# Top-level result keys that carry control/result semantics rather than bulk
# material. They always render first, in this declared order.
CONTROL_RESULT_KEYS = ("success", "status", "error", "reason", "nextAction")


class CommandSurfaceError(ValueError):
    pass


@dataclass(frozen=True)
class CommandSpec:
    action: str
    caption: str
    command_id: str
    argument_kind: str
    result_kind: str
    # True when the server implementation reads or executes the file from disk.
    # Those commands would silently act on the last saved bytes rather than the
    # text the user is looking at, so the palette refuses them on a dirty buffer
    # instead of reporting a result for source that is not on screen.
    reads_saved_file: bool


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
        True,
    ),
    # `perl.runTestFile` is not routed through the path-resolving execute-command
    # provider: the live handler looks the argument up directly in the open-document
    # map, which is keyed by document URI. A filesystem path can never match, so
    # this command takes the `file://` URI instead.
    "run_current_test": CommandSpec(
        "run_current_test",
        "Perl: Run Current Test File",
        "perl.runTestFile",
        "document_uri",
        "output",
        True,
    ),
    "run_workspace_tests": CommandSpec(
        "run_workspace_tests",
        "Perl: Run Workspace Tests",
        "perl.runTests",
        "workspace",
        "output",
        True,
    ),
    "run_critic_compatibility": CommandSpec(
        "run_critic_compatibility",
        "Perl: Run Critic Command (Compatibility Surface)",
        "perl.runCritic",
        "file",
        "output",
        True,
    ),
    "go_to_test": CommandSpec(
        "go_to_test",
        "Perl: Go to Test",
        "perl.goToTest",
        "file",
        "navigation",
        True,
    ),
    "go_to_implementation": CommandSpec(
        "go_to_implementation",
        "Perl: Go to Implementation",
        "perl.goToImplementation",
        "file",
        "navigation",
        True,
    ),
    "workspace_trust_report": CommandSpec(
        "workspace_trust_report",
        "Perl: Workspace Trust Report",
        "perl.workspaceTrustReport",
        "none",
        "report",
        False,
    ),
    "preview_safe_delete": CommandSpec(
        "preview_safe_delete",
        "Perl: Preview Safe Delete",
        "perl.previewSafeDelete",
        "position",
        "preview",
        # The safe-delete preview resolves against the live workspace index,
        # which tracks unsaved edits through didChange.
        False,
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
    is_dirty: bool = False,
) -> CommandInvocation:
    spec = COMMAND_SPECS.get(action)
    if spec is None:
        raise CommandSurfaceError(f"Unknown Perl command-palette action: {action}")
    if spec.reads_saved_file and is_dirty:
        raise CommandSurfaceError(
            f"Save the active Perl buffer before running {spec.caption}; "
            "the server reads the file from disk, not the unsaved buffer."
        )
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
        elif spec.argument_kind == "document_uri":
            arguments = [Path(normalized_file).as_uri()]
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


def _bounded_field(
    text: str, *, max_chars: int = MAX_FIELD_CHARS, preserve_tail: bool = False
) -> str:
    """Bound one rendered scalar field so bulk material cannot consume the
    global budget and erase control fields rendered after it."""
    if len(text) <= max_chars:
        return text
    # Reserve the notice itself.  Error text commonly ends with the useful
    # compiler diagnosis, so retain both ends when it is a control field.
    notice = "\n… {} character(s) of this field omitted by LSP-perllsp."
    omitted = max(1, len(text) - max_chars)
    while True:
        rendered_notice = notice.format(omitted)
        keep = max_chars - len(rendered_notice) - (3 if preserve_tail else 0)
        updated = len(text) - keep
        if updated == omitted:
            if preserve_tail:
                head = keep // 2
                tail = keep - head
                return text[:head] + "\n…\n" + text[-tail:] + rendered_notice
            return text[:keep] + rendered_notice
        omitted = updated


def _append_value(
    lines: list[str],
    label: str,
    value: Any,
    indent: int = 0,
    *,
    bound_fields: bool = False,
    depth: int = 0,
) -> None:
    prefix = "  " * indent
    if depth >= MAX_RENDER_DEPTH:
        lines.append(f"{prefix}{label}: … nested result omitted by LSP-perllsp.")
        return
    if isinstance(value, Mapping):
        lines.append(f"{prefix}{label}:")
        if not value:
            lines.append(f"{prefix}  (empty)")
            return
        for key in _ordered_keys(value):
            _append_value(
                lines,
                _humanize(str(key)),
                value[key],
                indent + 1,
                bound_fields=bound_fields,
                depth=depth + 1,
            )
        return
    if isinstance(value, (list, tuple)):
        lines.append(f"{prefix}{label}: {len(value)} item(s)")
        # A result list is an envelope too.  Render control-bearing items
        # before bulk items so the global bound cannot hide a later failure or
        # next action merely because an earlier item is large.
        ordered_items = _ordered_items(value)
        for index, item in ordered_items[:50]:
            # Bound each item as a unit.  Bounding only scalar leaves still
            # permits one deeply nested mapping/list item to consume the
            # complete envelope before later items (and their labels) render.
            item_lines: list[str] = []
            _append_value(
                item_lines,
                f"{index}",
                item,
                indent + 1,
                bound_fields=bound_fields,
                depth=depth + 1,
            )
            rendered_item = "\n".join(item_lines)
            if bound_fields:
                rendered_item = _bounded_field(rendered_item)
            lines.extend(rendered_item.splitlines())
        if len(value) > 50:
            lines.append(f"{prefix}  … {len(value) - 50} additional item(s) omitted")
        return
    if isinstance(value, str) and "\n" in value:
        rendered_lines = value.splitlines()
        field = "\n".join([f"{prefix}{label}:"] + [f"{prefix}  {line}" for line in rendered_lines])
        if bound_fields:
            field = _bounded_field(field, preserve_tail=label.lower() in {"error", "reason"})
        lines.extend(field.splitlines())
        return
    rendered_scalar = _scalar(value)
    field = f"{prefix}{label}: {rendered_scalar}"
    if bound_fields:
        field = _bounded_field(field, preserve_tail=label.lower() in {"error", "reason"})
    lines.append(field)


def _ordered_keys(value: Mapping) -> list[Any]:
    """Control keys first in declared order, then every remaining key in
    deterministic sorted order. Render order must never let a bulk payload
    determine whether control semantics survive the output bound."""
    keys = list(value)
    control_present = [k for k in CONTROL_RESULT_KEYS if k in keys]
    control_set = {str(k) for k in control_present}
    # A nested mapping can itself carry a control result.  Bring those
    # mappings forward too, otherwise a collection of large sibling payloads
    # can hide the nested diagnosis behind the final envelope bound.
    def contains_control(candidate: Any) -> bool:
        if isinstance(candidate, (list, tuple)):
            return any(contains_control(item) for item in candidate)
        if not isinstance(candidate, Mapping):
            return False
        return any(
            str(nested_key) in CONTROL_RESULT_KEYS or contains_control(nested_value)
            for nested_key, nested_value in candidate.items()
        )

    # A control can be several mapping levels below the envelope.  Promote
    # the whole branch before bulk siblings, then let its own invocation of
    # _ordered_keys put the control row first within that branch.
    nested_controls = [
        k
        for k in keys
        if str(k) not in control_set and contains_control(value[k])
    ]
    nested_set = {str(k) for k in nested_controls}
    rest = sorted(
        (k for k in keys if str(k) not in control_set and str(k) not in nested_set), key=str
    )
    return control_present + sorted(nested_controls, key=str) + rest


def _ordered_items(value: Sequence[Any]) -> list[tuple[int, Any]]:
    """Place items containing control/result fields before bulk items.

    Keep the source index in the label so reordering is presentation-only and
    callers can still identify the original result item.
    """

    def contains_control(candidate: Any) -> bool:
        if isinstance(candidate, Mapping):
            return any(
                str(key) in CONTROL_RESULT_KEYS or contains_control(item)
                for key, item in candidate.items()
            )
        if isinstance(candidate, (list, tuple)):
            return any(contains_control(item) for item in candidate)
        return False

    indexed = list(enumerate(value, start=1))
    return sorted(indexed, key=lambda pair: (not contains_control(pair[1]), pair[0]))


def _bounded(text: str) -> str:
    if len(text) <= MAX_OUTPUT_CHARS:
        return text
    omitted = len(text) - MAX_OUTPUT_CHARS
    notice = f"\n\n… {omitted} character(s) omitted by LSP-perllsp.\n"
    return text[: MAX_OUTPUT_CHARS - len(notice)] + notice


def _bounded_with_tail(text: str) -> str:
    if len(text) <= MAX_OUTPUT_CHARS:
        return text
    omitted = len(text) - MAX_OUTPUT_CHARS
    notice = f"\n\n… {omitted} character(s) omitted by LSP-perllsp.\n"
    keep = MAX_OUTPUT_CHARS - len(notice) - 3
    head = keep // 2
    tail = keep - head
    return text[:head] + "\n…\n" + text[-tail:] + notice


def format_result(caption: str, result: Any) -> str:
    lines = [caption, "=" * len(caption), ""]
    if result is None:
        lines.append("Completed. The server returned no additional details.")
    elif isinstance(result, str):
        # A top-level string is still one result field. Bound it independently
        # of the envelope so a scalar cannot consume the display budget.
        lines.append(_bounded_field(result))
    elif isinstance(result, Mapping):
        # Control/result semantics render ahead of bulk material so the
        # output bound below can never erase failure state or next actions;
        # bounded detail rows cannot guarantee it.
        # Apply the per-field bound during the normal render, not only when
        # the complete envelope later exceeds MAX_OUTPUT_CHARS.  A single
        # scalar can fit inside the envelope while still crowding out the
        # field-level safety contract.
        for key in _ordered_keys(result):
            _append_value(lines, _humanize(str(key)), result[key], bound_fields=True)
    elif isinstance(result, (list, tuple)):
        _append_value(lines, "Results", result, bound_fields=True)
    else:
        lines.append(_scalar(result))
    # Every result shape has already been rendered with bounded fields/items.
    # Keep the complete envelope here so list/tuple details are not discarded
    # by a mapping-only overflow fallback; the final bound supplies the single
    # envelope notice consistently for scalars, mappings, and sequences.
    return _bounded("\n".join(lines).rstrip() + "\n")


def format_error(caption: str, command_id: str, error: Any) -> str:
    message = getattr(error, "message", None) or str(error)
    return _bounded_with_tail(
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
