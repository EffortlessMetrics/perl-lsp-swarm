#!/usr/bin/env python3
"""#9534 — schema-derived standard initialize Capabilities projection.

Extracts every standard field of the pinned upstream `Capabilities` definition
into deterministic rows with wire shape, optional/required membership, links to
the #9404 nullability inventory (without duplicating nullability semantics),
and upstream request/event family hints derived from schema descriptions.

Repository custom initialize fields are listed separately as
`extension_candidate` rows. Production emission, backend mapping, and runtime
capability values are out of scope.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

REPO = Path(__file__).resolve().parents[2]
MANIFEST = REPO / ".ci" / "dap" / "protocol-authority.json"
PINNED_SCHEMA = REPO / ".ci" / "dap" / "upstream-debugAdapterProtocol.json"
NULLABILITY_REPORT = REPO / ".ci" / "dap" / "nullability-inventory.v1.json"
PROTOCOL_RS = REPO / "crates" / "perl-dap" / "src" / "protocol.rs"
REPORT = REPO / ".ci" / "dap" / "initialize-capability-schema.v1.json"

DEFINITION_REF_PREFIX = "#/definitions/"
UPSTREAM_DEFINITION = "Capabilities"
SCHEMA_VERSION = "dap_initialize_capability_schema.v1"

REQUEST_IN_DESC = re.compile(r"`([A-Za-z][A-Za-z0-9]*)`\s+request\b")
REQUESTS_PAREN = re.compile(r"\brequests\s*\(([^)]*)\)", re.IGNORECASE)
REQUESTS_ON_THE = re.compile(r"\bon the\b(.+\brequests\b)", re.IGNORECASE)
REQUESTS_VIA_THE = re.compile(r"\bvia the\b(.+\brequests\b)", re.IGNORECASE)
BACKTICK_NAME = re.compile(r"`([A-Za-z][A-Za-z0-9]*)`")
EVENT_IN_DESC = re.compile(r"`([A-Za-z][A-Za-z0-9]*)`\s+event")
CAPABILITIES_STRUCT_RE = re.compile(
    r"pub struct Capabilities\s*\{(.*?)\n\}",
    re.S,
)
CAP_FIELD_RE = re.compile(
    r"(?:#\[serde\([^\)]*\)\]\s*)*pub (\w+):\s*[^;\n]+[;,]",
    re.S,
)


class SchemaError(RuntimeError):
    """Fail-closed capability-schema extraction error."""


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise SchemaError(f"cannot read {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise SchemaError(f"malformed JSON in {path}: {exc}") from exc


def load_pinned_schema() -> tuple[dict[str, Any], dict[str, Any]]:
    manifest = load_json(MANIFEST)
    upstream = manifest["upstream"]
    raw = PINNED_SCHEMA.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != upstream["sha256"]:
        raise SchemaError(
            f"pinned schema drift: {PINNED_SCHEMA} hashes {digest}, "
            f"manifest pins {upstream['sha256']}"
        )
    schema = json.loads(raw.decode("utf-8"))
    return schema, upstream


def nullability_index() -> dict[tuple[str, str], dict[str, Any]]:
    report = load_json(NULLABILITY_REPORT)
    rows = report.get("rows")
    if not isinstance(rows, list):
        raise SchemaError("nullability inventory missing rows")
    indexed: dict[tuple[str, str], dict[str, Any]] = {}
    for row in rows:
        if not isinstance(row, dict):
            continue
        definition = row.get("definition")
        field = row.get("field")
        if isinstance(definition, str) and isinstance(field, str):
            indexed[(definition, field)] = row
    return indexed


def definition_name(reference: Any, context: str) -> str:
    if not isinstance(reference, str) or not reference.startswith(DEFINITION_REF_PREFIX):
        raise SchemaError(f"{context} contains unsupported schema reference: {reference!r}")
    name = reference.removeprefix(DEFINITION_REF_PREFIX)
    if not name or "/" in name:
        raise SchemaError(f"{context} contains non-local schema reference: {reference!r}")
    return name


def resolve_node(
    node: dict[str, Any],
    definitions: dict[str, Any],
    *,
    context: str,
    stack: tuple[str, ...] = (),
) -> dict[str, Any]:
    reference = node.get("$ref")
    if reference is None:
        return node
    name = definition_name(reference, context)
    if name in stack:
        chain = " -> ".join((*stack, name))
        raise SchemaError(f"cyclic schema reference while resolving {context}: {chain}")
    target = definitions.get(name)
    if not isinstance(target, dict):
        raise SchemaError(f"missing upstream definition {name} for {context}")
    merged = dict(target)
    merged.update({k: v for k, v in node.items() if k != "$ref"})
    return resolve_node(
        merged,
        definitions,
        context=f"upstream definition {name}",
        stack=(*stack, name),
    )


def unsupported_keywords(node: dict[str, Any], context: str) -> None:
    for keyword in ("oneOf", "anyOf", "allOf"):
        if keyword in node:
            raise SchemaError(
                f"{context} uses unsupported composition {keyword}; "
                "capability schema extraction fails closed"
            )


def wire_shape(
    property_schema: dict[str, Any],
    definitions: dict[str, Any],
    *,
    context: str,
) -> dict[str, Any]:
    node = resolve_node(property_schema, definitions, context=context)
    unsupported_keywords(node, context)

    node_type = node.get("type")
    if node_type == "boolean":
        return {"kind": "boolean"}
    if node_type == "array":
        items = node.get("items")
        if not isinstance(items, dict):
            raise SchemaError(f"{context}.items must be an object")
        if "$ref" in items:
            item_name = definition_name(items["$ref"], f"{context}.items")
            return {
                "kind": "array",
                "items": {"kind": "definition", "definition": item_name},
            }
        item_node = resolve_node(items, definitions, context=f"{context}.items")
        unsupported_keywords(item_node, f"{context}.items")
        item_type = item_node.get("type")
        if item_type == "string":
            return {"kind": "array", "items": {"kind": "string"}}
        if item_type == "object":
            return {"kind": "array", "items": {"kind": "object"}}
        raise SchemaError(f"{context}.items has unsupported shape: {item_node!r}")
    if node_type == "object":
        return {"kind": "object"}
    if "$ref" in property_schema:
        target = definition_name(property_schema["$ref"], context)
        resolved = resolve_node(property_schema, definitions, context=context)
        unsupported_keywords(resolved, context)
        if resolved.get("type") == "boolean":
            return {"kind": "boolean", "definition": target}
        return {"kind": "definition", "definition": target}
    raise SchemaError(f"{context} has unsupported wire shape: {node!r}")


def camel_to_wire(rust_field: str) -> str:
    parts = rust_field.split("_")
    return parts[0] + "".join(part[:1].upper() + part[1:] for part in parts[1:])


def rust_initialize_wire_fields() -> dict[str, str]:
    source = PROTOCOL_RS.read_text(encoding="utf-8")
    match = CAPABILITIES_STRUCT_RE.search(source)
    if match is None:
        raise SchemaError("cannot locate Capabilities struct in protocol.rs")
    body = match.group(1)
    mapping: dict[str, str] = {}
    for field_match in CAP_FIELD_RE.finditer(body):
        rust_name = field_match.group(1)
        mapping[camel_to_wire(rust_name)] = rust_name
    if not mapping:
        raise SchemaError("Capabilities struct contains no fields")
    return mapping


def related_requests(description: str, wire_name: str) -> list[str]:
    text = description or ""
    requests: list[str] = []

    paren = REQUESTS_PAREN.search(text)
    if paren is not None:
        requests.extend(BACKTICK_NAME.findall(paren.group(1)))

    on_the = REQUESTS_ON_THE.search(text)
    if on_the is not None:
        requests.extend(BACKTICK_NAME.findall(on_the.group(1)))

    via_the = REQUESTS_VIA_THE.search(text)
    if via_the is not None:
        requests.extend(BACKTICK_NAME.findall(via_the.group(1)))

    requests.extend(REQUEST_IN_DESC.findall(text))
    if wire_name.startswith("supports") and wire_name.endswith("Request") and not requests:
        base = wire_name[len("supports") : -len("Request")]
        if base:
            requests.append(base[0].lower() + base[1:])
    deduped: list[str] = []
    for name in requests:
        if name not in deduped:
            deduped.append(name)
    return deduped


def related_events(description: str) -> list[str]:
    events = EVENT_IN_DESC.findall(description or "")
    deduped: list[str] = []
    for name in events:
        if name not in deduped:
            deduped.append(name)
    return deduped


def build_rows(
    schema: dict[str, Any],
    upstream: dict[str, Any],
    nullability: dict[tuple[str, str], dict[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    definitions = schema.get("definitions")
    if not isinstance(definitions, dict):
        raise SchemaError("upstream schema missing definitions")
    capabilities = definitions.get(UPSTREAM_DEFINITION)
    if not isinstance(capabilities, dict):
        raise SchemaError("upstream schema missing Capabilities definition")
    properties = capabilities.get("properties")
    if not isinstance(properties, dict) or not properties:
        raise SchemaError("Capabilities definition has no properties")
    required = set(capabilities.get("required") or [])

    standard_rows: list[dict[str, Any]] = []
    for index, wire_name in enumerate(sorted(properties), start=1):
        property_schema = properties[wire_name]
        if not isinstance(property_schema, dict):
            raise SchemaError(f"Capabilities.{wire_name} must be an object")
        null_row = nullability.get((UPSTREAM_DEFINITION, wire_name))
        if null_row is None:
            raise SchemaError(
                f"Capabilities.{wire_name} missing from nullability inventory (#9404)"
            )
        row = {
            "row_id": f"CAP-STD-{index:03d}",
            "classification": "standard",
            "wire_name": wire_name,
            "upstream_definition": UPSTREAM_DEFINITION,
            "upstream_property": wire_name,
            "schema_required": wire_name in required,
            "wire_shape": wire_shape(
                property_schema,
                definitions,
                context=f"Capabilities.{wire_name}",
            ),
            "nullability_row": {
                "definition": UPSTREAM_DEFINITION,
                "field": wire_name,
                "class": null_row["class"],
            },
            "related_requests": related_requests(
                str(property_schema.get("description") or ""),
                wire_name,
            ),
            "related_events": related_events(
                str(property_schema.get("description") or ""),
            ),
        }
        standard_rows.append(row)

    rust_fields = rust_initialize_wire_fields()
    upstream_names = set(properties)
    extension_rows: list[dict[str, Any]] = []
    for index, wire_name in enumerate(sorted(set(rust_fields) - upstream_names), start=1):
        rust_name = rust_fields[wire_name]
        extension_rows.append(
            {
                "row_id": f"CAP-EXT-{index:03d}",
                "classification": "extension_candidate",
                "wire_name": wire_name,
                "rust_owner": f"Capabilities::{rust_name}",
                "reason": (
                    "repository custom initialize field; absent from pinned upstream "
                    "Capabilities schema"
                ),
            }
        )
    return standard_rows, extension_rows


def build_report() -> dict[str, Any]:
    schema, upstream = load_pinned_schema()
    nullability = nullability_index()
    standard_rows, extension_rows = build_rows(schema, upstream, nullability)
    return {
        "schema_version": SCHEMA_VERSION,
        "upstream": {
            "repository": upstream["repository"],
            "commit": upstream["commit"],
            "sha256": upstream["sha256"],
            "definition": UPSTREAM_DEFINITION,
        },
        "standard_row_count": len(standard_rows),
        "extension_candidate_count": len(extension_rows),
        "standard_rows": standard_rows,
        "extension_candidates": extension_rows,
        "claim_boundary": (
            "schema-derived standard initialize capability projection only; "
            "nullability semantics remain owned by #9404/#7362; production "
            "inventory and runtime values remain out of scope"
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify the committed report matches a fresh extraction",
    )
    args = parser.parse_args()

    try:
        report = build_report()
    except SchemaError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.check:
        try:
            committed = REPORT.read_text(encoding="utf-8")
        except OSError as exc:
            print(f"ERROR: missing committed report {REPORT}: {exc}", file=sys.stderr)
            return 1
        if committed != rendered:
            print(
                "ERROR: committed initialize capability schema is stale — rerun "
                "python3 scripts/ci/dap_initialize_capability_schema.py and review the diff",
                file=sys.stderr,
            )
            return 1
        print(
            "initialize capability schema current: "
            f"{report['standard_row_count']} standard rows, "
            f"{report['extension_candidate_count']} extension candidates"
        )
        return 0

    REPORT.write_text(rendered, encoding="utf-8")
    print(
        f"wrote {REPORT} ({report['standard_row_count']} standard rows; "
        f"{report['extension_candidate_count']} extension candidates)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
