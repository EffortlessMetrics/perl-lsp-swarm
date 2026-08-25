"""Shared types and manifest validation for the DAP authority gate."""

from __future__ import annotations

import hashlib
import json
import re
import urllib.parse
from pathlib import Path
from typing import Any, Mapping

MANIFEST_SCHEMA = "dap_protocol_authority.v1"
RECEIPT_SCHEMA = "dap_protocol_authority_receipt.v1"
MAX_SCHEMA_BYTES = 1_048_576
REQUIRED_DEFINITIONS = ("ProtocolMessage", "Request", "Response", "Event")
REQUIRED_FIELDS = {
    "ProtocolMessage": {"seq", "type"},
    "Request": {"seq", "type", "command"},
    "Response": {"seq", "type", "request_seq", "success", "command"},
    "Event": {"seq", "type", "event"},
}
DOC_PATHS = (
    Path("docs/reference/DAP_PROTOCOL_SCHEMA.md"),
    Path("book/src/dap/protocol-schema.md"),
)
DISPATCH_PATH = Path("crates/perl-dap/src/debug_adapter/dispatch.rs")
DEBUG_ADAPTER_ROOT = Path("crates/perl-dap/src/debug_adapter")
FORBIDDEN_DOC_PHRASES = (
    "JSON-RPC 2.0",
    "Schema Definitions Complete",
    "specification:complete",
)
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
SUPPORTED_COMMANDS_RE = re.compile(
    r"const\s+SUPPORTED_COMMANDS\s*:\s*\[&str;\s*(?P<count>\d+)\s*\]\s*=\s*"
    r"\[(?P<body>.*?)\];",
    re.DOTALL,
)
RUST_STRING_RE = re.compile(r'"([A-Za-z][A-Za-z0-9]*)"')
SEND_EVENT_CALL_RE = re.compile(r"\bself\.send_event\s*\(")
SEND_EVENT_LITERAL_RE = re.compile(r'\s*"([A-Za-z][A-Za-z0-9]*)"')
DEFINITION_REF_PREFIX = "#/definitions/"

# Closed vocabularies for the versioned custom-family section (#10138).
FAMILY_CLASSIFICATIONS = {"custom_dap_extension"}
FAMILY_CAPABILITY_MODES = {"unadvertised-until-r04", "advertised-namespaced"}
FAMILY_NEGOTIATION_POLICIES = {
    "unknown_version_policy": {"reject-closed"},
    "unknown_variant_policy": {"reject-closed"},
    "unknown_field_policy": {"reject-closed", "tolerate-ignored"},
}
FAMILY_BOUND_KEYS = (
    "max_request_bytes",
    "max_identity_chars",
    "max_digest_chars",
    "max_reasons",
    "max_reason_chars",
    "max_detail_chars",
    "max_retained_operations",
)


def namespaced_family_name(name: str) -> bool:
    """A custom family name is a non-empty namespace, one `/`, and a
    non-empty local name — the collision-resistant shape required by
    ADR-0046 §6 and mirrored in crates/perl-dap/src/reload/surface.rs."""
    separator = name.find("/")
    if separator <= 0 or separator == len(name) - 1:
        return False
    namespace, local = name.split("/", 1)
    return bool(namespace.strip()) and bool(local.strip())


class AuthorityError(RuntimeError):
    """A fail-closed authority validation error."""


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise AuthorityError(f"missing JSON input: {path}") from exc
    except json.JSONDecodeError as exc:
        raise AuthorityError(f"malformed JSON in {path}: {exc}") from exc
    except OSError as exc:
        raise AuthorityError(f"cannot read {path}: {exc}") from exc


def write_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def read_text(path: Path, context: str) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise AuthorityError(f"missing {context}: {path}") from exc
    except OSError as exc:
        raise AuthorityError(f"cannot read {context} {path}: {exc}") from exc


