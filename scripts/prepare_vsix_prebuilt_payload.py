#!/usr/bin/env python3
"""Materialize the validated release evidence into the existing VSIX inputs."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any

from release_archive_members import copy_selected_member, selected_member_digest
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
    if identity.get("candidate_identity") != args.candidate_id:
        raise ValueError("release build receipt candidate differs from requested candidate")
    if identity.get("release_version") != args.release_version:
        raise ValueError("release build receipt version differs from requested release")
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
    evidence_rows = {item["executable"]: item for item in evidence["binaries"]}
    payloads: list[dict[str, str]] = []
    for executable in ("perllsp", "perl-dap"):
        item = evidence_rows.get(executable)
        if item is None:
            raise ValueError(f"package evidence omits {executable}")
        observed = selected_member_digest(archive, item["member_path"])
        if observed != item["post_strip_sha256"]:
            raise ValueError(f"archive member digest mismatch for {executable}")
        payloads.append({"executable": executable, "source_member": item["member_path"], "sha256": observed})
    node_input = {
        "extension": {"id": args.extension_id, "version": args.release_version, "sourceSha": args.source_sha},
        "candidate": {"id": args.candidate_id, "release": args.release_version, "sourceSha": args.source_sha},
        "releaseTopologySha256": identity["release_topology_digest"],
        "projection": projection,
        "target": args.target,
        "packageInventorySha256": args.inventory_sha256,
        "server": {"candidateId": args.candidate_id, "target": args.target, "member": "", "sha256": payloads[0]["sha256"], "identityRef": f"{args.receipt}:binaries/perllsp"},
        "dap": {"candidateId": args.candidate_id, "target": args.target, "member": "", "sha256": payloads[1]["sha256"], "identityRef": f"{args.receipt}:binaries/perl-dap"},
    }
    builder = Path(__file__).with_name("build_vsix_candidate_manifest.js")
    result = subprocess.run(["node", str(builder)], input=json.dumps(node_input), text=True, capture_output=True, check=False)
    if result.returncode != 0:
        raise ValueError(result.stderr.strip() or "projection manifest builder failed")
    manifest = json.loads(result.stdout)
    vscode_target = manifest["package"]["vscodeTargetId"]
    for payload in payloads:
        native = manifest["server"] if payload["executable"] == "perllsp" else manifest["dap"]["payload"]
        if not native:
            raise ValueError(f"projection omitted required {payload['executable']}")
        payload["member"] = native["member"]
    output = args.output.resolve()
    target_dir = output / "bin" / vscode_target
    target_dir.mkdir(parents=True, exist_ok=True)
    created: list[Path] = []
    try:
        for payload in payloads:
            destination = target_dir / payload["member"]
            if destination.exists() or destination.is_symlink():
                raise ValueError(f"refusing to overwrite existing payload: {destination}")
            if copy_selected_member(archive, payload["source_member"], destination) != payload["sha256"]:
                raise ValueError(f"archive member changed while staging {payload['executable']}")
            created.append(destination)
        manifest_path = output / "vsix-candidate-payload.json"
        if manifest_path.exists() or manifest_path.is_symlink():
            raise ValueError(f"refusing to overwrite existing manifest: {manifest_path}")
        manifest_path.write_bytes(canonical(manifest))
        created.append(manifest_path)
    except Exception:
        for path in reversed(created):
            path.unlink(missing_ok=True)
        raise


def main() -> int:
    parser = argparse.ArgumentParser()
    for name in ("receipt", "package_evidence", "archive", "topology", "projection", "output"):
        parser.add_argument(f"--{name.replace('_', '-')}", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--candidate-id", required=True)
    parser.add_argument("--release-version", required=True)
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
