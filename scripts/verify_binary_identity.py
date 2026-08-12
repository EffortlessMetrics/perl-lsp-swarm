#!/usr/bin/env python3
"""Verify staged perl-lsp executables before installer promotion.

The verifier treats the executable identity packet as a statement by the binary
and binds it to the externally measured file digest and installer expectation.
It never infers product identity from a filename or a successful exit alone.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence

SCHEMA_VERSION = "perl_lsp.install_identity_verification.v1"
PACKET_SCHEMA = "perl_lsp.binary_identity.v1"
PRODUCT_NAME = "perl-lsp"
MAX_PACKET_BYTES = 128 * 1024
IDENTITY_PATTERN = re.compile(r"^[A-Za-z0-9._:@/+\-]+$")


class VerificationError(RuntimeError):
    """A deterministic packet, process, or identity failure."""


@dataclass(frozen=True)
class ExpectedBinary:
    path: Path
    executable: str
    cargo_package: str
    role: str


@dataclass(frozen=True)
class ObservedBinary:
    expected: ExpectedBinary
    sha256: str
    packet: dict[str, Any]


def _bounded_identity(value: Any, field: str, *, required: bool = True) -> str | None:
    if value is None and not required:
        return None
    if not isinstance(value, str) or not value or len(value) > 512:
        raise VerificationError(f"{field} must be a non-empty bounded string")
    if not IDENTITY_PATTERN.fullmatch(value):
        raise VerificationError(f"{field} contains unsupported characters")
    return value


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _load_packet(executable: Path, timeout_seconds: float) -> dict[str, Any]:
    if not executable.is_file():
        raise VerificationError(f"staged executable is missing: {executable.name}")
    if not os.access(executable, os.X_OK):
        raise VerificationError(f"staged executable is not executable: {executable.name}")

    try:
        completed = subprocess.run(
            [str(executable), "--identity-json"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        raise VerificationError(f"identity query timed out for {executable.name}") from error
    except OSError as error:
        raise VerificationError(f"identity query could not start for {executable.name}: {error}") from error

    if completed.returncode != 0:
        stderr = completed.stderr[:1024].decode("utf-8", errors="replace").strip()
        raise VerificationError(
            f"identity query failed for {executable.name} with exit {completed.returncode}: {stderr}"
        )
    if len(completed.stdout) > MAX_PACKET_BYTES:
        raise VerificationError(f"identity packet exceeds {MAX_PACKET_BYTES} bytes")

    try:
        packet = json.loads(completed.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise VerificationError(f"identity packet is not valid UTF-8 JSON for {executable.name}") from error
    if not isinstance(packet, dict):
        raise VerificationError("identity packet root must be an object")
    return packet


def _validate_packet(observed: ObservedBinary) -> list[str]:
    expected = observed.expected
    packet = observed.packet
    reasons: list[str] = []

    if packet.get("schema_version") != PACKET_SCHEMA:
        reasons.append("packet_schema_mismatch")

    product = packet.get("product")
    if not isinstance(product, dict) or product.get("name") != PRODUCT_NAME:
        reasons.append("product_mismatch")

    binary = packet.get("binary")
    if not isinstance(binary, dict):
        reasons.append("binary_identity_missing")
    else:
        if binary.get("executable") != expected.executable:
            reasons.append("executable_mismatch")
        if binary.get("cargo_package") != expected.cargo_package:
            reasons.append("cargo_package_mismatch")
        if binary.get("role") != expected.role:
            reasons.append("role_mismatch")
        try:
            _bounded_identity(binary.get("version"), "binary.version")
        except VerificationError:
            reasons.append("version_invalid")

    build = packet.get("build")
    if not isinstance(build, dict):
        reasons.append("build_identity_missing")
    elif build.get("identity_state") not in {"exact", "partial", "not_proven"}:
        reasons.append("build_identity_state_invalid")

    artifact = packet.get("artifact")
    if not isinstance(artifact, dict):
        reasons.append("artifact_identity_missing")
    elif artifact.get("digest") is not None:
        # A final executable cannot truthfully embed its own final digest. The
        # installer observation below is the authoritative byte binding.
        reasons.append("self_reported_artifact_digest_forbidden")

    return reasons


def observe(expected: ExpectedBinary, timeout_seconds: float) -> ObservedBinary:
    before = _sha256(expected.path)
    packet = _load_packet(expected.path, timeout_seconds)
    after = _sha256(expected.path)
    if before != after:
        raise VerificationError(f"staged executable changed during observation: {expected.path.name}")
    return ObservedBinary(expected=expected, sha256=before, packet=packet)


def _packet_value(packet: dict[str, Any], section: str, field: str) -> str | None:
    value = packet.get(section)
    if not isinstance(value, dict):
        return None
    item = value.get(field)
    return item if isinstance(item, str) and item else None


def verify(
    server: ObservedBinary,
    dap: ObservedBinary | None,
    *,
    expected_version: str,
    expected_target: str | None,
    expected_candidate: str | None,
    require_dap: bool,
) -> dict[str, Any]:
    reasons = _validate_packet(server)
    if dap is not None:
        reasons.extend(_validate_packet(dap))
    elif require_dap:
        reasons.append("dap_required_but_missing")

    server_version = _packet_value(server.packet, "binary", "version")
    server_target = _packet_value(server.packet, "build", "target")
    server_source = _packet_value(server.packet, "build", "source_revision")
    server_candidate = _packet_value(server.packet, "artifact", "candidate_identity")

    if server_version != expected_version:
        reasons.append("server_version_mismatch")
    if expected_target is not None and server_target != expected_target:
        reasons.append("server_target_mismatch_or_not_proven")
    if expected_candidate is not None and server_candidate != expected_candidate:
        reasons.append("server_candidate_mismatch_or_not_proven")

    if dap is not None:
        dap_version = _packet_value(dap.packet, "binary", "version")
        dap_target = _packet_value(dap.packet, "build", "target")
        dap_source = _packet_value(dap.packet, "build", "source_revision")
        dap_candidate = _packet_value(dap.packet, "artifact", "candidate_identity")
        if dap_version != expected_version or dap_version != server_version:
            reasons.append("dap_version_mismatch")
        if expected_target is not None and dap_target != expected_target:
            reasons.append("dap_target_mismatch_or_not_proven")
        if dap_target != server_target:
            reasons.append("server_dap_target_mismatch")
        if dap_source != server_source:
            reasons.append("server_dap_source_mismatch")
        if expected_candidate is not None and dap_candidate != expected_candidate:
            reasons.append("dap_candidate_mismatch_or_not_proven")
        if dap_candidate != server_candidate:
            reasons.append("server_dap_candidate_mismatch")

    reasons = sorted(set(reasons))
    verdict = "verified" if not reasons else "mismatch"
    binaries = [
        {
            "role": server.expected.role,
            "filename": server.expected.path.name,
            "sha256": server.sha256,
            "packet": server.packet,
        }
    ]
    if dap is not None:
        binaries.append(
            {
                "role": dap.expected.role,
                "filename": dap.expected.path.name,
                "sha256": dap.sha256,
                "packet": dap.packet,
            }
        )

    return {
        "schema_version": SCHEMA_VERSION,
        "expected": {
            "product": PRODUCT_NAME,
            "version": expected_version,
            "target": expected_target,
            "candidate_identity": expected_candidate,
            "dap_required": require_dap,
        },
        "binaries": binaries,
        "verdict": verdict,
        "reasons": reasons,
    }


def _write_receipt(receipt: dict[str, Any], destination: Path | None) -> None:
    payload = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if destination is None:
        sys.stdout.write(payload)
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.name}.tmp")
    temporary.write_text(payload, encoding="utf-8")
    os.replace(temporary, destination)


def _parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", required=True, type=Path)
    parser.add_argument("--dap", type=Path)
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--expected-target")
    parser.add_argument("--expected-candidate")
    parser.add_argument("--require-dap", action="store_true")
    parser.add_argument("--timeout-seconds", type=float, default=5.0)
    parser.add_argument("--receipt", type=Path)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = _parse_args(sys.argv[1:] if argv is None else argv)
    try:
        server = observe(
            ExpectedBinary(args.server, "perllsp", "perllsp", "server"),
            args.timeout_seconds,
        )
        dap = (
            observe(ExpectedBinary(args.dap, "perl-dap", "perl-dap", "dap"), args.timeout_seconds)
            if args.dap is not None
            else None
        )
        receipt = verify(
            server,
            dap,
            expected_version=_bounded_identity(args.expected_version, "expected-version") or "",
            expected_target=_bounded_identity(args.expected_target, "expected-target", required=False),
            expected_candidate=_bounded_identity(
                args.expected_candidate, "expected-candidate", required=False
            ),
            require_dap=args.require_dap,
        )
    except VerificationError as error:
        receipt = {
            "schema_version": SCHEMA_VERSION,
            "verdict": "not_proven",
            "reasons": [str(error)],
        }
        _write_receipt(receipt, args.receipt)
        return 2

    _write_receipt(receipt, args.receipt)
    return 0 if receipt["verdict"] == "verified" else 1


if __name__ == "__main__":
    raise SystemExit(main())
