"""Pinned upstream DAP schema validation and wire inventory extraction."""

from __future__ import annotations

import hashlib
import json
import urllib.error
import urllib.request
from typing import Any, Mapping

from dap_authority_common import (
    DEFINITION_REF_PREFIX,
    MAX_SCHEMA_BYTES,
    REQUIRED_DEFINITIONS,
    REQUIRED_FIELDS,
    AuthorityError,
    array_value,
    git_blob_sha1,
    manifest_rows,
    object_value,
    string_value,
)


def _definition_name_from_ref(reference: Any, context: str) -> str:
    if not isinstance(reference, str) or not reference.startswith(DEFINITION_REF_PREFIX):
        raise AuthorityError(f"{context} contains an unsupported schema reference: {reference!r}")
    name = reference.removeprefix(DEFINITION_REF_PREFIX)
    if not name or "/" in name:
        raise AuthorityError(f"{context} contains a non-local definition reference: {reference!r}")
    return name


def _required_fields_from_node(
    node: Mapping[str, Any],
    definitions: Mapping[str, Any],
    *,
    context: str,
    stack: tuple[str, ...],
) -> set[str]:
    fields: set[str] = set()
    required = node.get("required")
    if required is not None:
        if not isinstance(required, list) or not all(isinstance(item, str) for item in required):
            raise AuthorityError(f"{context}.required must be an array of strings")
        fields.update(required)

    reference = node.get("$ref")
    if reference is not None:
        target = _definition_name_from_ref(reference, context)
        fields.update(_required_fields(definitions, target, stack=stack))

    all_of = node.get("allOf")
    if all_of is not None:
        if not isinstance(all_of, list):
            raise AuthorityError(f"{context}.allOf must be an array")
        for index, item in enumerate(all_of):
            child = object_value(item, f"{context}.allOf[{index}]")
            fields.update(
                _required_fields_from_node(
                    child,
                    definitions,
                    context=f"{context}.allOf[{index}]",
                    stack=stack,
                )
            )
    return fields


def _required_fields(
    definitions: Mapping[str, Any], name: str, *, stack: tuple[str, ...] = ()
) -> set[str]:
    if name in stack:
        chain = " -> ".join((*stack, name))
        raise AuthorityError(f"cyclic upstream definition reference: {chain}")
    definition = object_value(definitions.get(name), f"upstream definition {name}")
    return _required_fields_from_node(
        definition,
        definitions,
        context=f"upstream definition {name}",
        stack=(*stack, name),
    )


def _direct_definition_refs(definition: Mapping[str, Any], context: str) -> set[str]:
    all_of = definition.get("allOf")
    if not isinstance(all_of, list):
        return set()
    refs: set[str] = set()
    for index, item in enumerate(all_of):
        child = object_value(item, f"{context}.allOf[{index}]")
        if "$ref" in child:
            refs.add(_definition_name_from_ref(child.get("$ref"), f"{context}.allOf[{index}]"))
    return refs


def _composition_nodes(definition: Mapping[str, Any]) -> list[Mapping[str, Any]]:
    nodes = [definition]
    all_of = definition.get("allOf")
    if isinstance(all_of, list):
        for item in all_of:
            if isinstance(item, dict) and "$ref" not in item:
                nodes.extend(_composition_nodes(item))
    return nodes


def _wire_values(definition: Mapping[str, Any], property_name: str) -> set[str]:
    values: set[str] = set()
    for node in _composition_nodes(definition):
        properties = node.get("properties")
        if not isinstance(properties, dict):
            continue
        property_schema = properties.get(property_name)
        if not isinstance(property_schema, dict):
            continue
        enum = property_schema.get("enum")
        if isinstance(enum, list):
            values.update(item for item in enum if isinstance(item, str))
    return values


def _upstream_wire_inventory(definitions: Mapping[str, Any]) -> tuple[set[str], set[str]]:
    requests: set[str] = set()
    events: set[str] = set()
    for name, raw_definition in definitions.items():
        if not isinstance(name, str) or not isinstance(raw_definition, dict):
            continue
        if name != "Request" and name.endswith("Request"):
            values = _wire_values(raw_definition, "command")
            if len(values) != 1:
                raise AuthorityError(
                    f"upstream request definition {name} must expose one command enum, "
                    f"observed {sorted(values)}"
                )
            requests.update(values)
        if name != "Event" and name.endswith("Event"):
            values = _wire_values(raw_definition, "event")
            if len(values) != 1:
                raise AuthorityError(
                    f"upstream event definition {name} must expose one event enum, "
                    f"observed {sorted(values)}"
                )
            events.update(values)
    return requests, events


