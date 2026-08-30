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
    dependency_universe,
    discover_consumer_contexts,
    feature_isolation,
    feature_names,
    incremental_surface,
    load_toml,
    rust_public_surface,
)

SCHEMA_VERSION = 1
ALLOWED_CLASSIFICATIONS = {
    "parser_kernel", "parser_output_contract", "incremental_parser",
    "compatibility_reexport", "product_composition", "workspace_or_lsp_adapter",
    "experimental", "retire", "test_dev_only",
}
ALLOWED_DISPOSITIONS = {"retain", "move", "gate", "deprecate", "remove", "review"}
ALLOWED_DEPENDENCY_CONTEXTS = {
    "normal", "dev", "build", "target:normal", "target:dev", "target:build",
}
PRODUCTION_DEPENDENCY_CONTEXTS = {"normal", "build", "target:normal", "target:build"}
ALLOWED_FEATURE_ISOLATIONS = {
    "dependencies_and_source", "dependencies_only", "source_only",
    "test_source_only", "feature_aggregate", "taxonomy_only",
}
PRODUCTION_FEATURE_ISOLATIONS = {
    "dependencies_and_source", "dependencies_only", "source_only",
}
ALLOWED_CONSUMER_USAGES = {"production", "dev_only", "mixed"}
PENDING_FIELDS = ("owner", "predecessor", "reason", "resolves_when")
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


def require_issue_reference(value: str, context: str) -> None:
    if not value.startswith("#") or not value[1:].isdigit():
        raise ValueError(f"{context} must be a GitHub issue reference")


