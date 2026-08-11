"""Pinned upstream DAP capability-shape reconciliation."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from typing import Any, Mapping

from dap_capability_common import MatrixError, object_value, string_value


def _definition_name(reference: Any, context: str) -> str:
    prefix = "#/definitions/"
    if not isinstance(reference, str) or not reference.startswith(prefix):
        raise MatrixError(f"{context} contains unsupported schema reference {reference!r}")
    name = reference.removeprefix(prefix)
    if not name or "/" in name:
        raise MatrixError(f"{context} contains non-local schema reference {reference!r}")
    return name


def schema_types(
    node: Mapping[str, Any],
    definitions: Mapping[str, Any],
    *,
    context: str,
    stack: tuple[str, ...] = (),
) -> set[str]:
    observed: set[str] = set()
    raw_type = node.get("type")
    if isinstance(raw_type, str):
        observed.add(raw_type)
    elif isinstance(raw_type, list) and all(isinstance(item, str) for item in raw_type):
        observed.update(raw_type)
    elif raw_type is not None:
        raise MatrixError(f"{context}.type must be a string or array of strings")

    reference = node.get("$ref")
    if reference is not None:
        name = _definition_name(reference, context)
        if name in stack:
            raise MatrixError(f"cyclic schema reference while resolving {context}: {name}")
        target = object_value(definitions.get(name), f"upstream definition {name}")
        observed.update(
            schema_types(
                target,
                definitions,
                context=f"upstream definition {name}",
                stack=(*stack, name),
            )
        )

    for keyword in ("allOf", "anyOf", "oneOf"):
        raw_children = node.get(keyword)
        if raw_children is None:
            continue
        if not isinstance(raw_children, list):
            raise MatrixError(f"{context}.{keyword} must be an array")
        for index, raw_child in enumerate(raw_children):
            child = object_value(raw_child, f"{context}.{keyword}[{index}]")
            observed.update(
                schema_types(
                    child,
                    definitions,
                    context=f"{context}.{keyword}[{index}]",
                    stack=stack,
                )
            )
    return observed


def validate_upstream_classification(
    matrix_rows: Mapping[str, Mapping[str, Any]], schema_raw: Any
) -> dict[str, list[str]]:
    schema = object_value(schema_raw, "upstream DAP schema")
    definitions = object_value(schema.get("definitions"), "upstream definitions")
    capabilities = object_value(definitions.get("Capabilities"), "upstream Capabilities")
    properties = object_value(
        capabilities.get("properties"), "upstream Capabilities.properties"
    )

    observed: dict[str, list[str]] = {}
    for name, row in matrix_rows.items():
        classification = row.get("classification")
        if classification == "standard":
            property_schema = object_value(
                properties.get(name), f"upstream Capabilities.{name}"
            )
            types = schema_types(
                property_schema,
                definitions,
                context=f"upstream Capabilities.{name}",
            )
            expected_type = string_value(row.get("wire_type"), f"matrix row {name}.wire_type")
            if types != {expected_type}:
                raise MatrixError(
                    f"upstream capability {name} wire shape mismatch: "
                    f"matrix={expected_type!r}, upstream={sorted(types)!r}"
                )
            observed[name] = sorted(types)
        elif classification == "extension":
            if name in properties:
                raise MatrixError(
                    f"extension capability {name} now exists upstream and must be "
                    "reclassified intentionally"
                )
            observed[name] = []
        else:
            raise MatrixError(f"unknown classification for {name}: {classification!r}")
    return observed


def _load_authority_module(root: Path):
    path = root / "scripts/ci/dap_protocol_authority.py"
    spec = importlib.util.spec_from_file_location("dap_protocol_authority", path)
    if spec is None or spec.loader is None:
        raise MatrixError(f"cannot load DAP authority module from {path}")
    module = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(module)
    except (ImportError, OSError, RuntimeError) as exc:
        raise MatrixError(f"cannot load DAP authority module from {path}: {exc}") from exc
    return module


def load_pinned_schema(
    root: Path, manifest_path: Path, schema_path: Path | None
) -> tuple[Any, Mapping[str, Any]]:
    authority = _load_authority_module(root)
    manifest = authority.validate_manifest(
        authority.read_json(manifest_path), require_sha256=True
    )
    try:
        data = schema_path.read_bytes() if schema_path is not None else authority.fetch_schema(manifest)
    except OSError as exc:
        raise MatrixError(f"cannot read pinned schema input: {exc}") from exc
    observed = authority.validate_schema_bytes(data, manifest, require_sha256=True)
    try:
        schema = json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise MatrixError(f"validated upstream schema could not be decoded: {exc}") from exc
    return schema, observed
