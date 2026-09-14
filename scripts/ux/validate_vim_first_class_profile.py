#!/usr/bin/env python3
"""Offline deterministic fan-in validator for the vim/vim-lsp first-class evidence
profile (#11408).

Composes the exact-subject baseline core receipt plus matching specialized subset
receipts into the `vim_first_class_exact_source` profile disposition stored in
`.ci/editor-clients/vim-vim-lsp-first-class-profile.v1.json`. Pure evidence
composition and validation: no host action, no new receipt dialect, no support
or docs claim.

The consumed receipts speak the shared `editor_client_compat.v1` dialect
(#7777). Every referenced receipt must satisfy exact-subject equality against
the #11369 pinned subject manifest and against every other composed receipt;
composition otherwise fails closed. A missing or open producer yields an honest
`not_proven` cell — never a manufactured pass.

Standard library only; no network access.

Usage:
    python scripts/ux/validate_vim_first_class_profile.py [--quiet] [--repo-root PATH]

Exit codes: 0 = composition holds, 1 = violation found, 64 = usage error.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

DEFAULT_REPO_ROOT = Path(__file__).resolve().parents[2]
PROFILE_RELPATH = ".ci/editor-clients/vim-vim-lsp-first-class-profile.v1.json"
SUBJECT_RELPATH = ".ci/editor-clients/vim-vim-lsp-subject.v1.json"

PROFILE_SCHEMA = "vim_first_class_profile.v1"
PROFILE_NAME = "vim_first_class_exact_source"
RECEIPT_DIALECT = "editor_client_compat.v1"

DISPOSITIONS = {
    "pass",
    "pass_with_explicit_limitations",
    "partial",
    "not_proven",
    "fail",
}
# The journey/result vocabulary of the shared receipt dialect; the fan-in never
# widens it.
CELL_RESULTS = {"pass", "fail", "partial", "not_proven", "unsupported"}
REQUIRED_FAMILIES = (
    "baseline_core",
    "freshness",
    "save",
    "recovery",
    "host_lifecycle",
    "expanded_activation",
)
OPTIONAL_FAMILIES = ("workspace_folders",)
ALL_FAMILIES = REQUIRED_FAMILIES + OPTIONAL_FAMILIES
EQUALITY_FIELDS = (
    "candidate_sha",
    "expectation_set_digest",
    "expectation_set_id",
    "perllsp_artifact_sha256",
    "perllsp_build_revision",
    "platform_arch",
    "platform_os",
    "vim_lsp_selected_commit",
    "vim_lsp_tree_digest",
    "workspace_fixture_digest",
    "workspace_fixture_id",
)

FORBIDDEN_KEY_SUBSTRINGS = (
    "support_tier",
    "supported_version_row",
    "maintained_row",
    "public_artifact_claim",
    "release_channel",
    "readiness",
)

# Worst-of lattice used to fold bound journey-cell results into one family
# disposition, and family dispositions into the aggregate. `unsupported` is an
# honest terminal disposition below `pass` but above `partial`: it may survive
# as an explicit limitation, never silently become one.
RESULT_RANK = {"pass": 0, "unsupported": 1, "partial": 2, "not_proven": 3, "fail": 4}


class Violations:
    def __init__(self) -> None:
        self.items: list[str] = []
        self.structural = False

    def add(self, message: str) -> None:
        self.items.append(message)

    def add_structural(self, message: str) -> None:
        self.structural = True
        self.items.append(message)

    @property
    def ok(self) -> bool:
        return not self.items


def load_json(path: Path, violations: Violations, label: str) -> dict | None:
    if not path.is_file():
        violations.add(f"{label}: required artifact missing: {path}")
        return None
    try:
        with path.open("r", encoding="utf-8") as handle:
            data = json.load(handle)
    except json.JSONDecodeError as exc:
        violations.add(f"{label}: {path.name} is not valid JSON: {exc}")
        return None
    if not isinstance(data, dict):
        violations.add(f"{label}: {path.name} is not a JSON object")
        return None
    return data


def canonical_digest(document: dict) -> str:
    # Canonical form shared with the Rust contract test's `canonical_digest`
    # (`serde_json::to_vec` over `serde_json::Value`): compact separators,
    # lexicographically sorted keys, and raw UTF-8 output — never `\uXXXX`
    # escapes — so both computations agree byte-for-byte on every document.
    encoded = json.dumps(
        document, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def iter_keys(node: object):
    if isinstance(node, dict):
        for key, value in node.items():
            yield key
            yield from iter_keys(value)
    elif isinstance(node, list):
        for item in node:
            yield from iter_keys(item)


def check_forbidden_keys(document: dict, name: str, violations: Violations) -> None:
    for key in iter_keys(document):
        lowered = str(key).lower()
        for forbidden in FORBIDDEN_KEY_SUBSTRINGS:
            if forbidden in lowered:
                violations.add(f"{name}: forbidden state key '{key}'")


def validate_structure(profile: dict, violations: Violations) -> bool:
    """Check the fixed denominator and vocabularies; False when recomposition is unsafe."""
    ok = True
    if profile.get("schema_version") != PROFILE_SCHEMA:
        violations.add_structural(
            f"profile: unexpected schema_version {profile.get('schema_version')!r}"
        )
        ok = False
    if profile.get("profile") != PROFILE_NAME:
        violations.add(f"profile: unexpected profile name {profile.get('profile')!r}")
    if set(profile.get("disposition_vocabulary") or []) != DISPOSITIONS:
        violations.add("profile: disposition_vocabulary drifted from the fixed five-value set")
    if set(profile.get("cell_result_vocabulary") or []) != CELL_RESULTS:
        violations.add("profile: cell_result_vocabulary drifted from the receipt dialect")
    if sorted(profile.get("subject_equality_required_fields") or []) != sorted(EQUALITY_FIELDS):
        violations.add("profile: subject_equality_required_fields drifted from the fixed set")

    generated = profile.get("generated_from") or {}
    if generated.get("receipt_dialect") != RECEIPT_DIALECT:
        violations.add(f"profile: generated_from.receipt_dialect must be {RECEIPT_DIALECT}")

    inputs = profile.get("inputs")
    if not isinstance(inputs, dict) or sorted(inputs) != sorted(ALL_FAMILIES):
        violations.add_structural(
            "inputs: family denominator drifted from the fixed set "
            f"(expected {sorted(ALL_FAMILIES)}, found {sorted(inputs) if isinstance(inputs, dict) else inputs!r})"
        )
        return False
    for family in ALL_FAMILIES:
        spec = inputs[family]
        if not isinstance(spec, dict):
            violations.add_structural(f"inputs.{family}: expected an object")
            return False
        required = spec.get("required")
        if family in OPTIONAL_FAMILIES:
            if required is not False or spec.get("consumption_policy") != "consumes_if_available":
                violations.add(
                    f"inputs.{family}: the only optional family must declare "
                    "required=false with consumes_if_available"
                )
        elif required is not True:
            violations.add(f"inputs.{family}: a required family must declare required=true")
        if spec.get("state") not in ("producer_open", "receipt_registered"):
            violations.add(f"inputs.{family}: unknown state {spec.get('state')!r}")
        if spec.get("stage") != "exact_source_local":
            violations.add(
                f"inputs.{family}: first-class fan-in composes exact_source_local receipts only "
                "(NC5)"
            )
        if not isinstance(spec.get("authority_issue"), int):
            violations.add(f"inputs.{family}: authority_issue must cite its owning issue number")

    cells = profile.get("cells")
    if not isinstance(cells, dict) or sorted(cells) != sorted(ALL_FAMILIES):
        violations.add_structural(
            "cells: the aggregate invented or dropped a cell; the denominator is fixed "
            f"(expected {sorted(ALL_FAMILIES)}, found {sorted(cells) if isinstance(cells, dict) else cells!r})"
        )
        return False
    for family in ALL_FAMILIES:
        if not isinstance(cells[family], dict):
            violations.add_structural(f"cells.{family}: expected an object")
            return False
    return ok


def validate_subject_binding(
    repo_root: Path, profile: dict, violations: Violations
) -> tuple[str, str] | None:
    """The profile consumes the #11369 pin by reference and digest, never a copy."""
    generated = profile.get("generated_from") or {}
    if generated.get("subject_manifest") != SUBJECT_RELPATH:
        violations.add("profile: subject_manifest must point at the governed #11369 artifact path")

    subject = load_json(repo_root / SUBJECT_RELPATH, violations, "subject")
    if subject is None:
        return None
    digest = canonical_digest(subject)
    if generated.get("subject_content_sha256") != digest:
        violations.add(
            "profile: subject_content_sha256 does not match the current subject manifest "
            f"(recorded {generated.get('subject_content_sha256')}, actual {digest}); "
            "regenerate the profile after any pin movement (NC12)"
        )
    upstream = subject.get("upstream") or {}
    commit = str(upstream.get("selected_commit") or "")
    tree = str((upstream.get("tree_digest") or {}).get("value") or "")
    return commit, tree


