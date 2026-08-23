"""Validated staged authority model for the perl-parser facade (#7058)."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

from parser_facade_inventory import (
    CargoTarget,
    FORBIDDEN_KERNEL_DEPENDENCY_TOKENS,
    cargo_targets,
    default_features,
    dependency_rows,
    discover_consumers,
    feature_names,
    incremental_surface,
    load_toml,
    rust_public_surface,
)

SCHEMA_VERSION = 1
ALLOWED_CLASSIFICATIONS = {
    "parser_kernel", "parser_output_contract", "incremental_parser",
    "compatibility_reexport", "product_composition", "workspace_or_lsp_adapter",
    "experimental", "retire",
}
ALLOWED_DISPOSITIONS = {"retain", "move", "gate", "deprecate", "remove", "review"}
LEDGER_FILES = (
    "ruling.json", "features.json", "dependencies.json", "public-surface.json",
    "incremental.json", "consumers.json",
)
CANONICAL_SOURCE_PATHS = {
    "manifest": "crates/perl-parser/Cargo.toml",
    "lib": "crates/perl-parser/src/lib.rs",
    "incremental": "crates/perl-parser/src/incremental/mod.rs",
    "parser_core_manifest": "crates/perl-parser-core/Cargo.toml",
    "generated_doc": "docs/project/PARSER_FACADE_AUTHORITY.md",
}


def load_json(path: Path) -> dict[str, Any]:
    try:
        value: Any = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read {path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def load_ledger(path: Path) -> dict[str, Any]:
    if not path.is_dir():
        raise ValueError(f"parser facade ledger directory does not exist: {path}")
    merged: dict[str, Any] = {"schema_version": SCHEMA_VERSION}
    for name in LEDGER_FILES:
        payload = load_json(path / name)
        if payload.get("schema_version") != SCHEMA_VERSION:
            raise ValueError(f"unsupported schema in {path / name}")
        for key, value in payload.items():
            if key == "schema_version":
                continue
            if key in merged:
                raise ValueError(f"duplicate parser facade ledger key {key!r} in {name}")
            merged[key] = value
    return merged


def require_string(item: dict[str, Any], key: str, context: str) -> str:
    value = item.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{context}.{key} must be a non-empty string")
    return value


def validate_row(item: Any, context: str) -> dict[str, Any]:
    if not isinstance(item, dict):
        raise ValueError(f"{context} must be an object")
    classification = require_string(item, "classification", context)
    disposition = require_string(item, "disposition", context)
    require_string(item, "owner", context)
    require_string(item, "target_owner", context)
    require_string(item, "exit_condition", context)
    if classification not in ALLOWED_CLASSIFICATIONS:
        raise ValueError(f"{context}.classification is unsupported: {classification}")
    if disposition not in ALLOWED_DISPOSITIONS:
        raise ValueError(f"{context}.disposition is unsupported: {disposition}")
    if not item["owner"].startswith("#") or not item["owner"][1:].isdigit():
        raise ValueError(f"{context}.owner must be a GitHub issue reference")
    return item


def table_by_name(value: Any, context: str) -> dict[str, dict[str, Any]]:
    if not isinstance(value, list):
        raise ValueError(f"{context} must be a list")
    result: dict[str, dict[str, Any]] = {}
    for index, raw_item in enumerate(value):
        item = validate_row(raw_item, f"{context}[{index}]")
        name = require_string(item, "name", f"{context}[{index}]")
        if name in result:
            raise ValueError(f"duplicate {context} name: {name}")
        result[name] = item
    return result


def member_table(value: Any, context: str) -> dict[str, dict[str, Any]]:
    if not isinstance(value, list):
        raise ValueError(f"{context} must be a list")
    result: dict[str, dict[str, Any]] = {}
    groups: set[str] = set()
    for index, raw_item in enumerate(value):
        item = validate_row(raw_item, f"{context}[{index}]")
        group = require_string(item, "name", f"{context}[{index}]")
        if group in groups:
            raise ValueError(f"duplicate {context} group name: {group}")
        groups.add(group)
        members = item.get("members")
        if not isinstance(members, list) or not members or any(not isinstance(x, str) or not x for x in members):
            raise ValueError(f"{context}[{index}].members must be a non-empty string list")
        if members != sorted(set(members)):
            raise ValueError(f"{context}[{index}].members must be unique and sorted")
        for member in members:
            if member in result:
                raise ValueError(f"duplicate {context} member: {member}")
            result[member] = item
    return result


def validate_exact(label: str, observed: set[str], expected: set[str]) -> None:
    missing = sorted(expected - observed)
    added = sorted(observed - expected)
    if missing or added:
        details = []
        if missing:
            details.append("missing=" + ",".join(missing))
        if added:
            details.append("unclassified=" + ",".join(added))
        raise ValueError(f"{label} differs from authority ledger: {'; '.join(details)}")


def normalized_json(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":")).encode()


def check(root: Path, ledger_path: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    ledger = load_ledger(ledger_path)
    ruling = ledger.get("ruling")
    if not isinstance(ruling, dict) or ruling.get("model") != "staged_migration":
        raise ValueError("parser facade ruling must be staged_migration")
    if ruling.get("controller") != "#2477" or ruling.get("implementation_issue") != "#7058":
        raise ValueError("parser facade ruling issue identities are invalid")

    if ledger.get("sources") != CANONICAL_SOURCE_PATHS:
        raise ValueError("parser facade governed source paths differ from canonical paths")

    manifest_path = root / CANONICAL_SOURCE_PATHS["manifest"]
    manifest = load_toml(manifest_path)
    features = table_by_name(ledger.get("features"), "features")
    validate_exact("Cargo features", feature_names(manifest), set(features))
    expected_defaults = tuple(ledger.get("default_features", []))
    if default_features(manifest) != expected_defaults:
        raise ValueError("default feature order differs from authority ledger")
    for name, row in features.items():
        if bool(row.get("default")) != (name in expected_defaults):
            raise ValueError(f"feature {name} has inconsistent default disposition")
        if row["classification"] == "experimental" and name in expected_defaults:
            raise ValueError(f"experimental feature {name} cannot be a default")

    dependencies = table_by_name(ledger.get("dependencies"), "dependencies")
    observed_dependencies = dependency_rows(manifest)
    validate_exact("Cargo dependencies", set(observed_dependencies), set(dependencies))
    for name, optional in observed_dependencies.items():
        if dependencies[name].get("optional") is not optional:
            raise ValueError(f"dependency {name} optionality differs from authority ledger")

    modules, exports = rust_public_surface(root / CANONICAL_SOURCE_PATHS["lib"])
    public_modules = table_by_name(ledger.get("public_modules"), "public_modules")
    public_exports = member_table(ledger.get("public_reexport_groups"), "public_reexport_groups")
    validate_exact("public modules", modules, set(public_modules))
    validate_exact("public re-exports", exports, set(public_exports))

    expected_targets: set[CargoTarget] = set()
    for index, raw_target in enumerate(ledger.get("targets", [])):
        item = validate_row(raw_target, f"targets[{index}]")
        kind = require_string(item, "kind", f"targets[{index}]")
        name = require_string(item, "name", f"targets[{index}]")
        required = item.get("required_features", [])
        if not isinstance(required, list) or any(not isinstance(x, str) for x in required):
            raise ValueError(f"targets[{index}].required_features must be a string list")
        expected_targets.add(CargoTarget(kind, name, tuple(required)))
    if cargo_targets(manifest) != expected_targets:
        raise ValueError("Cargo bin/bench/example targets differ from authority ledger")

    inc_modules, inc_exports, inc_functions = incremental_surface(root / CANONICAL_SOURCE_PATHS["incremental"])
    module_rows = table_by_name(ledger.get("incremental_public_modules"), "incremental_public_modules")
    export_rows = table_by_name(ledger.get("incremental_public_exports"), "incremental_public_exports")
    validate_exact("incremental public modules", inc_modules, set(module_rows))
    validate_exact("incremental public exports", inc_exports, set(export_rows))
    expected_functions = set(ledger.get("incremental_public_functions", []))
    validate_exact("incremental public functions", inc_functions, expected_functions)
    if any(row.get("production_eligible") is True for row in module_rows.values()):
        raise ValueError("historical incremental modules cannot be production-eligible")
    production_exports = sorted(name for name, row in export_rows.items() if row.get("production_eligible") is True)
    if production_exports != [
        "diagnostics::LexRestartReport",
        "diagnostics::LexRestartStrategy",
        "diagnostics::ReparseResult",
        "edit::Edit",
        "snapshot::ParseGeneration",
        "snapshot::ParseSnapshot",
        "snapshot::ParseSnapshotStrategy",
        "snapshot::ParseSnapshotValidationError",
        "snapshot::ParseTerminalDisposition",
        "state::IncrementalState",
    ]:
        raise ValueError("canonical incremental export marker differs from reviewed authority")
    if expected_functions != {"apply_edits"}:
        raise ValueError("apply_edits must be the sole canonical public incremental function")

    consumers = member_table(ledger.get("consumer_groups"), "consumer_groups")
    observed_consumers = discover_consumers(root, manifest_path)
    validate_exact("workspace consumers", observed_consumers, set(consumers))

    parser_core = load_toml(root / CANONICAL_SOURCE_PATHS["parser_core_manifest"])
    forbidden = sorted(name for name in dependency_rows(parser_core) if any(token in name for token in FORBIDDEN_KERNEL_DEPENDENCY_TOKENS))
    if forbidden:
        raise ValueError("parser-core has forbidden product/transport dependencies: " + ",".join(forbidden))

    summary = {
        "schema_version": SCHEMA_VERSION,
        "ruling": "staged_migration",
        "features": len(features),
        "default_features": list(expected_defaults),
        "dependencies": len(dependencies),
        "public_modules": len(public_modules),
        "public_reexports": len(public_exports),
        "incremental_public_modules": len(module_rows),
        "incremental_public_exports": len(export_rows),
        "consumers": len(consumers),
        "digest_scope": "full_normalized_ledger",
    }
    summary["authority_digest"] = hashlib.sha256(normalized_json(ledger)).hexdigest()
    return ledger, summary


def render_markdown(ledger: dict[str, Any], summary: dict[str, Any]) -> str:
    lines = [
        "<!-- Generated by scripts/ci/check_parser_facade_authority.py; do not edit by hand. -->",
        "# perl-parser facade authority", "",
        "`perl-parser` uses the staged-migration model accepted by #2477 and recorded by #7058.",
        "The native parser contract remains directly available while compatibility and product-composition surfaces receive explicit owners and exits.", "",
        "## Current boundary", "",
        f"- Authority digest: `{summary['authority_digest']}`",
        f"- Digest input: `{summary['digest_scope']}`",
        f"- Public modules: {summary['public_modules']}",
        f"- Public re-exports: {summary['public_reexports']}",
        f"- Cargo features: {summary['features']}",
        f"- Direct dependencies: {summary['dependencies']}",
        f"- Workspace consumers: {summary['consumers']}", "",
        "## Dependency direction", "", "```text", "perl-lexer", "    ↓",
        "perl-parser-core          canonical parser kernel", "    ↓",
        "perl-parser               parser facade + bounded compatibility", "    ↓",
        "workspace / semantic / refactor / LSP product adapters", "```", "",
        "`perl-parser-core` may not depend on LSP, VS Code, DAP, workspace orchestration, or provider implementations.", "",
        "## Default features", "",
    ]
    for feature in ledger["default_features"]:
        row = next(item for item in ledger["features"] if item["name"] == feature)
        lines.append(f"- `{feature}` — {row['classification']}; {row['exit_condition']}")
    lines += [
        "", "## Incremental authority", "",
        "The only production API marker is `Edit` + `IncrementalState` + `apply_edits`, with `ReparseResult` as its result contract.",
        "Historical named generations remain non-production until #6701/#6971/#6975 implement their disposition.", "",
        "## Next implementation PRs", "",
        "1. #7063 implements the staged boundary and compatibility gates.",
        "2. #7065 makes supported feature/API/dependency/downstream matrices load-bearing.",
        "3. #6701/#6971/#6975 converge the incremental implementation and public surface.", "",
    ]
    return "\n".join(lines)
