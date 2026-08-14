"""Reconcile the pinned DAP authority with production request and event names."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Mapping

from dap_authority_common import (
    DEBUG_ADAPTER_ROOT,
    DISPATCH_PATH,
    RUST_STRING_RE,
    SEND_EVENT_CALL_RE,
    SEND_EVENT_LITERAL_RE,
    SUPPORTED_COMMANDS_RE,
    AuthorityError,
    array_value,
    manifest_rows,
    read_text,
    string_value,
)


def _production_commands(root: Path) -> set[str]:
    text = read_text(root / DISPATCH_PATH, "DAP dispatch source")
    match = SUPPORTED_COMMANDS_RE.search(text)
    if match is None:
        raise AuthorityError(f"cannot locate exact SUPPORTED_COMMANDS inventory in {DISPATCH_PATH}")
    commands = RUST_STRING_RE.findall(match.group("body"))
    declared_count = int(match.group("count"))
    if len(commands) != declared_count:
        raise AuthorityError(
            f"SUPPORTED_COMMANDS declares {declared_count} entries but exposes {len(commands)}"
        )
    if len(set(commands)) != len(commands):
        raise AuthorityError("SUPPORTED_COMMANDS contains duplicate wire names")
    return set(commands)


def _production_events(root: Path) -> set[str]:
    source_root = root / DEBUG_ADAPTER_ROOT
    if not source_root.is_dir():
        raise AuthorityError(f"missing DAP production source root: {DEBUG_ADAPTER_ROOT}")
    events: set[str] = set()
    for path in sorted(source_root.rglob("*.rs")):
        text = read_text(path, "DAP production source")
        for call in SEND_EVENT_CALL_RE.finditer(text):
            literal = SEND_EVENT_LITERAL_RE.match(text, call.end())
            if literal is None:
                relative = path.relative_to(root)
                line = text.count("\n", 0, call.start()) + 1
                raise AuthorityError(
                    f"cannot derive self.send_event wire name at {relative}:{line}; "
                    "use a literal or extend the typed inventory"
                )
            events.add(literal.group(1))
    return events


def validate_production_boundary(
    root: Path,
    manifest: Mapping[str, Any],
    observed: Mapping[str, Any],
) -> Mapping[str, Any]:
    standard_requests = set(
        array_value(observed.get("standard_requests"), "observed.standard_requests")
    )
    standard_events = set(
        array_value(observed.get("standard_events"), "observed.standard_events")
    )
    commands = _production_commands(root)
    events = _production_events(root)

    production_extensions = {
        *(("request", name) for name in commands - standard_requests),
        *(("event", name) for name in events - standard_events),
    }
    declared_extensions: set[tuple[str, str]] = set()
    for index, extension in enumerate(manifest_rows(manifest, "project_extensions")):
        declared_extensions.add(
            (
                string_value(extension.get("kind"), f"project_extensions[{index}].kind"),
                string_value(
                    extension.get("wire_name"), f"project_extensions[{index}].wire_name"
                ),
            )
        )

    missing = sorted(production_extensions - declared_extensions)
    stale = sorted(declared_extensions - production_extensions)
    if missing or stale:
        raise AuthorityError(
            "production/project extension inventory mismatch: "
            f"unclassified production={missing}, stale manifest={stale}"
        )

    return {
        "dispatch_path": DISPATCH_PATH.as_posix(),
        "source_root": DEBUG_ADAPTER_ROOT.as_posix(),
        "commands": sorted(commands),
        "events": sorted(events),
        "project_extensions": [
            {"kind": kind, "wire_name": name} for kind, name in sorted(production_extensions)
        ],
    }
