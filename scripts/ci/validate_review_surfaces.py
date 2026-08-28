#!/usr/bin/env python3
"""Validate the sensitive-authority review-surface denominator.

Checks policy/review-surfaces.toml (issue #11793):

  - closed vocabularies: families, review profiles, risk classes,
    required-evidence kinds, routing kinds, lens/role sets reused from
    schemas/agent_review_packet.v1.schema.json (#10881);
  - every surface row carries controller, conflict key, review profile,
    first falsifier, enforcement successor, and a routing disposition;
  - every bound path exists, binds exactly one surface, never through a broad
    top-level glob, and no path is claimed by contradictory review profiles;
  - the closed sensitive-path detector set is totally covered by bindings
    (unclassified sensitive authority fails);
  - the manifest binds its own validator, fixtures, projection, and the
    CODEOWNERS routing declaration;
  - code-owner identities referenced here or in .github/CODEOWNERS carry
    platform validation evidence; invalid or unproven owners fail closed;
  - the generated human projection docs/policy/REVIEW_SURFACES.md matches the
    manifest byte-for-byte (--check-projection) and regenerates deterministically.

This is a fail-closed contract checker: it does not enforce GitHub policy and
it is never itself proof that review happened.

Usage:
  scripts/ci/validate_review_surfaces.py [--strict] [--check-projection]
      [--write-projection] [--manifest PATH] [--root PATH]
      [--codeowners PATH] [--projection PATH]

Exit codes:
  0 - denominator consistent (and projection current when checked)
  1 - one or more typed invariants failed
"""
from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path
from typing import Any

DEFAULT_MANIFEST = Path("policy/review-surfaces.toml")
DEFAULT_PROJECTION = Path("docs/policy/REVIEW_SURFACES.md")
DEFAULT_CODEOWNERS = Path(".github/CODEOWNERS")

# Closed vocabularies. These mirror policy/review-surfaces.toml and the closed
# lens/role vocabulary of schemas/agent_review_packet.v1.schema.json (#10881).
# Drift in either direction fails: unknown values here reject the manifest;
# renamed vocabulary there fails this constant set.
FAMILIES = (
    "semantic_issue_completion",
    "configuration_authority",
    "executable_policy_and_public_migration",
)

RISK_CLASSES = (
    "semantic_control",
    "configuration_control",
    "executable_policy_control",
    "public_surface_control",
    "live_repository_policy_control",
)

# Existing review machinery, by name. A required-evidence class that does not
# reference one of these kinds is an invented review kind and fails.
EVIDENCE_KINDS = (
    "current_head_reviewer_packet",
    "trusted_workflow_run",
    "checked_projection",
)

ROUTE_KINDS = ("validated_pattern", "not_proven", "alternate_route")

LENSES = (
    "semantic_correctness",
    "architecture_authority_duplication",
    "subject_evidence_identity",
    "lifecycle_currentness_concurrency",
    "security_trust_boundary",
    "resource_retention_cleanup",
    "platform_runtime_portability",
    "spec_test_docs_consistency",
    "release_external_boundary",
)

ROLES = (
    "builder_self_review",
    "adversarial_challenger",
    "specialist",
    "evidence_worker",
)

KNOWN_PACKET_CONTRACTS = ("schemas/agent_review_packet.v1.schema.json",)
KNOWN_HANDOFF_AUTHORITIES = ("#11701", "#10881")
MANDATORY_ROLE = "adversarial_challenger"

# Textual top-level fields must be non-empty strings. schema_version is not
# listed here: it is an integer literal checked by its own equality contract.
TOP_LEVEL_STRING_KEYS = (
    "policy",
    "owner",
    "status",
    "updated",
    "issue",
    "classification_rule",
    "enforcement_boundary",
    "successor_consumption",
    "projection_doc",
    "validator_script",
    "validator_test",
)
TOP_LEVEL_TABLE_KEYS = (
    "families",
    "profile",
    "code_owner_identity",
    "surface",
    "residue",
)
TOP_LEVEL_KEYS = ("schema_version",) + TOP_LEVEL_STRING_KEYS + TOP_LEVEL_TABLE_KEYS

