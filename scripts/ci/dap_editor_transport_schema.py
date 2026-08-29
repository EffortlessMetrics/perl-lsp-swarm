"""Schema, digest, and closed vocabularies for the DAP editor-transport inventory."""

from __future__ import annotations

import hashlib
import json
from typing import Any, Mapping

INVENTORY_SCHEMA = "dap_editor_transport_inventory.v1"
RECEIPT_SCHEMA = "dap_editor_transport_receipt.v1"
CONFLICT_KEY = "dap.transport.editor_socket"

TRANSPORT_KINDS = {
    "native_editor_stdio",
    "native_editor_tcp",
    "external_peer_editor_stdio",
    "external_peer_editor_tcp",
    "debugger_peer_tcp",
    "test_only_loopback",
    "dap_attach",
    "dap_to_dap_proxy",
}
ROLES = {"editor", "debugger_peer", "attach_backend", "test_only", "dap_proxy"}
DISPOSITIONS = {"retain", "retire", "not_product"}
AUTHORITIES = {"product", "test", "historical", "not_proven"}
EVIDENCE_STAGES = {
    "shipped",
    "package",
    "planned",
    "preview",
    "fixture",
    "not_proven",
    "none",
}
SUPPORT_STATUSES = {"supported", "preview", "planned", "not_proven", "unsupported"}
CLIENT_TRANSPORTS = {"stdio", "editor_tcp", "mixed", "none", "n/a"}

REQUIRED_TRANSPORT_FIELDS = (
    "id",
    "kind",
    "role",
    "disposition",
    "authority",
    "owner_issue",
    "paths",
    "claim_boundary",
)
REQUIRED_CLIENT_FIELDS = (
    "id",
    "evidence_stage",
    "support_status",
    "launch_mechanism",
    "transport",
    "editor_socket_required",
    "evidence_paths",
    "required_markers",
    "forbidden_markers",
    "support_owner",
    "migration",
    "claim_boundary",
    "blocks_retirement",
)
REQUIRED_INVARIANTS = (
    "stdio is the sole production editor-facing transport",
    "no product CLI path binds an ambient editor DAP listener as a supported run mode",
    "external-peer TCP is a debugger-backend transport, not an editor transport",
    "debugger-peer credentials never become editor credentials or vice versa",
    "DAP attach, external-peer connect/listen, and editor transport remain separate propositions",
    "no supported editor needs a DAP-to-DAP proxy or socket relay",
    "test-only TCP fixtures cannot enter package help/docs/capability/support claims",
    "removing editor TCP does not change DAP request schemas, native launch, PID-attach honesty, or peer protocol semantics",
    "a future editor-socket requirement must return through a new evidence-backed architecture decision",
)

TEST_ONLY_PATH_FRAGMENTS = ("/tests/", "/test/", "/host_tests/", "\\tests\\", "\\test\\")


class TransportInventoryError(RuntimeError):
    """A fail-closed editor-transport inventory error."""


