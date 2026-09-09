#!/usr/bin/env python3
"""Materialize the validated release evidence into the existing VSIX inputs."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any

from release_archive_members import selected_member_bytes, selected_member_digest
from release_build_identity import load_json_object, validate_topology
from release_terminal_manifest import (
    digest,
    validate_package_evidence,
    validate_receipt,
)


def canonical(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode("utf-8")


def build(args: argparse.Namespace) -> None:
    receipt = load_json_object(args.receipt, "release build receipt")
    identity = receipt.get("input")
    if not isinstance(identity, dict):
        raise ValueError("release build receipt has no identity input")
    if identity.get("source_revision") != args.source_sha:
        raise ValueError("release build receipt source differs from requested source")
    if identity.get("target") != args.target:
        raise ValueError("release build receipt target differs from requested target")
    binaries = validate_receipt(receipt, identity)
    evidence = load_json_object(args.package_evidence, "release package evidence")
    archive = args.archive.resolve(strict=True)
    if evidence.get("archive", {}).get("sha256") != digest(archive):
        raise ValueError("package evidence archive digest does not match archive")
    validate_package_evidence(
        evidence,
        archive,
        archive.parent,
        identity,
        binaries,
        evidence["archive"]["sha256"],
    )
    topology = load_json_object(args.topology, "release topology")
    if digest(args.topology) != identity.get("release_topology_digest"):
        raise ValueError("release topology digest differs from build receipt")
    validate_topology(
        topology,
        release_version=identity["release_version"],
        source_revision=args.source_sha,
        target=args.target,
    )
    projection = load_json_object(args.projection, "VSIX projection input")
    if projection.get("releaseTopologySha256") != identity.get("release_topology_digest"):
        raise ValueError("VSIX projection topology digest differs from build receipt")
    row = next(
        (item for item in projection.get("targets", []) if item.get("target") == args.target),
        None,
    )
    if not isinstance(row, dict):
        raise ValueError("VSIX projection has no requested target row")
    os_name = row.get("os")
    architecture = row.get("architecture")
    libc = row.get("libc")
    vscode_target = f"{'linux' if os_name == 'linux' else 'darwin' if os_name == 'macos' else 'win32'}-{'arm64' if architecture == 'aarch64' else 'x64'}"
    if os_name == "linux" and libc == "musl":
        vscode_target = f"alpine-{'arm64' if architecture == 'aarch64' else 'x64'}"
    suffix = ".exe" if os_name == "windows" else ""
    evidence_rows = {item["executable"]: item for item in evidence["binaries"]}
    payloads: list[tuple[str, str, bytes]] = []
    for executable, role in (("perllsp", "server"), ("perl-dap", "dap")):
        item = evidence_rows.get(executable)
        if item is None:
            raise ValueError(f"package evidence omits {executable}")
        member = f"{executable}{suffix}"
        raw = selected_member_bytes(archive, item["member_path"])
        observed = hashlib.sha256(raw).hexdigest()
        if observed != item["post_strip_sha256"]:
            raise ValueError(f"archive member digest mismatch for {executable}")
        payloads.append((executable, member, raw))
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    target_dir = output / "bin" / vscode_target
    target_dir.mkdir(parents=True, exist_ok=True)
    for _, member, raw in payloads:
        (target_dir / member).write_bytes(raw)
    manifest = {
        "schema": "vsix_candidate_payload.v1",
        "extension": {"id": args.extension_id, "version": identity["release_version"], "sourceSha": args.source_sha},
        "candidate": {"id": identity["candidate_identity"], "release": identity["release_version"], "sourceSha": args.source_sha},
        "releaseTopologySha256": identity["release_topology_digest"],
        "package": {"vscodeTargetId": vscode_target, "rustTarget": args.target, "mode": "target_specific", "inventorySha256": args.inventory_sha256},
        "server": {"candidateId": identity["candidate_identity"], "target": args.target, "member": payloads[0][1], "sha256": hashlib.sha256(payloads[0][2]).hexdigest(), "identityRef": f"{args.receipt}:binaries/perllsp"},
        "dap": {"disposition": "required_present", "payload": {"candidateId": identity["candidate_identity"], "target": args.target, "member": payloads[1][1], "sha256": hashlib.sha256(payloads[1][2]).hexdigest(), "identityRef": f"{args.receipt}:binaries/perl-dap"}},
    }
    (output / "vsix-candidate-payload.json").write_bytes(canonical(manifest))


def main() -> int:
    parser = argparse.ArgumentParser()
    for name in ("receipt", "package_evidence", "archive", "topology", "projection", "output"):
        parser.add_argument(f"--{name.replace('_', '-')}", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--inventory-sha256", required=True)
    parser.add_argument("--extension-id", required=True)
    args = parser.parse_args()
    try:
        build(args)
    except (OSError, KeyError, ValueError, TypeError) as error:
        print(f"prebuilt VSIX payload: NOT_PROVEN: {error}", file=sys.stderr)
        return 1
    print(f"prebuilt VSIX payload: PASS: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