PROFILE_KEYS = (
    "fresh_direction",
    "lenses",
    "required_roles",
    "packet_contract",
    "handoff_authority",
)

SURFACE_KEYS = (
    "family",
    "authority",
    "controller",
    "conflict_key",
    "risk_class",
    "review_profile",
    "required_evidence",
    "first_falsifier",
    "enforcement_successor",
    "code_owner_route",
    "paths",
)

SURFACE_OPTIONAL_KEYS = ("predecessor_exit",)

ROUTE_KEYS = ("kind", "identity", "resolution_owner", "note")

IDENTITY_KEYS = ("kind", "status", "validation_method", "evidence_date")
IDENTITY_OPTIONAL_KEYS = ("permission", "reason")

RESIDUE_KEYS = ("authority", "parent_issue", "reason", "resolution_owner")

# Required fields whose type is a container checked by its own typed logic;
# every other required field must be a non-empty string (#12272 review).
CONTAINER_FIELDS: dict[str, type] = {
    "lenses": list,
    "required_roles": list,
    "paths": list,
    "code_owner_route": dict,
}

VALID_OWNER_STATUSES = ("valid", "invalid_as_code_owner", "not_proven")

REQUIRED_SELF_PATHS = (
    "policy/review-surfaces.toml",
    "scripts/ci/validate_review_surfaces.py",
    "scripts/ci/test_validate_review_surfaces.py",
    "docs/policy/REVIEW_SURFACES.md",
    ".github/CODEOWNERS",
)
SELF_CONFLICT_KEY = "authority_review.manifest"

# Closed detector set: sensitive semantic-control paths identified by the
# denominator-loss investigation (#11793) that currently exist on main. Every
# file at or under these targets must be classified by exactly one surface
# binding. Deleting a bound target therefore fails instead of silently
# shrinking the denominator.
DETECTOR_FILES = (
    "docs/agents/CLOSE_PROOF_POLICY.md",
    "docs/reference/CONFIGURATION_SCHEMA.md",
    "docs/agents/pr-ledger.schema.json",
    "docs/policy/REVIEW_SURFACES.md",
    "xtask/src/bin/semantic-close-containment.rs",
    "xtask/src/tasks/ripr_evidence.rs",
    "xtask/src/tasks/pr_ledger.rs",
    "xtask/src/tasks/pr_close_proof.rs",
    "xtask/src/tasks/agent_review_packet.rs",
    "xtask/tests/public_api_ratchet_tests.rs",
    "scripts/ci/ripr_summary.py",
    "scripts/ci/validate_review_surfaces.py",
    "scripts/ci/test_validate_review_surfaces.py",
    "schemas/perllsp-settings.schema.json",
    "schemas/ripr-perl-facts-v1.schema.json",
    "schemas/agent_review_packet.v1.schema.json",
    "schemas/agent_review_finding.v1.schema.json",
    "schemas/stage_closure_projection.v1.schema.json",
    "policy/ripr-suppressions.toml",
    "policy/review-surfaces.toml",
    "ripr.toml",
    ".github/workflows/semantic-close-containment.yml",
    ".github/workflows/review-receipt-retirement.yml",
    ".github/workflows/ripr.yml",
    ".github/PULL_REQUEST_TEMPLATE.md",
    ".github/CODEOWNERS",
    ".github/settings.yaml",
)

DETECTOR_DIRS = (
    "xtask/src/close_proof",
    ".ci/close-proof-contract",
    ".ci/semantic-close-containment",
    ".ci/public-api-baselines",
    "fixtures/agent_review_packet",
    "crates/perl-lsp-rs-core/src/configuration_authority",
)


def normalize(path_text: str) -> str:
    return path_text.replace("\\", "/").strip()


def is_dir_glob(pattern: str) -> bool:
    return pattern.endswith("/**")


def glob_depth(pattern: str) -> int:
    return len(pattern[: -len("/**")].split("/"))


def is_non_empty_string(value: object) -> bool:
    return isinstance(value, str) and bool(value.strip())