def validate_receipt_reference(
    repo_root: Path,
    family: str,
    reference: object,
    violations: Violations,
    equality: dict[str, str],
    bound_cells: dict[str, str],
) -> tuple[list[str], bool]:
    """Validate one registered receipt reference; return its bound journey results."""
    label = f"cells.{family}"
    empty: tuple[list[str], bool] = ([], False)
    if not isinstance(reference, dict):
        violations.add(f"{label}: receipt reference must be an object")
        return empty
    relpath = reference.get("artifact")
    digest = reference.get("artifact_sha256")
    if not isinstance(relpath, str) or not relpath:
        violations.add(f"{label}: receipt reference missing artifact path")
        return empty
    if not isinstance(digest, str) or not digest.startswith("sha256:"):
        violations.add(f"{label}: receipt reference missing sha256 artifact digest")
        return empty

    # Receipts are repository artifacts: an absolute or traversing path would
    # let a reference hash and read arbitrary host files.
    path = (repo_root / relpath).resolve()
    if (
        Path(relpath).is_absolute()
        or ".." in Path(relpath).parts
        or not path.is_relative_to(repo_root)
    ):
        violations.add(f"{label}: receipt artifact path escapes the repository: {relpath}")
        return empty
    if not path.is_file():
        violations.add(f"{label}: referenced receipt artifact missing: {relpath}")
        return empty
    actual = "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != digest:
        violations.add(f"{label}: stale receipt digest for {relpath} (NC12)")
        return empty

    receipt = load_json(path, violations, label)
    if not isinstance(receipt, dict):
        return empty
    if receipt.get("schema_version") != RECEIPT_DIALECT:
        violations.add(
            f"{label}: substituted dialect {receipt.get('schema_version')!r}; first-class "
            f"cells compose {RECEIPT_DIALECT} host evidence only (NC13)"
        )
        return empty
    if receipt.get("stage") != "exact_source_local":
        violations.add(f"{label}: mixed evidence stage {receipt.get('stage')!r} (NC5)")
        return empty

    declared = reference.get("subject_equality")
    if not isinstance(declared, dict) or any(field not in declared for field in EQUALITY_FIELDS):
        missing = [
            field
            for field in EQUALITY_FIELDS
            if not isinstance(declared, dict) or field not in declared
        ]
        violations.add(f"{label}: subject equality block missing fields: {missing}")
        return empty
    # The declared block is an assertion beside the receipt, not the receipt's
    # own identity. Every field the dialect carries is derived from the receipt
    # and must agree with the declaration; the vim-lsp commit/tree pair is the
    # one identity the dialect does not carry, so it is checked only against the
    # #11369 pin and the other composed references.
    derived = receipt_identity(receipt)
    for field, value in derived.items():
        if declared.get(field) != value:
            violations.add(
                f"{label}: subject_equality.{field} declares {declared.get(field)!r} but the "
                f"receipt carries {value!r}; a foreign receipt cannot satisfy this subject (NC3)"
            )
    for field, value in declared.items():
        previous = equality.setdefault(field, value)
        if previous != value:
            violations.add(
                f"{label}: cross-receipt subject mismatch on {field}: {value!r} vs "
                f"{previous!r}; one Vim build cannot combine with another (NC3)"
            )

    if reference.get("fills") != family:
        violations.add(
            f"{label}: reference declares fills={reference.get('fills')!r}; manual or foreign "
            f"routes cannot fill the {family} family (NC6)"
        )

    journey_index = {
        cell.get("id"): cell
        for cell in receipt.get("journey") or []
        if isinstance(cell, dict) and isinstance(cell.get("id"), str)
    }
    # The shared dialect's own validator (`EditorClientCompatReceipt::validate`)
    # is the authority over receipt well-formedness and runs over every committed
    # reference in the xtask contract. This offline fan-in only refuses to fold a
    # journey result it cannot read honestly: unknown vocabulary, or a pass that
    # observed nothing.
    for cell_id, cell in journey_index.items():
        result = cell.get("result")
        observed = cell.get("observed")
        if result not in CELL_RESULTS or not isinstance(observed, bool):
            violations.add(
                f"{label}: journey cell {cell_id!r} is outside the {RECEIPT_DIALECT} "
                f"result/observed vocabulary (NC13)"
            )
            return empty
        if result == "pass" and not observed:
            violations.add(f"{label}: journey cell {cell_id!r} claims a pass it never observed")
            return empty
    bound_ids = reference.get("journey_cell_ids")
    if not isinstance(bound_ids, list) or not bound_ids:
        violations.add(f"{label}: reference binds no journey cell ids")
        return empty
    results: list[str] = []
    usable = True
    for cell_id in bound_ids:
        cell = journey_index.get(cell_id)
        if cell is None:
            violations.add(f"{label}: binds journey cell id {cell_id!r} absent from the receipt")
            usable = False
            continue
        owner = bound_cells.setdefault(cell_id, family)
        if owner != family:
            violations.add(
                f"{label}: journey cell {cell_id!r} already bound to {owner}; families cannot "
                "cross-fill each other's observations (NC7)"
            )
            usable = False
            continue
        results.append(str(cell.get("result")))
    return results, usable