def read_json(path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise TransportInventoryError(f"missing inventory file: {path}") from exc
    except OSError as exc:
        raise TransportInventoryError(f"cannot read inventory file {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise TransportInventoryError(f"invalid JSON in {path}: {exc}") from exc


def write_json(path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def canonical_inventory(inventory: Mapping[str, Any]) -> dict[str, Any]:
    """Return a digest-stable copy: lists sorted by id, digest omitted."""
    copy = json.loads(json.dumps(inventory))
    copy.pop("digest", None)
    for key in ("transports", "clients", "bind_sites", "cli_flags", "dap_to_dap_relays"):
        rows = copy.get(key)
        if isinstance(rows, list):
            copy[key] = sorted(rows, key=lambda row: str(row.get("id", row.get("flag", row.get("path", "")))))
    if isinstance(copy.get("invariants"), list):
        copy["invariants"] = sorted(str(item) for item in copy["invariants"])
    if isinstance(copy.get("first_mile_surfaces"), list):
        copy["first_mile_surfaces"] = sorted(str(item) for item in copy["first_mile_surfaces"])
    return copy


def inventory_digest(inventory: Mapping[str, Any]) -> str:
    canonical = canonical_inventory(inventory)
    payload = json.dumps(canonical, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def _require_mapping(value: Any, label: str, errors: list[str]) -> Mapping[str, Any] | None:
    if not isinstance(value, dict):
        errors.append(f"{label} must be an object")
        return None
    return value


def _require_list(value: Any, label: str, errors: list[str]) -> list[Any]:
    if not isinstance(value, list):
        errors.append(f"{label} must be an array")
        return []
    return value


def _check_vocab(value: Any, allowed: set[str], label: str, errors: list[str]) -> None:
    if value not in allowed:
        errors.append(f"{label}={value!r} is not in {sorted(allowed)}")


def _sorted_ids(rows: list[Any], field: str) -> list[str]:
    ids = []
    for row in rows:
        if isinstance(row, dict) and field in row:
            ids.append(str(row[field]))
    return ids


def validate_schema(inventory: Mapping[str, Any]) -> list[str]:
    errors: list[str] = []
    if inventory.get("schema_version") != INVENTORY_SCHEMA:
        errors.append(
            f"schema_version must be {INVENTORY_SCHEMA!r}, got {inventory.get('schema_version')!r}"
        )
    if inventory.get("conflict_key") != CONFLICT_KEY:
        errors.append(f"conflict_key must be {CONFLICT_KEY!r}")

    transports = _require_list(inventory.get("transports"), "transports", errors)
    clients = _require_list(inventory.get("clients"), "clients", errors)
    bind_sites = _require_list(inventory.get("bind_sites"), "bind_sites", errors)
    cli_flags = _require_list(inventory.get("cli_flags"), "cli_flags", errors)
    relays = _require_list(inventory.get("dap_to_dap_relays"), "dap_to_dap_relays", errors)
    invariants = _require_list(inventory.get("invariants"), "invariants", errors)
    first_mile = _require_list(inventory.get("first_mile_surfaces"), "first_mile_surfaces", errors)

    transport_ids: list[str] = []
    for index, row in enumerate(transports):
        mapping = _require_mapping(row, f"transports[{index}]", errors)
        if mapping is None:
            continue
        for field in REQUIRED_TRANSPORT_FIELDS:
            if field not in mapping:
                errors.append(f"transports[{index}] missing {field}")
        ident = mapping.get("id")
        if not isinstance(ident, str) or not ident:
            errors.append(f"transports[{index}].id must be a non-empty string")
        else:
            if ident in transport_ids:
                errors.append(f"duplicate transport id {ident!r}")
            transport_ids.append(ident)
        _check_vocab(mapping.get("kind"), TRANSPORT_KINDS, f"transports[{ident}].kind", errors)
        _check_vocab(mapping.get("role"), ROLES, f"transports[{ident}].role", errors)
        _check_vocab(mapping.get("disposition"), DISPOSITIONS, f"transports[{ident}].disposition", errors)
        _check_vocab(mapping.get("authority"), AUTHORITIES, f"transports[{ident}].authority", errors)
        if mapping.get("kind") in {"native_editor_tcp", "external_peer_editor_tcp"}:
            if mapping.get("disposition") != "retire":
                errors.append(f"transport {ident!r} is an editor TCP surface and must be disposition=retire")
            if mapping.get("role") != "editor":
                errors.append(f"transport {ident!r} is editor TCP and must have role=editor")
        if mapping.get("kind") == "debugger_peer_tcp":
            if mapping.get("disposition") != "retain":
                errors.append(f"transport {ident!r} is debugger-peer TCP and must remain disposition=retain")
            if mapping.get("role") != "debugger_peer":
                errors.append(f"transport {ident!r} is debugger-peer TCP and must have role=debugger_peer")
        if mapping.get("kind") == "dap_attach" and mapping.get("role") == "editor":
            errors.append(f"transport {ident!r} classifies DAP attach as editor transport")
        if mapping.get("kind") == "dap_to_dap_proxy" and mapping.get("disposition") == "retain":
            if mapping.get("authority") == "product":
                errors.append(f"transport {ident!r} retains a product DAP-to-DAP proxy")
        if mapping.get("kind") == "test_only_loopback":
            if mapping.get("authority") != "test":
                errors.append(f"transport {ident!r} test-only loopback must have authority=test")
            if mapping.get("role") != "test_only":
                errors.append(f"transport {ident!r} test-only loopback must have role=test_only")
        if not isinstance(mapping.get("paths"), list) or not all(
            isinstance(path, str) for path in mapping.get("paths", [])
        ):
            errors.append(f"transports[{ident}].paths must be an array of strings")

    file_transport_ids = _sorted_ids(transports, "id")
    if file_transport_ids != sorted(file_transport_ids):
        errors.append("transports must be sorted by id")

    client_ids: list[str] = []
    for index, row in enumerate(clients):
        mapping = _require_mapping(row, f"clients[{index}]", errors)
        if mapping is None:
            continue
        for field in REQUIRED_CLIENT_FIELDS:
            if field not in mapping:
                errors.append(f"clients[{index}] missing {field}")
        ident = mapping.get("id")
        if not isinstance(ident, str) or not ident:
            errors.append(f"clients[{index}].id must be a non-empty string")
        else:
            if ident in client_ids:
                errors.append(f"duplicate client id {ident!r}")
            client_ids.append(ident)
        _check_vocab(mapping.get("evidence_stage"), EVIDENCE_STAGES, f"clients[{ident}].evidence_stage", errors)
        _check_vocab(mapping.get("support_status"), SUPPORT_STATUSES, f"clients[{ident}].support_status", errors)
        _check_vocab(mapping.get("transport"), CLIENT_TRANSPORTS, f"clients[{ident}].transport", errors)
        if not isinstance(mapping.get("editor_socket_required"), bool):
            errors.append(f"clients[{ident}].editor_socket_required must be a boolean")
        if not isinstance(mapping.get("blocks_retirement"), bool):
            errors.append(f"clients[{ident}].blocks_retirement must be a boolean")
        if mapping.get("support_status") == "unsupported" and mapping.get("blocks_retirement"):
            errors.append(
                f"client {ident!r} is unsupported and must not block retirement"
            )
        if mapping.get("dap_claimed") is False and mapping.get("blocks_retirement"):
            errors.append(f"client {ident!r} does not claim DAP and must not block retirement")

    file_client_ids = _sorted_ids(clients, "id")
    if file_client_ids != sorted(file_client_ids):
        errors.append("clients must be sorted by id")

    for index, row in enumerate(bind_sites):
        mapping = _require_mapping(row, f"bind_sites[{index}]", errors)
        if mapping is None:
            continue
        for field in ("id", "path", "role", "transport_id", "disposition"):
            if field not in mapping:
                errors.append(f"bind_sites[{index}] missing {field}")
        if mapping.get("transport_id") not in transport_ids:
            errors.append(
                f"bind_sites[{mapping.get('id')!r}] transport_id {mapping.get('transport_id')!r} is not a transport"
            )
        _check_vocab(mapping.get("role"), ROLES, f"bind_sites[{mapping.get('id')}].role", errors)
        _check_vocab(
            mapping.get("disposition"), DISPOSITIONS, f"bind_sites[{mapping.get('id')}].disposition", errors
        )
        if mapping.get("role") == "editor" and mapping.get("disposition") != "retire":
            errors.append(
                f"bind_site {mapping.get('id')!r} is an editor listener and must be disposition=retire"
            )
        if mapping.get("role") == "debugger_peer" and mapping.get("disposition") != "retain":
            errors.append(
                f"bind_site {mapping.get('id')!r} is a debugger-peer listener and must remain retain"
            )
        if mapping.get("role") == "editor" and mapping.get("transport_id") == "debugger-peer-tcp":
            errors.append(
                f"bind_site {mapping.get('id')!r} mislabels a debugger-peer listener as editor transport"
            )

    bind_ids = _sorted_ids(bind_sites, "id")
    if bind_ids != sorted(bind_ids):
        errors.append("bind_sites must be sorted by id")

    for index, row in enumerate(cli_flags):
        mapping = _require_mapping(row, f"cli_flags[{index}]", errors)
        if mapping is None:
            continue
        for field in ("id", "flag", "applies_to", "disposition"):
            if field not in mapping:
                errors.append(f"cli_flags[{index}] missing {field}")
        if mapping.get("applies_to") == "perl-dap" and mapping.get("flag") in {"--socket", "--port"}:
            if mapping.get("disposition") != "retire":
                errors.append(
                    f"cli flag {mapping.get('flag')!r} on perl-dap is an editor socket surface and must be retire"
                )
            if mapping.get("disposition") == "retain":
                errors.append(f"cli flag {mapping.get('flag')!r} must not be classified supported/retain")

    flag_ids = _sorted_ids(cli_flags, "id")
    if flag_ids != sorted(flag_ids):
        errors.append("cli_flags must be sorted by id")

    for index, row in enumerate(relays):
        mapping = _require_mapping(row, f"dap_to_dap_relays[{index}]", errors)
        if mapping is None:
            continue
        for field in ("id", "path", "disposition", "authority"):
            if field not in mapping:
                errors.append(f"dap_to_dap_relays[{index}] missing {field}")
        if mapping.get("authority") == "product" and mapping.get("disposition") != "retire":
            errors.append(f"relay {mapping.get('id')!r} must not be a retained product DAP-to-DAP proxy")

    missing_invariants = [item for item in REQUIRED_INVARIANTS if item not in invariants]
    for item in missing_invariants:
        errors.append(f"missing required invariant: {item}")

    if not all(isinstance(path, str) and path for path in first_mile):
        errors.append("first_mile_surfaces must be an array of non-empty strings")

    expected = inventory_digest(inventory)
    actual = inventory.get("digest")
    if actual != expected:
        errors.append(f"digest mismatch: inventory has {actual!r}, canonical projection is {expected}")

    return errors


def is_test_only_path(path: str) -> bool:
    normalized = f"/{path.replace(chr(92), '/')}"
    return any(fragment in normalized for fragment in TEST_ONLY_PATH_FRAGMENTS)


def is_current_supported_client(row: Mapping[str, Any]) -> bool:
    if row.get("support_status") == "supported":
        return True
    if row.get("evidence_stage") == "shipped" and row.get("support_status") not in {
        "unsupported",
        "planned",
        "not_proven",
    }:
        return True
    return False
