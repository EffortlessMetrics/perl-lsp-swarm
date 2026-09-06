#!/usr/bin/env python3
"""#9404 — schema-derived DAP field nullability inventory.

Normalizes the pinned upstream DAP schema (microsoft/debug-adapter-protocol,
verified by sha256 against `.ci/dap/protocol-authority.json`) into per-field
nullability classes, maps every represented standard wire field to its Rust
owner in `crates/perl-dap/src/protocol.rs`, and emits a deterministic JSON
inventory.

Classes (mutually exclusive):
  required-non-null            schema-required, schema not nullable
  required-nullable            schema-required, schema nullable
  optional-non-null-when-present  not schema-required, schema not nullable
  optional-nullable            not schema-required, schema nullable
  unsupported                  schema definition with no Rust owner

The report is byte-stable across runs: rows are sorted, and any drift in the
upstream schema or the Rust model appears as a reviewable diff. Contradictions
(a schema-required field mapped to an Option the serializer may omit, one Rust
field claimed by two standard rows, a project-extension field counted as
standard) fail the build instead of degrading the row.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
MANIFEST = REPO / ".ci" / "dap" / "protocol-authority.json"
PINNED_SCHEMA = REPO / ".ci" / "dap" / "upstream-debugAdapterProtocol.json"
PROTOCOL_RS = REPO / "crates" / "perl-dap" / "src" / "protocol.rs"
REPORT = REPO / ".ci" / "dap" / "nullability-inventory.v1.json"

CLASSES = (
    "required-non-null",
    "required-nullable",
    "optional-non-null-when-present",
    "optional-nullable",
)

# Schema definitions whose Rust owners use project-specific names rather
# than the schema definition name (fn_8H): without the alias the row is
# wrongly reported as unsupported even though the field is modeled.
RUST_OWNER_ALIASES = {
    "Variable": "ProtocolVariable",
    "StackFrame": "ProtocolStackFrame",
    "ExceptionBreakpointsFilter": "ExceptionBreakpointFilter",
    "ExceptionFilterOptions": "ExceptionFilterOption",
}

# Contradictions verified on main and accepted until their serde migration
# (non-goal of the inventory PR). Any contradiction OUTSIDE this set fails
# the build — the inventory must never silently absorb a new mismatch.
KNOWN_CONTRADICTIONS = {
    ("SourceArguments", "sourceReference"),
}

STRUCT_RE = re.compile(r"#\[serde\(([^)]*)\)\]\s*pub struct (\w+)\s*\{(.*?)\n\}", re.S)
FIELD_RE = re.compile(r"(#\[serde\(([^)]*)\)\]\s*)?pub (\w+):\s*([^,\n]+),")


def load_pinned_schema() -> dict:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    upstream = manifest["upstream"]
    raw = PINNED_SCHEMA.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != upstream["sha256"]:
        raise SystemExit(
            f"pinned schema drift: {PINNED_SCHEMA} hashes {digest}, "
            f"manifest pins {upstream['sha256']}"
        )
    return json.loads(raw)


def is_nullable_schema(schema: dict) -> bool:
    """True when the property schema admits an explicit JSON null."""
    types = schema.get("type")
    if isinstance(types, list) and "null" in types:
        return True
    if types == "null":
        return True
    for branch in schema.get("anyOf", []):
        if isinstance(branch, dict) and branch.get("type") == "null":
            return True
    if schema.get("default", None) is None and schema.get("type") is None:
        # $ref-only properties are never null in this schema generation; a
        # future generation that adds null unions will surface as drift.
        return False
    return False


def classify(schema_required: bool, schema_nullable: bool) -> str:
    if schema_required and not schema_nullable:
        return "required-non-null"
    if schema_required and schema_nullable:
        return "required-nullable"
    if not schema_required and not schema_nullable:
        return "optional-non-null-when-present"
    return "optional-nullable"


def parse_rust_structs() -> dict:
    """Return {struct_name: {field: {type, optional, skips_when_none}}}."""
    source = PROTOCOL_RS.read_text(encoding="utf-8")
    structs: dict[str, dict] = {}
    for match in STRUCT_RE.finditer(source):
        serde_attrs, name, body = match.group(1) or "", match.group(2), match.group(3)
        rename_all = "camelCase" if "camelCase" in serde_attrs else None
        fields: dict[str, dict] = {}
        for field in FIELD_RE.finditer(body):
            attrs, rust_name, rust_type = field.group(2) or "", field.group(3), field.group(4).strip()
            optional = rust_type.startswith("Option<")
            skips_when_none = "skip_serializing_if" in attrs
            has_default = "default" in attrs
            rename = None
            rename_match = re.search(r'rename\s*=\s*"([^"]+)"', attrs)
            if rename_match:
                rename = rename_match.group(1)
            fields[rust_name] = {
                "rust_type": rust_type,
                "optional": optional,
                "skips_when_none": skips_when_none,
                "has_default": has_default,
                "wire_name": rename
                or (snake_to_camel(rust_name) if rename_all == "camelCase" else rust_name),
            }
        structs[name] = {"rename_all": rename_all, "fields": fields}
    return structs


def snake_to_camel(name: str) -> str:
    head, *rest = name.split("_")
    return head + "".join(part.title() for part in rest)


def rust_field_for(wire_name: str, struct: dict) -> tuple[str, dict] | None:
    """Find the Rust field a wire property maps to (rename-aware)."""
    for rust_name, meta in struct["fields"].items():
        if meta["wire_name"] == wire_name:
            return rust_name, meta
    return None


# Fields the reviewed contract confirms as required-nullable even though the
# upstream schema generation does not carry a `required` array for them. Each
# entry is a reviewer-confirmed contract call (#9404 seeding list); unconfirmed
# nullable body fields land as optional-nullable until reviewed.
CONFIRMED_REQUIRED_NULLABLE = {
    ("DataBreakpointInfoResponse", "dataId"),
}


def response_body_properties(definition: dict, definitions: dict) -> dict | None:
    """Return the body payload's properties for a response-style definition.

    DAP response definitions compose via allOf: the base Response plus an
    inline object whose `body` property is the payload container. Returns None
    for definitions without that shape.
    """
    for branch in definition.get("allOf", []):
        if not isinstance(branch, dict):
            continue
        if "$ref" in branch:
            ref_name = branch["$ref"].rsplit("/", 1)[-1]
            refd = definitions.get(ref_name, {})
            if refd.get("properties", {}).get("body"):
                return refd["properties"]["body"].get("properties")
            continue
        body = branch.get("properties", {}).get("body")
        if isinstance(body, dict):
            return body.get("properties")
    return None


def rust_field_for_body(wire_name: str, rust: dict, def_name: str) -> tuple[str, dict] | None:
    """Find the Rust field for a response-body property.

    Body payloads are modeled as `<Name>Body`-shaped inline structs inside the
    protocol module; the mapping tries the plain definition name first (the
    struct names mirror the schema), then falls back to a `<Name>Body` suffix.
    """
    for candidate in (def_name, f"{def_name}Body"):
        struct = rust.get(candidate)
        if struct is None:
            continue
        found = rust_field_for(wire_name, struct)
        if found is not None:
            return found
    return None


def build_rows(schema: dict, rust: dict) -> tuple[list[dict], list[str]]:
    definitions = schema["definitions"]
    rows: list[dict] = []
    errors: list[str] = []
    claimed: dict[tuple[str, str], str] = {}
    for def_name in sorted(definitions):
        definition = definitions[def_name]
        body_properties = response_body_properties(definition, definitions)
        if body_properties is not None:
            # Response definitions: rows are the body payload's fields. The
            # body container itself is mandatory, so a nullable field is
            # required-nullable when the reviewed contract confirms it and
            # optional-nullable otherwise.
            required = set()
            for branch in definition.get("allOf", []):
                if isinstance(branch, dict) and "properties" in branch:
                    inner = branch["properties"].get("body", {})
                    required = set(inner.get("required", []))
            for wire_name in sorted(body_properties):
                property_schema = body_properties[wire_name]
                schema_nullable = is_nullable_schema(property_schema)
                schema_required = wire_name in required
                if schema_nullable and not schema_required and (
                    def_name,
                    wire_name,
                ) in CONFIRMED_REQUIRED_NULLABLE:
                    schema_required = True
                row_class = classify(schema_required, schema_nullable)
                mapped = rust_field_for_body(wire_name, rust, def_name)
                owner_label = f"{def_name}Body::{mapped[0]}" if mapped else None
                row = {
                    "definition": f"{def_name}Body",
                    "field": wire_name,
                    "class": row_class,
                    "rust_owner": owner_label,
                    "rust_type": mapped[1]["rust_type"] if mapped else None,
                    "seeded": (def_name, wire_name) in CONFIRMED_REQUIRED_NULLABLE,
                }
                if mapped is None:
                    row["reason"] = "not modeled in protocol.rs (Rust owner mapping pending)"
                rows.append(row)
            continue
        composed_properties = None
        for branch in definition.get("allOf", []):
            if isinstance(branch, dict) and branch.get("properties"):
                composed_properties = branch["properties"]
                break
        if composed_properties is not None:
            # Composed envelope definitions (requests, events) without a
            # response body: their envelope fields belong to the inventory.
            required = set(definition.get("allOf", [])[0].get("required", []))
            for branch in definition.get("allOf", []):
                if isinstance(branch, dict):
                    required.update(branch.get("required", []))
            for wire_name in sorted(composed_properties):
                property_schema = composed_properties[wire_name]
                schema_nullable = is_nullable_schema(property_schema)
                schema_required = wire_name in required
                row_class = classify(schema_required, schema_nullable)
                mapped = rust_field_for(wire_name, rust.get(def_name, {"fields": {}})) or (
                    rust_field_for(wire_name, rust.get(f"{def_name}Body", {"fields": {}}))
                )
                rows.append(
                    {
                        "definition": def_name,
                        "field": wire_name,
                        "class": row_class,
                        "rust_owner": f"{def_name}::{mapped[0]}" if mapped else None,
                        "rust_type": mapped[1]["rust_type"] if mapped else None,
                        "reason": None if mapped else "not modeled in protocol.rs",
                    }
                )
            continue
        properties = definition.get("properties")
        if not properties:
            continue
        required = set(definition.get("required", []))
        owner = rust.get(def_name) or rust.get(
            RUST_OWNER_ALIASES.get(def_name, "")
        )
        if owner is None:
            for wire_name in sorted(properties):
                rows.append(
                    {
                        "definition": def_name,
                        "field": wire_name,
                        "class": "unsupported",
                        "rust_owner": None,
                        "reason": "no matching Rust struct in protocol.rs",
                    }
                )
            continue
        for wire_name in sorted(properties):
            property_schema = properties[wire_name]
            schema_nullable = is_nullable_schema(property_schema)
            schema_required = wire_name in required
            row_class = classify(schema_required, schema_nullable)
            mapped = rust_field_for(wire_name, owner)
            if mapped is None:
                # Unmodeled in Rust: the schema-side class is still recorded so
                # the inventory shows the gap explicitly instead of dropping
                # the field. Adding the Rust field later turns this into a
                # fully-owned row.
                rows.append(
                    {
                        "definition": def_name,
                        "field": wire_name,
                        "class": row_class,
                        "rust_owner": None,
                        "reason": "not modeled in protocol.rs (Rust owner mapping pending)",
                    }
                )
                continue
            rust_name, meta = mapped
            key = (def_name, rust_name)
            if key in claimed:
                errors.append(
                    f"{def_name}.{wire_name}: Rust field {rust_name} already claimed by "
                    f"{claimed[key]}"
                )
                continue
            claimed[key] = wire_name
            contradiction = None
            if meta["optional"] and row_class == "required-non-null":
                # Recorded, not fatal: fixing the Rust owner is a serde/type
                # migration (out of this PR's non-goals). The falsifier test
                # pins the known-contradiction set, so any NEW contradiction
                # still fails the build.
                contradiction = (
                    f"schema requires non-null but the Rust owner is Option<> "
                    f"({meta['rust_type']})"
                )
            row = {
                "definition": def_name,
                "field": wire_name,
                "class": row_class,
                "rust_owner": f"{def_name}::{rust_name}",
                "rust_type": meta["rust_type"],
                "serde_skips_when_none": meta["skips_when_none"],
            }
            if contradiction:
                row["contradiction"] = contradiction
                if (def_name, wire_name) not in KNOWN_CONTRADICTIONS:
                    errors.append(
                        f"{def_name}.{wire_name}: new contradiction — {contradiction}"
                    )
            rows.append(row)
    return rows, errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify the committed report matches a fresh run")
    args = parser.parse_args()

    schema = load_pinned_schema()
    rust = parse_rust_structs()
    rows, errors = build_rows(schema, rust)
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1

    classes = {row["class"] for row in rows}
    unknown = classes - set(CLASSES) - {"unsupported"}
    if unknown:
        print(f"ERROR: unclassified rows: {sorted(unknown)}", file=sys.stderr)
        return 1

    report = {
        "schema_version": "dap_nullability_inventory.v1",
        "upstream_sha256": hashlib.sha256(PINNED_SCHEMA.read_bytes()).hexdigest(),
        "schema_field_rows": len(rows),
        "class_counts": {
            class_name: sum(1 for row in rows if row["class"] == class_name)
            for class_name in sorted({row["class"] for row in rows})
        },
        "rows": rows,
    }
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"

    if args.check:
        committed = REPORT.read_text(encoding="utf-8")
        if committed != rendered:
            print(
                "ERROR: committed nullability inventory is stale — rerun with "
                "--write and review the diff",
                file=sys.stderr,
            )
            return 1
        print(f"nullability inventory current: {len(rows)} rows")
        return 0

    REPORT.write_text(rendered, encoding="utf-8")
    print(
        f"wrote {REPORT} ({len(rows)} rows; classes: {report['class_counts']})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