def receipt_identity(receipt: dict) -> dict[str, object]:
    """The subject-equality fields an `editor_client_compat.v1` receipt owns itself."""

    def field(section: str, key: str) -> object:
        block = receipt.get(section)
        return block.get(key) if isinstance(block, dict) else None

    return {
        "candidate_sha": receipt.get("candidate_sha"),
        "platform_os": field("platform", "os"),
        "platform_arch": field("platform", "arch"),
        "perllsp_build_revision": field("server", "build_revision"),
        "perllsp_artifact_sha256": field("server", "artifact_sha256"),
        "workspace_fixture_id": field("workspace_fixture", "id"),
        "workspace_fixture_digest": field("workspace_fixture", "digest"),
        "expectation_set_id": field("workspace_fixture", "expectation_set_id"),
        "expectation_set_digest": field("workspace_fixture", "expectation_set_digest"),
    }


def worst(results: list[str]) -> str:
    return max(results, key=lambda value: RESULT_RANK.get(value, RESULT_RANK["not_proven"]))


def recompose(
    repo_root: Path,
    profile: dict,
    pinned_subject: tuple[str, str],
    violations: Violations,
) -> dict | None:
    """Derive the expected cells and aggregate deterministically from the inputs."""
    inputs = profile["inputs"]
    equality: dict[str, str] = {
        "vim_lsp_selected_commit": pinned_subject[0],
        "vim_lsp_tree_digest": pinned_subject[1],
    }
    bound_cells: dict[str, str] = {}

    expected_cells: dict[str, dict] = {}
    for family in ALL_FAMILIES:
        spec = inputs[family]
        references = spec.get("receipt_references") or []
        if spec.get("state") == "producer_open":
            if references:
                violations.add(
                    f"inputs.{family}: producer_open state cannot carry receipt references"
                )
            expected_cells[family] = {
                "result": "not_proven",
                "observed": False,
                "receipt_references": [],
            }
            continue
        if not references:
            violations.add(
                f"inputs.{family}: receipt_registered state requires at least one receipt "
                "reference"
            )
        folded_results: list[str] = []
        kept_references: list[object] = []
        for reference in references:
            results, usable = validate_receipt_reference(
                repo_root, family, reference, violations, equality, bound_cells
            )
            if not usable:
                continue
            kept_references.append(reference)
            if results:
                folded_results.append(worst(results))
            else:
                folded_results.append("not_proven")
        expected_cells[family] = {
            "result": worst(folded_results) if folded_results else "not_proven",
            "observed": bool(kept_references),
            "receipt_references": kept_references,
        }

    def aggregate(families: tuple[str, ...]) -> str:
        results = [expected_cells[family]["result"] for family in families]
        if any(value == "fail" for value in results):
            return "fail"
        if any(value == "not_proven" for value in results):
            return "not_proven"
        if any(value == "partial" for value in results):
            return "partial"
        if any(value == "unsupported" for value in results):
            return "pass_with_explicit_limitations"
        return "pass"

    # The required families alone set the aggregate floor. An optional family
    # composes under `consumes_if_available` (#11376): it can neither lift nor
    # lower that floor, but any non-pass optional evidence (missing, unsupported,
    # mismatched, or failed) turns a clean pass into an explicit limitation and
    # must stay visible in the aggregate rather than vanish from promotion (NC9,
    # NC10). It never touches the narrower #10962 core profile.
    required_only = aggregate(REQUIRED_FAMILIES)
    optional_limited = sorted(
        family for family in OPTIONAL_FAMILIES if expected_cells[family]["result"] != "pass"
    )
    aggregate_disposition = required_only
    if required_only == "pass" and optional_limited:
        aggregate_disposition = "pass_with_explicit_limitations"
    allowed = sorted(
        [
            family
            for family in REQUIRED_FAMILIES
            if expected_cells[family]["result"] == "unsupported"
        ]
        + optional_limited
    )
    open_required = sorted(
        family for family in REQUIRED_FAMILIES if inputs[family].get("state") == "producer_open"
    )
    return {
        "cells": expected_cells,
        "aggregate": aggregate_disposition,
        "allowed_limitations_retained": allowed,
        "open_required": open_required,
        "optional_limited": optional_limited,
    }


