#!/usr/bin/env python3
"""Run and guard the bounded parser-integration proof set for issue #6107."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence

ROOT = Path(__file__).resolve().parents[2]
TARGETS_PATH = ROOT / ".ci/parser-integration-targets.json"
LOCK_PATH = ROOT / ".ci/parser-integration-targets.lock.json"
DEFAULT_RECEIPT_PATH = ROOT / "target/receipts/parser-integration.json"

MANIFEST_SCHEMA_VERSION = 2
LOCK_SCHEMA_VERSION = 1
RECEIPT_SCHEMA_VERSION = 1
ALLOWED_DISPOSITIONS = {"execute"}
ALLOWED_BOUNDEDNESS = {"focused"}
TARGET_FIELDS = {
    "id",
    "package",
    "target",
    "features",
    "no_default_features",
    "cargo_args",
    "test_args",
    "owner",
    "reason",
    "disposition",
    "boundedness",
}
RESERVED_CARGO_ARGS = {
    "-p",
    "--package",
    "--test",
    "--features",
    "--all-features",
    "--no-default-features",
    "--manifest-path",
    "--workspace",
    "--exclude",
    "--",
}


@dataclass(frozen=True)
class TargetPlan:
    """One exact parser proof invocation."""

    proof_id: str
    package: str
    target: str
    features: tuple[str, ...]
    no_default_features: bool
    cargo_args: tuple[str, ...]
    test_args: tuple[str, ...]
    owner: str
    reason: str
    disposition: str
    boundedness: str


def _non_empty_string(item: dict[str, Any], key: str) -> str:
    value = item.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"parser integration target field {key!r} must be a non-empty string")
    return value


def _string_list(item: dict[str, Any], key: str) -> tuple[str, ...]:
    value = item.get(key)
    if not isinstance(value, list) or any(not isinstance(entry, str) or not entry for entry in value):
        raise ValueError(f"parser integration target field {key!r} must be a list of non-empty strings")
    return tuple(value)


def load_targets(path: Path = TARGETS_PATH) -> list[TargetPlan]:
    """Load and validate the exact manifest-owned execution plan."""

    try:
        payload: Any = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read parser integration target manifest: {error}") from error

    if (
        not isinstance(payload, dict)
        or payload.get("schema_version") != MANIFEST_SCHEMA_VERSION
        or not isinstance(payload.get("targets"), list)
    ):
        raise ValueError("unsupported parser integration target manifest")

    plans: list[TargetPlan] = []
    proof_ids: set[str] = set()
    invocations: set[str] = set()
    for index, raw_item in enumerate(payload["targets"]):
        if not isinstance(raw_item, dict):
            raise ValueError(f"parser integration target at index {index} must be an object")
        unknown = sorted(set(raw_item) - TARGET_FIELDS)
        missing = sorted(TARGET_FIELDS - set(raw_item))
        if unknown:
            raise ValueError(f"parser integration target contains unknown fields: {', '.join(unknown)}")
        if missing:
            raise ValueError(f"parser integration target is missing fields: {', '.join(missing)}")

        proof_id = _non_empty_string(raw_item, "id")
        package = _non_empty_string(raw_item, "package")
        target = _non_empty_string(raw_item, "target")
        features = _string_list(raw_item, "features")
        cargo_args = _string_list(raw_item, "cargo_args")
        test_args = _string_list(raw_item, "test_args")
        owner = _non_empty_string(raw_item, "owner")
        reason = _non_empty_string(raw_item, "reason")
        disposition = _non_empty_string(raw_item, "disposition")
        boundedness = _non_empty_string(raw_item, "boundedness")
        no_default_features = raw_item.get("no_default_features")

        if not isinstance(no_default_features, bool):
            raise ValueError("parser integration target field 'no_default_features' must be boolean")
        if proof_id in proof_ids:
            raise ValueError(f"duplicate parser integration proof id: {proof_id}")
        if tuple(sorted(set(features))) != features:
            raise ValueError(f"features for {proof_id} must be unique and sorted")
        if any("," in feature or feature.startswith("-") for feature in features):
            raise ValueError(f"invalid Cargo feature token for {proof_id}")
        if any(argument in RESERVED_CARGO_ARGS for argument in cargo_args):
            raise ValueError(f"cargo_args for {proof_id} override manifest-owned invocation identity")
        if not owner.startswith("#") or not owner[1:].isdigit():
            raise ValueError(f"owner for {proof_id} must be a GitHub issue reference")
        if disposition not in ALLOWED_DISPOSITIONS:
            raise ValueError(f"unsupported disposition for {proof_id}: {disposition}")
        if boundedness not in ALLOWED_BOUNDEDNESS:
            raise ValueError(f"unsupported boundedness class for {proof_id}: {boundedness}")

        plan = TargetPlan(
            proof_id=proof_id,
            package=package,
            target=target,
            features=features,
            no_default_features=no_default_features,
            cargo_args=cargo_args,
            test_args=test_args,
            owner=owner,
            reason=reason,
            disposition=disposition,
            boundedness=boundedness,
        )
        invocation = invocation_digest(plan)
        if invocation in invocations:
            raise ValueError(f"duplicate parser integration invocation: {proof_id}")
        proof_ids.add(proof_id)
        invocations.add(invocation)
        plans.append(plan)

    if not plans:
        raise ValueError("parser integration target manifest is empty")
    return plans


def invocation_payload(plan: TargetPlan) -> dict[str, Any]:
    """Return the behavior-bearing invocation identity for a proof row."""

    return {
        "package": plan.package,
        "target": plan.target,
        "features": list(plan.features),
        "no_default_features": plan.no_default_features,
        "cargo_args": list(plan.cargo_args),
        "test_args": list(plan.test_args),
        "disposition": plan.disposition,
    }


def invocation_digest(plan: TargetPlan) -> str:
    encoded = json.dumps(
        invocation_payload(plan),
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def lock_payload(plans: Sequence[TargetPlan]) -> dict[str, Any]:
    """Build the deterministic accepted identity lock."""

    return {
        "schema_version": LOCK_SCHEMA_VERSION,
        "manifest_schema_version": MANIFEST_SCHEMA_VERSION,
        "accepted_for_issue": "#6107",
        "proofs": [
            {"id": plan.proof_id, "invocation_sha256": invocation_digest(plan)}
            for plan in sorted(plans, key=lambda entry: entry.proof_id)
        ],
    }


def write_json_atomic(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(payload, indent=2, sort_keys=False) + "\n"
    file_descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.",
        suffix=".tmp",
        dir=path.parent,
        text=True,
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(file_descriptor, "w", encoding="utf-8") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def load_lock(path: Path = LOCK_PATH) -> dict[str, str]:
    try:
        payload: Any = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read parser integration identity lock: {error}") from error

    if (
        not isinstance(payload, dict)
        or payload.get("schema_version") != LOCK_SCHEMA_VERSION
        or payload.get("manifest_schema_version") != MANIFEST_SCHEMA_VERSION
        or payload.get("accepted_for_issue") != "#6107"
        or not isinstance(payload.get("proofs"), list)
    ):
        raise ValueError("unsupported parser integration identity lock")

    result: dict[str, str] = {}
    for raw_item in payload["proofs"]:
        if (
            not isinstance(raw_item, dict)
            or set(raw_item) != {"id", "invocation_sha256"}
            or not isinstance(raw_item.get("id"), str)
            or not raw_item["id"]
            or not isinstance(raw_item.get("invocation_sha256"), str)
            or len(raw_item["invocation_sha256"]) != 64
        ):
            raise ValueError("invalid parser integration identity-lock row")
        if raw_item["id"] in result:
            raise ValueError(f"duplicate parser integration lock id: {raw_item['id']}")
        result[raw_item["id"]] = raw_item["invocation_sha256"]
    if not result:
        raise ValueError("parser integration identity lock is empty")
    return result


def validate_lock(plans: Sequence[TargetPlan], lock: dict[str, str]) -> None:
    current = {plan.proof_id: invocation_digest(plan) for plan in plans}
    missing = sorted(set(lock) - set(current))
    added = sorted(set(current) - set(lock))
    changed = sorted(
        proof_id
        for proof_id in set(current) & set(lock)
        if current[proof_id] != lock[proof_id]
    )
    if missing or added or changed:
        details: list[str] = []
        if missing:
            details.append("missing=" + ",".join(missing))
        if added:
            details.append("unaccepted=" + ",".join(added))
        if changed:
            details.append("changed=" + ",".join(changed))
        raise ValueError(
            "parser integration plan differs from the accepted identity lock: "
            + "; ".join(details)
            + ". Run with --write-lock only for an intentional reviewed reset."
        )


def available_targets() -> set[tuple[str, str]]:
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            "cargo metadata failed while validating parser integration targets:\n"
            + completed.stderr
        )
    try:
        payload: Any = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"cargo metadata returned invalid JSON: {error}") from error

    result: set[tuple[str, str]] = set()
    for package in payload.get("packages", []):
        package_name = package.get("name")
        for target in package.get("targets", []):
            if package_name and "test" in target.get("kind", []):
                result.add((package_name, target.get("name", "")))
    return result


def cargo_command(plan: TargetPlan) -> list[str]:
    command = [
        "cargo",
        "test",
        "--locked",
        "--package",
        plan.package,
    ]
    if plan.no_default_features:
        command.append("--no-default-features")
    if plan.features:
        command.extend(["--features", ",".join(plan.features)])
    command.extend(plan.cargo_args)
    command.extend(["--test", plan.target])
    if plan.test_args:
        command.append("--")
        command.extend(plan.test_args)
    return command


def file_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute_plans(plans: Sequence[TargetPlan]) -> tuple[int, list[dict[str, Any]]]:
    """Execute every selected row and retain the complete denominator."""

    first_failure = 0
    results: list[dict[str, Any]] = []
    for plan in plans:
        command = cargo_command(plan)
        print(f"parser integration proof {plan.proof_id}: {plan.reason}")
        print("running:", " ".join(command), flush=True)
        completed = subprocess.run(command, cwd=ROOT, check=False)
        results.append(
            {
                "id": plan.proof_id,
                "package": plan.package,
                "target": plan.target,
                "invocation_sha256": invocation_digest(plan),
                "returncode": completed.returncode,
                "result": "passed" if completed.returncode == 0 else "failed",
            }
        )
        if completed.returncode != 0 and first_failure == 0:
            first_failure = completed.returncode
    return first_failure, results


def receipt_payload(
    plans: Sequence[TargetPlan],
    results: Sequence[dict[str, Any]],
    *,
    manifest_path: Path,
    lock_path: Path,
) -> dict[str, Any]:
    return {
        "schema_version": RECEIPT_SCHEMA_VERSION,
        "manifest_schema_version": MANIFEST_SCHEMA_VERSION,
        "manifest_sha256": file_digest(manifest_path),
        "lock_sha256": file_digest(lock_path),
        "planned": len(plans),
        "executed": len(results),
        "passed": sum(item["result"] == "passed" for item in results),
        "failed": sum(item["result"] == "failed" for item in results),
        "results": list(results),
    }


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=TARGETS_PATH)
    parser.add_argument("--lock", type=Path, default=LOCK_PATH)
    parser.add_argument("--receipt", type=Path, default=DEFAULT_RECEIPT_PATH)
    parser.add_argument(
        "--write-lock",
        action="store_true",
        help="Intentionally accept the current manifest invocation identities and exit.",
    )
    parser.add_argument(
        "--validate-only",
        action="store_true",
        help="Validate manifest, lock, and Cargo target existence without running tests.",
    )
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        plans = load_targets(args.manifest)
        if args.write_lock:
            write_json_atomic(args.lock, lock_payload(plans))
            print(f"wrote parser integration identity lock: {args.lock}")
            return 0

        lock = load_lock(args.lock)
        validate_lock(plans, lock)

        missing = sorted(
            {(plan.package, plan.target) for plan in plans} - available_targets()
        )
        if missing:
            details = ", ".join(f"{package}:{target}" for package, target in missing)
            raise ValueError(f"parser integration target manifest is stale: {details}")
        if args.validate_only:
            print(f"validated {len(plans)} exact parser integration proof rows")
            return 0

        returncode, results = execute_plans(plans)
        write_json_atomic(
            args.receipt,
            receipt_payload(
                plans,
                results,
                manifest_path=args.manifest,
                lock_path=args.lock,
            ),
        )
        print(f"parser integration receipt: {args.receipt}")
        return returncode
    except (OSError, RuntimeError, ValueError) as error:
        print(f"parser integration guard failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
