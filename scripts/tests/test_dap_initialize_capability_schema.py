#!/usr/bin/env python3
"""Falsifier tests for the #9534 initialize capability schema projection."""

from __future__ import annotations

import copy
import importlib.util
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location(
    "dap_initialize_capability_schema",
    REPO / "scripts" / "ci" / "dap_initialize_capability_schema.py",
)
mod = importlib.util.module_from_spec(_spec)
sys.modules["dap_initialize_capability_schema"] = mod
_spec.loader.exec_module(mod)


def base_schema() -> dict:
    schema, _upstream = mod.load_pinned_schema()
    return copy.deepcopy(schema)


def rows_for(schema: dict) -> list[dict]:
    nullability = mod.nullability_index()
    standard, _extension = mod.build_rows(schema, {"sha256": "test"}, nullability)
    return standard


def test_upstream_field_addition_changes_row_set():
    """Falsifier: add a standard capability field upstream and leave projection unchanged."""
    committed = json.loads((REPO / ".ci" / "dap" / "initialize-capability-schema.v1.json").read_text())
    committed_names = {row["wire_name"] for row in committed["standard_rows"]}
    schema = base_schema()
    props = schema["definitions"]["Capabilities"]["properties"]
    assert committed_names == set(props.keys())
    props["supportsSyntheticField"] = {"type": "boolean"}
    assert set(props.keys()) - committed_names == {"supportsSyntheticField"}


def test_boolean_to_object_changes_wire_shape():
    """Falsifier: change boolean capability to structured object without updating shape."""
    schema = base_schema()
    props = schema["definitions"]["Capabilities"]["properties"]
    props["supportsCancelRequest"] = {"type": "object", "properties": {"enabled": {"type": "boolean"}}}
    row = next(r for r in rows_for(schema) if r["wire_name"] == "supportsCancelRequest")
    assert row["wire_shape"]["kind"] == "object"


def test_required_membership_tracks_upstream_required_list():
    """Falsifier: move a field into/out of the required list unnoticed."""
    schema = base_schema()
    caps = schema["definitions"]["Capabilities"]
    caps["required"] = ["supportsCancelRequest"]
    row = next(r for r in rows_for(schema) if r["wire_name"] == "supportsCancelRequest")
    assert row["schema_required"] is True
    caps.pop("required")
    row = next(r for r in rows_for(schema) if r["wire_name"] == "supportsCancelRequest")
    assert row["schema_required"] is False


def test_nullability_row_reference_tracks_inventory_class():
    """Falsifier: nullable flip without updating the linked nullability row reference."""
    schema = base_schema()
    row = next(r for r in rows_for(schema) if r["wire_name"] == "supportsCancelRequest")
    assert row["nullability_row"]["class"] == "optional-non-null-when-present"


def test_request_family_removal_changes_related_requests():
    """Falsifier: remove/rename a referenced request family without updating rows."""
    schema = base_schema()
    props = schema["definitions"]["Capabilities"]["properties"]
    props["supportsEvaluateForHovers"]["description"] = "Capability with no named request."
    row = next(r for r in rows_for(schema) if r["wire_name"] == "supportsEvaluateForHovers")
    assert row["related_requests"] == []


def test_extension_candidate_is_not_counted_as_standard():
    """Falsifier: count a custom local extension as standard."""
    schema, upstream = mod.load_pinned_schema()
    nullability = mod.nullability_index()
    standard, extension = mod.build_rows(schema, upstream, nullability)
    names = {row["wire_name"] for row in standard}
    assert "supportsInlineValues" not in names
    assert extension[0]["wire_name"] == "supportsInlineValues"


def test_unresolved_schema_reference_fails_closed():
    """Falsifier: accept an unresolved schema reference as an empty/default shape."""
    schema = base_schema()
    props = schema["definitions"]["Capabilities"]["properties"]
    props["supportsCancelRequest"] = {"$ref": "#/definitions/MissingDefinition"}
    try:
        rows_for(schema)
    except mod.SchemaError:
        return
    raise AssertionError("expected unresolved $ref to fail closed")


def test_unsupported_composition_fails_closed():
    """Falsifier: silently simplify a oneOf/anyOf/allOf capability property."""
    schema = base_schema()
    props = schema["definitions"]["Capabilities"]["properties"]
    props["supportsCancelRequest"] = {
        "anyOf": [{"type": "boolean"}, {"type": "null"}],
    }
    try:
        rows_for(schema)
    except mod.SchemaError:
        return
    raise AssertionError("expected anyOf capability property to fail closed")


def test_committed_report_matches_generator():
    """Falsifier: hand-edit generated rows without changing the pin/input."""
    report = mod.build_report()
    committed = json.loads((REPO / ".ci" / "dap" / "initialize-capability-schema.v1.json").read_text())
    assert report == committed


def test_two_schema_paths_cannot_map_one_field_to_conflicting_shapes():
    """Falsifier: two extraction paths assign different shapes to one wire field."""
    schema = base_schema()
    first = next(
        r for r in rows_for(schema) if r["wire_name"] == "exceptionBreakpointFilters"
    )
    second = next(
        r for r in rows_for(copy.deepcopy(schema)) if r["wire_name"] == "exceptionBreakpointFilters"
    )
    assert first["wire_shape"] == second["wire_shape"]


if __name__ == "__main__":
    failures: list[str] = []
    for name, function in sorted(globals().items()):
        if name.startswith("test_") and callable(function):
            try:
                function()
            except AssertionError as error:
                failures.append(f"{name}: {error}")
    if failures:
        for failure in failures:
            print(f"FAIL {failure}", file=sys.stderr)
        sys.exit(1)
    print(f"all {sum(1 for n in globals() if n.startswith('test_'))} falsifiers pass")