def compare_stored_vs_recomposed(
    profile: dict, recomputed: dict, violations: Violations
) -> None:
    cells = profile["cells"]
    inputs = profile["inputs"]
    for family in ALL_FAMILIES:
        expected = recomputed["cells"][family]
        stored = cells[family]
        stored_refs = stored.get("receipt_references") or []
        input_refs = inputs[family].get("receipt_references") or []
        if stored_refs != input_refs:
            violations.add(
                f"cells.{family}.receipt_references drifted from its inputs; a cell cannot "
                "cite evidence that did not determine its result"
            )
        if stored.get("result") != expected["result"]:
            violations.add(
                f"cells.{family}.result: stored {stored.get('result')!r} but inputs compose "
                f"{expected['result']!r}; regenerate the aggregate (stale generation)"
            )
        if bool(stored.get("observed")) != expected["observed"]:
            violations.add(f"cells.{family}.observed disagrees with composed reality")
        limitation = str(stored.get("limitation") or "")
        if expected["result"] in ("partial", "not_proven", "unsupported"):
            if not limitation.strip():
                violations.add(f"cells.{family}: non-passing cell requires a visible limitation")
            issue = inputs[family].get("authority_issue")
            if issue is not None and not input_refs and str(issue) not in limitation:
                violations.add(f"cells.{family}.limitation must name its open producer #{issue}")
        if input_refs == [] and stored.get("result") == "pass":
            violations.add(f"cells.{family}: claims pass without any registered receipt (NC1)")

    stored_aggregate = profile.get("aggregate_disposition")
    if stored_aggregate not in DISPOSITIONS:
        violations.add(
            f"profile: aggregate_disposition {stored_aggregate!r} is outside the vocabulary"
        )
    elif stored_aggregate != recomputed["aggregate"]:
        violations.add(
            f"profile: aggregate_disposition stored {stored_aggregate!r} but inputs compose "
            f"{recomputed['aggregate']!r}"
        )
    if stored_aggregate is not None and stored_aggregate != "pass":
        limitations = profile.get("aggregate_limitations") or []
        joined = " ".join(str(item) for item in limitations)
        if not any(str(item).strip() for item in limitations):
            violations.add("profile: non-pass aggregate requires visible aggregate limitations")
        for family in recomputed["open_required"]:
            issue = inputs[family].get("authority_issue")
            if str(issue) not in joined:
                violations.add(
                    f"profile: aggregate limitations must surface open required family "
                    f"{family} (#{issue})"
                )
        for family in recomputed["optional_limited"]:
            issue = inputs[family].get("authority_issue")
            if str(issue) not in joined:
                violations.add(
                    f"profile: aggregate limitations must surface the optional family "
                    f"{family} limitation (#{issue}) instead of dropping it (NC10)"
                )
    stored_allowed = profile.get("allowed_limitations_retained") or []
    if sorted(str(item) for item in stored_allowed) != recomputed["allowed_limitations_retained"]:
        violations.add(
            "profile: allowed_limitations_retained drifted; permitted limitations must stay "
            "visible and unearned ones must not appear (NC10)"
        )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--quiet", action="store_true", help="print nothing on success")
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=DEFAULT_REPO_ROOT,
        help="repository root (test seam; defaults to the checkout containing this script)",
    )
    args = parser.parse_args(argv)
    repo_root = args.repo_root.resolve()

    violations = Violations()
    profile = load_json(repo_root / PROFILE_RELPATH, violations, "profile")
    if profile is None:
        return finish(violations, args.quiet)

    check_forbidden_keys(profile, "profile", violations)
    structurally_sound = validate_structure(profile, violations)
    pinned_subject = validate_subject_binding(repo_root, profile, violations)
    if not structurally_sound or pinned_subject is None:
        return finish(violations, args.quiet)

    recomputed = recompose(repo_root, profile, pinned_subject, violations)
    if recomputed is not None:
        compare_stored_vs_recomposed(profile, recomputed, violations)

    return finish(violations, args.quiet)


def finish(violations: Violations, quiet: bool) -> int:
    if not violations.ok:
        for item in violations.items:
            print(f"FAIL: {item}")
        print(
            "vim/vim-lsp first-class fan-in validation FAILED with "
            f"{len(violations.items)} violation(s)"
        )
        return 1
    if not quiet:
        print("vim/vim-lsp first-class fan-in validation PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