def check_required_fields(
    issues: list[str],
    where: str,
    doc: dict[str, Any],
    fields: tuple[str, ...],
) -> None:
    """Require presence with an honest type. Textual fields must be non-empty
    strings; container fields are validated by their own typed checks."""
    for field in fields:
        value = doc.get(field)
        if value is None:
            issues.append(f"{where}.{field}: missing_field")
            continue
        container_kind = CONTAINER_FIELDS.get(field)
        if container_kind is not None:
            if not isinstance(value, container_kind):
                issues.append(
                    f"{where}.{field}: wrong_type ({container_kind.__name__} required)"
                )
        elif isinstance(value, str):
            if not value.strip():
                issues.append(f"{where}.{field}: missing_field (non-empty string required)")
        else:
            issues.append(f"{where}.{field}: wrong_type (non-empty string required)")


def load_manifest(manifest_path: Path, issues: list[str]) -> dict[str, Any]:
    try:
        with manifest_path.open("rb") as handle:
            return tomllib.load(handle)
    except FileNotFoundError:
        issues.append(f"{manifest_path}: manifest_unreadable (missing)")
    except OSError as error:
        issues.append(f"{manifest_path}: manifest_unreadable ({error})")
    except tomllib.TOMLDecodeError as error:
        issues.append(f"{manifest_path}: toml_parse_error ({error})")
    return {}


def check_unknown_keys(
    issues: list[str],
    where: str,
    actual: dict[str, Any],
    allowed: tuple[str, ...],
) -> None:
    for key in sorted(set(actual) - set(allowed)):
        issues.append(f"{where}.{key}: unknown_field (not part of the contract)")


def validate_header(doc: dict[str, Any], issues: list[str]) -> None:
    check_unknown_keys(issues, "manifest", doc, TOP_LEVEL_KEYS)
    check_required_fields(issues, "manifest", doc, TOP_LEVEL_STRING_KEYS)
    for table_key in TOP_LEVEL_TABLE_KEYS:
        if not isinstance(doc.get(table_key), dict):
            issues.append(f"manifest.{table_key}: missing_field (table required)")
    if doc.get("schema_version") != 1:
        issues.append("manifest.schema_version: schema_mismatch (must be 1)")
    if doc.get("policy") != "review-surfaces":
        issues.append('manifest.policy: schema_mismatch (must be "review-surfaces")')
    if doc.get("status") != "advisory":
        issues.append('manifest.status: schema_mismatch (must be "advisory")')


def validate_profiles(
    profiles: object,
    issues: list[str],
) -> None:
    if not isinstance(profiles, dict) or not profiles:
        issues.append("profile: missing_field (at least one profile required)")
        return
    for name in sorted(profiles):
        where = f"profile.{name}"
        body = profiles[name]
        if not isinstance(body, dict):
            issues.append(f"{where}: malformed_table")
            continue
        check_unknown_keys(issues, where, body, PROFILE_KEYS)
        check_required_fields(issues, where, body, PROFILE_KEYS)
        lenses = body.get("lenses")
        if not isinstance(lenses, list) or not lenses:
            issues.append(f"{where}.lenses: missing_field (non-empty list required)")
        else:
            for lens in lenses:
                if lens not in LENSES:
                    issues.append(f"{where}.lenses: invented_review_kind ({lens!r} is not in the closed #10881 lens vocabulary)")
        roles = body.get("required_roles")
        if not isinstance(roles, list) or not roles:
            issues.append(f"{where}.required_roles: missing_field (non-empty list required)")
        else:
            for role in roles:
                if role not in ROLES:
                    issues.append(f"{where}.required_roles: invented_review_kind ({role!r} is not in the closed #10881 role vocabulary)")
            if MANDATORY_ROLE not in roles:
                issues.append(
                    f"{where}.required_roles: missing_independent_challenge "
                    f"(builder self-review alone cannot satisfy review; {MANDATORY_ROLE} required)"
                )
        packet = body.get("packet_contract")
        if packet is not None and packet not in KNOWN_PACKET_CONTRACTS:
            issues.append(f"{where}.packet_contract: invented_review_kind (must reuse {', '.join(KNOWN_PACKET_CONTRACTS)})")
        handoff = body.get("handoff_authority")
        if handoff is not None and handoff not in KNOWN_HANDOFF_AUTHORITIES:
            issues.append(f"{where}.handoff_authority: invented_review_kind (must name {', '.join(KNOWN_HANDOFF_AUTHORITIES)})")


