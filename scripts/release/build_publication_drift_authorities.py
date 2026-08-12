#!/usr/bin/env python3
"""Build an exact authority packet for a publication-drift observation."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
from pathlib import Path
from typing import Any, Sequence

SCHEMA = "perl_lsp.publication_drift_authorities.v1"
CONTROL_SCHEMA = "perl_lsp.publication_drift_control.v1"
SHA40 = re.compile(r"^[0-9a-f]{40}$")
SHA64 = re.compile(r"^[0-9a-f]{64}$")


class AuthorityError(RuntimeError):
    """Malformed, unbound, or out-of-root authority evidence."""


def _load(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AuthorityError(f"cannot load {label}: {error}") from error
    if not isinstance(value, dict):
        raise AuthorityError(f"{label} root must be an object")
    return value


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _authority(path: Path, control_root: Path, role: str) -> dict[str, str]:
    root = control_root.resolve(strict=True)
    resolved = path.resolve(strict=True)
    try:
        relative = resolved.relative_to(root)
    except ValueError as error:
        raise AuthorityError(f"{role} authority escapes the reviewed control checkout") from error
    if not resolved.is_file():
        raise AuthorityError(f"{role} authority is not a file")
    return {"path": relative.as_posix(), "sha256": _sha256(resolved)}


def _subject(observation: dict[str, Any], side: str) -> dict[str, str]:
    value = observation.get(side)
    if not isinstance(value, dict):
        raise AuthorityError(f"observation {side} subject is missing")
    result: dict[str, str] = {}
    for field, pattern in (("repository", None), ("sha", SHA40), ("tree_digest", SHA64), ("version", None)):
        item = value.get(field)
        if not isinstance(item, str) or not item or (pattern is not None and not pattern.fullmatch(item)):
            raise AuthorityError(f"observation {side}.{field} is invalid")
        result[field] = item
    return result


def build(
    observation: dict[str, Any],
    control: dict[str, Any],
    control_root: Path,
    topology: Path,
    public_claims: Path,
    api_audit: Path,
    runtime_bundle: Path | None,
) -> dict[str, Any]:
    if control.get("schema_version") != CONTROL_SCHEMA:
        raise AuthorityError("unsupported control identity schema")
    for field, pattern in (("control_sha", SHA40), ("control_tree_digest", SHA64), ("workflow_sha256", SHA64)):
        value = control.get(field)
        if not isinstance(value, str) or not pattern.fullmatch(value):
            raise AuthorityError(f"control identity {field} is invalid")

    manifest = observation.get("manifest")
    if not isinstance(manifest, dict) or not isinstance(manifest.get("sha256"), str):
        raise AuthorityError("observation manifest identity is missing")
    if not SHA64.fullmatch(manifest["sha256"]):
        raise AuthorityError("observation manifest digest is invalid")

    return {
        "schema_version": SCHEMA,
        "control": control,
        "subjects": {
            "swarm": _subject(observation, "swarm"),
            "public": _subject(observation, "public"),
        },
        "manifest": {
            "path": manifest.get("path"),
            "sha256": manifest["sha256"],
        },
        "topology": _authority(topology, control_root, "topology"),
        "public_claims": _authority(public_claims, control_root, "public claims"),
        "api_audit": _authority(api_audit, control_root, "API audit"),
        "runtime_bundle": (
            _authority(runtime_bundle, control_root, "runtime bundle")
            if runtime_bundle is not None
            else None
        ),
    }


def _write(value: dict[str, Any], destination: Path) -> None:
    payload = json.dumps(value, indent=2, sort_keys=True) + "\n"
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.name}.tmp")
    temporary.write_text(payload, encoding="utf-8")
    os.replace(temporary, destination)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--observation", type=Path, required=True)
    parser.add_argument("--control", type=Path, required=True)
    parser.add_argument("--control-root", type=Path, required=True)
    parser.add_argument("--topology", type=Path, required=True)
    parser.add_argument("--public-claims", type=Path, required=True)
    parser.add_argument("--api-audit", type=Path, required=True)
    parser.add_argument("--runtime-bundle", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(sys.argv[1:] if argv is None else argv)
    try:
        value = build(
            _load(args.observation, "publication-drift observation"),
            _load(args.control, "control identity"),
            args.control_root,
            args.topology,
            args.public_claims,
            args.api_audit,
            args.runtime_bundle,
        )
        _write(value, args.out)
    except AuthorityError as error:
        print(f"publication drift authorities: not_proven: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
