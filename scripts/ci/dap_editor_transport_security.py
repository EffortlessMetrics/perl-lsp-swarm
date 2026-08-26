#!/usr/bin/env python3
"""Composed DAP editor-transport security instrument (#10567).

This is the cheapest static/recurrence owner for the S04 proof. Exact Linux
process/socket rows live in `crates/perl-dap/tests/dap_editor_transport_security.rs`
and emit a `dap_editor_transport_security.v1` receipt. Missing OS socket
observation is `instrument_failure` or `not_proven`, never a zero-listener pass.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Mapping, Sequence

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from dap_editor_transport_inventory import check_inventory  # noqa: E402
from dap_editor_transport_schema import (  # noqa: E402
    TransportInventoryError,
    read_json,
    write_json,
)

SECURITY_SCHEMA = "dap_editor_transport_security.v1"
CONFLICT_KEYS = (
    "dap.transport.editor_socket",
    "dap.transport.external_peer",
    "dap.exact_process.matrix",
)
VERDICTS = ("pass", "failed", "not_proven", "instrument_failure")
MODES = ("native", "external_peer_connect", "external_peer_listen")
EDITOR_TRANSPORTS = ("stdio",)
LISTENER_ROLES = ("editor_dap", "debugger_peer", "unknown")
SOCKET_INSTRUMENTS = (
    "linux_procfs",
    "missing",
    "error",
    "unsupported_platform",
    "unavailable",
)
CLEANUP_STATES = ("clean", "unknown", "leaked")
SECRET_KEYS = (
    "token",
    "peer_token",
    "peerToken",
    "PERL_DAP_PEER_TOKEN",
    "session_token",
    "sessionToken",
    "credential",
    "secret",
)
REDACTED_VALUES = {"<redacted>", "redacted", "[redacted]", "absent"}

REQUIRED_RECEIPT_FIELDS = (
    "schema_version",
    "candidate",
    "binary",
    "runner",
    "modes",
    "static_recurrence",
    "limitations",
    "verdict",
)

REQUIRED_MODE_FIELDS = (
    "mode",
    "editor_transport",
    "listeners",
    "historical_port_probes",
    "old_cli_refusal",
    "dap_discriminator",
    "peer_authentication",
    "cross_session_replay",
    "stdout_stderr_purity",
    "cleanup",
    "verdict",
)

REQUIRED_BINARY_FIELDS = ("path", "sha256", "source")
REQUIRED_CANDIDATE_FIELDS = ("git_sha", "tree")
REQUIRED_LISTENER_OBS_FIELDS = ("instrument", "inventory")
SUBPROOF_FIELDS = (
    "old_cli_refusal",
    "dap_discriminator",
    "peer_authentication",
    "cross_session_replay",
    "stdout_stderr_purity",
)


class SecurityProofError(RuntimeError):
    """A fail-closed transport-security proof error."""


def _is_mapping(value: Any) -> bool:
    return isinstance(value, dict)


def combine_verdicts(verdicts: Sequence[str]) -> str:
    """Collapse row verdicts. Failure outranks instrument failure, which
    outranks not_proven. Missing/empty input is not_proven, never pass.
    """
    if not verdicts:
        return "not_proven"
    unknown = [item for item in verdicts if item not in VERDICTS]
    if unknown:
        return "failed"
    if "failed" in verdicts:
        return "failed"
    if "instrument_failure" in verdicts:
        return "instrument_failure"
    if "not_proven" in verdicts:
        return "not_proven"
    if all(item == "pass" for item in verdicts):
        return "pass"
    return "not_proven"


def socket_observation_verdict(observation: Mapping[str, Any] | None) -> str:
    """Classify a process-owned listener observation.

    A missing, failed, or unsupported instrument must not become "zero
    listeners, pass". An observed empty inventory may be pass; an unobserved
    empty inventory must not.
    """
    if not _is_mapping(observation):
        return "instrument_failure"
    instrument = observation.get("instrument")
    if instrument in {None, "missing", "error"}:
        return "instrument_failure"
    if instrument in {"unsupported_platform", "unavailable"}:
        return "not_proven"
    if instrument != "linux_procfs":
        return "not_proven"
    inventory = observation.get("inventory")
    if inventory is None:
        return "instrument_failure"
    if not isinstance(inventory, list):
        return "instrument_failure"
    return "pass"


def classify_listener_roles(
    mode: str, observation: Mapping[str, Any] | None
) -> tuple[str, list[str]]:
    """Return (verdict, errors) for role classification.

    Native and peer-connect must own zero editor listeners. Peer-listen must
    classify every retained listener as `debugger_peer`, never `editor_dap`.
    """
    obs_verdict = socket_observation_verdict(observation)
    if obs_verdict != "pass":
        return obs_verdict, []
    assert observation is not None
    errors: list[str] = []
    inventory = observation.get("inventory") or []
    roles = []
    for row in inventory:
        if not _is_mapping(row):
            errors.append("listener inventory row is not an object")
            continue
        role = row.get("role")
        if role not in LISTENER_ROLES:
            errors.append(f"listener role {role!r} is not in {list(LISTENER_ROLES)}")
            continue
        roles.append(role)
        if role == "editor_dap":
            errors.append("adapter-owned editor_dap listener is forbidden")
        if mode in {"native", "external_peer_connect"} and role != "unknown":
            errors.append(
                f"{mode} must not own a classified TCP listener; got role {role!r}"
            )
        if mode == "external_peer_listen" and role == "editor_dap":
            errors.append("peer-listen listener labeled editor_dap (role confusion)")
        if mode == "external_peer_listen" and role == "unknown":
            errors.append("peer-listen listener left unclassified")

    if mode in {"native", "external_peer_connect"} and roles:
        errors.append(f"{mode} owned TCP listeners {roles}; editor-facing inventory must be empty")
    if mode == "external_peer_listen":
        if not roles:
            errors.append("peer-listen must positively classify the debugger-peer listener")
        elif len(roles) != 1 or roles[0] != "debugger_peer":
            errors.append(
                f"peer-listen must own exactly one debugger_peer listener, got {roles}"
            )

    return ("failed" if errors else "pass", errors)


def identity_verdict(
    claimed_path: str | None,
    actual_path: str | None,
    claimed_sha256: str | None,
    actual_sha256: str | None,
) -> tuple[str, list[str]]:
    """Stale/other binary is failed. Missing identity is not_proven, not pass."""
    errors: list[str] = []
    if not claimed_path or not actual_path or not claimed_sha256 or not actual_sha256:
        return "not_proven", ["binary identity is incomplete"]
    if claimed_path != actual_path:
        errors.append(
            f"stale or other perl-dap binary path: claimed {claimed_path!r} actual {actual_path!r}"
        )
    if claimed_sha256 != actual_sha256:
        errors.append(
            f"stale or other perl-dap binary hash: claimed {claimed_sha256} actual {actual_sha256}"
        )
    if claimed_sha256 == "0" * 64 or actual_sha256 == "0" * 64:
        errors.append("all-zero binary hash is not a bound artifact identity")
    return ("failed" if errors else "pass", errors)


def subproof_verdict(field: str, value: Any) -> str | None:
    """Extract a combinable verdict from a required mode sub-proof.

    ``not_applicable`` is omitted rather than treated as pass. A missing
    object is instrument_failure. ``peer_authentication`` may use ``class``
    when it has no explicit verdict.
    """
    if not _is_mapping(value):
        return "instrument_failure"
    declared = value.get("verdict")
    if declared in {"not_applicable"}:
        return None
    if declared in VERDICTS:
        return str(declared)
    if field == "peer_authentication":
        cls = value.get("class")
        if cls in {"not_applicable"}:
            return None
        if cls == "authenticated":
            return "pass"
        if cls in {None, ""}:
            return "not_proven"
        return "failed"
    return "not_proven"


def cleanup_verdict(state: str | None) -> tuple[str, list[str]]:
    """Unknown cleanup cannot be recorded as pass."""
    if state == "clean":
        return "pass", []
    if state == "leaked":
        return "failed", ["adapter/debuggee/peer/socket/task remained after terminal cleanup"]
    if state == "unknown":
        return "not_proven", ["cleanup unknown; missing process/socket observation is not pass"]
    return "instrument_failure", [f"cleanup state {state!r} is not a known vocabulary"]


def secret_leakage_errors(payload: Any, *, path: str = "$") -> list[str]:
    """Reject token material in receipts/logs. Hashes may be 64-hex; tokens are not."""
    errors: list[str] = []
    if _is_mapping(payload):
        for key, value in payload.items():
            child = f"{path}.{key}"
            if key in SECRET_KEYS:
                if isinstance(value, str) and value and value not in REDACTED_VALUES:
                    errors.append(f"{child} serializes secret material")
                elif value not in REDACTED_VALUES and value is not None:
                    errors.append(f"{child} serializes secret material")
            errors.extend(secret_leakage_errors(value, path=child))
        return errors
    if isinstance(payload, list):
        for index, item in enumerate(payload):
            errors.extend(secret_leakage_errors(item, path=f"{path}[{index}]"))
        return errors
    if isinstance(payload, str) and "PERL_DAP_PEER_TOKEN=" in payload:
        errors.append(f"{path} leaked PERL_DAP_PEER_TOKEN assignment")
    return errors


def canary_leakage_errors(payload: Any, canaries: Sequence[str]) -> list[str]:
    if not canaries:
        return []
    blob = json.dumps(payload, sort_keys=True)
    errors: list[str] = []
    for canary in canaries:
        if canary and canary in blob:
            errors.append("receipt leaked a peer-credential canary")
    return errors


def scan_bind_site_role_confusion(inventory: Mapping[str, Any]) -> list[str]:
    """Debugger-peer TCP must not be inventoried as an editor listener, and
    editor TCP must not be inventoried as debugger-peer.
    """
    errors: list[str] = []
    transports = {
        row.get("id"): row
        for row in inventory.get("transports") or []
        if _is_mapping(row)
    }
    for site in inventory.get("bind_sites") or []:
        if not _is_mapping(site):
            continue
        ident = site.get("id")
        role = site.get("role")
        transport_id = site.get("transport_id")
        transport = transports.get(transport_id) if isinstance(transport_id, str) else None
        if role == "editor" and (
            transport_id == "debugger-peer-tcp" or (transport and transport.get("kind") == "debugger_peer_tcp")
        ):
            errors.append(
                f"bind_site {ident!r} labels debugger-peer TCP as an editor listener"
            )
        if role == "debugger_peer" and transport and transport.get("kind") in {
            "native_editor_tcp",
            "external_peer_editor_tcp",
        }:
            errors.append(
                f"bind_site {ident!r} labels an editor TCP transport as debugger_peer"
            )
        if transport and transport.get("role") == "editor" and role == "debugger_peer":
            errors.append(
                f"bind_site {ident!r} role debugger_peer contradicts transport {transport_id!r} editor role"
            )
        if transport and transport.get("role") == "debugger_peer" and role == "editor":
            errors.append(
                f"bind_site {ident!r} role editor contradicts transport {transport_id!r} debugger_peer role"
            )
    return errors


def validate_receipt(receipt: Mapping[str, Any], *, canaries: Sequence[str] = ()) -> list[str]:
    errors: list[str] = []
    if receipt.get("schema_version") != SECURITY_SCHEMA:
        errors.append(
            f"schema_version must be {SECURITY_SCHEMA!r}, got {receipt.get('schema_version')!r}"
        )
    for field in REQUIRED_RECEIPT_FIELDS:
        if field not in receipt:
            errors.append(f"receipt missing {field}")
    if receipt.get("verdict") not in VERDICTS:
        errors.append(f"receipt verdict {receipt.get('verdict')!r} is not in {list(VERDICTS)}")

    binary = receipt.get("binary")
    if _is_mapping(binary):
        for field in REQUIRED_BINARY_FIELDS:
            if not binary.get(field):
                errors.append(f"binary.{field} is required")
        source = binary.get("source")
        if source not in {"cargo_bin_exe", "explicit_env", "identity_cli"}:
            errors.append(f"binary.source {source!r} is not a bound identity source")
    else:
        errors.append("binary must be an object")

    candidate = receipt.get("candidate")
    if _is_mapping(candidate):
        for field in REQUIRED_CANDIDATE_FIELDS:
            if not candidate.get(field):
                errors.append(f"candidate.{field} is required")
        if candidate.get("tree") not in {"clean", "dirty", "not_proven"}:
            errors.append(f"candidate.tree {candidate.get('tree')!r} is not a known state")
    else:
        errors.append("candidate must be an object")

    modes = receipt.get("modes")
    if not isinstance(modes, list) or not modes:
        errors.append("modes must be a non-empty array")
        modes = []
    seen: set[str] = set()
    row_verdicts: list[str] = []
    for index, row in enumerate(modes):
        if not _is_mapping(row):
            errors.append(f"modes[{index}] must be an object")
            continue
        for field in REQUIRED_MODE_FIELDS:
            if field not in row:
                errors.append(f"modes[{index}] missing {field}")
        mode = row.get("mode")
        if mode not in MODES:
            errors.append(f"modes[{index}].mode {mode!r} is not in {list(MODES)}")
        elif mode in seen:
            errors.append(f"duplicate mode row {mode!r}")
        else:
            seen.add(mode)
        if row.get("editor_transport") not in EDITOR_TRANSPORTS:
            errors.append(
                f"modes[{index}] editor_transport must be stdio, got {row.get('editor_transport')!r}"
            )
        listeners = row.get("listeners")
        role_verdict, role_errors = classify_listener_roles(
            str(mode) if isinstance(mode, str) else "",
            listeners if _is_mapping(listeners) else None,
        )
        errors.extend(f"modes[{index}]: {item}" for item in role_errors)
        cleanup_state = None
        cleanup = row.get("cleanup")
        if _is_mapping(cleanup):
            cleanup_state = cleanup.get("state")
        clean_verdict, clean_errors = cleanup_verdict(
            str(cleanup_state) if isinstance(cleanup_state, str) else cleanup_state
        )
        errors.extend(f"modes[{index}]: {item}" for item in clean_errors)
        declared = row.get("verdict")
        if declared not in VERDICTS:
            errors.append(f"modes[{index}].verdict {declared!r} is not in {list(VERDICTS)}")
        else:
            subproofs = [
                subproof_verdict(field, row.get(field)) for field in SUBPROOF_FIELDS
            ]
            expected = combine_verdicts(
                [
                    role_verdict,
                    clean_verdict,
                    *[item for item in subproofs if item is not None],
                    str(declared)
                    if declared in {"failed", "instrument_failure", "not_proven"}
                    else "pass",
                ]
            )
            # A row may declare a stricter failure. It must not declare pass when
            # observation, cleanup, or a required sub-proof is non-pass.
            if declared == "pass" and expected != "pass":
                errors.append(
                    f"modes[{index}] declared pass but observation/cleanup/subproof verdict is {expected}"
                )
            row_verdicts.append(str(declared))

    required_modes = set(MODES)
    missing_modes = sorted(required_modes - seen)
    if missing_modes:
        if receipt.get("verdict") == "pass":
            errors.append(
                f"receipt verdict pass skipped modes {missing_modes}; skipped mode is not pass"
            )
        # Skipped mode is allowed only when the overall verdict is non-pass.
        elif receipt.get("verdict") not in {"not_proven", "failed", "instrument_failure"}:
            errors.append(f"missing mode rows {missing_modes}")

    static = receipt.get("static_recurrence")
    if _is_mapping(static):
        if static.get("verdict") not in VERDICTS:
            errors.append(f"static_recurrence.verdict {static.get('verdict')!r} is invalid")
        else:
            row_verdicts.append(str(static.get("verdict")))
    else:
        errors.append("static_recurrence must be an object")

    if _is_mapping(binary) and _is_mapping(candidate):
        identity, identity_errors = identity_verdict(
            binary.get("path"),
            binary.get("observed_path") or binary.get("path"),
            binary.get("sha256"),
            binary.get("observed_sha256") or binary.get("sha256"),
        )
        errors.extend(identity_errors)
        row_verdicts.append(identity)

    errors.extend(secret_leakage_errors(receipt))
    errors.extend(canary_leakage_errors(receipt, canaries))

    combined = combine_verdicts(row_verdicts)
    declared_overall = receipt.get("verdict")
    if declared_overall == "pass" and combined != "pass":
        errors.append(
            f"receipt verdict pass contradicts combined row verdict {combined}"
        )
    if declared_overall == "pass" and errors:
        # Schema/leak errors already recorded; keep pass invalid.
        pass
    return errors


def static_security_errors(root: Path, inventory: Mapping[str, Any]) -> list[str]:
    errors = check_inventory(root, inventory)
    errors.extend(scan_bind_site_role_confusion(inventory))
    return errors


def build_static_receipt(
    inventory: Mapping[str, Any], errors: Sequence[str]
) -> dict[str, Any]:
    return {
        "schema_version": SECURITY_SCHEMA,
        "conflict_keys": list(CONFLICT_KEYS),
        "static_recurrence": {
            "verdict": "failed" if errors else "pass",
            "error_count": len(errors),
            "errors": list(errors),
        },
        "limitations": [
            "static check does not observe process-owned sockets",
            "exact native/peer rows are owned by crates/perl-dap/tests/dap_editor_transport_security.rs",
            "installed editor/client consumption remains #6694",
            "Windows/macOS socket rows are not_proven unless a release claim requires them",
        ],
        "verdict": "failed" if errors else "not_proven",
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    check = sub.add_parser("check")
    check.add_argument("--root", default=".")
    check.add_argument("--manifest", default=".ci/dap/editor-transport-inventory.v1.json")
    check.add_argument("--receipt", required=True)
    check.add_argument("--runtime-receipt", default=None)
    check.add_argument("--canary", action="append", default=[])
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    root = Path(args.root).resolve()
    manifest_path = Path(args.manifest)
    if not manifest_path.is_absolute():
        manifest_path = root / manifest_path

    try:
        inventory = read_json(manifest_path)
        errors = static_security_errors(root, inventory)
        static_receipt = build_static_receipt(inventory, errors)
        write_json(Path(args.receipt), static_receipt)

        runtime_errors: list[str] = []
        if args.runtime_receipt:
            runtime_path = Path(args.runtime_receipt)
            if not runtime_path.is_absolute():
                runtime_path = root / runtime_path
            runtime = read_json(runtime_path)
            if not _is_mapping(runtime):
                runtime_errors.append("runtime receipt is not an object")
            else:
                runtime_errors.extend(validate_receipt(runtime, canaries=args.canary))
                overall = runtime.get("verdict")
                if overall != "pass":
                    runtime_errors.append(
                        f"runtime receipt verdict {overall!r} is not pass; "
                        "non-pass runtime evidence is not a successful check"
                    )

        all_errors = list(errors) + runtime_errors
        if all_errors:
            print("DAP editor-transport security errors:", file=sys.stderr)
            for item in all_errors:
                print(f"  {item}", file=sys.stderr)
            return 1
        print(f"DAP editor-transport security static check: valid ({args.receipt})")
        print(
            "static-only verdict is not_proven; exact process/socket rows are required for pass"
        )
        return 0
    except TransportInventoryError as exc:
        print(f"DAP editor-transport security error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