def validate_identities(
    identities: object,
    issues: list[str],
) -> set[str]:
    valid: set[str] = set()
    if not isinstance(identities, dict) or not identities:
        issues.append("code_owner_identity: missing_field (platform-validation table required)")
        return valid
    for name in sorted(identities):
        where = f"code_owner_identity.{name}"
        body = identities[name]
        if not isinstance(body, dict):
            issues.append(f"{where}: malformed_table")
            continue
        check_unknown_keys(issues, where, body, IDENTITY_KEYS + IDENTITY_OPTIONAL_KEYS)
        check_required_fields(issues, where, body, IDENTITY_KEYS)
        status = body.get("status")
        if status not in VALID_OWNER_STATUSES:
            issues.append(f"{where}.status: unknown_status (must be one of {', '.join(VALID_OWNER_STATUSES)})")
        elif status == "valid":
            valid.add(name)
            if not is_non_empty_string(body.get("permission")):
                issues.append(f"{where}.permission: missing_field (valid owners must record granted permission)")
        elif status == "invalid_as_code_owner" and not is_non_empty_string(body.get("reason")):
            issues.append(f"{where}.reason: missing_field (invalid owners must record why)")
    return valid


def validate_route(
    issues: list[str],
    where: str,
    route: object,
    valid_owners: set[str],
) -> None:
    if not isinstance(route, dict):
        issues.append(f"{where}.code_owner_route: missing_field (routing disposition required)")
        return
    check_unknown_keys(issues, f"{where}.code_owner_route", route, ROUTE_KEYS)
    kind = route.get("kind")
    if kind not in ROUTE_KINDS:
        issues.append(f"{where}.code_owner_route.kind: unknown_route_kind (must be one of {', '.join(ROUTE_KINDS)})")
        return
    if kind == "validated_pattern":
        identity = route.get("identity")
        if not is_non_empty_string(identity):
            issues.append(f"{where}.code_owner_route.identity: missing_field")
        elif identity not in valid_owners:
            issues.append(
                f"{where}.code_owner_route.identity: invalid_code_owner_claimed_valid "
                f"({identity!r} lacks platform validation evidence in [code_owner_identity])"
            )
        return
    if not is_non_empty_string(route.get("resolution_owner")):
        issues.append(f"{where}.code_owner_route.resolution_owner: missing_field ({kind} routes need a named resolution owner)")
    if not is_non_empty_string(route.get("note")):
        issues.append(f"{where}.code_owner_route.note: missing_field ({kind} routes need their disposition recorded)")


