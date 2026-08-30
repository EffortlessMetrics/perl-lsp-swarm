#!/usr/bin/env python3
"""Row-level oracle for the public_release_claims.v2 inventory derivative.

Contract: `.spec/11548-inventory-derivative/acceptance.md` (#11548 shape (a),
per the #11549 scoping receipt). NON-authoritative scope: no route IDs, no
projection contexts, no true-v2 (#10333/#10334) authority claims.

Unlike a whole-document digest check, this oracle re-derives every generated
row from its inventory source region and compares row-by-row (D5): summary,
location, drift status, notes, finding relations (D4 cited-file join), the
crates.io anti-claim identity set (D2), the release-receipt binding (D1), and
the closed key sets (D6). Editing any single row value must fail validation
naming that row; `--prove-tamper` demonstrates this per row.

Stdlib only; no external dependencies.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tempfile
from pathlib import Path
from typing import Any

DOC_PATH = "docs/distribution/INSTALL_CLAIM_SURFACES.md"
SCHEMA_PATH = "schemas/public_release_claims.v2.schema.json"
RECEIPT_MANIFEST_PATH = "distribution/release_receipts/v0.17.0.assets.json"
SCHEMA_VERSION = "public_release_claims.v2"

EXPECTED_SURFACES = [f"S{i:02d}" for i in range(1, 14)]
EXPECTED_CLAIM_IDS = (
    [f"C1{i:02d}" for i in range(1, 9)]
    + [f"C2{i:02d}" for i in range(1, 6)]
    + [f"C{i:02d}" for i in range(206, 217)]
    + ["C301", "C302", "C303", "C401", "C402", "C403", "C404", "C405", "C406"]
    + ["C501", "C502", "C503", "C601", "C701", "C702", "C703", "C801", "C901", "C902"]
    + [f"C100{i}" for i in range(1, 9)]
    + ["C1101", "C1102", "C1201", "C1202", "C1203", "C1204", "C1205", "C1206", "C1207", "C1208"]
    + [f"C130{i}" for i in range(1, 10)]
)
DRIFT_STATUSES = {
    "current", "pending", "stale_example", "future_example",
    "mutable_pin", "cross_surface_drift", "source_drift", "volatile_number",
}
FINDING_IDS = [f"FND-{i}" for i in range(1, 13)]
FINDING_OWNERS = {
    "FND-4": "#11549-classifier",
    "FND-10": "#10342-ci-cutover",
    "FND-11": "distribution-docs-sync",
}
REGRESSION_FINDING = "FND-1"
REGRESSION_CLAIM = "C401"

ANTI_CLAIM = {
    "kind": "crates_io_name_collision",
    "foreign_name": "perl-lsp",
    "owned_name": "perllsp",
    "disposition": "do_not_install",
}
CITATION_EXTENSIONS = ("md", "yml", "yaml", "sh", "ps1", "json", "toml")
CITATION_RE = re.compile(
    r"([A-Za-z0-9_][A-Za-z0-9_./-]*\.(?:md|yml|yaml|sh|ps1|json|toml)):\d"
)
CLAIM_REF_RE = re.compile(r"(?<![A-Za-z0-9_])(C\d{3,4})")

RESTATED_GROUPS = {
    "C103": "bootstrap_identity",
    "C204": "bootstrap_identity",
    "C1004": "bootstrap_identity",
    "C1206": "bootstrap_identity",
    "C106": "verification_probes",
    "C216": "verification_probes",
    "C1008": "verification_probes",
    "C801": "verification_probes",
}
OMITTED_CAVEATS = {"C1304": ["homebrew_tap_version_unproven"], "C1305": ["homebrew_tap_version_unproven"]}

DIMENSION_FACTS = {
    "C210": {
        "windows_arm64": {
            "user_prose": "x64_fallback_build_from_source",
            "tracked_source": "built",
            "finding_refs": ["FND-4", "FND-11"],
        },
        "product_units": {
            "build_from_source_units": ["perllsp"],
            "tracked_installer_ships_adapter": True,
            "finding_refs": ["FND-11"],
        },
    },
    "C1204": {
        "windows_arm64": {
            "user_prose": "x64_fallback_build_from_source",
            "tracked_source": "built",
            "finding_refs": ["FND-4", "FND-11"],
        }
    },
    "C405": {
        "windows_arm64": {
            "user_prose": "unspecified",
            "tracked_source": "built",
            "finding_refs": ["FND-4"],
        }
    },
    "C501": {
        "windows_arm64": {
            "user_prose": "supported",
            "tracked_source": "built",
            "finding_refs": ["FND-4"],
        }
    },
    "C207": {"sha256sums_enforcement": {"mode": "fail_closed_required"}},
    "C1005": {
        "sha256sums_enforcement": {"mode": "fail_open_conditional", "finding_refs": ["FND-7"]}
    },
    "C302": {"sha256sums_enforcement": {"mode": "verify_present_no_mode"}},
    "C406": {"sha256sums_enforcement": {"mode": "verify_present_no_mode"}},
    "C208": {
        "product_units": {
            "build_from_source_units": ["perllsp"],
            "archive_units_claimed": ["perllsp", "perl-dap"],
        }
    },
    "C209": {
        "product_units": {
            "build_from_source_units": [],
            "archive_units_claimed": ["perllsp", "perl-dap"],
        }
    },
}


class ValidationError(Exception):
    """A row-level or structural validation failure (names its subject)."""


def finding_sort_key(finding_id: str) -> int:
    try:
        return int(finding_id.removeprefix("FND-"))
    except ValueError:
        return 1 << 30


def claim_sort_key(claim_id: str) -> int:
    try:
        return int(claim_id.removeprefix("C"))
    except ValueError:
        return 1 << 30


# ---------------------------------------------------------------------------
# D3: symmetric whole-cell code-span extraction
# ---------------------------------------------------------------------------

def resolve_link_label(cell: str) -> str:
    text = cell.strip()
    if text.startswith("[") and "]" in text:
        return text[1 : text.index("]")]
    return text


def backticks_balanced(text: str) -> bool:
    return text.count("`") % 2 == 0


def trim_code_span_pair(text: str) -> str:
    """Drop boundary backticks only as one matched whole-cell code span."""
    trimmed = text.strip()
    if len(trimmed) >= 2 and trimmed.startswith("`") and trimmed.endswith("`"):
        inner = trimmed[1:]
        second = inner.find("`")
        if second + 1 == len(inner):
            return inner[:second]
    return trimmed


def extract_cell_text(cell: str, context: str) -> str:
    normalized = trim_code_span_pair(resolve_link_label(cell))
    if not backticks_balanced(normalized):
        raise ValidationError(
            f"{DOC_PATH}: {context}: unbalanced code-span delimiters (D3): `{normalized}`"
        )
    return normalized


def code_spans(raw_row: str) -> list[str]:
    parts = raw_row.split("`")
    return [parts[i] for i in range(1, len(parts) - (1 - len(parts) % 2), 2)] if len(parts) > 1 else []


# ---------------------------------------------------------------------------
# D2: anti-claim derivation
# ---------------------------------------------------------------------------

def states_crates_io_collision(raw_row: str) -> bool:
    foreign_backticked = f"`{ANTI_CLAIM['foreign_name']}`"
    return ("crates.io" in raw_row and foreign_backticked in raw_row) or "collision" in raw_row


def asserts_registry_route(raw_row: str) -> bool:
    owned = ANTI_CLAIM["owned_name"]
    return any(
        span.strip().startswith("cargo install")
        and owned in span
        and "--path" not in span
        and "--git" not in span
        for span in code_spans(raw_row)
    )


def derive_anti_claim_ids(claims: list[dict[str, Any]]) -> list[str]:
    direct = [
        claim["claim_id"]
        for claim in claims
        if states_crates_io_collision(claim["raw_row"]) or asserts_registry_route(claim["raw_row"])
    ]
    direct_set = set(direct)
    ids: list[str] = []
    for claim in claims:
        referenced = any(ref in direct_set for ref in CLAIM_REF_RE.findall(claim["raw_row"]))
        if claim["claim_id"] in direct_set or referenced:
            ids.append(claim["claim_id"])
    return ids


# ---------------------------------------------------------------------------
# D4: cited-file join
# ---------------------------------------------------------------------------

def location_file(location: str) -> str:
    file_part = location.split(":", 1)[0]
    return file_part.rsplit("/", 1)[-1]


def extract_cited_files(body: str) -> list[str]:
    files: list[str] = []
    for token in CITATION_RE.findall(body):
        base = token.rsplit("/", 1)[-1]
        extension = base.rsplit(".", 1)[-1]
        if extension in CITATION_EXTENSIONS and base not in files:
            files.append(base)
    return sorted(files)


def derive_relations(claims: list[dict[str, Any]], findings: list[dict[str, Any]]) -> dict[str, list[str]]:
    relations: dict[str, list[str]] = {}
    for finding in findings:
        related = [
            claim["claim_id"]
            for claim in claims
            if location_file(claim["location"]) in finding["cited_files"]
        ]
        related.sort(key=claim_sort_key)
        relations[finding["finding_id"]] = related
    return relations


# ---------------------------------------------------------------------------
# Inventory parsing
# ---------------------------------------------------------------------------

def split_table_row(row: str) -> list[str]:
    inner = row.strip().strip("|")
    return [cell.strip() for cell in inner.split("|")]


def parse_audited_anchor(joined: str) -> tuple[str, str]:
    marker = joined.find("**Audited against:**")
    if marker < 0:
        raise ValidationError(f"{DOC_PATH}: could not locate the **Audited against:** anchor")
    region = joined[marker : marker + 400]
    commit_match = re.search(r"`([0-9a-fA-F]{7,40})`", region)
    date_match = re.search(r"\((\d{4}-\d{2}-\d{2})\)", region)
    if not commit_match or not date_match:
        raise ValidationError(f"{DOC_PATH}: could not locate the audited commit/date")
    return commit_match.group(1).lower(), date_match.group(1)


def parse_release_anchor(joined: str) -> str:
    marker = joined.find("**Drift anchor:**")
    if marker < 0:
        raise ValidationError(f"{DOC_PATH}: could not locate the drift anchor")
    match = re.search(r"(v\d+\.\d+\.\d+)", joined[marker : marker + 400])
    if not match:
        raise ValidationError(f"{DOC_PATH}: could not locate the drift-anchor release receipt")
    return match.group(1)


def parse_findings(doc: str) -> list[dict[str, Any]]:
    joined = doc.replace("\n", " ")
    findings: list[dict[str, Any]] = []
    for number in range(1, 13):
        marker = f"**FND-{number} \u2014 "
        start = joined.find(marker)
        if start < 0:
            continue
        after_marker = start + len(marker)
        end = joined.find(".**", after_marker)
        if end < 0:
            raise ValidationError(f"{DOC_PATH}: FND-{number} has no `.**` title terminator")
        title = " ".join(joined[after_marker:end].split())
        after_title = end + 3
        next_bullet = joined.find(f"- **FND-{number + 1}", after_title)
        next_section = joined.find("## ", after_title)
        bounds = [b for b in (next_bullet, next_section) if b >= 0]
        body_end = min(bounds) if bounds else len(joined)
        findings.append(
            {
                "finding_id": f"FND-{number}",
                "title": title,
                "cited_files": extract_cited_files(joined[after_title:body_end]),
            }
        )
    for finding_id in FINDING_IDS:
        if not any(f["finding_id"] == finding_id for f in findings):
            raise ValidationError(f"{DOC_PATH}: findings section is missing `{finding_id}`")
    return findings


def parse_inventory(doc: str) -> dict[str, Any]:
    joined = doc.replace("\n", " ")
    audited_commit, audited_date = parse_audited_anchor(joined)
    release_anchor = parse_release_anchor(joined)

    surfaces: list[dict[str, Any]] = []
    claims: list[dict[str, Any]] = []
    in_surface_index = False
    current_section: str | None = None

    for line in doc.splitlines():
        trimmed = line.strip()
        if trimmed.startswith("## "):
            in_surface_index = trimmed == "## Surface index"
            continue
        if trimmed.startswith("### "):
            heading = trimmed[4:].strip()
            token = re.split(r"[ \u2014-]", heading, maxsplit=1)[0]
            current_section = token if len(token) == 3 and token.startswith("S") else None
            continue
        if in_surface_index:
            if trimmed.startswith("| S"):
                cells = split_table_row(trimmed)
                if len(cells) < 5:
                    raise ValidationError(f"{DOC_PATH}: malformed surface row: {trimmed}")
                if cells[0].startswith("S"):
                    surfaces.append(
                        {
                            "surface_id": cells[0],
                            "path": extract_cell_text(cells[1], f"{cells[0]}.path"),
                            "role": cells[2],
                            "claim_class": cells[3],
                            "registry_cross_ref": extract_cell_text(
                                cells[4], f"{cells[0]}.registry_cross_ref"
                            )
                            .replace("\u2014", "")
                            .strip(),
                        }
                    )
            continue
        if trimmed.startswith("| C"):
            cells = split_table_row(trimmed)
            if len(cells) < 4:
                raise ValidationError(f"{DOC_PATH}: malformed claim row: {trimmed}")
            claim_id = cells[0]
            if not claim_id.startswith("C"):
                continue
            if current_section is None:
                raise ValidationError(
                    f"{DOC_PATH}: claim row {claim_id} appeared before any `### Sxx` heading"
                )
            notes_cell = cells[4] if len(cells) > 4 else ""
            claims.append(
                {
                    "claim_id": claim_id,
                    "surface_id": current_section,
                    "location": cells[1],
                    "summary": extract_cell_text(cells[2], f"{claim_id}.summary"),
                    "drift_status": extract_cell_text(cells[3], f"{claim_id}.drift_status"),
                    "notes": extract_cell_text(notes_cell, f"{claim_id}.notes"),
                    "raw_row": trimmed,
                }
            )

    surfaces.sort(key=lambda surface: surface["surface_id"])
    claims.sort(key=lambda claim: claim_sort_key(claim["claim_id"]))

    for expected in EXPECTED_SURFACES:
        if not any(surface["surface_id"] == expected for surface in surfaces):
            raise ValidationError(f"{DOC_PATH}: missing denominator surface {expected}")
    if len(surfaces) != len(EXPECTED_SURFACES):
        raise ValidationError(f"{DOC_PATH}: surface denominator moved ({len(surfaces)} rows)")
    for expected in EXPECTED_CLAIM_IDS:
        if not any(claim["claim_id"] == expected for claim in claims):
            raise ValidationError(f"{DOC_PATH}: missing denominator claim row {expected}")
    if len(claims) != len(EXPECTED_CLAIM_IDS):
        extras = [c["claim_id"] for c in claims if c["claim_id"] not in set(EXPECTED_CLAIM_IDS)]
        raise ValidationError(
            f"{DOC_PATH}: {len(extras)} claim row(s) outside the recorded denominator: {extras}"
        )
    for claim in claims:
        if claim["drift_status"] not in DRIFT_STATUSES:
            raise ValidationError(
                f"{DOC_PATH}: {claim['claim_id']} uses unknown drift status `{claim['drift_status']}`"
            )

    return {
        "audited_commit": audited_commit,
        "audited_date": audited_date,
        "release_anchor": release_anchor,
        "surfaces": surfaces,
        "claims": claims,
        "findings": parse_findings(doc),
    }


# ---------------------------------------------------------------------------
# D1: release receipt manifest
# ---------------------------------------------------------------------------

def parse_receipt_manifest(raw: bytes) -> dict[str, Any]:
    manifest = json.loads(raw)
    if not isinstance(manifest, dict):
        raise ValidationError(f"{RECEIPT_MANIFEST_PATH}: root must be an object")
    expected_keys = {"release", "source", "verified_date", "assets"}
    unknown = sorted(set(manifest) - expected_keys)
    if unknown:
        raise ValidationError(f"{RECEIPT_MANIFEST_PATH}: unknown root key(s) {unknown}")
    release = manifest.get("release")
    source = manifest.get("source")
    verified_date = manifest.get("verified_date")
    assets = manifest.get("assets")
    if not isinstance(release, str) or not release:
        raise ValidationError(f"{RECEIPT_MANIFEST_PATH}: release: missing")
    release_parts = release[1:].split(".") if release.startswith("v") else []
    if (
        len(release_parts) != 3
        or any(not part or not part.isascii() or not part.isdigit() for part in release_parts)
    ):
        raise ValidationError(
            f"{RECEIPT_MANIFEST_PATH}: release `{release}` must match v<major>.<minor>.<patch>"
        )
    if not isinstance(source, str) or not source:
        raise ValidationError(f"{RECEIPT_MANIFEST_PATH}: source: missing or empty")
    if not isinstance(verified_date, str) or not verified_date:
        raise ValidationError(f"{RECEIPT_MANIFEST_PATH}: verified_date: missing")
    if (
        len(verified_date) != 10
        or verified_date[4] != "-"
        or verified_date[7] != "-"
        or any(
            not character.isascii() or not character.isdigit()
            for index, character in enumerate(verified_date)
            if index not in (4, 7)
        )
    ):
        raise ValidationError(
            f"{RECEIPT_MANIFEST_PATH}: verified_date `{verified_date}` must match YYYY-MM-DD"
        )
    if not isinstance(assets, list) or not assets:
        raise ValidationError(f"{RECEIPT_MANIFEST_PATH}: assets must be a non-empty array")
    names = []
    for asset in assets:
        if not isinstance(asset, dict):
            raise ValidationError(f"{RECEIPT_MANIFEST_PATH}: assets[]: expected object")
        unknown_asset = sorted(set(asset) - {"name"})
        if unknown_asset:
            raise ValidationError(
                f"{RECEIPT_MANIFEST_PATH}: assets[]: unknown key(s) {unknown_asset}"
            )
        name = asset.get("name")
        if not isinstance(name, str) or not name:
            raise ValidationError(f"{RECEIPT_MANIFEST_PATH}: assets[]: missing name")
        names.append(name)
    if names != sorted(names) or len(set(names)) != len(names):
        raise ValidationError(
            f"{RECEIPT_MANIFEST_PATH}: asset names must be unique and stored in sorted order"
        )
    return {
        "release": release,
        "source": source,
        "verified_date": verified_date,
        "asset_names": names,
    }


def derive_windows_arm64_receipt(manifest: dict[str, Any]) -> str:
    shipped = any("aarch64-pc-windows-msvc" in name for name in manifest["asset_names"])
    return "present" if shipped else "absent"


# ---------------------------------------------------------------------------
# Expected catalog construction
# ---------------------------------------------------------------------------

def build_expected_claims(inventory: dict[str, Any], manifest: dict[str, Any]) -> list[dict[str, Any]]:
    receipt_value = derive_windows_arm64_receipt(manifest)
    relations = derive_relations(inventory["claims"], inventory["findings"])
    anti_claim_ids = derive_anti_claim_ids(inventory["claims"])

    expected: list[dict[str, Any]] = []
    for claim in inventory["claims"]:
        claim_id = claim["claim_id"]
        finding_refs = [
            finding_id
            for finding_id, related in relations.items()
            if claim_id in related
        ]
        for dimension in DIMENSION_FACTS.get(claim_id, {}).values():
            finding_refs.extend(dimension.get("finding_refs", []))
        finding_refs = sorted(set(finding_refs), key=finding_sort_key)

        row: dict[str, Any] = {
            "claim_id": claim_id,
            "surface_id": claim["surface_id"],
            "location": claim["location"],
            "summary": claim["summary"],
            "drift_status": claim["drift_status"],
            "notes": claim["notes"],
            "finding_refs": finding_refs,
            "restatement_group": RESTATED_GROUPS.get(claim_id),
            "omitted_caveats": OMITTED_CAVEATS.get(claim_id, []),
            "identity_anti_claims": [ANTI_CLAIM] if claim_id in anti_claim_ids else [],
        }
        if claim_id in DIMENSION_FACTS:
            dimensions = json.loads(json.dumps(DIMENSION_FACTS[claim_id]))
            if "windows_arm64" in dimensions:
                dimensions["windows_arm64"]["published_receipt_v0_17_0"] = receipt_value
            row["dimensions"] = dimensions
        expected.append(row)
    return expected


def build_expected_findings(
    inventory: dict[str, Any], relations: dict[str, list[str]]
) -> list[dict[str, Any]]:
    expected: list[dict[str, Any]] = []
    for finding in inventory["findings"]:
        expected.append(
            {
                "finding_id": finding["finding_id"],
                "title": finding["title"],
                "cited_files": finding["cited_files"],
                "related_claims": relations.get(finding["finding_id"], []),
                "owner_route": FINDING_OWNERS.get(finding["finding_id"], "none_recorded"),
            }
        )
    return expected


# ---------------------------------------------------------------------------
# Closed key sets (D6)
# ---------------------------------------------------------------------------

CLAIM_KEYS = [
    "claim_id", "surface_id", "location", "summary", "drift_status", "notes",
    "finding_refs", "restatement_group", "omitted_caveats", "identity_anti_claims", "dimensions",
]
SURFACE_KEYS = ["surface_id", "path", "role", "claim_class", "registry_cross_ref"]
FINDING_KEYS = ["finding_id", "title", "cited_files", "related_claims", "owner_route"]
ANTI_CLAIM_KEYS = ["kind", "foreign_name", "owned_name", "disposition"]
DIMENSION_FAMILIES = {
    "windows_arm64": ["user_prose", "tracked_source", "published_receipt_v0_17_0", "finding_refs"],
    "sha256sums_enforcement": ["mode", "finding_refs"],
    "product_units": [
        "build_from_source_units", "archive_units_claimed",
        "tracked_installer_ships_adapter", "finding_refs",
    ],
}
ROOT_KEYS = [
    "schema_version", "status", "generator", "issue", "source_inventory", "input_digests",
    "release_receipts", "surfaces", "claims", "findings",
]
DIGEST_KEYS = ["inventory_document", "schema", "release_receipt_manifest"]
RECEIPT_KEYS = ["release", "source", "verified_date", "assets"]


def validate_schema_closure(schema: dict[str, Any]) -> int:
    """D6 schema-side walk: every object-typed node must close its keys."""
    closed = 0
    stack = [schema]
    while stack:
        node = stack.pop()
        if not isinstance(node, dict):
            continue
        is_object_schema = node.get("type") == "object" or "properties" in node
        if is_object_schema:
            if node.get("additionalProperties") is not False:
                raise ValidationError(
                    "schema: object node without `additionalProperties: false` (D6 closure violation)"
                )
            closed += 1
        stack.extend(node.values())
    return closed


# ---------------------------------------------------------------------------
# Row-level comparison (D5)
# ---------------------------------------------------------------------------

def compare_catalog(artifact: dict[str, Any], inventory: dict[str, Any], manifest: dict[str, Any]) -> dict[str, int]:
    """Re-derive every row and compare; raises ValidationError naming the row."""
    if not isinstance(artifact, dict):
        raise ValidationError("catalog: expected a JSON object")
    extra_root = sorted(set(artifact) - set(ROOT_KEYS))
    missing_root = sorted(set(ROOT_KEYS) - set(artifact))
    if extra_root:
        raise ValidationError(f"catalog: schema-forbidden root key(s) {extra_root} (D6)")
    if missing_root:
        raise ValidationError(f"catalog: missing root key(s) {missing_root}")
    if artifact.get("schema_version") != SCHEMA_VERSION:
        raise ValidationError(
            f"catalog.schema_version: expected `{SCHEMA_VERSION}`, found `{artifact.get('schema_version')}`"
        )
    # Generated-provenance root constants (schema consts): an artifact that
    # mutates any of these is no longer the sanctioned generator's output.
    for key, expected in (
        ("status", "generated"),
        ("generator", "cargo xtask public-release-claims-v2 build --write"),
        ("issue", 11548),
    ):
        if artifact.get(key) != expected:
            raise ValidationError(
                f"catalog.{key}: expected `{expected}`, found `{artifact.get(key)}` "
                "(generated-provenance constant)"
            )

    # D1: the receipt manifest is bound to the inventory's own drift anchor.
    # A manifest copied from another release must fail before any
    # published_receipt_* value is derived from it.
    if manifest["release"] != inventory["release_anchor"]:
        raise ValidationError(
            f"catalog: receipt manifest release `{manifest['release']}` does not match the "
            f"inventory drift anchor `{inventory['release_anchor']}` "
            "(D1: receipt facts derive only from the anchored release)"
        )

    source_inventory = artifact.get("source_inventory") or {}
    expected_source = {
        "path": DOC_PATH,
        "audited_commit": inventory["audited_commit"],
        "audited_date": inventory["audited_date"],
        "release_anchor": inventory["release_anchor"],
        "track": "public-beta",
    }
    if source_inventory != expected_source:
        raise ValidationError(f"catalog.source_inventory: expected {expected_source}, found {source_inventory}")

    # Per-surface rows.
    artifact_surfaces = artifact.get("surfaces") or []
    expected_surfaces = [
        {
            "surface_id": surface["surface_id"],
            "path": surface["path"],
            "role": surface["role"],
            "claim_class": surface["claim_class"],
            "registry_cross_ref": surface["registry_cross_ref"],
        }
        for surface in inventory["surfaces"]
    ]
    if artifact_surfaces != expected_surfaces:
        for expected_row, actual_row in zip(expected_surfaces, artifact_surfaces):
            if expected_row != actual_row:
                raise ValidationError(
                    f"catalog.surfaces[{actual_row.get('surface_id', '?')}]: "
                    f"expected {expected_row}, found {actual_row}"
                )
        raise ValidationError(
            f"catalog.surfaces: {len(artifact_surfaces)} row(s) but the inventory holds {len(expected_surfaces)}"
        )

    # Receipt manifest projection (D1 receipt authority).
    receipts = artifact.get("release_receipts") or []
    expected_receipt = {
        "release": manifest["release"],
        "source": manifest["source"],
        "verified_date": manifest["verified_date"],
        "assets": [{"name": name} for name in manifest["asset_names"]],
    }
    if receipts != [expected_receipt]:
        raise ValidationError(f"catalog.release_receipts: expected [{expected_receipt}], found {receipts}")

    # Per-claim rows (the D5 core: every derived field, every row).
    artifact_claims = artifact.get("claims") or []
    expected_claims = build_expected_claims(inventory, manifest)
    by_id = {row.get("claim_id"): row for row in artifact_claims if isinstance(row, dict)}
    if len(by_id) != len(artifact_claims):
        raise ValidationError("catalog.claims: duplicate route authority")
    if sorted(by_id) != sorted(EXPECTED_CLAIM_IDS):
        raise ValidationError(
            f"catalog.claims: claim denominator moved (found {len(by_id)}, denominator holds {len(EXPECTED_CLAIM_IDS)})"
        )
    for expected_row in expected_claims:
        claim_id = expected_row["claim_id"]
        actual_row = by_id[claim_id]
        extra = sorted(set(actual_row) - set(CLAIM_KEYS))
        if extra:
            raise ValidationError(f"catalog.claims[{claim_id}]: schema-forbidden key(s) {extra} (D6)")
        for field in ("summary", "notes"):
            text = actual_row.get(field, "")
            if not backticks_balanced(text):
                raise ValidationError(
                    f"catalog.claims[{claim_id}].{field}: unbalanced code-span delimiters (D3): `{text}`"
                )
        for field, expected_value in expected_row.items():
            actual_value = actual_row.get(field, "<missing>")
            if field == "dimensions":
                continue
            if actual_value != expected_value:
                raise ValidationError(
                    f"catalog.claims[{claim_id}].{field}: expected {expected_value!r}, found {actual_value!r}"
                )
        # Dimensions: closed families plus receipt binding (D1).
        expected_dimensions = expected_row.get("dimensions") or {}
        actual_dimensions = actual_row.get("dimensions") or {}
        if set(actual_dimensions) - set(DIMENSION_FAMILIES):
            raise ValidationError(
                f"catalog.claims[{claim_id}].dimensions: schema-forbidden family "
                f"{sorted(set(actual_dimensions) - set(DIMENSION_FAMILIES))} (D6)"
            )
        if set(actual_dimensions) != set(expected_dimensions):
            raise ValidationError(
                f"catalog.claims[{claim_id}].dimensions: expected families "
                f"{sorted(expected_dimensions)}, found {sorted(actual_dimensions)}"
            )
        for family, expected_family in expected_dimensions.items():
            actual_family = actual_dimensions.get(family) or {}
            allowed = DIMENSION_FAMILIES[family]
            rogue = sorted(set(actual_family) - set(allowed))
            if rogue:
                raise ValidationError(
                    f"catalog.claims[{claim_id}].dimensions.{family}: schema-forbidden key(s) {rogue} (D6)"
                )
            for field, expected_value in expected_family.items():
                actual_value = actual_family.get(field, "<missing>")
                if actual_value != expected_value:
                    raise ValidationError(
                        f"catalog.claims[{claim_id}].dimensions.{family}.{field}: "
                        f"expected {expected_value!r}, found {actual_value!r}"
                    )
            if family == "windows_arm64":
                receipt_value = actual_family.get("published_receipt_v0_17_0")
                derived = derive_windows_arm64_receipt(manifest)
                if receipt_value != derived:
                    raise ValidationError(
                        f"catalog.claims[{claim_id}]: published_receipt_v0_17_0 `{receipt_value}` "
                        f"contradicts the release-asset manifest (`{derived}` for {manifest['release']}; D1)"
                    )

    # Per-finding rows (D4).
    artifact_findings = artifact.get("findings") or []
    relations = derive_relations(inventory["claims"], inventory["findings"])
    expected_findings = build_expected_findings(inventory, relations)
    if len(artifact_findings) != len(expected_findings):
        raise ValidationError(
            f"catalog.findings: {len(artifact_findings)} row(s) but the inventory holds {len(expected_findings)}"
        )
    by_finding_id = {row.get("finding_id"): row for row in artifact_findings if isinstance(row, dict)}
    for expected_row in expected_findings:
        finding_id = expected_row["finding_id"]
        actual_row = by_finding_id.get(finding_id)
        if actual_row is None:
            raise ValidationError(f"catalog.findings: missing {finding_id}")
        extra = sorted(set(actual_row) - set(FINDING_KEYS))
        if extra:
            raise ValidationError(f"catalog.findings[{finding_id}]: schema-forbidden key(s) {extra} (D6)")
        for field, expected_value in expected_row.items():
            actual_value = actual_row.get(field, "<missing>")
            if actual_value != expected_value:
                raise ValidationError(
                    f"catalog.findings[{finding_id}].{field}: expected {expected_value!r}, found {actual_value!r}"
                )
    regression = by_finding_id.get(REGRESSION_FINDING, {}).get("related_claims", [])
    if REGRESSION_CLAIM not in regression:
        raise ValidationError(
            f"catalog.findings[{REGRESSION_FINDING}]: relation to {REGRESSION_CLAIM} missing (D4 regression)"
        )

    # Input digests bind all three sources.
    return {
        "surfaces": len(artifact_surfaces),
        "claims": len(artifact_claims),
        "findings": len(artifact_findings),
        "dimensioned": sum(1 for row in artifact_claims if row.get("dimensions")),
        "anti_claimed": sum(1 for row in artifact_claims if row.get("identity_anti_claims")),
        "relations": sum(len(row.get("related_claims") or []) for row in artifact_findings),
    }


def check_input_digests(artifact: dict[str, Any], doc: bytes, schema: bytes, manifest: bytes) -> None:
    digests = artifact.get("input_digests") or {}
    rogue = sorted(set(digests) - set(DIGEST_KEYS))
    if rogue:
        raise ValidationError(f"catalog.input_digests: schema-forbidden key(s) {rogue} (D6)")
    expected = {
        "inventory_document": "sha256:" + hashlib.sha256(doc).hexdigest(),
        "schema": "sha256:" + hashlib.sha256(schema).hexdigest(),
        "release_receipt_manifest": "sha256:" + hashlib.sha256(manifest).hexdigest(),
    }
    for key, value in expected.items():
        if digests.get(key) != value:
            raise ValidationError(
                f"catalog.input_digests.{key}: expected {value}, found {digests.get(key)} "
                "(a stale or swapped source digest is a tamper signal)"
            )


# ---------------------------------------------------------------------------
# Tamper probes
# ---------------------------------------------------------------------------

def load_json_bytes(raw: bytes) -> dict[str, Any]:
    return json.loads(raw)


def mutate(copy: dict[str, Any], path: list[Any], value: Any) -> None:
    node: Any = copy
    for key in path[:-1]:
        node = node[key]
    if value is None:
        del node[path[-1]]
    else:
        node[path[-1]] = value


def run_tamper_probes(
    artifact: dict[str, Any],
    inventory: dict[str, Any],
    manifest: dict[str, Any],
    doc: bytes,
    schema: bytes,
    manifest_raw: bytes,
) -> bool:
    all_caught = True
    results: list[tuple[str, bool, str]] = []

    def probe(name: str, mutate_fn) -> None:
        nonlocal all_caught
        copy = json.loads(json.dumps(artifact))
        mutate_fn(copy)
        try:
            compare_catalog(copy, inventory, manifest)
            check_input_digests(copy, doc, schema, manifest_raw)
        except ValidationError as error:
            results.append((name, True, str(error)))
        else:
            results.append((name, False, "tampered catalog still passed"))
            all_caught = False

    claims = artifact.get("claims") or []
    claim_index = {row["claim_id"]: index for index, row in enumerate(claims)}

    # Per-row probes: editing any one row must fail naming that row (D5).
    drift_alternates = {"current": "pending", "pending": "current", "mutable_pin": "current",
                        "source_drift": "current", "cross_surface_drift": "current",
                        "volatile_number": "current", "stale_example": "current",
                        "future_example": "current"}
    for index, row in enumerate(claims):
        claim_id = row["claim_id"]
        kind = index % 4
        if kind == 0:
            new_status = drift_alternates.get(row["drift_status"], "pending")
            probe(f"row:{claim_id}:drift_status",
                  lambda copy, i=index, v=new_status: mutate(copy, ["claims", i, "drift_status"], v))
        elif kind == 1:
            probe(f"row:{claim_id}:summary",
                  lambda copy, i=index: mutate(copy, ["claims", i, "summary"], copy["claims"][i]["summary"] + " TAMPERED"))
        elif kind == 2:
            probe(f"row:{claim_id}:notes",
                  lambda copy, i=index: mutate(copy, ["claims", i, "notes"], copy["claims"][i]["notes"] + " TAMPERED"))
        else:
            probe(f"row:{claim_id}:location",
                  lambda copy, i=index: mutate(copy, ["claims", i, "location"], copy["claims"][i]["location"] + "X"))

    # D2: removing an anti-claim must fail naming the row.
    for row in claims:
        if row.get("identity_anti_claims"):
            claim_id = row["claim_id"]
            probe(f"d2:{claim_id}:anti_claim_removed",
                  lambda copy, cid=claim_id: mutate(
                      copy, ["claims", claim_index[cid], "identity_anti_claims"], []))

    # D1: receipt fields must track the manifest.
    for row in claims:
        dimensions = row.get("dimensions") or {}
        if "windows_arm64" in dimensions:
            claim_id = row["claim_id"]
            probe(f"d1:{claim_id}:receipt_flipped",
                  lambda copy, cid=claim_id: mutate(
                      copy,
                      ["claims", claim_index[cid], "dimensions", "windows_arm64",
                       "published_receipt_v0_17_0"],
                      "present" if dimensions["windows_arm64"]["published_receipt_v0_17_0"] == "absent" else "absent"))

    # D6: rogue keys per dimension family, per nested object family.
    families_seen: dict[str, str] = {}
    for row in claims:
        for family in (row.get("dimensions") or {}):
            families_seen.setdefault(family, row["claim_id"])
    for family, claim_id in families_seen.items():
        probe(f"d6:{family}:rogue_key",
              lambda copy, cid=claim_id, fam=family: mutate(
                  copy, ["claims", claim_index[cid], "dimensions", fam, "rogue"], True))
    first_anti = next(
        (index for index, row in enumerate(claims) if row.get("identity_anti_claims")), None
    )

    def anti_claim_rogue(copy: dict[str, Any]) -> None:
        if first_anti is None:
            raise AssertionError("no anti-claimed row available for the probe")
        mutate(copy, ["claims", first_anti, "identity_anti_claims", 0, "rogue"], True)

    probe("d6:identity_anti_claim:rogue_key", anti_claim_rogue)
    probe("d6:root:rogue_key", lambda copy: mutate(copy, ["rogue"], True))
    probe("d6:input_digests:rogue_key", lambda copy: mutate(copy, ["input_digests", "rogue"], "x"))
    probe("d6:surface:rogue_key", lambda copy: mutate(copy, ["surfaces", 0, "rogue"], True))
    probe("d6:finding:rogue_key", lambda copy: mutate(copy, ["findings", 0, "rogue"], True))
    probe("d6:release_receipt:rogue_key", lambda copy: mutate(copy, ["release_receipts", 0, "rogue"], True))
    probe("d6:asset:rogue_key", lambda copy: mutate(copy, ["release_receipts", 0, "assets", 0, "rogue"], True))
    probe("digest:inventory_document_swapped",
          lambda copy: mutate(copy, ["input_digests", "inventory_document"], "sha256:" + "0" * 64))

    # Review hardening probes: root provenance constants and the D1 manifest
    # anchor binding (a copied successor manifest must fail, not silently
    # rewrite the historical receipt facts).
    probe("d6:root:status_flipped",
          lambda copy: mutate(copy, ["status"], "hand_edited"))
    probe("d6:root:generator_flipped",
          lambda copy: mutate(copy, ["generator"], "hand-edited"))
    probe("d6:root:issue_flipped", lambda copy: mutate(copy, ["issue"], 99999))
    anchor = inventory["release_anchor"]

    def manifest_release_mismatch(copy_manifest: dict[str, Any]) -> None:
        bad = json.loads(json.dumps(copy_manifest))
        bad["release"] = "v9.9.9" if anchor != "v9.9.9" else "v0.0.1"
        compare_catalog(artifact, inventory, bad)

    try:
        manifest_release_mismatch(manifest)
    except ValidationError as error:
        results.append(("d1:manifest_release_mismatch", True, str(error)))
    else:
        results.append(("d1:manifest_release_mismatch", False,
                        "a successor manifest still derived the v0.17.0 receipt facts"))
        all_caught = False

    def manifest_probe(name: str, mutate_fn) -> None:
        nonlocal all_caught
        copy = json.loads(manifest_raw)
        mutate_fn(copy)
        try:
            parse_receipt_manifest(json.dumps(copy).encode())
        except ValidationError as error:
            results.append((name, True, str(error)))
        else:
            results.append((name, False, "tampered receipt manifest still parsed"))
            all_caught = False

    manifest_probe("d1:manifest_unknown_root_key",
                   lambda copy: copy.update({"rogue": True}))
    manifest_probe("d1:manifest_unknown_asset_key",
                   lambda copy: copy["assets"][0].update({"rogue": True}))
    manifest_probe("d1:manifest_source_missing",
                   lambda copy: mutate(copy, ["source"], None))
    manifest_probe("d1:manifest_source_empty",
                   lambda copy: mutate(copy, ["source"], ""))
    manifest_probe("d1:manifest_release_malformed",
                   lambda copy: mutate(copy, ["release"], "0.17.0"))
    manifest_probe("d1:manifest_verified_date_malformed",
                   lambda copy: mutate(copy, ["verified_date"], "2026/08/28"))

    row_probes = [r for r in results if r[0].startswith("row:")]
    d1_probes = [r for r in results if r[0].startswith("d1:")]
    d2_probes = [r for r in results if r[0].startswith("d2:")]
    d6_probes = [r for r in results if r[0].startswith("d6:")]

    print()
    print("Tamper probes (each must fail validation naming its subject):")
    for name, caught, message in results:
        marker = "caught" if caught else "MISSED"
        print(f"  [{marker:>6}] {name}: {message if not caught else message[:110]}")
    print()
    print(
        f"tamper sweep: {len(row_probes)}/{len(claims)} row probes, "
        f"{sum(1 for _, c, _ in d1_probes if c)}/{len(d1_probes)} receipt probes, "
        f"{sum(1 for _, c, _ in d2_probes if c)}/{len(d2_probes)} anti-claim probes, "
        f"{sum(1 for _, c, _ in d6_probes if c)}/{len(d6_probes)} rogue-key probes"
    )
    for name, caught, message in results:
        if not caught:
            print(f"FAIL: {name}: {message}")
    return all_caught


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("artifact", nargs="?", default="distribution/public_release_claims.v2.json")
    parser.add_argument("--root", default=None, help="repository root (default: parent of this script)")
    parser.add_argument(
        "--prove-tamper", action="store_true",
        help="after a green validation, run the per-row and rogue-key tamper sweep",
    )
    args = parser.parse_args(argv)

    root = Path(args.root) if args.root else Path(__file__).resolve().parent.parent
    artifact_path = root / args.artifact if not Path(args.artifact).is_absolute() else Path(args.artifact)

    try:
        artifact_raw = artifact_path.read_bytes()
        doc = (root / DOC_PATH).read_bytes()
        schema = (root / SCHEMA_PATH).read_bytes()
        manifest_raw = (root / RECEIPT_MANIFEST_PATH).read_bytes()
    except OSError as error:
        print(f"FAIL: cannot read repository inputs: {error}")
        return 1

    try:
        artifact = load_json_bytes(artifact_raw)
        inventory = parse_inventory(doc.decode("utf-8"))
        manifest = parse_receipt_manifest(manifest_raw)
        stats = compare_catalog(artifact, inventory, manifest)
        check_input_digests(artifact, doc, schema, manifest_raw)
        closed_objects = validate_schema_closure(json.loads(schema))
        if closed_objects < 12:
            raise ValidationError(
                f"schema: closure walk covered only {closed_objects} object nodes (D6)"
            )
    except (ValidationError, json.JSONDecodeError) as error:
        print(f"FAIL: {error}")
        return 1

    print(
        "public_release_claims.v2 oracle green: "
        f"{stats['surfaces']} surfaces, {stats['claims']} claims, {stats['findings']} findings, "
        f"{stats['dimensioned']} dimensioned rows, {stats['anti_claimed']} anti-claimed rows, "
        f"{stats['relations']} derived relations, {closed_objects} closed schema objects"
    )

    if args.prove_tamper:
        if not run_tamper_probes(artifact, inventory, manifest, doc, schema, manifest_raw):
            print("FAIL: at least one tamper probe was missed")
            return 1
        print("tamper sweep verdict: every probe was caught")

    return 0


if __name__ == "__main__":
    sys.exit(main())
