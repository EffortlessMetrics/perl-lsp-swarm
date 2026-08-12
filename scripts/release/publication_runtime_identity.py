#!/usr/bin/env python3
"""Observe staged runtime identity and compose it into publication-drift input.

The `observe` subcommand is the only process-executing boundary. It hashes an
explicitly supplied executable before and after `--identity-json`, records a
bounded path role, and emits an inert bundle. The `compose` subcommand is pure:
it validates that bundle against exact expected subjects and projects runtime
contradictions into the existing publication-drift differences/invariants.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Iterable, Sequence

BUNDLE_SCHEMA = "perl_lsp.publication_runtime_identity.v1"
PACKET_SCHEMA = "perl_lsp.binary_identity.v1"
MAX_PACKET_BYTES = 128 * 1024
SHA40 = re.compile(r"^[0-9a-f]{40}$")
SHA64 = re.compile(r"^[0-9a-f]{64}$")
IDENTITY = re.compile(r"^[A-Za-z0-9._:@/+\-]+$")
PATH_ROLES = {"staged_archive", "package_install", "vsix_managed", "workspace", "user_supplied"}


class RuntimeIdentityError(RuntimeError):
    """A deterministic observation or composition failure."""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeIdentityError(f"cannot load JSON {path.name}: {error}") from error
    if not isinstance(value, dict):
        raise RuntimeIdentityError(f"JSON root must be an object: {path.name}")
    return value


def _bounded(value: Any, field: str, *, pattern: re.Pattern[str] | None = IDENTITY) -> str:
    if not isinstance(value, str) or not value or len(value) > 512:
        raise RuntimeIdentityError(f"{field} must be a non-empty bounded string")
    if pattern is not None and not pattern.fullmatch(value):
        raise RuntimeIdentityError(f"{field} has invalid syntax")
    return value


def _query_packet(path: Path, timeout: float) -> tuple[str, dict[str, Any]]:
    if not path.is_file() or not os.access(path, os.X_OK):
        raise RuntimeIdentityError(f"executable is missing or not executable: {path.name}")
    before = _sha256(path)
    try:
        completed = subprocess.run(
            [str(path), "--identity-json"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=timeout,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RuntimeIdentityError(f"identity query failed to execute for {path.name}: {error}") from error
    after = _sha256(path)
    if before != after:
        raise RuntimeIdentityError(f"executable changed during observation: {path.name}")
    if completed.returncode != 0:
        stderr = completed.stderr[:1024].decode("utf-8", errors="replace").strip()
        raise RuntimeIdentityError(
            f"identity query failed for {path.name} with exit {completed.returncode}: {stderr}"
        )
    if len(completed.stdout) > MAX_PACKET_BYTES:
        raise RuntimeIdentityError(f"identity packet too large for {path.name}")
    try:
        packet = json.loads(completed.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeIdentityError(f"identity packet is not valid JSON for {path.name}") from error
    if not isinstance(packet, dict):
        raise RuntimeIdentityError(f"identity packet root is not an object for {path.name}")
    return before, packet


def _subject(path: Path, path_role: str, timeout: float) -> dict[str, Any]:
    if path_role not in PATH_ROLES:
        raise RuntimeIdentityError(f"unsupported path role: {path_role}")
    digest, packet = _query_packet(path, timeout)
    return {
        "filename": path.name,
        "path_role": path_role,
        "executable_sha256": digest,
        "packet": packet,
    }


def observe(args: argparse.Namespace) -> dict[str, Any]:
    bundle: dict[str, Any] = {
        "schema_version": BUNDLE_SCHEMA,
        "expected": {
            "tree_sha": _bounded(args.expected_tree_sha, "expected-tree-sha", pattern=SHA40),
            "version": _bounded(args.expected_version, "expected-version"),
            "target": _bounded(args.expected_target, "expected-target"),
            "candidate_identity": (
                _bounded(args.expected_candidate, "expected-candidate")
                if args.expected_candidate
                else None
            ),
        },
        "server": _subject(args.server, args.server_path_role, args.timeout_seconds),
        "extension": {
            "id": _bounded(args.extension_id, "extension-id"),
            "version": _bounded(args.extension_version, "extension-version"),
            "candidate_identity": (
                _bounded(args.extension_candidate, "extension-candidate")
                if args.extension_candidate
                else None
            ),
            "package_sha256": (
                _bounded(args.extension_sha256, "extension-sha256", pattern=SHA64)
                if args.extension_sha256
                else None
            ),
        },
        "topology": {
            "digest": _bounded(args.topology_digest, "topology-digest", pattern=SHA64),
            "selected_target": _bounded(args.expected_target, "expected-target"),
        },
    }
    if args.dap is not None:
        bundle["dap"] = _subject(args.dap, args.dap_path_role, args.timeout_seconds)
    return bundle


def _packet_value(packet: dict[str, Any], section: str, field: str) -> str | None:
    container = packet.get(section)
    if not isinstance(container, dict):
        return None
    value = container.get(field)
    return value if isinstance(value, str) and value else None


def _difference(path: str, classification: str, evidence: Iterable[str]) -> dict[str, Any]:
    return {
        "path": path,
        "classification": classification,
        "behavior_changed": classification == "product_drift",
        "manifest_rule": None,
        "owner": "#6856",
        "evidence": sorted(set(evidence)),
    }


def _evaluate_subject(
    subject: dict[str, Any],
    *,
    expected_role: str,
    expected_executable: str,
    expected_package: str,
    expected: dict[str, Any],
) -> tuple[list[str], list[str]]:
    drift: list[str] = []
    unknown: list[str] = []
    packet = subject.get("packet")
    if not isinstance(packet, dict):
        return drift, ["packet_missing"]
    if packet.get("schema_version") != PACKET_SCHEMA:
        unknown.append("packet_schema_unsupported")
    product = packet.get("product")
    if not isinstance(product, dict) or product.get("name") != "perl-lsp":
        drift.append("product_mismatch")
    binary = packet.get("binary")
    if not isinstance(binary, dict):
        unknown.append("binary_identity_missing")
    else:
        if binary.get("role") != expected_role:
            drift.append("role_mismatch")
        if binary.get("executable") != expected_executable:
            drift.append("executable_mismatch")
        if binary.get("cargo_package") != expected_package:
            drift.append("cargo_package_mismatch")
        if binary.get("version") != expected.get("version"):
            drift.append("version_mismatch")
    build = packet.get("build")
    if not isinstance(build, dict):
        unknown.append("build_identity_missing")
    else:
        source = build.get("source_revision")
        target = build.get("target")
        if source is None:
            unknown.append("source_revision_not_proven")
        elif source != expected.get("tree_sha"):
            drift.append("source_revision_mismatch")
        if target is None:
            unknown.append("target_not_proven")
        elif target != expected.get("target"):
            drift.append("target_mismatch")
    artifact = packet.get("artifact")
    if not isinstance(artifact, dict):
        unknown.append("artifact_identity_missing")
    else:
        if artifact.get("digest") is not None:
            unknown.append("self_reported_artifact_digest_not_authoritative")
        candidate = artifact.get("candidate_identity")
        expected_candidate = expected.get("candidate_identity")
        if expected_candidate is not None:
            if candidate is None:
                unknown.append("candidate_not_proven")
            elif candidate != expected_candidate:
                drift.append("candidate_mismatch")
    digest = subject.get("executable_sha256")
    if not isinstance(digest, str) or not SHA64.fullmatch(digest):
        unknown.append("external_executable_digest_invalid")
    if subject.get("path_role") not in PATH_ROLES:
        unknown.append("path_role_invalid")
    return sorted(set(drift)), sorted(set(unknown))


def _replace_invariant(
    observation: dict[str, Any], invariant_id: str, status: str, evidence: list[str]
) -> None:
    invariants = observation.get("invariants")
    if not isinstance(invariants, list):
        raise RuntimeIdentityError("base observation invariants are unavailable")
    matches = [item for item in invariants if isinstance(item, dict) and item.get("id") == invariant_id]
    if len(matches) != 1:
        raise RuntimeIdentityError(f"expected exactly one invariant {invariant_id}")
    matches[0]["status"] = status
    matches[0]["owner"] = "#6856"
    matches[0]["evidence"] = sorted(set(evidence))


def compose(observation: dict[str, Any], bundle: dict[str, Any]) -> dict[str, Any]:
    if bundle.get("schema_version") != BUNDLE_SCHEMA:
        raise RuntimeIdentityError("unsupported runtime identity bundle schema")
    expected = bundle.get("expected")
    if not isinstance(expected, dict):
        raise RuntimeIdentityError("runtime bundle expected identity is missing")
    public = observation.get("public")
    if not isinstance(public, dict) or public.get("sha") != expected.get("tree_sha"):
        raise RuntimeIdentityError("runtime bundle tree SHA does not match public observation")

    result = copy.deepcopy(observation)
    differences = result.get("differences")
    if not isinstance(differences, list):
        raise RuntimeIdentityError("base observation differences are unavailable")

    server = bundle.get("server")
    if not isinstance(server, dict):
        raise RuntimeIdentityError("runtime bundle server subject is missing")
    server_drift, server_unknown = _evaluate_subject(
        server,
        expected_role="server",
        expected_executable="perllsp",
        expected_package="perllsp",
        expected=expected,
    )
    if server_drift:
        differences.append(_difference("runtime/server_identity", "product_drift", server_drift))
    if server_unknown:
        differences.append(
            _difference("runtime/server_identity_evidence", "unknown_or_not_proven", server_unknown)
        )

    dap = bundle.get("dap")
    pair_drift: list[str] = []
    pair_unknown: list[str] = []
    if isinstance(dap, dict):
        dap_drift, dap_unknown = _evaluate_subject(
            dap,
            expected_role="dap",
            expected_executable="perl-dap",
            expected_package="perl-dap",
            expected=expected,
        )
        if dap_drift:
            differences.append(_difference("runtime/dap_identity", "product_drift", dap_drift))
        if dap_unknown:
            differences.append(
                _difference("runtime/dap_identity_evidence", "unknown_or_not_proven", dap_unknown)
            )
        server_packet = server.get("packet") if isinstance(server.get("packet"), dict) else {}
        dap_packet = dap.get("packet") if isinstance(dap.get("packet"), dict) else {}
        for section, field, code in [
            ("binary", "version", "server_dap_version_mismatch"),
            ("build", "source_revision", "server_dap_source_mismatch"),
            ("build", "target", "server_dap_target_mismatch"),
            ("artifact", "candidate_identity", "server_dap_candidate_mismatch"),
        ]:
            left = _packet_value(server_packet, section, field)
            right = _packet_value(dap_packet, section, field)
            if left is None or right is None:
                pair_unknown.append(f"{field}_pair_not_proven")
            elif left != right:
                pair_drift.append(code)
    else:
        pair_unknown.append("dap_identity_absent")

    if pair_drift:
        differences.append(_difference("runtime/server_dap_pair", "product_drift", pair_drift))
    if pair_unknown:
        differences.append(
            _difference("runtime/server_dap_pair_evidence", "unknown_or_not_proven", pair_unknown)
        )

    extension = bundle.get("extension")
    extension_drift: list[str] = []
    extension_unknown: list[str] = []
    if not isinstance(extension, dict):
        extension_unknown.append("extension_identity_missing")
    else:
        if extension.get("id") != "EffortlessMetrics.perl-lsp-rs":
            extension_drift.append("extension_id_mismatch")
        if extension.get("version") != expected.get("version"):
            extension_drift.append("extension_version_mismatch")
        expected_candidate = expected.get("candidate_identity")
        if expected_candidate is not None:
            candidate = extension.get("candidate_identity")
            if candidate is None:
                extension_unknown.append("extension_candidate_not_proven")
            elif candidate != expected_candidate:
                extension_drift.append("extension_candidate_mismatch")
        package_sha = extension.get("package_sha256")
        if package_sha is None:
            extension_unknown.append("extension_package_digest_not_proven")
        elif not isinstance(package_sha, str) or not SHA64.fullmatch(package_sha):
            extension_unknown.append("extension_package_digest_invalid")
    if extension_drift:
        differences.append(_difference("runtime/extension_identity", "product_drift", extension_drift))
    if extension_unknown:
        differences.append(
            _difference("runtime/extension_identity_evidence", "unknown_or_not_proven", extension_unknown)
        )

    topology = bundle.get("topology")
    topology_unknown: list[str] = []
    topology_drift: list[str] = []
    if not isinstance(topology, dict):
        topology_unknown.append("topology_identity_missing")
    else:
        if topology.get("selected_target") != expected.get("target"):
            topology_drift.append("topology_target_mismatch")
        if not isinstance(topology.get("digest"), str) or not SHA64.fullmatch(topology["digest"]):
            topology_unknown.append("topology_digest_invalid")
    if topology_drift:
        differences.append(_difference("runtime/topology_identity", "product_drift", topology_drift))
    if topology_unknown:
        differences.append(
            _difference("runtime/topology_identity_evidence", "unknown_or_not_proven", topology_unknown)
        )

    _replace_invariant(
        result,
        "server_dap_pairing",
        "fail" if pair_drift else ("not_proven" if pair_unknown else "pass"),
        pair_drift or pair_unknown or ["runtime identity packets agree"],
    )
    _replace_invariant(
        result,
        "extension_claims_match_vsix",
        "fail" if extension_drift else ("not_proven" if extension_unknown else "pass"),
        extension_drift or extension_unknown or ["extension identity agrees with runtime expectation"],
    )
    trace_unknown = server_unknown + extension_unknown + topology_unknown
    trace_drift = server_drift + extension_drift + topology_drift
    _replace_invariant(
        result,
        "artifact_traceable_to_public_sha",
        "fail" if trace_drift else ("not_proven" if trace_unknown else "pass"),
        trace_drift or trace_unknown or ["runtime subjects trace to exact public SHA"],
    )

    differences.sort(key=lambda item: (str(item.get("path")), str(item.get("classification"))))
    return result


def _write_json(value: dict[str, Any], path: Path) -> None:
    payload = json.dumps(value, indent=2, sort_keys=True) + "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(payload, encoding="utf-8")
    os.replace(temporary, path)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    observe_parser = subparsers.add_parser("observe")
    observe_parser.add_argument("--server", type=Path, required=True)
    observe_parser.add_argument("--server-path-role", choices=sorted(PATH_ROLES), required=True)
    observe_parser.add_argument("--dap", type=Path)
    observe_parser.add_argument("--dap-path-role", choices=sorted(PATH_ROLES), default="staged_archive")
    observe_parser.add_argument("--expected-tree-sha", required=True)
    observe_parser.add_argument("--expected-version", required=True)
    observe_parser.add_argument("--expected-target", required=True)
    observe_parser.add_argument("--expected-candidate")
    observe_parser.add_argument("--extension-id", default="EffortlessMetrics.perl-lsp-rs")
    observe_parser.add_argument("--extension-version", required=True)
    observe_parser.add_argument("--extension-candidate")
    observe_parser.add_argument("--extension-sha256")
    observe_parser.add_argument("--topology-digest", required=True)
    observe_parser.add_argument("--timeout-seconds", type=float, default=5.0)
    observe_parser.add_argument("--out", type=Path, required=True)

    compose_parser = subparsers.add_parser("compose")
    compose_parser.add_argument("--observation", type=Path, required=True)
    compose_parser.add_argument("--runtime-bundle", type=Path, required=True)
    compose_parser.add_argument("--out", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(sys.argv[1:] if argv is None else argv)
    try:
        if args.command == "observe":
            value = observe(args)
        else:
            value = compose(_load_json(args.observation), _load_json(args.runtime_bundle))
        _write_json(value, args.out)
    except RuntimeIdentityError as error:
        print(f"publication runtime identity: not_proven: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