def collect_surface_bindings(
    doc: dict[str, Any],
    root: Path,
    issues: list[str],
    valid_owners: set[str],
) -> list[tuple[str, str, str]]:
    """Validate every surface row structurally and collect its path bindings."""
    surfaces = doc.get("surface") if isinstance(doc.get("surface"), dict) else {}
    profiles = doc.get("profile") if isinstance(doc.get("profile"), dict) else {}
    bindings: list[tuple[str, str, str]] = []
    if not isinstance(surfaces, dict) or not surfaces:
        issues.append("surface: missing_field (at least one surface row required)")
        return bindings
    for surface_id in sorted(surfaces):
        where = f"surface.{surface_id}"
        body = surfaces[surface_id]
        if not isinstance(body, dict):
            issues.append(f"{where}: malformed_table")
            continue
        check_unknown_keys(issues, where, body, SURFACE_KEYS + SURFACE_OPTIONAL_KEYS)
        check_required_fields(issues, where, body, SURFACE_KEYS)
        if body.get("family") not in FAMILIES:
            issues.append(f"{where}.family: unknown_family ({body.get('family')!r})")
        if body.get("risk_class") not in RISK_CLASSES:
            issues.append(f"{where}.risk_class: unknown_risk_class ({body.get('risk_class')!r})")
        if body.get("required_evidence") not in EVIDENCE_KINDS:
            issues.append(
                f"{where}.required_evidence: invented_review_kind "
                f"({body.get('required_evidence')!r} does not name existing review machinery)"
            )
        profile_name = body.get("review_profile")
        if is_non_empty_string(profile_name) and profiles and profile_name not in profiles:
            issues.append(f"{where}.review_profile: unknown_profile ({profile_name!r})")
        validate_route(issues, where, body.get("code_owner_route"), valid_owners)
        paths = body.get("paths")
        if not isinstance(paths, list) or not paths:
            issues.append(f"{where}.paths: missing_field (each surface binds exact current paths)")
            continue
        for raw_path in paths:
            if not isinstance(raw_path, str):
                issues.append(f"{where}.paths: malformed_path ({raw_path!r})")
                continue
            pattern = normalize(raw_path)
            if pattern in ("**", "**/*", "*"):
                issues.append(f"{where}.paths: broad_glob_binding ({pattern} masks unclassified authority)")
                continue
            if is_dir_glob(pattern):
                if glob_depth(pattern) < 2:
                    issues.append(
                        f"{where}.paths: broad_glob_binding ({pattern} covers a whole top-level tree)"
                    )
                    continue
                target = root / pattern[: -len("/**")]
                if not target.is_dir():
                    issues.append(
                        f"{where}.paths: binding_target_missing ({pattern} does not resolve to a directory)"
                    )
                bindings.append((surface_id, str(profile_name), pattern))
                continue
            target = root / pattern
            if not target.exists():
                issues.append(f"{where}.paths: binding_target_missing ({pattern})")
            bindings.append((surface_id, str(profile_name), pattern))
    return bindings


def detect_sensitive_files(root: Path, issues: list[str]) -> list[str]:
    found: list[str] = []
    for rel in DETECTOR_FILES:
        candidate = root / rel
        if not candidate.exists():
            issues.append(f"{rel}: detector_target_missing (bound denominator target vanished)")
            continue
        found.append(rel)
    for rel in DETECTOR_DIRS:
        directory = root / rel
        if not directory.is_dir():
            issues.append(f"{rel}: detector_target_missing (bound denominator directory vanished)")
            continue
        for path in sorted(directory.rglob("*")):
            if path.is_file():
                found.append(normalize(str(path.relative_to(root))))
    return sorted(set(found))


def binding_covers(pattern: str, file_rel: str) -> bool:
    if is_dir_glob(pattern):
        return file_rel.startswith(pattern[: -len("/**")] + "/")
    return pattern == file_rel


def bindings_overlap(pattern_a: str, pattern_b: str) -> bool:
    def covers(outer: str, inner: str) -> bool:
        if is_dir_glob(outer):
            prefix = outer[: -len("/**")]
            if inner == prefix or inner.startswith(prefix + "/"):
                return True
            return is_dir_glob(inner) and inner[: -len("/**")].startswith(prefix + "/")
        return outer == inner

    return covers(pattern_a, pattern_b) or covers(pattern_b, pattern_a)


def check_binding_conflicts(bindings: list[tuple[str, str, str]], issues: list[str]) -> None:
    """Duplicate and contradictory ownership across every collected binding,
    independent of the sensitive-path detector set (#12272 review)."""
    ordered = sorted(bindings)
    for index, (surface_a, profile_a, pattern_a) in enumerate(ordered):
        for surface_b, profile_b, pattern_b in ordered[index + 1 :]:
            if surface_a == surface_b or not bindings_overlap(pattern_a, pattern_b):
                continue
            issues.append(
                f"{pattern_b}: duplicate_path_binding (claimed by {surface_a}, {surface_b})"
            )
            if profile_a != profile_b:
                issues.append(
                    f"{pattern_b}: contradictory_path_ownership "
                    f"(profiles {profile_a}, {profile_b})"
                )


