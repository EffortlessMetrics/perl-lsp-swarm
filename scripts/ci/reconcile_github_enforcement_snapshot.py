#!/usr/bin/env python3
"""Reconcile a captured GitHub enforcement union against static policy truth.

The command is offline and read-only. It performs no GitHub API calls and
cannot mutate branch protection or rulesets.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable

SNAPSHOT_VERSION = 1
RECEIPT_VERSION = 1
STATIC_VERSION = 2
INSTRUMENT = {"observed", "unreadable", "missing", "error"}
SOURCES = {"fixture", "operator", "trusted_default_branch"}
PERMISSIONS = {"complete", "partial", "unknown"}
ENFORCEMENT = {
    "github-branch-protection": frozenset({"classic"}),
    "github-ruleset": frozenset({"ruleset"}),
    "github-branch-protection+ruleset": frozenset({"classic", "ruleset"}),
}


class ContractError(ValueError):
    """Closed-contract validation failure."""


def obj(value: Any, field: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ContractError(f"{field} must be an object")
    return value


def seq(value: Any, field: str) -> list[Any]:
    if not isinstance(value, list):
        raise ContractError(f"{field} must be a list")
    return value


def closed(
    value: dict[str, Any],
    field: str,
    required: set[str],
    optional: set[str] | None = None,
) -> None:
    optional = optional or set()
    missing = required - set(value)
    unknown = set(value) - required - optional
    if missing:
        raise ContractError(f"{field} missing fields: {sorted(missing)}")
    if unknown:
        raise ContractError(f"{field} unsupported fields: {sorted(unknown)}")


def text(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ContractError(f"{field} must be a non-empty string")
    return value


def hex_digest(value: Any, field: str, length: int) -> str:
    value = text(value, field).lower()
    if len(value) != length or any(char not in "0123456789abcdef" for char in value):
        raise ContractError(f"{field} must be a {length}-character lowercase hex digest")
    return value


def app_id(value: Any, field: str) -> int | None:
    if value is None:
        return None
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise ContractError(f"{field} must be a positive integer or null")
    return value


def timestamp(value: Any, field: str) -> str:
    value = text(value, field)
    normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError as error:
        raise ContractError(f"{field} must be ISO-8601") from error
    if parsed.tzinfo is None:
        raise ContractError(f"{field} must include a timezone")
    return value


def canonical(value: Any) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def digest(value: Any) -> str:
    return hashlib.sha256(canonical(value)).hexdigest()


def check(value: Any, field: str) -> dict[str, Any]:
    value = obj(value, field)
    closed(value, field, {"context", "app_id"})
    return {
        "context": text(value["context"], f"{field}.context"),
        "app_id": app_id(value["app_id"], f"{field}.app_id"),
    }


def checks(value: Any, field: str) -> list[dict[str, Any]]:
    result = [check(item, f"{field}[{index}]") for index, item in enumerate(seq(value, field))]
    return sorted(
        result,
        key=lambda item: (
            item["context"],
            -1 if item["app_id"] is None else item["app_id"],
        ),
    )


def validate_snapshot(raw: Any) -> dict[str, Any]:
    raw = obj(raw, "snapshot")
    closed(
        raw,
        "snapshot",
        {
            "schema_version",
            "repository",
            "observation",
            "static_contract",
            "classic_branch_protection",
            "rulesets",
        },
    )
    if raw["schema_version"] != SNAPSHOT_VERSION:
        raise ContractError(f"snapshot.schema_version must be {SNAPSHOT_VERSION}")

    repository = obj(raw["repository"], "snapshot.repository")
    closed(
        repository,
        "snapshot.repository",
        {"full_name", "repository_id", "default_branch", "branch_sha", "observed_at"},
    )
    repository_id = repository["repository_id"]
    if not isinstance(repository_id, int) or isinstance(repository_id, bool) or repository_id <= 0:
        raise ContractError("snapshot.repository.repository_id must be positive")

    observation = obj(raw["observation"], "snapshot.observation")
    closed(observation, "snapshot.observation", {"source", "permission", "limitations"})
    source = text(observation["source"], "snapshot.observation.source")
    permission = text(observation["permission"], "snapshot.observation.permission")
    if source not in SOURCES:
        raise ContractError(f"snapshot.observation.source must be one of {sorted(SOURCES)}")
    if permission not in PERMISSIONS:
        raise ContractError(
            f"snapshot.observation.permission must be one of {sorted(PERMISSIONS)}"
        )
    limitations = sorted(
        {
            text(item, f"snapshot.observation.limitations[{index}]")
            for index, item in enumerate(
                seq(observation["limitations"], "snapshot.observation.limitations")
            )
        }
    )

    static = obj(raw["static_contract"], "snapshot.static_contract")
    closed(
        static,
        "snapshot.static_contract",
        {"subject_sha256", "policy_sha256", "repository_sha"},
    )

    classic = obj(raw["classic_branch_protection"], "snapshot.classic_branch_protection")
    closed(
        classic,
        "snapshot.classic_branch_protection",
        {"instrument_state", "branch", "required_status_checks"},
    )
    classic_state = text(
        classic["instrument_state"],
        "snapshot.classic_branch_protection.instrument_state",
    )
    if classic_state not in INSTRUMENT:
        raise ContractError(
            "snapshot.classic_branch_protection.instrument_state must be one of "
            f"{sorted(INSTRUMENT)}"
        )

    rulesets = obj(raw["rulesets"], "snapshot.rulesets")
    closed(rulesets, "snapshot.rulesets", {"instrument_state", "items"})
    ruleset_state = text(rulesets["instrument_state"], "snapshot.rulesets.instrument_state")
    if ruleset_state not in INSTRUMENT:
        raise ContractError(
            f"snapshot.rulesets.instrument_state must be one of {sorted(INSTRUMENT)}"
        )

    normalized_rulesets: list[dict[str, Any]] = []
    ids: set[int] = set()
    for index, item in enumerate(seq(rulesets["items"], "snapshot.rulesets.items")):
        field = f"snapshot.rulesets.items[{index}]"
        item = obj(item, field)
        closed(
            item,
            field,
            {
                "id",
                "name",
                "target",
                "enforcement",
                "targets_default_branch",
                "bypass_actors",
                "required_status_checks",
            },
        )
        ruleset_id = item["id"]
        if not isinstance(ruleset_id, int) or isinstance(ruleset_id, bool) or ruleset_id <= 0:
            raise ContractError(f"{field}.id must be positive")
        if ruleset_id in ids:
            raise ContractError(f"duplicate ruleset id: {ruleset_id}")
        ids.add(ruleset_id)
        target = text(item["target"], f"{field}.target")
        enforcement = text(item["enforcement"], f"{field}.enforcement")
        if target != "branch":
            raise ContractError(f"{field}.target must be branch")
        if enforcement not in {"active", "evaluate", "disabled"}:
            raise ContractError(f"{field}.enforcement is unsupported")
        targeted = item["targets_default_branch"]
        if not isinstance(targeted, bool):
            raise ContractError(f"{field}.targets_default_branch must be boolean")

        bypasses: list[dict[str, Any]] = []
        for bypass_index, bypass in enumerate(seq(item["bypass_actors"], f"{field}.bypass_actors")):
            bypass_field = f"{field}.bypass_actors[{bypass_index}]"
            bypass = obj(bypass, bypass_field)
            closed(bypass, bypass_field, {"actor_type", "actor_id", "bypass_mode"})
            actor = bypass["actor_id"]
            if actor is not None and (
                not isinstance(actor, int) or isinstance(actor, bool) or actor <= 0
            ):
                raise ContractError(f"{bypass_field}.actor_id must be positive or null")
            mode = text(bypass["bypass_mode"], f"{bypass_field}.bypass_mode")
            if mode not in {"always", "pull_request"}:
                raise ContractError(f"{bypass_field}.bypass_mode is unsupported")
            bypasses.append(
                {
                    "actor_type": text(
                        bypass["actor_type"], f"{bypass_field}.actor_type"
                    ),
                    "actor_id": actor,
                    "bypass_mode": mode,
                }
            )
        bypasses.sort(
            key=lambda row: (
                row["actor_type"],
                -1 if row["actor_id"] is None else row["actor_id"],
                row["bypass_mode"],
            )
        )
        normalized_rulesets.append(
            {
                "id": ruleset_id,
                "name": text(item["name"], f"{field}.name"),
                "target": target,
                "enforcement": enforcement,
                "targets_default_branch": targeted,
                "bypass_actors": bypasses,
                "required_status_checks": checks(
                    item["required_status_checks"],
                    f"{field}.required_status_checks",
                ),
            }
        )
    normalized_rulesets.sort(key=lambda item: item["id"])

    return {
        "schema_version": SNAPSHOT_VERSION,
        "repository": {
            "full_name": text(repository["full_name"], "snapshot.repository.full_name"),
            "repository_id": repository_id,
            "default_branch": text(
                repository["default_branch"], "snapshot.repository.default_branch"
            ),
            "branch_sha": hex_digest(
                repository["branch_sha"], "snapshot.repository.branch_sha", 40
            ),
            "observed_at": timestamp(
                repository["observed_at"], "snapshot.repository.observed_at"
            ),
        },
        "observation": {
            "source": source,
            "permission": permission,
            "limitations": limitations,
        },
        "static_contract": {
            "subject_sha256": hex_digest(
                static["subject_sha256"],
                "snapshot.static_contract.subject_sha256",
                64,
            ),
            "policy_sha256": hex_digest(
                static["policy_sha256"],
                "snapshot.static_contract.policy_sha256",
                64,
            ),
            "repository_sha": hex_digest(
                static["repository_sha"],
                "snapshot.static_contract.repository_sha",
                40,
            ),
        },
        "classic_branch_protection": {
            "instrument_state": classic_state,
            "branch": text(
                classic["branch"], "snapshot.classic_branch_protection.branch"
            ),
            "required_status_checks": checks(
                classic["required_status_checks"],
                "snapshot.classic_branch_protection.required_status_checks",
            ),
        },
        "rulesets": {"instrument_state": ruleset_state, "items": normalized_rulesets},
    }


def validate_static(raw: Any) -> dict[str, Any]:
    raw = obj(raw, "static_receipt")
    for field in ("schema_version", "status", "subject_sha256", "subjects"):
        if field not in raw:
            raise ContractError(f"static_receipt missing {field}")
    if raw["schema_version"] != STATIC_VERSION:
        raise ContractError(f"static_receipt.schema_version must be {STATIC_VERSION}")
    status = text(raw["status"], "static_receipt.status")
    if status != "SUCCESS":
        return {"status": status, "subject_sha256": raw["subject_sha256"]}

    subjects = obj(raw["subjects"], "static_receipt.subjects")
    for field in ("repository_sha", "policy", "contexts"):
        if field not in subjects:
            raise ContractError(f"static_receipt.subjects missing {field}")
    policy = obj(subjects["policy"], "static_receipt.subjects.policy")
    if "sha256" not in policy:
        raise ContractError("static_receipt.subjects.policy missing sha256")

    contexts: list[dict[str, Any]] = []
    names: set[str] = set()
    for index, entry in enumerate(seq(subjects["contexts"], "static_receipt.subjects.contexts")):
        field = f"static_receipt.subjects.contexts[{index}]"
        entry = obj(entry, field)
        for required in ("name", "policy_role", "enforcement"):
            if required not in entry:
                raise ContractError(f"{field} missing {required}")
        name = text(entry["name"], f"{field}.name")
        if name in names:
            raise ContractError(f"duplicate static context: {name}")
        names.add(name)
        row = {
            "name": name,
            "policy_role": text(entry["policy_role"], f"{field}.policy_role"),
            "enforcement": text(entry["enforcement"], f"{field}.enforcement"),
        }
        if "app_id" in entry:
            row["app_id"] = app_id(entry["app_id"], f"{field}.app_id")
        contexts.append(row)
    contexts.sort(key=lambda row: row["name"])
    return {
        "status": status,
        "subject_sha256": hex_digest(
            raw["subject_sha256"], "static_receipt.subject_sha256", 64
        ),
        "repository_sha": hex_digest(
            subjects["repository_sha"], "static_receipt.subjects.repository_sha", 40
        ),
        "policy_sha256": hex_digest(
            policy["sha256"], "static_receipt.subjects.policy.sha256", 64
        ),
        "contexts": contexts,
    }


def live_union(snapshot: dict[str, Any]) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    rows: dict[str, dict[str, Any]] = {}
    excluded: list[dict[str, Any]] = []

    def add(item: dict[str, Any], source: str) -> None:
        row = rows.setdefault(item["context"], {"app_ids": set(), "sources": set()})
        row["app_ids"].add(item["app_id"])
        row["sources"].add(source)

    for item in snapshot["classic_branch_protection"]["required_status_checks"]:
        add(item, "classic")
    for ruleset in snapshot["rulesets"]["items"]:
        active = ruleset["enforcement"] == "active" and ruleset["targets_default_branch"]
        if not active:
            excluded.append(
                {
                    key: ruleset[key]
                    for key in (
                        "id",
                        "name",
                        "target",
                        "enforcement",
                        "targets_default_branch",
                    )
                }
            )
            continue
        for item in ruleset["required_status_checks"]:
            add(item, "ruleset")

    union = []
    for name in sorted(rows):
        sources = sorted(rows[name]["sources"])
        union.append(
            {
                "context": name,
                "app_ids": sorted(
                    rows[name]["app_ids"],
                    key=lambda value: -1 if value is None else value,
                ),
                "sources": sources,
                "source_class": "both" if len(sources) == 2 else sources[0],
            }
        )
    return union, sorted(excluded, key=lambda row: row["id"])


def reconcile(snapshot_raw: Any, static_raw: Any) -> dict[str, Any]:
    try:
        snapshot = validate_snapshot(snapshot_raw)
        static = validate_static(static_raw)
    except ContractError as error:
        return {
            "schema_version": RECEIPT_VERSION,
            "status": "NOT_PROVEN",
            "repository": None,
            "snapshot_sha256": None,
            "static_contract_subject_sha256": None,
            "surface_states": None,
            "live_union": [],
            "excluded_rulesets": [],
            "differences": [],
            "limitations": [{"code": "invalid_input", "message": str(error)}],
        }

    base = {
        "schema_version": RECEIPT_VERSION,
        "repository": snapshot["repository"],
        "observation": snapshot["observation"],
        "snapshot_sha256": digest(snapshot),
        "static_contract_subject_sha256": static.get("subject_sha256"),
        "surface_states": {
            "classic_branch_protection": snapshot["classic_branch_protection"][
                "instrument_state"
            ],
            "rulesets": snapshot["rulesets"]["instrument_state"],
        },
    }
    union, excluded = live_union(snapshot)
    limitations: list[dict[str, str]] = []

    if static["status"] != "SUCCESS":
        limitations.append(
            {
                "code": "static_contract_not_success",
                "message": f"static contract status is {static['status']}",
            }
        )
    else:
        identity = (
            (
                "static_subject_mismatch",
                snapshot["static_contract"]["subject_sha256"],
                static["subject_sha256"],
            ),
            (
                "policy_digest_mismatch",
                snapshot["static_contract"]["policy_sha256"],
                static["policy_sha256"],
            ),
            (
                "static_repository_sha_mismatch",
                snapshot["static_contract"]["repository_sha"],
                static["repository_sha"],
            ),
            (
                "branch_sha_mismatch",
                snapshot["repository"]["branch_sha"],
                static["repository_sha"],
            ),
        )
        for code, observed, expected in identity:
            if observed != expected:
                limitations.append(
                    {
                        "code": code,
                        "message": f"observed={observed}, expected={expected}",
                    }
                )

    if snapshot["classic_branch_protection"]["branch"] != snapshot["repository"][
        "default_branch"
    ]:
        limitations.append(
            {
                "code": "classic_branch_mismatch",
                "message": "classic protection targets another branch",
            }
        )
    for surface, state in base["surface_states"].items():
        if state != "observed":
            limitations.append(
                {
                    "code": f"{surface}_not_observed",
                    "message": f"{surface} instrument state is {state}",
                }
            )
    if snapshot["observation"]["permission"] != "complete":
        limitations.append(
            {
                "code": "observation_permission_incomplete",
                "message": f"permission is {snapshot['observation']['permission']}",
            }
        )
    limitations.extend(
        {"code": "capture_limitation", "message": message}
        for message in snapshot["observation"]["limitations"]
    )
    if limitations:
        return {
            **base,
            "status": "NOT_PROVEN",
            "live_union": union,
            "excluded_rulesets": excluded,
            "differences": [],
            "limitations": sorted(
                limitations, key=lambda row: (row["code"], row["message"])
            ),
        }

    expected: dict[str, dict[str, Any]] = {}
    differences: list[dict[str, Any]] = []
    for row in static["contexts"]:
        if row["policy_role"] != "required":
            continue
        if row["enforcement"] not in ENFORCEMENT:
            differences.append(
                {
                    "code": "required_policy_has_unsupported_enforcement",
                    "context": row["name"],
                    "expected": row["enforcement"],
                    "observed": None,
                }
            )
            continue
        expected[row["name"]] = row

    observed = {row["context"]: row for row in union}
    for name in sorted(expected):
        wanted = expected[name]
        actual = observed.get(name)
        wanted_sources = ENFORCEMENT[wanted["enforcement"]]
        if actual is None:
            differences.append(
                {
                    "code": "policy_context_missing_live",
                    "context": name,
                    "expected": sorted(wanted_sources),
                    "observed": [],
                }
            )
            continue
        actual_sources = frozenset(actual["sources"])
        if actual_sources != wanted_sources:
            differences.append(
                {
                    "code": "enforcement_source_mismatch",
                    "context": name,
                    "expected": sorted(wanted_sources),
                    "observed": sorted(actual_sources),
                }
            )
        if "app_id" in wanted and wanted["app_id"] not in actual["app_ids"]:
            differences.append(
                {
                    "code": "app_identity_mismatch",
                    "context": name,
                    "expected": wanted["app_id"],
                    "observed": actual["app_ids"],
                }
            )
    for name in sorted(set(observed) - set(expected)):
        differences.append(
            {
                "code": "live_context_missing_from_policy",
                "context": name,
                "expected": None,
                "observed": observed[name]["sources"],
            }
        )
    differences.sort(key=lambda row: (row["context"], row["code"]))

    return {
        **base,
        "status": "DRIFT" if differences else "MATCH",
        "live_union": union,
        "excluded_rulesets": excluded,
        "differences": differences,
        "limitations": [],
    }


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"failed to read {path}: {error}") from error


def explain(receipt: dict[str, Any]) -> str:
    lines = [f"GitHub enforcement union: {receipt.get('status', 'NOT_PROVEN')}"]
    repository = receipt.get("repository")
    if isinstance(repository, dict):
        lines.append(
            f"- subject: {repository['full_name']} "
            f"{repository['default_branch']}@{repository['branch_sha']}"
        )
    lines.extend(
        f"- not proven: {row['code']}: {row['message']}"
        for row in receipt.get("limitations", [])
    )
    lines.extend(
        f"- drift: {row['code']}: {row['context']}: "
        f"expected={row['expected']!r} observed={row['observed']!r}"
        for row in receipt.get("differences", [])
    )
    if receipt.get("status") == "MATCH":
        lines.append("- checked-in required contexts match the complete live union")
    return "\n".join(lines)


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate")
    validate.add_argument("snapshot", type=Path)
    compare = commands.add_parser("reconcile")
    compare.add_argument("--snapshot", type=Path, required=True)
    compare.add_argument("--static-receipt", type=Path, required=True)
    compare.add_argument("--receipt", type=Path)
    describe = commands.add_parser("explain")
    describe.add_argument("receipt", type=Path)
    args = parser.parse_args(list(argv) if argv is not None else None)

    try:
        if args.command == "validate":
            print(json.dumps(validate_snapshot(read_json(args.snapshot)), indent=2, sort_keys=True))
            return 0
        if args.command == "reconcile":
            result = reconcile(read_json(args.snapshot), read_json(args.static_receipt))
            if args.receipt:
                args.receipt.parent.mkdir(parents=True, exist_ok=True)
                args.receipt.write_text(
                    json.dumps(result, indent=2, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
            print(explain(result))
            return 0 if result["status"] == "MATCH" else 1
        print(explain(obj(read_json(args.receipt), "receipt")))
        return 0
    except ContractError as error:
        print(f"GitHub enforcement union: NOT_PROVEN\n- {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