def validate_pending(item: dict[str, Any], disposition: str, context: str) -> None:
    """A pending row must name its owner, predecessor, reason, and resolving event.

    Without all four a `review` row is an ownerless bucket that no later leaf can
    mechanically consume, so the ledger rejects it.
    """
    pending = item.get("pending")
    if disposition != "review":
        if pending is not None:
            raise ValueError(f"{context}.pending is only valid for a review disposition")
        return
    if not isinstance(pending, dict):
        raise ValueError(f"{context}.pending must be an object for a review disposition")
    unknown = sorted(set(pending) - set(PENDING_FIELDS))
    if unknown:
        raise ValueError(f"{context}.pending has unsupported fields: {','.join(unknown)}")
    for field in PENDING_FIELDS:
        require_string(pending, field, f"{context}.pending")
    require_issue_reference(pending["owner"], f"{context}.pending.owner")
    predecessor = pending["predecessor"]
    if predecessor != "none":
        require_issue_reference(predecessor, f"{context}.pending.predecessor")


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
    require_issue_reference(item["owner"], f"{context}.owner")
    validate_pending(item, disposition, context)
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
    observed_isolation = feature_isolation(manifest, manifest_path.parent)
    for name, row in features.items():
        if bool(row.get("default")) != (name in expected_defaults):
            raise ValueError(f"feature {name} has inconsistent default disposition")
        if row["classification"] == "experimental" and name in expected_defaults:
            raise ValueError(f"experimental feature {name} cannot be a default")
        isolation = require_string(row, "isolation", f"features[{name}]")
        if isolation not in ALLOWED_FEATURE_ISOLATIONS:
            raise ValueError(f"feature {name} isolation is unsupported: {isolation}")
        if isolation != observed_isolation[name]:
            raise ValueError(
                f"feature {name} claims isolation {isolation} but selects "
                f"{observed_isolation[name]} in current source and manifest"
            )

    dependencies = table_by_name(ledger.get("dependencies"), "dependencies")
    observed_dependencies = dependency_universe(manifest)
    validate_exact("Cargo dependencies", set(observed_dependencies), set(dependencies))
    for name, fact in observed_dependencies.items():
        row = dependencies[name]
        if row.get("optional") is not fact.optional:
            raise ValueError(f"dependency {name} optionality differs from authority ledger")
        contexts = row.get("contexts")
        if not isinstance(contexts, list) or any(not isinstance(x, str) for x in contexts):
            raise ValueError(f"dependency {name} must record a contexts string list")
        unsupported = sorted(set(contexts) - ALLOWED_DEPENDENCY_CONTEXTS)
        if unsupported:
            raise ValueError(f"dependency {name} has unsupported contexts: {','.join(unsupported)}")
        if contexts != sorted(set(contexts)):
            raise ValueError(f"dependency {name} contexts must be unique and sorted")
        if tuple(contexts) != fact.contexts:
            raise ValueError(
                f"dependency {name} claims contexts {contexts} but is declared in "
                f"{list(fact.contexts)}"
            )
        if row["classification"] == "test_dev_only" and (
            set(contexts) & PRODUCTION_DEPENDENCY_CONTEXTS
        ):
            raise ValueError(
                f"dependency {name} is classified test_dev_only but is reachable from "
                "a production dependency context"
            )

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
    observed_consumers = discover_consumer_contexts(root, manifest_path)
    validate_exact("workspace consumers", set(observed_consumers), set(consumers))
    for group in ledger["consumer_groups"]:
        usage = require_string(group, "usage", f"consumer_groups[{group['name']}]")
        if usage not in ALLOWED_CONSUMER_USAGES:
            raise ValueError(f"consumer group {group['name']} usage is unsupported: {usage}")
        production = {
            member
            for member in group["members"]
            if set(observed_consumers[member]) & PRODUCTION_DEPENDENCY_CONTEXTS
        }
        if production == set(group["members"]):
            observed_usage = "production"
        elif not production:
            observed_usage = "dev_only"
        else:
            observed_usage = "mixed"
        if usage != observed_usage:
            raise ValueError(
                f"consumer group {group['name']} claims {usage} usage but reaches the "
                f"facade as {observed_usage}"
            )

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
        "production_dependencies": sum(
            1 for fact in observed_dependencies.values()
            if set(fact.contexts) & PRODUCTION_DEPENDENCY_CONTEXTS
        ),
        "dev_only_dependencies": sum(
            1 for fact in observed_dependencies.values()
            if not set(fact.contexts) & PRODUCTION_DEPENDENCY_CONTEXTS
        ),
        "test_profile_features": sorted(
            name for name, isolation in observed_isolation.items()
            if isolation == "test_source_only"
        ),
        "taxonomy_only_features": sorted(
            name for name, isolation in observed_isolation.items()
            if isolation == "taxonomy_only"
        ),
        "production_boundary_features": sorted(
            name for name, isolation in observed_isolation.items()
            if isolation in PRODUCTION_FEATURE_ISOLATIONS
        ),
        "pending_rows": sum(
            1 for section in ledger.values() if isinstance(section, list)
            for row in section if isinstance(row, dict) and row.get("disposition") == "review"
        ),
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
        f"- Declared dependencies: {summary['dependencies']}",
        f"- Production-context dependencies: {summary['production_dependencies']}",
        f"- Development-only dependencies: {summary['dev_only_dependencies']}",
        f"- Workspace consumers: {summary['consumers']}",
        f"- Pending rows awaiting a named owner: {summary['pending_rows']}", "",
        "## Feature isolation", "",
        "A declared feature is a production boundary only when it selects dependencies or",
        "gates `src/`. A feature that gates only test, bench, or example source is a test",
        "profile, and a feature that gates nothing is taxonomy. Neither may be presented as",
        "an architectural boundary.", "",
        f"Production boundaries ({len(summary['production_boundary_features'])}): "
        + ", ".join(f"`{name}`" for name in summary["production_boundary_features"]) + ".", "",
        f"Test profiles ({len(summary['test_profile_features'])}): "
        + ", ".join(f"`{name}`" for name in summary["test_profile_features"]) + ".", "",
        f"Taxonomy only, isolating nothing ({len(summary['taxonomy_only_features'])}): "
        + ", ".join(f"`{name}`" for name in summary["taxonomy_only_features"]) + ".", "",
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