def check_coverage(
    sensitive_files: list[str],
    bindings: list[tuple[str, str, str]],
    issues: list[str],
) -> None:
    for file_rel in sensitive_files:
        claims = [
            (surface_id, profile)
            for surface_id, profile, pattern in bindings
            if binding_covers(pattern, file_rel)
        ]
        if not claims:
            issues.append(f"{file_rel}: unclassified_sensitive_path (no review-surface row binds this authority)")
            continue
        owners = {surface_id for surface_id, _ in claims}
        if len(owners) > 1:
            issues.append(
                f"{file_rel}: duplicate_path_binding (claimed by {', '.join(sorted(owners))})"
            )
        profiles = {profile for _, profile in claims}
        if len(profiles) > 1:
            issues.append(
                f"{file_rel}: contradictory_path_ownership (profiles {', '.join(sorted(profiles))})"
            )


def check_self_surface(surfaces: object, bindings: list[tuple[str, str, str]], issues: list[str]) -> None:
    bound = {pattern.rstrip("/") for _, _, pattern in bindings}
    for required in REQUIRED_SELF_PATHS:
        covered = any(binding_covers(pattern, required) for _, _, pattern in bindings)
        if not covered and required not in bound:
            issues.append(f"{required}: self_surface_missing (the manifest must bind its own authority paths)")
    if isinstance(surfaces, dict):
        keys = {key for key, body in surfaces.items() if isinstance(body, dict)}
        manifest_rows = {
            key
            for key in keys
            if isinstance(surfaces[key], dict) and surfaces[key].get("conflict_key") == SELF_CONFLICT_KEY
        }
        if not manifest_rows:
            issues.append(f"surface: self_surface_missing (no row carries conflict_key {SELF_CONFLICT_KEY})")


def validate_residue(residue: object, issues: list[str]) -> int:
    if not isinstance(residue, dict):
        # Absence is already reported by the header table check.
        return 0
    for name in sorted(residue):
        where = f"residue.{name}"
        body = residue[name]
        if not isinstance(body, dict):
            issues.append(f"{where}: malformed_table")
            continue
        check_unknown_keys(issues, where, body, RESIDUE_KEYS)
        check_required_fields(issues, where, body, RESIDUE_KEYS)
    return len(residue)