def object_value(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise AuthorityError(f"{context} must be a JSON object")
    return value


def string_value(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value:
        raise AuthorityError(f"{context} must be a non-empty string")
    return value


def array_value(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise AuthorityError(f"{context} must be a JSON array")
    return value


def manifest_rows(manifest: Mapping[str, Any], key: str) -> list[Mapping[str, Any]]:
    return [
        object_value(item, f"manifest.{key}[{index}]")
        for index, item in enumerate(array_value(manifest.get(key), f"manifest.{key}"))
    ]


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
    manifest = object_value(raw, "authority manifest")
    if manifest.get("schema_version") != MANIFEST_SCHEMA:
        raise AuthorityError(
            f"authority manifest schema must be {MANIFEST_SCHEMA!r}, "
            f"got {manifest.get('schema_version')!r}"
        )

    upstream = object_value(manifest.get("upstream"), "manifest.upstream")
    repository = string_value(upstream.get("repository"), "manifest.upstream.repository")
    commit = string_value(upstream.get("commit"), "manifest.upstream.commit")
    path = string_value(upstream.get("path"), "manifest.upstream.path")
    blob_sha1 = string_value(upstream.get("git_blob_sha1"), "manifest.upstream.git_blob_sha1")
    raw_url = string_value(upstream.get("raw_url"), "manifest.upstream.raw_url")

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

    base = object_value(manifest.get("base_protocol"), "manifest.base_protocol")
    if base.get("name") != "Debug Adapter Protocol":
        raise AuthorityError("base protocol name must be 'Debug Adapter Protocol'")
    if base.get("transport") != "Content-Length framed JSON":
        raise AuthorityError("base protocol transport must be 'Content-Length framed JSON'")
    if base.get("json_rpc") is not False:
        raise AuthorityError("DAP must not be classified as JSON-RPC")
    declared_defs = array_value(
        base.get("required_definitions"), "base_protocol.required_definitions"
    )
    if declared_defs != list(REQUIRED_DEFINITIONS):
        raise AuthorityError(
            f"base protocol definitions must be ordered as {list(REQUIRED_DEFINITIONS)!r}"
        )

    seen_extensions: set[tuple[str, str]] = set()
    inline_values_found = False
    for index, extension in enumerate(manifest_rows(manifest, "project_extensions")):
        name = string_value(extension.get("wire_name"), f"project_extensions[{index}].wire_name")
        kind = string_value(extension.get("kind"), f"project_extensions[{index}].kind")
        if kind not in {"request", "event"}:
            raise AuthorityError(
                f"project_extensions[{index}].kind must be 'request' or 'event', got {kind!r}"
            )
        identity = (kind, name)
        if identity in seen_extensions:
            raise AuthorityError(f"duplicate project extension identity: {kind}:{name}")
        seen_extensions.add(identity)
        if extension.get("classification") != "extension":
            raise AuthorityError(f"project extension {kind}:{name} is not classified as extension")
        string_value(extension.get("version"), f"project_extensions[{index}].version")
        string_value(extension.get("owner"), f"project_extensions[{index}].owner")
        inline_values_found |= identity == ("request", "inlineValues")
    if not inline_values_found:
        raise AuthorityError("custom inlineValues request is missing from the extension inventory")

    configurations = manifest_rows(manifest, "project_configuration")
    if not configurations:
        raise AuthorityError("adapter configuration inventory must not be empty")
    for index, configuration in enumerate(configurations):
        string_value(configuration.get("surface"), f"project_configuration[{index}].surface")
        if configuration.get("classification") != "adapter-configuration":
            raise AuthorityError(
                f"project_configuration[{index}] must be classified as adapter-configuration"
            )
        string_value(configuration.get("owner"), f"project_configuration[{index}].owner")

    families = manifest_rows(manifest, "project_families")
    if not families:
        raise AuthorityError("custom family inventory must not be empty")
    extension_identities: set[tuple[str, str]] = set()
    for index, extension in enumerate(manifest_rows(manifest, "project_extensions")):
        extension_identities.add(
            (
                string_value(extension.get("kind"), f"project_extensions[{index}].kind"),
                string_value(
                    extension.get("wire_name"), f"project_extensions[{index}].wire_name"
                ),
            )
        )
    seen_families: set[str] = set()
    for index, family in enumerate(families):
        where = f"project_families[{index}]"
        name = string_value(family.get("family"), f"{where}.family")
        if not namespaced_family_name(name):
            raise AuthorityError(
                f"{where}.family must be a non-empty 'namespace/name' pair, got {name!r}"
            )
        if name in seen_families:
            raise AuthorityError(f"duplicate project family record: {name}")
        seen_families.add(name)
        request_name = string_value(family.get("request_name"), f"{where}.request_name")
        if not namespaced_family_name(request_name):
            raise AuthorityError(
                f"{where}.request_name must be namespaced like the family, got {request_name!r}"
            )
        for event_entry in array_value(family.get("event_names"), f"{where}.event_names"):
            event = string_value(event_entry, f"{where}.event_names entry")
            if not namespaced_family_name(event):
                raise AuthorityError(
                    f"{where} event {event!r} must be namespaced; a bare standard event "
                    "name can never belong to a custom family"
                )
        if ("request", request_name) in extension_identities:
            raise AuthorityError(
                f"{where} request {request_name!r} duplicates a project extension identity; "
                "a registered family request stays here until it is dispatched, at which "
                "point it must graduate to project_extensions and leave the family record"
            )
        if family.get("classification") not in FAMILY_CLASSIFICATIONS:
            raise AuthorityError(
                f"{where}.classification must be one of {sorted(FAMILY_CLASSIFICATIONS)}, "
                f"got {family.get('classification')!r}"
            )
        version = family.get("version")
        if not isinstance(version, int) or isinstance(version, bool) or version < 1:
            raise AuthorityError(f"{where}.version must be an integer >= 1")
        if family.get("capability_advertisement") not in FAMILY_CAPABILITY_MODES:
            raise AuthorityError(
                f"{where}.capability_advertisement must be one of "
                f"{sorted(FAMILY_CAPABILITY_MODES)}; a standard DAP capability spelling is "
                "never valid"
            )
        if not isinstance(family.get("dispatched"), bool):
            raise AuthorityError(f"{where}.dispatched must be a boolean")
        if not isinstance(family.get("backed"), bool):
            raise AuthorityError(f"{where}.backed must be a boolean")
        string_value(family.get("owner"), f"{where}.owner")
        string_value(family.get("contract"), f"{where}.contract")
        negotiation = object_value(family.get("negotiation"), f"{where}.negotiation")
        string_value(negotiation.get("mode"), f"{where}.negotiation.mode")
        string_value(negotiation.get("selection"), f"{where}.negotiation.selection")
        string_value(negotiation.get("session_binding"), f"{where}.negotiation.session_binding")
        string_value(negotiation.get("restart_effect"), f"{where}.negotiation.restart_effect")
        for policy, vocabulary in FAMILY_NEGOTIATION_POLICIES.items():
            if negotiation.get(policy) not in vocabulary:
                raise AuthorityError(
                    f"{where}.negotiation.{policy} must be one of {sorted(vocabulary)}"
                )
        identity = object_value(family.get("identity_policy"), f"{where}.identity_policy")
        for field in (
            "subject_shape",
            "raw_client_input",
            "correlation",
            "terminal_vocabulary",
            "possibly_applied_boundary",
        ):
            string_value(identity.get(field), f"{where}.identity_policy.{field}")
        bounds = object_value(family.get("bounds"), f"{where}.bounds")
        for key in FAMILY_BOUND_KEYS:
            value = bounds.get(key)
            if not isinstance(value, int) or isinstance(value, bool) or value < 1:
                raise AuthorityError(f"{where}.bounds.{key} must be a positive integer")
        string_value(family.get("redaction"), f"{where}.redaction")
        string_value(family.get("cancellation"), f"{where}.cancellation")
        if family.get("standard_dap_exclusion") is not True:
            raise AuthorityError(f"{where}.standard_dap_exclusion must be true")
        for field in (
            "schema",
            "typescript_projection",
            "rust_contract",
            "vectors",
            "generator_check",
        ):
            string_value(family.get(field), f"{where}.{field}")

    return manifest


def git_blob_sha1(data: bytes) -> str:
    header = f"blob {len(data)}\0".encode("ascii")
    return hashlib.sha1(header + data).hexdigest()  # noqa: S324 - Git object identity
