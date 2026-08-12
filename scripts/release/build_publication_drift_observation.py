#!/usr/bin/env python3
"""Build a publication-drift observation from two exact read-only checkouts."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Sequence

SHA40 = re.compile(r"^[0-9a-f]{40}$")
SHA64 = re.compile(r"^[0-9a-f]{64}$")
REPOSITORIES = {
    "swarm": "EffortlessMetrics/perl-lsp-swarm",
    "public": "EffortlessMetrics/perl-lsp",
}
INVARIANT_SCHEMA = "perl_lsp.publication_drift_invariants.v1"


class ObservationError(RuntimeError):
    """A deterministic acquisition or identity failure."""


@dataclass(frozen=True)
class TreeEntry:
    """The Git tree identity needed to compare one tracked path."""

    mode: str
    object_type: str
    object_id: str


def _run(root: Path, *arguments: str, binary: bool = False) -> bytes | str:
    try:
        completed = subprocess.run(
            ["git", "-C", str(root), *arguments],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ObservationError(f"git {' '.join(arguments)} failed to execute: {error}") from error
    if completed.returncode != 0:
        detail = completed.stderr[:2048].decode("utf-8", errors="replace").strip()
        raise ObservationError(f"git {' '.join(arguments)} failed: {detail}")
    if binary:
        return completed.stdout
    return completed.stdout.decode("utf-8", errors="strict").strip()


def _repository_from_remote(value: str) -> str | None:
    value = value.strip()
    if value.endswith(".git"):
        value = value[:-4]
    if value.startswith("git@github.com:"):
        return value.removeprefix("git@github.com:")
    marker = "github.com/"
    if marker in value:
        return value.split(marker, 1)[1]
    return None


def _checkout_identity(root: Path, expected_repository: str, expected_sha: str) -> dict[str, str]:
    if not root.is_dir() or not (root / ".git").exists():
        raise ObservationError(f"checkout is not a Git repository: {root.name}")
    if not SHA40.fullmatch(expected_sha):
        raise ObservationError("requested commit SHA is invalid")
    actual_sha = str(_run(root, "rev-parse", "HEAD"))
    if actual_sha != expected_sha:
        raise ObservationError(f"checkout HEAD mismatch for {expected_repository}")
    remote = _repository_from_remote(str(_run(root, "remote", "get-url", "origin")))
    if remote != expected_repository:
        raise ObservationError(
            f"checkout repository mismatch: expected {expected_repository}, observed {remote or 'unknown'}"
        )
    tree_listing = _run(root, "ls-tree", "-r", "-z", "--full-tree", "HEAD", binary=True)
    assert isinstance(tree_listing, bytes)
    tree_digest = hashlib.sha256(tree_listing).hexdigest()
    return {"repository": expected_repository, "sha": actual_sha, "tree_digest": tree_digest}


def _tracked_paths(root: Path) -> set[str]:
    output = _run(root, "ls-tree", "-r", "--name-only", "-z", "HEAD", binary=True)
    assert isinstance(output, bytes)
    paths: set[str] = set()
    for raw in output.split(b"\0"):
        if not raw:
            continue
        path = raw.decode("utf-8", errors="strict")
        if path.startswith(".git/") or path == ".git":
            continue
        paths.add(path)
    return paths


def _tree_entry(root: Path, path: str) -> TreeEntry | None:
    output = _run(root, "ls-tree", "-z", "HEAD", "--", f":(literal){path}", binary=True)
    assert isinstance(output, bytes)
    if not output:
        return None
    records = output.rstrip(b"\0").split(b"\0")
    if len(records) != 1:
        raise ObservationError(f"git ls-tree returned multiple entries for {path}")
    header, separator, _observed_path = records[0].partition(b"\t")
    fields = header.split()
    if separator != b"\t" or len(fields) != 3:
        raise ObservationError(f"git ls-tree returned an invalid entry for {path}")
    try:
        mode, object_type, object_id = (field.decode("ascii") for field in fields)
    except UnicodeDecodeError as error:
        raise ObservationError(f"git ls-tree returned a non-ASCII entry for {path}") from error
    return TreeEntry(mode=mode, object_type=object_type, object_id=object_id)


def _blob_digest(root: Path, entry: TreeEntry | None) -> str | None:
    if entry is None or entry.object_type != "blob":
        return None
    payload = _run(root, "cat-file", "blob", entry.object_id, binary=True)
    assert isinstance(payload, bytes)
    return hashlib.sha256(payload).hexdigest()


def _entry_evidence(label: str, entry: TreeEntry | None, digest: str | None) -> str:
    if entry is None:
        return f"{label}=absent"
    digest_value = digest if digest is not None else "not_applicable"
    return (
        f"{label}=mode:{entry.mode},type:{entry.object_type},object:{entry.object_id},"
        f"sha256:{digest_value}"
    )


def _load_json(path: Path, allowed_root: Path, label: str) -> dict[str, Any]:
    resolved_root = allowed_root.resolve(strict=True)
    resolved_path = path.resolve(strict=True)
    try:
        resolved_path.relative_to(resolved_root)
    except ValueError as error:
        raise ObservationError(f"{label} escapes the approved control checkout") from error
    try:
        value = json.loads(resolved_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ObservationError(f"cannot load {label}: {error}") from error
    if not isinstance(value, dict):
        raise ObservationError(f"{label} root must be an object")
    return value


def _manifest_rules(manifest: dict[str, Any]) -> dict[str, dict[str, Any]]:
    rules = manifest.get("rules")
    if not isinstance(rules, list):
        raise ObservationError("publication manifest rules are missing")
    by_path: dict[str, dict[str, Any]] = {}
    for rule in rules:
        if not isinstance(rule, dict):
            raise ObservationError("publication manifest rule must be an object")
        path = rule.get("path")
        if not isinstance(path, str) or not path or path.startswith("/") or ".." in Path(path).parts:
            raise ObservationError("publication manifest rule path is invalid")
        if path in by_path:
            raise ObservationError(f"duplicate publication manifest path rule: {path}")
        by_path[path] = rule
    return by_path


def _verify_manifest(
    manifest: dict[str, Any],
    swarm: dict[str, str],
    public: dict[str, str],
    version: str,
) -> None:
    expected = {
        "schema_version": 1,
        "swarm_repository": swarm["repository"],
        "public_repository": public["repository"],
        "swarm_sha": swarm["sha"],
        "public_sha": public["sha"],
        "swarm_tree_digest": swarm["tree_digest"],
        "public_tree_digest": public["tree_digest"],
        "version": version,
    }
    for field, value in expected.items():
        if manifest.get(field) != value:
            raise ObservationError(f"publication manifest {field} does not match the exact comparison")


def _load_invariants(
    packet: dict[str, Any], swarm: dict[str, str], public: dict[str, str]
) -> list[dict[str, Any]]:
    if packet.get("schema_version") != INVARIANT_SCHEMA:
        raise ObservationError("unsupported invariant packet schema")
    for prefix, identity in (("swarm", swarm), ("public", public)):
        if packet.get(f"{prefix}_sha") != identity["sha"]:
            raise ObservationError(f"invariant packet {prefix} SHA mismatch")
        if packet.get(f"{prefix}_tree_digest") != identity["tree_digest"]:
            raise ObservationError(f"invariant packet {prefix} tree digest mismatch")
    invariants = packet.get("invariants")
    if not isinstance(invariants, list):
        raise ObservationError("invariant packet rows are missing")
    result: list[dict[str, Any]] = []
    seen: set[str] = set()
    for row in invariants:
        if not isinstance(row, dict):
            raise ObservationError("invariant row must be an object")
        identity = row.get("id")
        status = row.get("status")
        owner = row.get("owner")
        evidence = row.get("evidence")
        if not isinstance(identity, str) or identity in seen:
            raise ObservationError("invariant identity is missing or duplicated")
        if status not in {"pass", "fail", "not_proven"}:
            raise ObservationError(f"invariant {identity} has invalid status")
        if not isinstance(owner, str) or not owner:
            raise ObservationError(f"invariant {identity} has no owner")
        if not isinstance(evidence, list) or not evidence or not all(
            isinstance(item, str) and item for item in evidence
        ):
            raise ObservationError(f"invariant {identity} has no bounded evidence")
        seen.add(identity)
        result.append(
            {"id": identity, "status": status, "owner": owner, "evidence": sorted(set(evidence))}
        )
    result.sort(key=lambda row: row["id"])
    return result


def build_observation(
    swarm_root: Path,
    public_root: Path,
    control_root: Path,
    manifest_path: Path,
    invariant_path: Path,
    swarm_sha: str,
    public_sha: str,
    version: str,
) -> dict[str, Any]:
    swarm = _checkout_identity(swarm_root, REPOSITORIES["swarm"], swarm_sha)
    public = _checkout_identity(public_root, REPOSITORIES["public"], public_sha)
    manifest = _load_json(manifest_path, control_root, "publication manifest")
    invariants_packet = _load_json(invariant_path, control_root, "invariant packet")
    _verify_manifest(manifest, swarm, public, version)
    rules = _manifest_rules(manifest)

    differences: list[dict[str, Any]] = []
    for path in sorted(_tracked_paths(swarm_root) | _tracked_paths(public_root)):
        swarm_entry = _tree_entry(swarm_root, path)
        public_entry = _tree_entry(public_root, path)
        if swarm_entry == public_entry:
            continue
        swarm_digest = _blob_digest(swarm_root, swarm_entry)
        public_digest = _blob_digest(public_root, public_entry)
        rule = rules.get(path)
        if rule is None:
            differences.append(
                {
                    "path": path,
                    "classification": "unknown_or_not_proven",
                    "behavior_changed": False,
                    "manifest_rule": None,
                    "owner": "#6857",
                    "evidence": [
                        _entry_evidence("public_object", public_entry, public_digest),
                        _entry_evidence("swarm_object", swarm_entry, swarm_digest),
                        "no approved path rule",
                    ],
                }
            )
            continue
        differences.append(
            {
                "path": path,
                "classification": rule.get("classification"),
                "behavior_changed": False,
                "manifest_rule": rule.get("id"),
                "owner": rule.get("owner"),
                "evidence": [
                    _entry_evidence("public_object", public_entry, public_digest),
                    _entry_evidence("swarm_object", swarm_entry, swarm_digest),
                ],
            }
        )

    manifest_relative = manifest_path.resolve(strict=True).relative_to(
        control_root.resolve(strict=True)
    )
    return {
        "schema_version": 1,
        "swarm": {**swarm, "version": version},
        "public": {**public, "version": version},
        "manifest": {
            "path": manifest_relative.as_posix(),
            "sha256": hashlib.sha256(
                manifest_path.resolve(strict=True).read_bytes()
            ).hexdigest(),
        },
        "differences": differences,
        "invariants": _load_invariants(invariants_packet, swarm, public),
    }


def _write_json(value: dict[str, Any], destination: Path) -> None:
    payload = json.dumps(value, indent=2, sort_keys=True) + "\n"
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.name}.tmp")
    temporary.write_text(payload, encoding="utf-8")
    os.replace(temporary, destination)


def _redacted_failure_reason(error: ObservationError) -> str:
    """Return a stable cause without retaining paths, URLs, or tool stderr."""
    message = str(error)
    if "failed to execute" in message or " failed:" in message:
        return "git_tool_failure"
    if "checkout is not a Git repository" in message:
        return "checkout_missing_or_unreadable"
    if "repository mismatch" in message:
        return "repository_identity_failure"
    if "HEAD mismatch" in message or "SHA is invalid" in message:
        return "subject_identity_failure"
    if "cannot load" in message or "root must be an object" in message:
        return "authority_read_failure"
    if "escapes the approved control checkout" in message:
        return "authority_containment_failure"
    return "acquisition_not_proven"


def _safe_sha(value: str) -> str:
    return value if SHA40.fullmatch(value) else "0" * 40


def _safe_version(value: str) -> str:
    if value and value.strip() == value and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]*", value):
        return value
    return "not_proven"


def _failure_observation(args: argparse.Namespace, error: ObservationError) -> dict[str, Any]:
    """Build the classifier input retained when acquisition cannot complete."""
    reason = _redacted_failure_reason(error)
    return {
        "schema_version": 1,
        "swarm": {
            "repository": REPOSITORIES["swarm"],
            "sha": _safe_sha(args.swarm_sha),
            "tree_digest": "0" * 64,
            "version": _safe_version(args.version),
        },
        "public": {
            "repository": REPOSITORIES["public"],
            "sha": _safe_sha(args.public_sha),
            "tree_digest": "0" * 64,
            "version": _safe_version(args.version),
        },
        "manifest": None,
        "differences": [
            {
                "path": "__acquisition__/failure",
                "classification": "unknown_or_not_proven",
                "behavior_changed": False,
                "manifest_rule": None,
                "owner": "release-engineering",
                "evidence": [
                    f"acquisition_failure={reason}",
                    f"requested_swarm_sha={_safe_sha(args.swarm_sha)}",
                    f"requested_public_sha={_safe_sha(args.public_sha)}",
                    "cause=redacted",
                ],
            }
        ],
        "invariants": [],
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--swarm-root", type=Path, required=True)
    parser.add_argument("--public-root", type=Path, required=True)
    parser.add_argument("--control-root", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--invariants", type=Path, required=True)
    parser.add_argument("--swarm-sha", required=True)
    parser.add_argument("--public-sha", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(sys.argv[1:] if argv is None else argv)
    try:
        observation = build_observation(
            args.swarm_root,
            args.public_root,
            args.control_root,
            args.manifest,
            args.invariants,
            args.swarm_sha,
            args.public_sha,
            args.version,
        )
        _write_json(observation, args.out)
    except ObservationError as error:
        try:
            _write_json(_failure_observation(args, error), args.out)
        except OSError as write_error:
            print(
                "publication drift observation: not_proven: failure receipt unavailable: "
                f"{type(write_error).__name__}",
                file=sys.stderr,
            )
            return 2
        print(
            f"publication drift observation: not_proven: {_redacted_failure_reason(error)}",
            file=sys.stderr,
        )
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
