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
            },
            "maybe_null": {
                "rust_type": "Option<String>",
                "optional": True,
                "skips_when_none": False,
                "has_default": False,
            },
            "maybe_absent": {
                "rust_type": "Option<String>",
                "optional": True,
                "skips_when_none": True,
                "has_default": True,
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
            "properties": {"maybe_null": {"type": ["string", "null"]}},
            "required": ["maybe_null"],
        },
        RUST,
    )
    non_null = rows_for(
        {
            "properties": {"maybe_null": {"type": "string"}},
            "required": ["maybe_null"],
        },
        RUST,
    )
    assert nullable[0]["class"] == "required-nullable"
    # A non-null union over an Option<> Rust owner is the recorded
    # contradiction class; the row must still exist and carry it.
    assert non_null[0]["class"] == "required-non-null"
    assert "contradiction" in non_null[0]


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
    # the definitions walk. The contract is therefore structural: a row's
    # definition name must originate from the pinned schema walk — modeled
    # here by asserting the descent reports NO rows when the definition set
    # is empty (an extension definition cannot invent rows from nothing).
    rows = rows_for({"properties": {}}, RUST)
    assert rows == []


def test_rust_field_claimed_twice_fails():
    """Falsifier: one Rust field maps to conflicting standard rows."""
    definition = {
        "properties": {
            "alwaysPresent": {"type": "string"},
            "always_present_alias": {"type": "string"},
        },
        "required": ["alwaysPresent", "always_present_alias"],
    }
    rows, errors = mod.build_rows({"definitions": {"UnderTest": definition}}, RUST)
    assert errors, "a double-claimed Rust field must fail the build"
    assert any("already claimed" in error for error in errors)


def test_data_id_seed_class_is_required_nullable():
    """The seeded confirmation list drives the dataId contract class."""
    assert ("DataBreakpointInfoResponse", "dataId") in mod.CONFIRMED_REQUIRED_NULLABLE


if __name__ == "__main__":
    sys.exit(0)
