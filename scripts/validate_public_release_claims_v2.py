#!/usr/bin/env python3
"""Validate the deterministic public_release_claims.v2 install-claim catalog (#11548).

Independent of the Rust gate (`cargo xtask public-release-claims-v2 check`):
re-hashes the live inputs, re-derives every denominator expectation, enforces
the closed schema surface, and rejects v1 documents outright so both versions
coexist without one silently satisfying the other's validator.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

SCHEMA_VERSION = "public_release_claims.v2"
DRIFT_STATUSES = {
    "current",
    "pending",
    "stale_example",
    "future_example",
    "mutable_pin",
    "cross_surface_drift",
    "source_drift",
    "volatile_number",
}
OMITTED_CAVEATS = {"homebrew_tap_version_unproven", "crates_io_name_collision", "identity_bound_wrapper_framing"}
RESTATEMENT_GROUPS = {"bootstrap_identity", "verification_probes", None}
OWNER_ROUTES = {"#10342-ci-cutover", "#11549-classifier", "distribution-docs-sync", "none_recorded"}
EXPECTED_FINDINGS = [f"FND-{number}" for number in range(1, 13)]
EXPECTED_SURFACES = [f"S{index:02d}" for index in range(1, 14)]
EXPECTED_CLAIM_IDS = [
    "C101", "C102", "C103", "C104", "C105", "C106", "C107", "C108",
    "C201", "C202", "C203", "C204", "C205", "C206", "C207", "C208", "C209",
    "C210", "C211", "C212", "C213", "C214", "C215", "C216",
    "C301", "C302", "C303",
    "C401", "C402", "C403", "C404", "C405", "C406",
    "C501", "C502", "C503",
    "C601",
    "C701", "C702", "C703",
    "C801",
    "C901", "C902",
    "C1001", "C1002", "C1003", "C1004", "C1005", "C1006", "C1007", "C1008",
    "C1101", "C1102",
    "C1201", "C1202", "C1203", "C1204", "C1205", "C1206", "C1207", "C1208",
    "C1301", "C1302", "C1303", "C1304", "C1305", "C1306", "C1307", "C1308",
    "C1309",
]

SURFACE_KEYS = {"surface_id", "path", "role", "claim_class", "registry_cross_ref"}
CLAIM_KEYS = {
    "claim_id",
    "surface_id",
    "location",
    "summary",
    "drift_status",
    "notes",
    "finding_refs",
    "restatement_group",
    "omitted_caveats",
}
TOP_LEVEL_KEYS = {
    "schema_version",
    "status",
    "generator",
    "issue",
    "source_inventory",
    "input_digests",
    "surfaces",
    "claims",
    "findings",
}
DIMENSION_KEYS = {"windows_arm64", "sha256sums_enforcement", "product_units"}


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def _sha256(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def validate_catalog(root: Path) -> dict[str, int]:
    catalog_path = root / "distribution" / "public_release_claims.v2.json"
    doc_path = root / "docs" / "distribution" / "INSTALL_CLAIM_SURFACES.md"
    schema_path = root / "schemas" / "public_release_claims.v2.schema.json"

    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    _require(isinstance(catalog, dict), "catalog must be an object")
    version = catalog.get("schema_version")
    if version == "public_release_claims.v1":
        raise ValueError(
            "public_release_claims.v1 document passed to the v2 validator; "
            "v1 stays historical and keeps its own validator"
        )
    _require(version == SCHEMA_VERSION, f"schema_version must be {SCHEMA_VERSION}")
    _require(set(catalog) == TOP_LEVEL_KEYS, f"top-level keys closed: {sorted(catalog)}")
    _require(catalog.get("status") == "generated", "status must be 'generated'")
    _require(catalog.get("generator") == "cargo xtask public-release-claims-v2 build --write", "generator stamp mismatch")

    inventory = catalog["source_inventory"]
    _require(inventory.get("path") == "docs/distribution/INSTALL_CLAIM_SURFACES.md", "source inventory path pinned")
    _require(re.fullmatch(r"[0-9a-f]{7,40}", inventory.get("audited_commit", "")) is not None, "audited_commit shape")
    _require(re.fullmatch(r"\d{4}-\d{2}-\d{2}", inventory.get("audited_date", "")) is not None, "audited_date shape")
    _require(re.fullmatch(r"v\d+\.\d+\.\d+", inventory.get("release_anchor", "")) is not None, "release_anchor shape")
    _require(inventory.get("track") == "public-beta", "track pinned to public-beta")

    digests = catalog["input_digests"]
    _require(digests.get("inventory_document") == _sha256(doc_path), "inventory document digest is stale or tampered")
    _require(digests.get("schema") == _sha256(schema_path), "schema digest is stale or tampered")

    surfaces = catalog["surfaces"]
    _require(len(surfaces) == len(EXPECTED_SURFACES), f"surfaces must total {len(EXPECTED_SURFACES)}")
    seen_surfaces: list[str] = []
    for surface in surfaces:
        _require(set(surface) == SURFACE_KEYS, f"surface keys closed: {sorted(surface)}")
        _require(surface["surface_id"] in EXPECTED_SURFACES, f"unknown surface id {surface['surface_id']}")
        _require(surface["surface_id"] not in seen_surfaces, f"duplicate surface {surface['surface_id']}")
        seen_surfaces.append(surface["surface_id"])
        _require(isinstance(surface["path"], str) and surface["path"], "surface path required")
        _require(surface["registry_cross_ref"] == "" or isinstance(surface["registry_cross_ref"], str), "cross-ref shape")

    claims = catalog["claims"]
    _require(len(claims) == len(EXPECTED_CLAIM_IDS), f"claims must total {len(EXPECTED_CLAIM_IDS)}, found {len(claims)}")
    claim_ids: set[str] = set()
    dimensioned_rows = 0
    for claim in claims:
        _require(set(claim) <= CLAIM_KEYS | {"dimensions"}, f"{claim.get('claim_id')}: unknown keys present")
        _require(CLAIM_KEYS <= set(claim), f"{claim.get('claim_id')}: missing keys")
        claim_id = claim["claim_id"]
        _require(re.fullmatch(r"C\d{3,4}", claim_id) is not None, f"bad claim id {claim_id}")
        _require(claim_id not in claim_ids, f"duplicate route authority {claim_id}")
        claim_ids.add(claim_id)
        _require(claim_id in EXPECTED_CLAIM_IDS, f"{claim_id} outside recorded denominator")
        _require(claim["surface_id"] in EXPECTED_SURFACES, f"{claim_id}: unknown surface")
        _require(claim["location"] and isinstance(claim["location"], str), f"{claim_id}: location required")
        _require(claim["summary"] and isinstance(claim["summary"], str), f"{claim_id}: summary required")
        _require(claim["drift_status"] in DRIFT_STATUSES, f"{claim_id}: bad drift status {claim['drift_status']}")
        _require(isinstance(claim["notes"], str), f"{claim_id}: notes must be a string")
        refs = claim["finding_refs"]
        _require(all(ref in EXPECTED_FINDINGS for ref in refs), f"{claim_id}: unknown finding ref")
        _require(len(set(refs)) == len(refs), f"{claim_id}: duplicate finding ref")
        _require(claim["restatement_group"] in RESTATEMENT_GROUPS, f"{claim_id}: bad restatement group")
        caveats = claim["omitted_caveats"]
        _require(set(caveats) <= OMITTED_CAVEATS, f"{claim_id}: unknown omitted caveat")
        _require(len(caveats) == len(set(caveats)) <= 4, f"{claim_id}: caveat list malformed")

        dimensions = claim.get("dimensions")
        if dimensions is not None:
            _require(dimensions.keys() <= DIMENSION_KEYS, f"{claim_id}: unknown dimension")
            if dimensions:
                dimensioned_rows += 1
                arm = dimensions.get("windows_arm64")
                if arm is not None:
                    # The three conjunctive fields stay independent, never collapsed.
                    _require({"user_prose", "tracked_source", "published_receipt_v0_17_0"} <= set(arm),
                             f"{claim_id}: windows_arm64 needs all three independent directions")
                    _require(arm["user_prose"] in {"unsupported", "x64_fallback_build_from_source", "supported", "unspecified"},
                             f"{claim_id}: bad user_prose {arm['user_prose']}")
                    _require(arm["tracked_source"] in {"not_built", "built", "unknown"},
                             f"{claim_id}: bad tracked_source")
                    _require(arm["published_receipt_v0_17_0"] in {"absent", "present", "unverified"},
                             f"{claim_id}: bad receipt state")
                enforcement = dimensions.get("sha256sums_enforcement")
                if enforcement is not None:
                    _require(enforcement.get("mode") in {"fail_closed_required", "fail_open_conditional", "verify_present_no_mode", "not_applicable"},
                             f"{claim_id}: bad SHA256SUMS mode")
                units = dimensions.get("product_units")
                if units is not None:
                    _require("build_from_source_units" in units, f"{claim_id}: product_units needs build_from_source_units")
                    for entry in units.get("build_from_source_units", []):
                        _require(entry in {"perllsp", "perl-dap", "extension"}, f"{claim_id}: bad unit {entry}")
                    tracked_adapter = units.get("tracked_installer_ships_adapter")
                    _require(tracked_adapter is None or isinstance(tracked_adapter, bool),
                             f"{claim_id}: tracked_installer_ships_adapter must be boolean/null")

    findings = catalog["findings"]
    _require([finding["finding_id"] for finding in findings] == EXPECTED_FINDINGS,
             "findings must be FND-1..FND-12 in order")
    for finding in findings:
        _require(set(finding) == {"finding_id", "title", "related_claims", "owner_route"}, "finding keys closed")
        _require(bool(finding["title"]), f"{finding['finding_id']}: title required")
        _require(finding["owner_route"] in OWNER_ROUTES, f"{finding['finding_id']}: bad owner route")
        for related in finding["related_claims"]:
            _require(related in claim_ids, f"{finding['finding_id']}: dangling related claim {related}")

    return {
        "surfaces": len(surfaces),
        "claims": len(claims),
        "findings": len(findings),
        "dimensioned_rows": dimensioned_rows,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    arguments = parser.parse_args()
    try:
        stats = validate_catalog(arguments.root)
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print(
        "public_release_claims.v2 ok: "
        f"{stats['surfaces']} surfaces, {stats['claims']} claims, "
        f"{stats['findings']} findings, {stats['dimensioned_rows']} dimensioned rows"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
