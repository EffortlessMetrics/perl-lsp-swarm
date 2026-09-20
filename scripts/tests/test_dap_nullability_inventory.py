#!/usr/bin/env python3
"""Falsifier tests for the #9404 nullability inventory descent.

Each test targets one plausible wrong implementation named by the issue: the
new proof must fail under it.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location(
    "dap_nullability_inventory",
    REPO / "scripts" / "ci" / "dap_nullability_inventory.py",
)
mod = importlib.util.module_from_spec(_spec)
sys.modules["dap_nullability_inventory"] = mod
_spec.loader.exec_module(mod)


def rows_for(definition: dict, rust: dict) -> list[dict]:
    rows, errors = mod.build_rows(
        {"definitions": {"UnderTest": definition}}, rust
    )
    assert not errors, errors
    return [row for row in rows if row["definition"] == "UnderTest"]


RUST = {
    "UnderTest": {
        "rename_all": "camelCase",
        "fields": {
            "always_present": {
                "rust_type": "String",
                "optional": False,
                "skips_when_none": False,
                "has_default": False,
                "wire_name": "alwaysPresent",
            },
            "maybe_null": {
                "rust_type": "Option<String>",
                "optional": True,
                "skips_when_none": False,
                "has_default": False,
                "wire_name": "maybeNull",
            },
            "maybe_absent": {
                "rust_type": "Option<String>",
                "optional": True,
                "skips_when_none": True,
                "has_default": True,
                "wire_name": "maybeAbsent",
            },
        },
    }
}


def test_upstream_required_change_changes_the_class():
    """Falsifier: upstream required list changes but the row stays unchanged."""
    non_null = rows_for(
        {"properties": {"always_present": {"type": "string"}}, "required": ["always_present"]},
        RUST,
    )
    optional = rows_for(
        {"properties": {"always_present": {"type": "string"}}},
        RUST,
    )
    assert non_null[0]["class"] == "required-non-null"
    assert optional[0]["class"] == "optional-non-null-when-present"
    assert non_null[0]["class"] != optional[0]["class"]


def test_nullable_union_flip_changes_the_class():
    """Falsifier: nullable union becomes non-null but Rust stays Option."""
    nullable = rows_for(
        {
            "properties": {"maybeNull": {"type": ["string", "null"]}},
            "required": ["maybeNull"],
        },
        RUST,
    )
    assert nullable[0]["class"] == "required-nullable"
    # The flip: a non-null required union over an Option<> Rust owner is a
    # contradiction — the build must fail on it (fail-closed on new
    # contradictions), never silently absorb it.
    flipped_definition = {
        "properties": {"maybeNull": {"type": "string"}},
        "required": ["maybeNull"],
    }
    _, errors = mod.build_rows({"definitions": {"UnderTest": flipped_definition}}, RUST)
    assert any("new contradiction" in error for error in errors), errors


def test_required_nullable_and_optional_nullable_do_not_collapse():
    """Falsifier: required-nullable and optional-nullable are distinct rows."""
    required = rows_for(
        {
            "properties": {"maybe_null": {"type": ["string", "null"]}},
            "required": ["maybe_null"],
        },
        RUST,
    )
    optional = rows_for(
        {"properties": {"maybe_null": {"type": ["string", "null"]}}},
        RUST,
    )
    assert required[0]["class"] == "required-nullable"
    assert optional[0]["class"] == "optional-nullable"
    assert required[0]["class"] != optional[0]["class"]


def test_extension_field_is_never_standard():
    """Falsifier: a custom extension field is counted as standard."""
    # build_rows only walks the upstream pinned schema; project-extension
    # definitions live in the manifest's project_extensions and never enter
    # the definitions walk. The structural edge: an extension-shaped
    # definition set (nothing the pinned schema owns) yields NO rows, while
    # the same properties under a schema-owned definition do.
    extension_only = {"x_vendor_extension": {"properties": {"always_present": {"type": "string"}}}}
    rows, errors = mod.build_rows({"definitions": extension_only}, RUST)
    assert not errors, errors
    # The extension definition may surface as an unowned observation, but it
    # can never mint an OWNED standard row: an implementation that started
    # attributing extension fields to Rust owners fails here.
    assert all(
        row["rust_owner"] is None and row["class"] == "unsupported" for row in rows
    ), rows
    rows = rows_for({"properties": {}}, RUST)
    assert rows == []


def test_two_properties_never_share_one_rust_owner():
    """Falsifier, restated for rename-aware mapping: one Rust field maps to
    conflicting standard rows. Under exact wire-name matching this is
    structurally prevented (each field owns exactly one wire name), so the
    invariant asserted is distinctness of the mapped owners; the generator
    keeps the claimed-guard as defense in depth."""
    definition = {
        "properties": {
            "alwaysPresent": {"type": "string"},
            "maybeAbsent": {"type": "string"},
        },
        "required": ["alwaysPresent"],
    }
    rows, errors = mod.build_rows({"definitions": {"UnderTest": definition}}, RUST)
    assert not errors, errors
    owners = [row.get("rust_owner") for row in rows if row.get("rust_owner")]
    assert len(owners) == len(set(owners)), "a Rust field was claimed twice"
    # A property with no matching wire name surfaces as an explicit
    # not-modeled row rather than silently claiming a neighbor.
    definition_with_stranger = {
        "properties": {
            "alwaysPresent": {"type": "string"},
            "unrelatedWire": {"type": "string"},
        },
        "required": ["alwaysPresent"],
    }
    stranger_rows, _ = mod.build_rows(
        {"definitions": {"UnderTest": definition_with_stranger}}, RUST
    )
    stranger = [
        row for row in stranger_rows if row["field"] == "unrelatedWire"
    ]
    assert stranger and stranger[0].get("rust_owner") is None


def test_data_id_seed_class_is_required_nullable():
    """The seeded confirmation list drives the dataId contract class — and
    is load-bearing: with the seed the nullable body field classifies
    required-nullable, without it the same shape degrades to
    optional-nullable."""
    definition = {
        "allOf": [
            {"$ref": "#/definitions/Response"},
            {
                "properties": {
                    "body": {
                        "properties": {"dataId": {"type": ["string", "null"]}},
                    }
                }
            },
        ]
    }
    definitions = {
        "DataBreakpointInfoResponse": definition,
        "Response": {"properties": {}},
    }
    rows, errors = mod.build_rows({"definitions": definitions}, RUST)
    assert not errors, errors
    data_id = next(row for row in rows if row["field"] == "dataId")
    assert data_id["class"] == "required-nullable", data_id
    assert data_id["seeded"] is True, data_id

    saved = set(mod.CONFIRMED_REQUIRED_NULLABLE)
    try:
        mod.CONFIRMED_REQUIRED_NULLABLE = set()
        rows, errors = mod.build_rows({"definitions": definitions}, RUST)
        assert not errors, errors
        data_id = next(row for row in rows if row["field"] == "dataId")
        assert data_id["class"] == "optional-nullable", data_id
        assert data_id["seeded"] is False, data_id
    finally:
        mod.CONFIRMED_REQUIRED_NULLABLE = saved


def test_duplicate_wire_names_fail_the_build_and_lose_owner():
    """Falsifier: two Rust fields claiming one wire name are a build error,
    and the ambiguous row records no owner (declaration order must never
    decide silently)."""
    ambiguous = {
        "UnderTest": {
            "rename_all": "camelCase",
            "fields": {
                "always_present": {
                    "rust_type": "String",
                    "optional": False,
                    "skips_when_none": False,
                    "has_default": False,
                    "wire_name": "alwaysPresent",
                },
                "shadow": {
                    "rust_type": "Option<String>",
                    "optional": True,
                    "skips_when_none": True,
                    "has_default": True,
                    "wire_name": "alwaysPresent",
                },
            },
        }
    }
    rows, errors = mod.build_rows(
        {"definitions": {"UnderTest": {"properties": {"alwaysPresent": {"type": "string"}}}}},
        ambiguous,
    )
    assert any(
        "ambiguous wire name 'alwaysPresent'" in error for error in errors
    ), errors
    row = next(row for row in rows if row["field"] == "alwaysPresent")
    assert row["rust_owner"] is None, row
    assert "ambiguous" in row.get("reason", ""), row


if __name__ == "__main__":
    # The dap-protocol-authority workflow invokes this file directly with
    # python3; a bare exit would silently skip every falsifier.
    failures = []
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