def parse_codeowners(codeowners_path: Path, issues: list[str]) -> list[tuple[str, list[str]]]:
    rows: list[tuple[str, list[str]]] = []
    try:
        text = codeowners_path.read_text(encoding="utf-8")
    except OSError as error:
        issues.append(f"{codeowners_path}: codeowners_unreadable ({error})")
        return rows
    for line_number, raw_line in enumerate(text.splitlines(), start=1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        tokens = line.split()
        owners = [token for token in tokens if token.startswith("@")]
        pattern = next((token for token in tokens if not token.startswith("@")), "")
        if not owners or not pattern:
            issues.append(
                f"{codeowners_path}:{line_number}: malformed_codeowners_row ({raw_line!r})"
            )
            continue
        rows.append((pattern, owners))
    return rows


def check_codeowners_owners(rows: list[tuple[str, list[str]]], valid_owners: set[str], issues: list[str]) -> None:
    for _pattern, owners in rows:
        for owner in owners:
            identity = owner[1:]
            if identity.startswith("org/") or "/" in identity:
                lookup = identity
            else:
                lookup = identity
            if lookup not in valid_owners:
                issues.append(
                    f".github/CODEOWNERS: unproven_code_owner ({owner} lacks platform-valid "
                    f"status in [code_owner_identity]; it must not appear governed)"
                )


def render_projection(doc: dict[str, Any]) -> str:
    lines: list[str] = []
    lines.append("<!-- Generated by scripts/ci/validate_review_surfaces.py --write-projection.")
    lines.append("     Do not hand-edit; regenerate from policy/review-surfaces.toml. -->")
    lines.append("")
    lines.append("# Sensitive Authority Review Surfaces")
    lines.append("")
    lines.append(f"Issue #{doc.get('issue', '?')} · status `{doc.get('status', '?')}` · updated {doc.get('updated', '?')}")
    lines.append("")
    lines.append(f"- Manifest: `{DEFAULT_MANIFEST.as_posix()}` (issue #{doc.get('issue', '?')}).")
    lines.append(f"- Checked by: `{doc.get('validator_script', 'scripts/ci/validate_review_surfaces.py')}`")
    lines.append(f"- Boundary: {doc.get('enforcement_boundary')}")
    successor = doc.get("successor_consumption")
    if isinstance(successor, str) and successor:
        lines.append(f"- Successors: {successor}")
    lines.append(f"- Rule: {doc.get('classification_rule')}")
    lines.append("- Routing declaration, current-head review evidence, and live required-review")
    lines.append("  enforcement are distinct things; nothing here proves review happened.")
    lines.append("")
    surfaces = doc.get("surface", {}) if isinstance(doc.get("surface"), dict) else {}
    families = doc.get("families", {}) if isinstance(doc.get("families"), dict) else {}
    for family in FAMILIES:
        lines.append(f"## Family: {family}")
        lines.append("")
        description = families.get(family)
        if isinstance(description, str):
            lines.append(description)
            lines.append("")
        family_rows = sorted(key for key, row in surfaces.items() if isinstance(row, dict) and row.get("family") == family)
        if not family_rows:
            lines.append("(no surface rows)")
            lines.append("")
            continue
        for surface_id in family_rows:
            row = surfaces[surface_id]
            lines.append(f"### `{surface_id}`")
            lines.append("")
            lines.append(f"- Authority: {row.get('authority', '')}")
            lines.append(f"- Controller: {row.get('controller', '')}")
            lines.append(f"- Conflict key: `{row.get('conflict_key', '')}`")
            lines.append(f"- Risk class: `{row.get('risk_class', '')}`")
            lines.append(f"- Review profile: `{row.get('review_profile', '')}`")
            lines.append(f"- Required evidence: `{row.get('required_evidence', '')}`")
            lines.append(f"- First falsifier: {row.get('first_falsifier', '')}")
            lines.append(f"- Enforcement successor: {row.get('enforcement_successor', '')}")
            exit_note = row.get("predecessor_exit")
            if isinstance(exit_note, str) and exit_note:
                lines.append(f"- Predecessor exit: {exit_note}")
            route = row.get("code_owner_route")
            if isinstance(route, dict):
                kind = route.get("kind", "?")
                if kind == "validated_pattern":
                    lines.append(f"- Code-owner route: validated pattern for `@{route.get('identity', '?')}`")
                else:
                    lines.append(
                        f"- Code-owner route: `{kind}` (resolution owner {route.get('resolution_owner', '?')}): {route.get('note', '')}"
                    )
            lines.append("- Bound paths:")
            for path in row.get("paths", []) if isinstance(row.get("paths"), list) else []:
                lines.append(f"  - `{path}`")
            lines.append("")
    lines.append("## Deferred authority (explicit residue)")
    lines.append("")
    residue = doc.get("residue", {}) if isinstance(doc.get("residue"), dict) else {}
    if not residue:
        lines.append("(none)")
        lines.append("")
    for name in sorted(residue):
        row = residue[name]
        if not isinstance(row, dict):
            continue
        lines.append(f"### `{name}`")
        lines.append("")
        lines.append(f"- Authority: {row.get('authority', '')}")
        lines.append(f"- Parent issue: {row.get('parent_issue', '')}")
        lines.append(f"- Reason deferred: {row.get('reason', '')}")
        lines.append(f"- Resolution owner: {row.get('resolution_owner', '')}")
        lines.append("")
    lines.append("## Platform-validated code-owner identities")
    lines.append("")
    identities = doc.get("code_owner_identity", {}) if isinstance(doc.get("code_owner_identity"), dict) else {}
    for name in sorted(identities):
        row = identities[name]
        if not isinstance(row, dict):
            continue
        lines.append(f"### `@{name}` — `{row.get('status', '?')}`")
        lines.append("")
        lines.append(f"- Kind: {row.get('kind', '')}")
        permission = row.get("permission")
        if isinstance(permission, str) and permission:
            lines.append(f"- Permission: {permission}")
        reason = row.get("reason")
        if isinstance(reason, str) and reason:
            lines.append(f"- Finding: {reason}")
        lines.append(f"- Validation: {row.get('validation_method', '')}")
        lines.append(f"- Evidence date: {row.get('evidence_date', '')}")
        lines.append("")
    lines.append("## Review profiles (reused machinery, no new review kinds)")
    lines.append("")
    profiles = doc.get("profile", {}) if isinstance(doc.get("profile"), dict) else {}
    for name in sorted(profiles):
        row = profiles[name]
        if not isinstance(row, dict):
            continue
        lenses = ", ".join(f"`{item}`" for item in row.get("lenses", []))
        roles = ", ".join(f"`{item}`" for item in row.get("required_roles", []))
        lines.append(f"### `{name}`")
        lines.append("")
        lines.append(f"- Fresh direction: {row.get('fresh_direction', '')}")
        lines.append(f"- Lenses (#10881 closed vocabulary): {lenses}")
        lines.append(f"- Required roles: {roles}")
        lines.append(f"- Packet contract: `{row.get('packet_contract', '')}`")
        lines.append(f"- Handoff authority: {row.get('handoff_authority', '')}")
        lines.append("")
    return "\n".join(lines) + "\n"


def resolve_repo_root(explicit_root: Path | None, manifest: Path) -> Path:
    if explicit_root is not None:
        return explicit_root
    return Path(__file__).resolve().parents[2]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--root", type=Path, default=None)
    parser.add_argument("--codeowners", type=Path, default=None)
    parser.add_argument("--projection", type=Path, default=None)
    parser.add_argument("--strict", action="store_true")
    parser.add_argument("--check-projection", action="store_true")
    parser.add_argument("--write-projection", action="store_true")
    args = parser.parse_args()

    issues: list[str] = []

    root = resolve_repo_root(args.root, args.manifest)
    manifest_path = args.manifest if args.manifest.is_absolute() else root / args.manifest
    projection_path = (
        args.projection
        if args.projection is not None
        else root / DEFAULT_PROJECTION
    )
    codeowners_path = (
        args.codeowners if args.codeowners is not None else root / DEFAULT_CODEOWNERS
    )

    doc = load_manifest(manifest_path, issues)
    if doc:
        validate_header(doc, issues)
        validate_profiles(doc.get("profile"), issues)
        valid_owners = validate_identities(doc.get("code_owner_identity"), issues)
        surfaces = doc.get("surface") if isinstance(doc.get("surface"), dict) else {}
        bindings = collect_surface_bindings(doc, root, issues, valid_owners)
        check_binding_conflicts(bindings, issues)
        sensitive_files = detect_sensitive_files(root, issues)
        check_coverage(sensitive_files, bindings, issues)
        check_self_surface(surfaces, bindings, issues)
        residue_count = validate_residue(doc.get("residue"), issues)
        codeowners_rows = parse_codeowners(codeowners_path, issues)
        check_codeowners_owners(codeowners_rows, valid_owners, issues)

        rendered = render_projection(doc)
        if args.write_projection:
            projection_path.parent.mkdir(parents=True, exist_ok=True)
            projection_path.write_text(rendered, encoding="utf-8", newline="\n")
            print(f"Wrote projection: {projection_path}")
        elif args.check_projection:
            try:
                committed = projection_path.read_text(encoding="utf-8")
            except OSError:
                issues.append(
                    f"{projection_path}: projection_stale (expected generated projection is missing)"
                )
                committed = ""
            if committed != rendered:
                issues.append(
                    f"{projection_path}: projection_stale (regenerated projection differs from the committed file; rerun with --write-projection)"
                )
    else:
        residue_count = 0
        bindings = []
        codeowners_rows = []

    surface_count = len(doc.get("surface", {})) if isinstance(doc.get("surface"), dict) else 0
    print(f"Review surfaces in {manifest_path}: {surface_count}")
    print(
        f"Sensitive files detected under {len(DETECTOR_FILES)} file and "
        f"{len(DETECTOR_DIRS)} directory targets: covered by {len(bindings)} bindings"
    )
    print(f"Residue entries: {residue_count}")
    print(f"CODEOWNERS routing rows parsed: {len(codeowners_rows)}")

    if issues:
        print(f"Issues ({len(issues)}):")
        for issue in issues:
            print(f"  - {issue}")
    else:
        print("Denominator contract valid.")

    if args.strict and issues:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
