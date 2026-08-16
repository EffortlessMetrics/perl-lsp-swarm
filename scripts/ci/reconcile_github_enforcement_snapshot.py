#!/usr/bin/env python3
"""Reconcile a captured GitHub enforcement union against static policy truth.

The command is deterministic, offline, and read-only. It performs no GitHub API
calls and cannot mutate branch protection, rulesets, bypass actors, or policy.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

SNAPSHOT_VERSION = 1
RECEIPT_VERSION = 1
STATIC_VERSION = 2

INSTRUMENT_STATES = {"observed", "unreadable", "missing", "error"}
OBSERVATION_SOURCES = {
    "fixture",
    "operator",
    "trusted_default_branch",
    "connector",
}
LIVE_OBSERVATION_SOURCES = OBSERVATION_SOURCES - {"fixture"}
PERMISSIONS = {"complete", "partial", "unknown"}
RULESET_ENFORCEMENT = {"active", "evaluate", "disabled"}
BYPASS_MODES = {"always", "pull_request"}
ENFORCEMENT_SOURCES = {
    "github-branch-protection": frozenset({"classic"}),
    "github-ruleset": frozenset({"ruleset"}),
    "github-branch-protection+ruleset": frozenset({"classic", "ruleset"}),
}
WILDCARD_CHARACTERS = frozenset("*?[")
GITHUB_ACTIONS_APP_ID = 15368


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


def boolean(value: Any, field: str) -> bool:
    if not isinstance(value, bool):
        raise ContractError(f"{field} must be a boolean")
    return value


def positive_int(value: Any, field: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise ContractError(f"{field} must be a positive integer")
    return value


def app_id(value: Any, field: str) -> int | None:
    if value is None:
        return None
    return positive_int(value, field)


def hex_digest(value: Any, field: str, length: int, *, nullable: bool = False) -> str | None:
    if value is None and nullable:
        return None
    value = text(value, field).lower()
    if len(value) != length or any(char not in "0123456789abcdef" for char in value):
        raise ContractError(f"{field} must be a {length}-character lowercase hex digest")
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
    return parsed.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def canonical(value: Any) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def digest(value: Any) -> str:
    return hashlib.sha256(canonical(value)).hexdigest()


def normalized_text_list(value: Any, field: str, *, allow_empty: bool = True) -> list[str]:
    items = [
        text(item, f"{field}[{index}]")
        for index, item in enumerate(seq(value, field))
    ]
    if not allow_empty and not items:
        raise ContractError(f"{field} must not be empty")
    return sorted(set(items))


def normalize_check(value: Any, field: str) -> dict[str, Any]:
    value = obj(value, field)
    closed(value, field, {"context", "app_id"})
    return {
        "context": text(value["context"], f"{field}.context"),
        "app_id": app_id(value["app_id"], f"{field}.app_id"),
    }


def normalize_checks(value: Any, field: str) -> list[dict[str, Any]]:
    result = [
        normalize_check(item, f"{field}[{index}]")
        for index, item in enumerate(seq(value, field))
    ]
    identities: set[tuple[str, int | None]] = set()
    for row in result:
        identity = (row["context"], row["app_id"])
        if identity in identities:
            raise ContractError(
                f"{field} contains duplicate context/app identity: {identity!r}"
            )
        identities.add(identity)
    return sorted(
        result,
        key=lambda item: (
            item["context"],
            -1 if item["app_id"] is None else item["app_id"],
        ),
    )


def normalize_bypasses(value: Any, field: str) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    identities: set[tuple[str, int | None, str]] = set()
    for index, raw in enumerate(seq(value, field)):
        item_field = f"{field}[{index}]"
        raw = obj(raw, item_field)
        closed(raw, item_field, {"actor_type", "actor_id", "bypass_mode"})
        mode = text(raw["bypass_mode"], f"{item_field}.bypass_mode")
        if mode not in BYPASS_MODES:
            raise ContractError(
                f"{item_field}.bypass_mode must be one of {sorted(BYPASS_MODES)}"
            )
        actor = raw["actor_id"]
        if actor is not None:
            actor = positive_int(actor, f"{item_field}.actor_id")
        row = {
            "actor_type": text(raw["actor_type"], f"{item_field}.actor_type"),
            "actor_id": actor,
            "bypass_mode": mode,
        }
        identity = (row["actor_type"], actor, mode)
        if identity in identities:
            raise ContractError(f"{field} contains duplicate bypass actor: {identity!r}")
        identities.add(identity)
        rows.append(row)
    return sorted(
        rows,
        key=lambda row: (
            row["actor_type"],
            -1 if row["actor_id"] is None else row["actor_id"],
            row["bypass_mode"],
        ),
    )


def normalize_ref_conditions(value: Any, field: str) -> dict[str, list[str]]:
    value = obj(value, field)
    closed(value, field, {"include", "exclude"})
    return {
        "include": normalized_text_list(
            value["include"], f"{field}.include", allow_empty=False
        ),
        "exclude": normalized_text_list(value["exclude"], f"{field}.exclude"),
    }


def selector_match(
    selector: str, *, default_branch: str
) -> bool | None:
    reference = f"refs/heads/{default_branch}"
    if selector == "~DEFAULT_BRANCH":
        return True
    if selector == "~ALL":
        return True
    if selector == reference:
        return True
    if any(character in selector for character in WILDCARD_CHARACTERS):
        return None
    return False


def derive_targeting(
    conditions: dict[str, list[str]], *, default_branch: str
) -> dict[str, Any]:
    include_results = [
        (selector, selector_match(selector, default_branch=default_branch))
        for selector in conditions["include"]
    ]
    exclude_results = [
        (selector, selector_match(selector, default_branch=default_branch))
        for selector in conditions["exclude"]
    ]

    matched_includes = sorted(
        selector for selector, result in include_results if result is True
    )
    matched_excludes = sorted(
        selector for selector, result in exclude_results if result is True
    )
    unresolved_includes = sorted(
        selector for selector, result in include_results if result is None
    )
    unresolved_excludes = sorted(
        selector for selector, result in exclude_results if result is None
    )

    if matched_excludes:
        status = "NOT_TARGETED"
    elif matched_includes and not unresolved_excludes:
        status = "TARGETED"
    elif not matched_includes and not unresolved_includes:
        status = "NOT_TARGETED"
    else:
        status = "NOT_PROVEN"

    return {
        "status": status,
        "reference": f"refs/heads/{default_branch}",
        "matched_includes": matched_includes,
        "matched_excludes": matched_excludes,
        "unresolved_includes": unresolved_includes,
        "unresolved_excludes": unresolved_excludes,
    }


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
        {
            "full_name",
            "repository_id",
            "default_branch",
            "branch_sha",
            "observed_at",
        },
    )
    full_name = text(repository["full_name"], "snapshot.repository.full_name")
    if full_name.count("/") != 1 or any(not part for part in full_name.split("/")):
        raise ContractError("snapshot.repository.full_name must be owner/name")
    repository_id = positive_int(
        repository["repository_id"], "snapshot.repository.repository_id"
    )
    default_branch = text(
        repository["default_branch"], "snapshot.repository.default_branch"
    )

    observation = obj(raw["observation"], "snapshot.observation")
    closed(
        observation,
        "snapshot.observation",
        {"source", "permission", "limitations"},
    )
    source = text(observation["source"], "snapshot.observation.source")
    permission = text(
        observation["permission"], "snapshot.observation.permission"
    )
    if source not in OBSERVATION_SOURCES:
        raise ContractError(
            "snapshot.observation.source must be one of "
            f"{sorted(OBSERVATION_SOURCES)}"
        )
    if permission not in PERMISSIONS:
        raise ContractError(
            "snapshot.observation.permission must be one of "
            f"{sorted(PERMISSIONS)}"
        )
    limitations = normalized_text_list(
        observation["limitations"], "snapshot.observation.limitations"
    )

    static = obj(raw["static_contract"], "snapshot.static_contract")
    closed(
        static,
        "snapshot.static_contract",
        {"subject_sha256", "policy_sha256", "repository_sha"},
    )

    classic = obj(
        raw["classic_branch_protection"],
        "snapshot.classic_branch_protection",
    )
    closed(
        classic,
        "snapshot.classic_branch_protection",
        {
            "instrument_state",
            "response_sha256",
            "branch",
            "strict",
            "required_status_checks",
        },
    )
    classic_state = text(
        classic["instrument_state"],
        "snapshot.classic_branch_protection.instrument_state",
    )
    if classic_state not in INSTRUMENT_STATES:
        raise ContractError(
            "snapshot.classic_branch_protection.instrument_state must be one of "
            f"{sorted(INSTRUMENT_STATES)}"
        )
    classic_response = hex_digest(
        classic["response_sha256"],
        "snapshot.classic_branch_protection.response_sha256",
        64,
        nullable=True,
    )
    classic_checks = normalize_checks(
        classic["required_status_checks"],
        "snapshot.classic_branch_protection.required_status_checks",
    )
    classic_strict = classic["strict"]
    if classic_strict is not None:
        classic_strict = boolean(
            classic_strict, "snapshot.classic_branch_protection.strict"
        )
    if classic_state == "observed" and classic_response is None:
        raise ContractError(
            "observed classic branch protection requires response_sha256"
        )
    if classic_state != "observed" and classic_checks:
        raise ContractError(
            "non-observed classic branch protection must not carry status checks"
        )

    rulesets = obj(raw["rulesets"], "snapshot.rulesets")
    closed(
        rulesets,
        "snapshot.rulesets",
        {"instrument_state", "list_response_sha256", "items"},
    )
    ruleset_state = text(
        rulesets["instrument_state"], "snapshot.rulesets.instrument_state"
    )
    if ruleset_state not in INSTRUMENT_STATES:
        raise ContractError(
            f"snapshot.rulesets.instrument_state must be one of "
            f"{sorted(INSTRUMENT_STATES)}"
        )
    list_response = hex_digest(
        rulesets["list_response_sha256"],
        "snapshot.rulesets.list_response_sha256",
        64,
        nullable=True,
    )
    if ruleset_state == "observed" and list_response is None:
        raise ContractError("observed rulesets require list_response_sha256")

    normalized_rulesets: list[dict[str, Any]] = []
    ruleset_ids: set[int] = set()
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
                "source_type",
                "source",
                "enforcement",
                "detail_response_sha256",
                "conditions",
                "bypass_actors",
                "strict_required_status_checks_policy",
                "do_not_enforce_on_create",
                "required_status_checks",
            },
        )
        ruleset_id = positive_int(item["id"], f"{field}.id")
        if ruleset_id in ruleset_ids:
            raise ContractError(f"duplicate ruleset id: {ruleset_id}")
        ruleset_ids.add(ruleset_id)

        target = text(item["target"], f"{field}.target")
        if target != "branch":
            raise ContractError(f"{field}.target must be branch")
        enforcement = text(item["enforcement"], f"{field}.enforcement")
        if enforcement not in RULESET_ENFORCEMENT:
            raise ContractError(
                f"{field}.enforcement must be one of "
                f"{sorted(RULESET_ENFORCEMENT)}"
            )

        conditions_container = obj(item["conditions"], f"{field}.conditions")
        closed(conditions_container, f"{field}.conditions", {"ref_name"})
        conditions = normalize_ref_conditions(
            conditions_container["ref_name"],
            f"{field}.conditions.ref_name",
        )

        strict = item["strict_required_status_checks_policy"]
        if strict is not None:
            strict = boolean(
                strict,
                f"{field}.strict_required_status_checks_policy",
            )
        do_not_enforce = item["do_not_enforce_on_create"]
        if do_not_enforce is not None:
            do_not_enforce = boolean(
                do_not_enforce,
                f"{field}.do_not_enforce_on_create",
            )

        normalized_rulesets.append(
            {
                "id": ruleset_id,
                "name": text(item["name"], f"{field}.name"),
                "target": target,
                "source_type": text(item["source_type"], f"{field}.source_type"),
                "source": text(item["source"], f"{field}.source"),
                "enforcement": enforcement,
                "detail_response_sha256": hex_digest(
                    item["detail_response_sha256"],
                    f"{field}.detail_response_sha256",
                    64,
                ),
                "conditions": {"ref_name": conditions},
                "targeting": derive_targeting(
                    conditions,
                    default_branch=default_branch,
                ),
                "bypass_actors": normalize_bypasses(
                    item["bypass_actors"], f"{field}.bypass_actors"
                ),
                "strict_required_status_checks_policy": strict,
                "do_not_enforce_on_create": do_not_enforce,
                "required_status_checks": normalize_checks(
                    item["required_status_checks"],
                    f"{field}.required_status_checks",
                ),
            }
        )
    normalized_rulesets.sort(key=lambda item: item["id"])
    if ruleset_state != "observed" and normalized_rulesets:
        raise ContractError("non-observed rulesets must not carry ruleset items")

    return {
        "schema_version": SNAPSHOT_VERSION,
        "repository": {
            "full_name": full_name,
            "repository_id": repository_id,
            "default_branch": default_branch,
            "branch_sha": hex_digest(
                repository["branch_sha"],
                "snapshot.repository.branch_sha",
                40,
            ),
            "observed_at": timestamp(
                repository["observed_at"],
                "snapshot.repository.observed_at",
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
            "response_sha256": classic_response,
            "branch": text(
                classic["branch"],
                "snapshot.classic_branch_protection.branch",
            ),
            "strict": classic_strict,
            "required_status_checks": classic_checks,
        },
        "rulesets": {
            "instrument_state": ruleset_state,
            "list_response_sha256": list_response,
            "items": normalized_rulesets,
        },
    }


def validate_static(raw: Any) -> dict[str, Any]:
    raw = obj(raw, "static_receipt")
    for field in ("schema_version", "status", "subject_sha256", "subjects"):
        if field not in raw:
            raise ContractError(f"static_receipt missing {field}")
    if raw["schema_version"] != STATIC_VERSION:
        raise ContractError(
            f"static_receipt.schema_version must be {STATIC_VERSION}"
        )
    status = text(raw["status"], "static_receipt.status")
    if status != "SUCCESS":
        subject = raw["subject_sha256"]
        if subject is not None:
            subject = hex_digest(
                subject, "static_receipt.subject_sha256", 64
            )
        return {"status": status, "subject_sha256": subject}

    subjects = obj(raw["subjects"], "static_receipt.subjects")
    for field in ("repository_sha", "policy", "contexts"):
        if field not in subjects:
            raise ContractError(f"static_receipt.subjects missing {field}")
    policy = obj(subjects["policy"], "static_receipt.subjects.policy")
    if "sha256" not in policy:
        raise ContractError("static_receipt.subjects.policy missing sha256")

    contexts: list[dict[str, Any]] = []
    names: set[str] = set()
    for index, entry in enumerate(
        seq(subjects["contexts"], "static_receipt.subjects.contexts")
    ):
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
            "policy_role": text(
                entry["policy_role"], f"{field}.policy_role"
            ),
            "enforcement": text(
                entry["enforcement"], f"{field}.enforcement"
            ),
        }
        producer = entry.get("producer")
        if producer is not None:
            producer = text(producer, f"{field}.producer")
            row["producer"] = producer
        if "app_id" in entry:
            row["app_id"] = app_id(entry["app_id"], f"{field}.app_id")
        elif producer == "repository-job":
            # Repository-owned GitHub Actions jobs emit checks under the
            # GitHub Actions app. This makes the existing static producer
            # identity consumable without adding a second policy registry.
            row["app_id"] = GITHUB_ACTIONS_APP_ID
        contexts.append(row)
    contexts.sort(key=lambda row: row["name"])
    return {
        "status": status,
        "subject_sha256": hex_digest(
            raw["subject_sha256"],
            "static_receipt.subject_sha256",
            64,
        ),
        "repository_sha": hex_digest(
            subjects["repository_sha"],
            "static_receipt.subjects.repository_sha",
            40,
        ),
        "policy_sha256": hex_digest(
            policy["sha256"],
            "static_receipt.subjects.policy.sha256",
            64,
        ),
        "contexts": contexts,
    }


def app_sort(value: int | None) -> int:
    return -1 if value is None else value


def live_union(
    snapshot: dict[str, Any],
) -> tuple[
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
]:
    rows: dict[str, dict[str, Any]] = {}
    excluded: list[dict[str, Any]] = []
    limitations: list[dict[str, Any]] = []

    def add(
        item: dict[str, Any],
        *,
        source: str,
        ruleset_id: int | None = None,
    ) -> None:
        row = rows.setdefault(
            item["context"],
            {
                "bindings": {
                    "classic": {"app_ids": set(), "ruleset_ids": set()},
                    "ruleset": {"app_ids": set(), "ruleset_ids": set()},
                }
            },
        )
        binding = row["bindings"][source]
        binding["app_ids"].add(item["app_id"])
        if ruleset_id is not None:
            binding["ruleset_ids"].add(ruleset_id)

    for item in snapshot["classic_branch_protection"][
        "required_status_checks"
    ]:
        add(item, source="classic")

    for ruleset in snapshot["rulesets"]["items"]:
        targeting = ruleset["targeting"]["status"]
        if ruleset["enforcement"] != "active":
            excluded.append(
                {
                    "id": ruleset["id"],
                    "name": ruleset["name"],
                    "reason": "inactive",
                    "enforcement": ruleset["enforcement"],
                    "targeting": targeting,
                }
            )
            continue
        if targeting == "NOT_TARGETED":
            excluded.append(
                {
                    "id": ruleset["id"],
                    "name": ruleset["name"],
                    "reason": "untargeted",
                    "enforcement": ruleset["enforcement"],
                    "targeting": targeting,
                }
            )
            continue
        if targeting == "NOT_PROVEN":
            limitations.append(
                {
                    "code": "ruleset_targeting_not_proven",
                    "message": (
                        f"ruleset {ruleset['id']} ({ruleset['name']}) has "
                        "unsupported or ambiguous default-branch selectors"
                    ),
                }
            )
            continue
        for item in ruleset["required_status_checks"]:
            add(item, source="ruleset", ruleset_id=ruleset["id"])

    union: list[dict[str, Any]] = []
    for context in sorted(rows):
        bindings = rows[context]["bindings"]
        sources = [
            source
            for source in ("classic", "ruleset")
            if bindings[source]["app_ids"]
        ]
        source_bindings = []
        all_app_ids: set[int | None] = set()
        for source in sources:
            app_ids = sorted(
                bindings[source]["app_ids"], key=app_sort
            )
            all_app_ids.update(app_ids)
            source_bindings.append(
                {
                    "source": source,
                    "app_ids": app_ids,
                    "ruleset_ids": sorted(
                        bindings[source]["ruleset_ids"]
                    ),
                }
            )
        union.append(
            {
                "context": context,
                "app_ids": sorted(all_app_ids, key=app_sort),
                "sources": sources,
                "source_class": (
                    "both" if sources == ["classic", "ruleset"] else sources[0]
                ),
                "source_bindings": source_bindings,
            }
        )
    return (
        union,
        sorted(excluded, key=lambda row: row["id"]),
        sorted(limitations, key=lambda row: (row["code"], row["message"])),
    )


def invalid_receipt(message: str) -> dict[str, Any]:
    return {
        "schema_version": RECEIPT_VERSION,
        "status": "NOT_PROVEN",
        "repository": None,
        "observation": None,
        "snapshot_sha256": None,
        "static_contract_subject_sha256": None,
        "surface_states": None,
        "evidence_digests": None,
        "ruleset_inventory": [],
        "live_union": [],
        "excluded_rulesets": [],
        "differences": [],
        "limitations": [{"code": "invalid_input", "message": message}],
    }


def reconcile(snapshot_raw: Any, static_raw: Any) -> dict[str, Any]:
    try:
        snapshot = validate_snapshot(snapshot_raw)
        static = validate_static(static_raw)
    except ContractError as error:
        return invalid_receipt(str(error))

    union, excluded, union_limitations = live_union(snapshot)
    base = {
        "schema_version": RECEIPT_VERSION,
        "repository": snapshot["repository"],
        "observation": snapshot["observation"],
        "snapshot_sha256": digest(snapshot),
        "static_contract_subject_sha256": static.get("subject_sha256"),
        "surface_states": {
            "classic_branch_protection": snapshot[
                "classic_branch_protection"
            ]["instrument_state"],
            "rulesets": snapshot["rulesets"]["instrument_state"],
        },
        "evidence_digests": {
            "classic_branch_protection": snapshot[
                "classic_branch_protection"
            ]["response_sha256"],
            "ruleset_list": snapshot["rulesets"]["list_response_sha256"],
            "ruleset_details": [
                {
                    "id": ruleset["id"],
                    "sha256": ruleset["detail_response_sha256"],
                }
                for ruleset in snapshot["rulesets"]["items"]
            ],
        },
        "ruleset_inventory": snapshot["rulesets"]["items"],
    }
    limitations: list[dict[str, Any]] = list(union_limitations)

    if static["status"] != "SUCCESS":
        limitations.append(
            {
                "code": "static_contract_not_success",
                "message": f"static contract status is {static['status']}",
            }
        )
    else:
        identities = (
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
        for code, observed, expected in identities:
            if observed != expected:
                limitations.append(
                    {
                        "code": code,
                        "message": f"observed={observed}, expected={expected}",
                    }
                )

    if (
        snapshot["classic_branch_protection"]["branch"]
        != snapshot["repository"]["default_branch"]
    ):
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
                "message": (
                    "permission is "
                    f"{snapshot['observation']['permission']}"
                ),
            }
        )
    if snapshot["observation"]["source"] not in LIVE_OBSERVATION_SOURCES:
        limitations.append(
            {
                "code": "non_live_observation_source",
                "message": (
                    f"source {snapshot['observation']['source']} "
                    "cannot establish current live enforcement"
                ),
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
                limitations,
                key=lambda row: (row["code"], row["message"]),
            ),
        }

    expected: dict[str, dict[str, Any]] = {}
    static_by_name = {row["name"]: row for row in static["contexts"]}
    differences: list[dict[str, Any]] = []
    for row in static["contexts"]:
        if row["policy_role"] != "required":
            continue
        if row["enforcement"] not in ENFORCEMENT_SOURCES:
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
        wanted_sources = ENFORCEMENT_SOURCES[wanted["enforcement"]]
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

        if "app_id" in wanted:
            bindings = {
                binding["source"]: binding["app_ids"]
                for binding in actual["source_bindings"]
            }
            mismatched_sources = {
                source: bindings.get(source, [])
                for source in sorted(wanted_sources)
                if wanted["app_id"] not in bindings.get(source, [])
            }
            if mismatched_sources:
                differences.append(
                    {
                        "code": "app_identity_mismatch",
                        "context": name,
                        "expected": {
                            source: wanted["app_id"]
                            for source in sorted(wanted_sources)
                        },
                        "observed": mismatched_sources,
                    }
                )

    for name in sorted(set(observed) - set(expected)):
        static_row = static_by_name.get(name)
        if static_row is not None and static_row["policy_role"] == "required":
            # An unsupported required enforcement already emitted its specific
            # difference above; do not misclassify the same row as a role drift.
            continue
        if static_row is None:
            code = "live_context_missing_from_policy"
            expected_value: Any = None
        else:
            code = "live_context_role_mismatch"
            expected_value = static_row["policy_role"]
        differences.append(
            {
                "code": code,
                "context": name,
                "expected": expected_value,
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
    lines = [
        f"GitHub enforcement union: "
        f"{receipt.get('status', 'NOT_PROVEN')}"
    ]
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
        lines.append(
            "- checked-in required contexts match the complete live union"
        )
    return "\n".join(lines)


def write_receipt(path: Path | None, result: dict[str, Any]) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


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

    if args.command == "validate":
        try:
            normalized = validate_snapshot(read_json(args.snapshot))
        except ContractError as error:
            print(
                "GitHub enforcement union: NOT_PROVEN\n"
                f"- invalid_input: {error}",
                file=sys.stderr,
            )
            return 1
        print(json.dumps(normalized, indent=2, sort_keys=True))
        return 0

    if args.command == "reconcile":
        try:
            snapshot_raw = read_json(args.snapshot)
            static_raw = read_json(args.static_receipt)
            result = reconcile(snapshot_raw, static_raw)
        except ContractError as error:
            result = invalid_receipt(str(error))
        write_receipt(args.receipt, result)
        print(explain(result))
        return 0 if result["status"] == "MATCH" else 1

    try:
        receipt = obj(read_json(args.receipt), "receipt")
    except ContractError as error:
        print(
            "GitHub enforcement union: NOT_PROVEN\n"
            f"- invalid_input: {error}",
            file=sys.stderr,
        )
        return 1
    print(explain(receipt))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
