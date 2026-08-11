#!/usr/bin/env python3
"""Validate the pinned Debug Adapter Protocol authority and project boundary.

The upstream schema remains the authority for standard DAP wire definitions.
This repository keeps a small manifest that pins one upstream commit and Git
blob, classifies project-specific extensions, and records the independently
computed SHA-256 once observed. The validator deliberately does not generate or
replace the Rust domain model; it proves that the authority, project docs, and
standard/extension boundary agree.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Mapping, Sequence

MANIFEST_SCHEMA = "dap_protocol_authority.v1"
RECEIPT_SCHEMA = "dap_protocol_authority_receipt.v1"
MAX_SCHEMA_BYTES = 1_048_576
REQUIRED_DEFINITIONS = ("ProtocolMessage", "Request", "Response", "Event")
REQUIRED_FIELDS = {
    "ProtocolMessage": {"seq", "type"},
    "Request": {"type", "command"},
    "Response": {"type", "request_seq", "success", "command"},
    "Event": {"type", "event"},
}
DOC_PATHS = (
    Path("docs/reference/DAP_PROTOCOL_SCHEMA.md"),
    Path("book/src/dap/protocol-schema.md"),
)
FORBIDDEN_DOC_PHRASES = (
    "JSON-RPC 2.0",
    "Schema Definitions Complete",
    "specification:complete",
)
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


class AuthorityError(RuntimeError):
    """A fail-closed authority validation error."""


def _read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise AuthorityError(f"missing JSON input: {path}") from exc
    except json.JSONDecodeError as exc:
        raise AuthorityError(f"malformed JSON in {path}: {exc}") from exc
    except OSError as exc:
        raise AuthorityError(f"cannot read {path}: {exc}") from exc


def _write_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _object(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise AuthorityError(f"{context} must be a JSON object")
    return value


def _string(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value:
        raise AuthorityError(f"{context} must be a non-empty string")
    return value


def _array(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise AuthorityError(f"{context} must be a JSON array")
    return value


def _validate_pin_url(url: str, repository: str, commit: str, path: str) -> None:
    parsed = urllib.parse.urlparse(url)
    expected_path = f"/{repository}/{commit}/{path}"
    if parsed.scheme != "https":
        raise AuthorityError("upstream raw URL must use HTTPS")
    if parsed.netloc != "raw.githubusercontent.com":
        raise AuthorityError("upstream raw URL must use raw.githubusercontent.com")
    if parsed.path != expected_path:
        raise AuthorityError(
            f"upstream raw URL is not bound to the declared repository/commit/path: {parsed.path}"
        )
    if parsed.params or parsed.query or parsed.fragment:
        raise AuthorityError("upstream raw URL must not contain parameters, query, or fragment")


def validate_manifest(raw: Any, *, require_sha256: bool) -> Mapping[str, Any]:
    manifest = _object(raw, "authority manifest")
    if manifest.get("schema_version") != MANIFEST_SCHEMA:
        raise AuthorityError(
            f"authority manifest schema must be {MANIFEST_SCHEMA!r}, "
            f"got {manifest.get('schema_version')!r}"
        )

    upstream = _object(manifest.get("upstream"), "manifest.upstream")
    repository = _string(upstream.get("repository"), "manifest.upstream.repository")
    commit = _string(upstream.get("commit"), "manifest.upstream.commit")
    path = _string(upstream.get("path"), "manifest.upstream.path")
    blob_sha1 = _string(upstream.get("git_blob_sha1"), "manifest.upstream.git_blob_sha1")
    raw_url = _string(upstream.get("raw_url"), "manifest.upstream.raw_url")

    if repository != "microsoft/debug-adapter-protocol":
        raise AuthorityError(f"unexpected upstream repository: {repository}")
    if path != "debugAdapterProtocol.json":
        raise AuthorityError(f"unexpected upstream schema path: {path}")
    if HEX40.fullmatch(commit) is None:
        raise AuthorityError("upstream commit must be a lowercase 40-character Git SHA")
    if HEX40.fullmatch(blob_sha1) is None:
        raise AuthorityError("upstream Git blob SHA must be lowercase 40-character hexadecimal")
    _validate_pin_url(raw_url, repository, commit, path)

    expected_sha256 = upstream.get("sha256")
    if expected_sha256 is None:
        if require_sha256:
            raise AuthorityError("upstream SHA-256 is not pinned")
    elif not isinstance(expected_sha256, str) or HEX64.fullmatch(expected_sha256) is None:
        raise AuthorityError("upstream SHA-256 must be null or lowercase 64-character hexadecimal")

    base = _object(manifest.get("base_protocol"), "manifest.base_protocol")
    if base.get("name") != "Debug Adapter Protocol":
        raise AuthorityError("base protocol name must be 'Debug Adapter Protocol'")
    if base.get("transport") != "Content-Length framed JSON":
        raise AuthorityError("base protocol transport must be 'Content-Length framed JSON'")
    if base.get("json_rpc") is not False:
        raise AuthorityError("DAP must not be classified as JSON-RPC")
    declared_defs = _array(base.get("required_definitions"), "base_protocol.required_definitions")
    if declared_defs != list(REQUIRED_DEFINITIONS):
        raise AuthorityError(
            f"base protocol definitions must be ordered as {list(REQUIRED_DEFINITIONS)!r}"
        )

    extensions = _array(manifest.get("project_extensions"), "manifest.project_extensions")
    seen_extensions: set[tuple[str, str]] = set()
    inline_values_found = False
    for index, raw_extension in enumerate(extensions):
        extension = _object(raw_extension, f"manifest.project_extensions[{index}]")
        name = _string(extension.get("wire_name"), f"project_extensions[{index}].wire_name")
        kind = _string(extension.get("kind"), f"project_extensions[{index}].kind")
        key = (kind, name)
        if key in seen_extensions:
            raise AuthorityError(f"duplicate project extension identity: {kind}:{name}")
        seen_extensions.add(key)
        if extension.get("classification") != "extension":
            raise AuthorityError(f"project extension {kind}:{name} is not classified as extension")
        _string(extension.get("version"), f"project_extensions[{index}].version")
        _string(extension.get("owner"), f"project_extensions[{index}].owner")
        if name == "inlineValues" and kind == "request":
            inline_values_found = True
    if not inline_values_found:
        raise AuthorityError("custom inlineValues request is missing from the extension inventory")

    configurations = _array(
        manifest.get("project_configuration"), "manifest.project_configuration"
    )
    if not configurations:
        raise AuthorityError("adapter configuration inventory must not be empty")
    for index, raw_configuration in enumerate(configurations):
        configuration = _object(raw_configuration, f"manifest.project_configuration[{index}]")
        _string(configuration.get("surface"), f"project_configuration[{index}].surface")
        if configuration.get("classification") != "adapter-configuration":
            raise AuthorityError(
                f"project_configuration[{index}] must be classified as adapter-configuration"
            )
        _string(configuration.get("owner"), f"project_configuration[{index}].owner")

    return manifest


def git_blob_sha1(data: bytes) -> str:
    header = f"blob {len(data)}\0".encode("ascii")
    return hashlib.sha1(header + data).hexdigest()  # noqa: S324 - Git object identity


def _required_fields(definition: Mapping[str, Any], name: str) -> set[str]:
    required = definition.get("required")
    if isinstance(required, list):
        return {item for item in required if isinstance(item, str)}

    all_of = definition.get("allOf")
    if not isinstance(all_of, list):
        raise AuthorityError(f"upstream definition {name} has neither required nor allOf")
    fields: set[str] = set()
    for item in all_of:
        if isinstance(item, dict) and isinstance(item.get("required"), list):
            fields.update(value for value in item["required"] if isinstance(value, str))
    return fields


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

    upstream = _object(manifest.get("upstream"), "manifest.upstream")
    observed_blob = git_blob_sha1(data)
    expected_blob = _string(upstream.get("git_blob_sha1"), "manifest.upstream.git_blob_sha1")
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
    schema = _object(schema, "upstream schema")

    if schema.get("$schema") != "http://json-schema.org/draft-04/schema#":
        raise AuthorityError(f"unexpected upstream JSON Schema dialect: {schema.get('$schema')!r}")
    if schema.get("title") != "Debug Adapter Protocol":
        raise AuthorityError(f"unexpected upstream schema title: {schema.get('title')!r}")

    definitions = _object(schema.get("definitions"), "upstream schema definitions")
    for name in REQUIRED_DEFINITIONS:
        definition = _object(definitions.get(name), f"upstream definition {name}")
        fields = _required_fields(definition, name)
        expected_fields = REQUIRED_FIELDS[name]
        if not expected_fields.issubset(fields):
            raise AuthorityError(
                f"upstream definition {name} lost required fields: "
                f"expected at least {sorted(expected_fields)}, observed {sorted(fields)}"
            )

    continued = _object(definitions.get("ContinuedEvent"), "upstream ContinuedEvent")
    continued_text = json.dumps(continued, sort_keys=True)
    if "not expected to send this event" not in continued_text:
        raise AuthorityError("upstream ContinuedEvent no longer contains request-implied resume guidance")

    return {
        "git_blob_sha1": observed_blob,
        "sha256": observed_sha256,
        "byte_length": len(data),
        "definition_count": len(definitions),
        "title": schema.get("title"),
        "schema_dialect": schema.get("$schema"),
    }


def fetch_schema(manifest: Mapping[str, Any]) -> bytes:
    upstream = _object(manifest.get("upstream"), "manifest.upstream")
    raw_url = _string(upstream.get("raw_url"), "manifest.upstream.raw_url")
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


def validate_docs(root: Path, manifest: Mapping[str, Any]) -> None:
    upstream = _object(manifest.get("upstream"), "manifest.upstream")
    commit = _string(upstream.get("commit"), "manifest.upstream.commit")
    blob = _string(upstream.get("git_blob_sha1"), "manifest.upstream.git_blob_sha1")

    documents: list[str] = []
    for relative in DOC_PATHS:
        path = root / relative
        try:
            text = path.read_text(encoding="utf-8")
        except FileNotFoundError as exc:
            raise AuthorityError(f"missing protocol authority document: {relative}") from exc
        except OSError as exc:
            raise AuthorityError(f"cannot read protocol authority document {relative}: {exc}") from exc
        documents.append(text)

        for phrase in FORBIDDEN_DOC_PHRASES:
            if phrase in text:
                raise AuthorityError(f"{relative} retains forbidden stale claim {phrase!r}")
        for required in (
            "Content-Length framed JSON",
            "not JSON-RPC",
            "standard DAP",
            "project extension",
            "inlineValues",
            commit,
            blob,
            "#6737",
        ):
            if required not in text:
                raise AuthorityError(f"{relative} is missing authority marker {required!r}")

    if documents[0] != documents[1]:
        raise AuthorityError(
            "canonical DAP authority doc and committed book copy differ; run the documentation sync"
        )


def build_receipt(
    manifest: Mapping[str, Any],
    observed: Mapping[str, Any],
) -> Mapping[str, Any]:
    upstream = _object(manifest.get("upstream"), "manifest.upstream")
    return {
        "schema_version": RECEIPT_SCHEMA,
        "created_unix_seconds": int(time.time()),
        "upstream": {
            "repository": upstream.get("repository"),
            "commit": upstream.get("commit"),
            "path": upstream.get("path"),
            "raw_url": upstream.get("raw_url"),
        },
        "observed": dict(observed),
        "classification": {
            "base_protocol": "Debug Adapter Protocol",
            "transport": "Content-Length framed JSON",
            "json_rpc": False,
            "project_extensions": [
                extension.get("wire_name")
                for extension in _array(
                    manifest.get("project_extensions"), "manifest.project_extensions"
                )
                if isinstance(extension, dict)
            ],
        },
    }


def _load_schema_file(path: Path) -> bytes:
    try:
        data = path.read_bytes()
    except FileNotFoundError as exc:
        raise AuthorityError(f"missing schema file: {path}") from exc
    except OSError as exc:
        raise AuthorityError(f"cannot read schema file {path}: {exc}") from exc
    return data


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    for command in ("observe", "check"):
        sub = subparsers.add_parser(command)
        sub.add_argument("--root", default=".")
        sub.add_argument("--manifest", default=".ci/dap/protocol-authority.json")
        sub.add_argument("--schema")
        sub.add_argument("--receipt", required=True)

    docs = subparsers.add_parser("check-docs")
    docs.add_argument("--root", default=".")
    docs.add_argument("--manifest", default=".ci/dap/protocol-authority.json")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    root = Path(args.root).resolve()
    manifest_path = Path(args.manifest)
    if not manifest_path.is_absolute():
        manifest_path = root / manifest_path

    try:
        require_sha256 = args.command == "check"
        manifest = validate_manifest(_read_json(manifest_path), require_sha256=require_sha256)
        validate_docs(root, manifest)
        if args.command == "check-docs":
            print("DAP protocol authority docs: valid")
            return 0

        data = _load_schema_file(Path(args.schema)) if args.schema else fetch_schema(manifest)
        observed = validate_schema_bytes(data, manifest, require_sha256=require_sha256)
        receipt = build_receipt(manifest, observed)
        _write_json(Path(args.receipt), receipt)
        print(f"DAP upstream commit: {manifest['upstream']['commit']}")
        print(f"DAP upstream Git blob: {observed['git_blob_sha1']}")
        print(f"DAP upstream SHA-256: {observed['sha256']}")
        print(f"DAP authority receipt: {args.receipt}")
    except AuthorityError as exc:
        print(f"DAP protocol authority error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