def validate_schema_bytes(
    data: bytes,
    manifest: Mapping[str, Any],
    *,
    require_sha256: bool,
) -> Mapping[str, Any]:
    if not data:
        raise AuthorityError("upstream schema body is empty")
    if len(data) > MAX_SCHEMA_BYTES:
        raise AuthorityError(
            f"upstream schema exceeds the {MAX_SCHEMA_BYTES}-byte bounded download limit"
        )

    upstream = object_value(manifest.get("upstream"), "manifest.upstream")
    observed_blob = git_blob_sha1(data)
    expected_blob = string_value(upstream.get("git_blob_sha1"), "manifest.upstream.git_blob_sha1")
    if observed_blob != expected_blob:
        raise AuthorityError(
            f"upstream Git blob mismatch: expected {expected_blob}, observed {observed_blob}"
        )

    observed_sha256 = hashlib.sha256(data).hexdigest()
    expected_sha256 = upstream.get("sha256")
    if expected_sha256 is None:
        if require_sha256:
            raise AuthorityError(
                f"upstream SHA-256 is not pinned; observed candidate is {observed_sha256}"
            )
    elif observed_sha256 != expected_sha256:
        raise AuthorityError(
            f"upstream SHA-256 mismatch: expected {expected_sha256}, observed {observed_sha256}"
        )

    try:
        schema = json.loads(data.decode("utf-8"))
    except UnicodeDecodeError as exc:
        raise AuthorityError("upstream schema is not UTF-8") from exc
    except json.JSONDecodeError as exc:
        raise AuthorityError(f"upstream schema is malformed JSON: {exc}") from exc
    schema = object_value(schema, "upstream schema")

    if schema.get("$schema") != "http://json-schema.org/draft-04/schema#":
        raise AuthorityError(f"unexpected upstream JSON Schema dialect: {schema.get('$schema')!r}")
    if schema.get("title") != "Debug Adapter Protocol":
        raise AuthorityError(f"unexpected upstream schema title: {schema.get('title')!r}")

    definitions = object_value(schema.get("definitions"), "upstream schema definitions")
    for name in REQUIRED_DEFINITIONS:
        fields = _required_fields(definitions, name)
        expected_fields = REQUIRED_FIELDS[name]
        if not expected_fields.issubset(fields):
            raise AuthorityError(
                f"upstream definition {name} lost required fields: "
                f"expected at least {sorted(expected_fields)}, observed {sorted(fields)}"
            )

    for name in ("Request", "Response", "Event"):
        definition = object_value(definitions.get(name), f"upstream definition {name}")
        if "ProtocolMessage" not in _direct_definition_refs(
            definition, f"upstream definition {name}"
        ):
            raise AuthorityError(
                f"upstream definition {name} no longer directly inherits ProtocolMessage"
            )

    requests, events = _upstream_wire_inventory(definitions)
    for index, extension in enumerate(manifest_rows(manifest, "project_extensions")):
        name = string_value(extension.get("wire_name"), f"project_extensions[{index}].wire_name")
        kind = string_value(extension.get("kind"), f"project_extensions[{index}].kind")
        standard = requests if kind == "request" else events
        if name in standard:
            raise AuthorityError(
                f"project extension {kind}:{name} now exists in the pinned upstream schema"
            )

    continued = object_value(definitions.get("ContinuedEvent"), "upstream ContinuedEvent")
    continued_text = json.dumps(continued, sort_keys=True)
    if "not expected to send this event" not in continued_text:
        raise AuthorityError(
            "upstream ContinuedEvent no longer contains request-implied resume guidance"
        )

    return {
        "git_blob_sha1": observed_blob,
        "sha256": observed_sha256,
        "byte_length": len(data),
        "definition_count": len(definitions),
        "title": schema.get("title"),
        "schema_dialect": schema.get("$schema"),
        "standard_requests": sorted(requests),
        "standard_events": sorted(events),
    }


def fetch_schema(manifest: Mapping[str, Any]) -> bytes:
    upstream = object_value(manifest.get("upstream"), "manifest.upstream")
    raw_url = string_value(upstream.get("raw_url"), "manifest.upstream.raw_url")
    request = urllib.request.Request(
        raw_url,
        headers={"User-Agent": "perl-lsp-swarm-dap-authority/1"},
        method="GET",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            content_length = response.headers.get("Content-Length")
            if content_length is not None:
                try:
                    declared_length = int(content_length)
                except ValueError as exc:
                    raise AuthorityError(
                        f"upstream Content-Length is not numeric: {content_length!r}"
                    ) from exc
                if declared_length < 0 or declared_length > MAX_SCHEMA_BYTES:
                    raise AuthorityError(
                        f"upstream Content-Length is outside the bounded range: {declared_length}"
                    )
            data = response.read(MAX_SCHEMA_BYTES + 1)
    except AuthorityError:
        raise
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise AuthorityError(f"cannot fetch pinned upstream schema: {exc}") from exc
    if len(data) > MAX_SCHEMA_BYTES:
        raise AuthorityError("upstream schema exceeded the bounded download limit")
    return data
