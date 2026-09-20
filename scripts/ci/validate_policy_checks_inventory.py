#!/usr/bin/env python3
"""Validate the source-derived policy_checks member inventory.

This validator owns inventory truth only. It does not execute, split, route,
activate, quarantine, or retire any policy_checks member.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

DEFAULT_GATE_POLICY = Path(".ci/gate-policy.yaml")
DEFAULT_INVENTORY = Path(".ci/policy-checks-inventory.json")
DEFAULT_DOC = Path("docs/ci/policy-checks-inventory.md")

ALLOWED_OVERLAP_DISPOSITIONS = {
    "authoritative_elsewhere",
    "intentional_fanout",
    "migrate_here",
    "obsolete",
    "not_proven",
}
STABLE_ID_RE = re.compile(r"^[a-z0-9_]+$")


class ValidationError(ValueError):
    """Raised when the checked inventory cannot establish its claim."""


def _normalize_command(command: str) -> str:
    return " ".join(command.split())


def extract_policy_checks(gate_policy_text: str) -> dict[str, Any]:
    gate_match = re.search(
        r"(?ms)^  - name: policy_checks\n(?P<body>.*?)(?=^  - name: |\Z)",
        gate_policy_text,
    )
    if gate_match is None:
        raise ValidationError("gate-policy.yaml must contain exactly one policy_checks gate")

    if len(re.findall(r"(?m)^  - name: policy_checks$", gate_policy_text)) != 1:
        raise ValidationError("gate-policy.yaml must contain exactly one policy_checks gate")

    body = gate_match.group("body")

    def scalar(name: str) -> str:
        match = re.search(rf"(?m)^    {re.escape(name)}: (?P<value>.+)$", body)
        if match is None:
            raise ValidationError(f"policy_checks is missing {name}")
        return match.group("value").strip()

    command_match = re.search(
        r"(?m)^    command: >-\n(?P<command>(?:^      [^\n]*(?:\n|\Z))+)",
        body,
    )
    if command_match is None:
        raise ValidationError("policy_checks command must use a block scalar")

    command_lines = [
        line.strip()
        for line in command_match.group("command").splitlines()
        if line.strip()
    ]
    joined = " ".join(command_lines)
    if "||" in joined or ";" in joined:
        raise ValidationError(
            "policy_checks inventory parser only accepts the current ordered && chain"
        )
    commands = [
        _normalize_command(command)
        for command in re.split(r"\s*&&\s*", joined)
        if command.strip()
    ]
    if not commands:
        raise ValidationError("policy_checks command chain is empty")

    required_raw = scalar("required").split("#", 1)[0].strip()
    if required_raw not in {"true", "false"}:
        raise ValidationError(f"unsupported policy_checks required value: {required_raw}")

    budget_match = re.search(
        r"(?m)^    budgets:\n(?P<body>(?:^      [^\n]*(?:\n|\Z))+)",
        body,
    )
    if budget_match is None:
        raise ValidationError("policy_checks is missing budgets")
    max_duration_match = re.search(
        r"(?m)^      max_duration_ms: (?P<value>\d+)$",
        budget_match.group("body"),
    )
    if max_duration_match is None:
        raise ValidationError("policy_checks is missing budgets.max_duration_ms")

    return {
        "gate_id": "policy_checks",
        "tier": scalar("tier"),
        "required": required_raw == "true",
        "timeout_seconds": int(scalar("timeout_seconds")),
        "budget_ms": int(max_duration_match.group("value")),
        "working_directory": "repository_root",
        "execution_semantics": "ordered_aborting_shell_chain",
        "commands": commands,
        "command_fingerprint_sha256": hashlib.sha256(
            "\n".join(commands).encode("utf-8")
        ).hexdigest(),
    }


def load_inventory(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ValidationError(f"inventory missing: {path}") from error
    except json.JSONDecodeError as error:
        raise ValidationError(f"inventory is not valid JSON: {error}") from error
    if not isinstance(data, dict):
        raise ValidationError("inventory root must be an object")
    return data


def canonical_inventory_text(inventory: dict[str, Any]) -> str:
    return json.dumps(inventory, indent=2, sort_keys=True) + "\n"


def validate_inventory(
    inventory: dict[str, Any], source: dict[str, Any], *, raw_text: str | None = None
) -> list[str]:
    errors: list[str] = []

    if inventory.get("schema_version") != 1:
        errors.append("schema_version must be 1")

    source_record = inventory.get("source")
    if not isinstance(source_record, dict):
        errors.append("source must be an object")
        source_record = {}

    expected_source = {
        "path": ".ci/gate-policy.yaml",
        "gate_id": source["gate_id"],
        "tier": source["tier"],
        "required": source["required"],
        "timeout_seconds": source["timeout_seconds"],
        "budget_ms": source["budget_ms"],
        "working_directory": source["working_directory"],
        "execution_semantics": source["execution_semantics"],
        "command_fingerprint_sha256": source["command_fingerprint_sha256"],
    }
    for key, expected in expected_source.items():
        if source_record.get(key) != expected:
            errors.append(
                f"source.{key} must equal current policy value {expected!r}, "
                f"got {source_record.get(key)!r}"
            )

    claim_boundary = inventory.get("claim_boundary")
    if not isinstance(claim_boundary, str) or not claim_boundary.strip():
        errors.append("claim_boundary must be a non-empty string")

    members = inventory.get("members")
    if not isinstance(members, list):
        errors.append("members must be a list")
        members = []

    if len(members) != len(source["commands"]):
        errors.append(
            "inventory member count must equal current policy_checks command count "
            f"({len(members)} != {len(source['commands'])})"
        )

    ids: set[str] = set()
    commands: set[str] = set()
    inventory_commands: list[str] = []

    for index, member in enumerate(members, start=1):
        prefix = f"members[{index - 1}]"
        if not isinstance(member, dict):
            errors.append(f"{prefix} must be an object")
            continue

        if member.get("position") != index:
            errors.append(f"{prefix}.position must be {index}")

        stable_id = member.get("stable_id")
        if not isinstance(stable_id, str) or not STABLE_ID_RE.fullmatch(stable_id):
            errors.append(f"{prefix}.stable_id must match {STABLE_ID_RE.pattern}")
        elif stable_id in ids:
            errors.append(f"duplicate stable_id: {stable_id}")
        else:
            ids.add(stable_id)

        command = member.get("command")
        if not isinstance(command, str) or not command.strip():
            errors.append(f"{prefix}.command must be a non-empty string")
        else:
            command = _normalize_command(command)
            inventory_commands.append(command)
            if command in commands:
                errors.append(f"duplicate member command: {command}")
            commands.add(command)
            if index <= len(source["commands"]) and command != source["commands"][index - 1]:
                errors.append(
                    f"{prefix}.command does not match current source order: "
                    f"{command!r} != {source['commands'][index - 1]!r}"
                )

        for field in ("owner", "claim", "selector", "working_directory"):
            value = member.get(field)
            if not isinstance(value, str) or not value.strip():
                errors.append(f"{prefix}.{field} must be a non-empty string")

        if member.get("profile") != "merge_gate":
            errors.append(f"{prefix}.profile must remain merge_gate")
        if member.get("policy_role") != "advisory":
            errors.append(f"{prefix}.policy_role must remain advisory")
        if member.get("working_directory") != "repository_root":
            errors.append(f"{prefix}.working_directory must be repository_root")

        timeout = member.get("timeout")
        if timeout != {"kind": "shared_composite", "seconds": source["timeout_seconds"]}:
            errors.append(
                f"{prefix}.timeout must retain the shared composite timeout "
                f"of {source['timeout_seconds']} seconds"
            )

        receipt = member.get("receipt")
        if receipt != {"status": "composite_only", "schema": None}:
            errors.append(
                f"{prefix}.receipt must state composite_only with no independent schema"
            )

        overlap = member.get("overlap")
        if not isinstance(overlap, dict):
            errors.append(f"{prefix}.overlap must be an object")
            continue
        disposition = overlap.get("disposition")
        if disposition not in ALLOWED_OVERLAP_DISPOSITIONS:
            errors.append(
                f"{prefix}.overlap.disposition must be one of "
                f"{sorted(ALLOWED_OVERLAP_DISPOSITIONS)}"
            )
        targets = overlap.get("targets")
        if not isinstance(targets, list) or not all(
            isinstance(target, str) and target.strip() for target in targets
        ):
            errors.append(f"{prefix}.overlap.targets must be a list of non-empty strings")
            targets = []
        if disposition in {"authoritative_elsewhere", "intentional_fanout"} and not targets:
            errors.append(f"{prefix}.overlap.targets is required for {disposition}")
        reason = overlap.get("reason")
        if not isinstance(reason, str) or not reason.strip():
            errors.append(f"{prefix}.overlap.reason must be a non-empty string")

    if inventory_commands != source["commands"]:
        errors.append("inventory command sequence must exactly match policy_checks")

    if raw_text is not None and raw_text != canonical_inventory_text(inventory):
        errors.append("inventory JSON is not canonical (sorted keys, two-space indent, newline)")

    return errors


def markdown_projection(inventory: dict[str, Any]) -> str:
    source = inventory["source"]
    lines = [
        "# `policy_checks` member inventory",
        "",
        "<!-- Generated by scripts/ci/validate_policy_checks_inventory.py. -->",
        "",
        "This projection inventories the current composite. It does not execute, "
        "route, activate, quarantine, or retire any member.",
        "",
        f"- Source: `{source['path']}#{source['gate_id']}`",
        f"- Source fingerprint: `{source['command_fingerprint_sha256']}`",
        f"- Current role: `{'required' if source['required'] else 'advisory'}` "
        f"in `{source['tier']}`",
        f"- Execution: `{source['execution_semantics']}` from "
        f"`{source['working_directory']}`",
        f"- Shared timeout/budget: `{source['timeout_seconds']}s` / "
        f"`{source['budget_ms']}ms`",
        "",
        "| # | Stable ID | Command | Owner | Claim | Overlap disposition |",
        "|---:|---|---|---|---|---|",
    ]
    for member in inventory["members"]:
        def cell(value: str) -> str:
            return value.replace("|", r"\|").replace("\n", " ")
        overlap = member["overlap"]
        targets = ", ".join(overlap["targets"]) if overlap["targets"] else "none"
        disposition = f"{overlap['disposition']} ({targets})"
        lines.append(
            "| {position} | `{stable_id}` | `{command}` | {owner} | {claim} | "
            "`{disposition}` |".format(
                position=member["position"],
                stable_id=cell(member["stable_id"]),
                command=cell(member["command"]).replace("`", r"\`"),
                owner=cell(member["owner"]),
                claim=cell(member["claim"]),
                disposition=cell(disposition),
            )
        )
    lines.extend(
        [
            "",
            "Every row currently inherits the composite timeout and has only a "
            "`composite_only` receipt. #9436 owns named obligation contracts and "
            "independent result identities.",
            "",
        ]
    )
    return "\n".join(lines)


def check_paths(
    gate_policy_path: Path, inventory_path: Path, doc_path: Path
) -> list[str]:
    try:
        source = extract_policy_checks(gate_policy_path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        return [f"gate policy missing: {gate_policy_path}: {error}"]
    except ValidationError as error:
        return [str(error)]

    try:
        raw_inventory = inventory_path.read_text(encoding="utf-8")
        inventory = load_inventory(inventory_path)
    except ValidationError as error:
        return [str(error)]

    errors = validate_inventory(inventory, source, raw_text=raw_inventory)
    expected_doc = markdown_projection(inventory)
    try:
        actual_doc = doc_path.read_text(encoding="utf-8")
    except FileNotFoundError:
        errors.append(f"generated projection missing: {doc_path}")
    else:
        if actual_doc != expected_doc:
            errors.append(
                f"generated projection drift: run "
                f"`python3 {Path(__file__).as_posix()} --write-doc`"
            )
    return errors


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--gate-policy", type=Path, default=DEFAULT_GATE_POLICY)
    parser.add_argument("--inventory", type=Path, default=DEFAULT_INVENTORY)
    parser.add_argument("--doc", type=Path, default=DEFAULT_DOC)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--write-doc", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.write_doc:
        try:
            source = extract_policy_checks(args.gate_policy.read_text(encoding="utf-8"))
            inventory = load_inventory(args.inventory)
        except (FileNotFoundError, ValidationError) as error:
            print(f"FAIL: {error}", file=sys.stderr)
            return 1
        errors = validate_inventory(
            inventory,
            source,
            raw_text=args.inventory.read_text(encoding="utf-8"),
        )
        if errors:
            for error in errors:
                print(f"FAIL: {error}", file=sys.stderr)
            return 1
        args.doc.parent.mkdir(parents=True, exist_ok=True)
        args.doc.write_text(markdown_projection(inventory), encoding="utf-8")
        print(f"WROTE: {args.doc}")
        return 0

    errors = check_paths(args.gate_policy, args.inventory, args.doc)
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1

    inventory = load_inventory(args.inventory)
    print(
        "OK: policy_checks inventory matches "
        f"{len(inventory['members'])} current ordered members"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
