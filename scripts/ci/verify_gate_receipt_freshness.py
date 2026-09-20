#!/usr/bin/env python3
"""Verify gate-receipt artifacts bind to the current subject SHA before upload.

The CI cache restores the whole ``target/`` tree, so an uploaded
``gate-receipt-<shard>-<sha>`` artifact could otherwise contain receipts
written by earlier runs of unrelated SHAs and lie about which gates actually
executed for this subject (#12085). The shard lane clears every uploaded
surface before it runs (ci.yml) and sweeps its receipt directory again before
executing (run_gate_shard.py); this instrument independently re-checks that
every JSON artifact offered for upload carries a freshness binding to the
exact subject SHA being proven. Anything unbindable fails closed.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Sequence

SHARD_SUMMARY_SCHEMA_VERSION = "ci_gate_shard.v1"


def load_json_object(path: Path) -> dict[str, Any]:
    try:
        payload: Any = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ValueError(f"unreadable: {error}") from error
    if not isinstance(payload, dict):
        raise ValueError("root must be a JSON object")
    return payload


def bound_subject(payload: dict[str, Any]) -> str | None:
    """Return the artifact's freshness binding, or None when unverifiable.

    Shard summaries bind via the top-level ``subject_sha`` of
    ``ci_gate_shard.v1``; per-gate gate receipts bind via
    ``metadata.git_sha``. Any other shape cannot prove which run produced it.
    """
    if (
        payload.get("schema_version") == SHARD_SUMMARY_SCHEMA_VERSION
        and isinstance(payload.get("subject_sha"), str)
    ):
        return payload["subject_sha"]
    metadata = payload.get("metadata")
    if isinstance(metadata, dict) and isinstance(metadata.get("git_sha"), str):
        return metadata["git_sha"]
    return None


def find_stale_artifacts(
    directories: Sequence[Path], subject_sha: str
) -> list[str]:
    offenders: list[str] = []
    for directory in directories:
        if not directory.is_dir():
            continue
        for path in sorted(directory.glob("*.json")):
            if path.is_symlink() or not path.is_file():
                offenders.append(f"{path}: not a regular non-symlink file")
                continue
            try:
                payload = load_json_object(path)
            except ValueError as error:
                offenders.append(f"{path}: {error}")
                continue
            bound = bound_subject(payload)
            if bound is None:
                offenders.append(f"{path}: no verifiable subject binding")
            elif bound != subject_sha:
                offenders.append(
                    f"{path}: binds subject {bound!r}, expected {subject_sha!r}"
                )
    return offenders


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--subject-sha", required=True)
    parser.add_argument("--receipt-dir", type=Path, default=Path("target/receipts/shards"))
    parser.add_argument(
        "--summary-dir", type=Path, default=Path("target/receipts/shard-summaries")
    )
    parser.add_argument("--extra-dir", type=Path, action="append", default=[])
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    directories = [args.receipt_dir, args.summary_dir, *args.extra_dir]
    offenders = find_stale_artifacts(directories, args.subject_sha)
    if offenders:
        for offender in offenders:
            print(f"stale gate-receipt artifact: {offender}", file=sys.stderr)
        print(
            f"refusing fresh-evidence claim for {args.subject_sha!r}: "
            f"{len(offenders)} unbound or foreign receipt artifact(s)",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
