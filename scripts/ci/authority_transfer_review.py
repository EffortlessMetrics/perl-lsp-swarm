#!/usr/bin/env python3
"""Advisory exact-head Authority Transfer Review evaluator (issue #11795).

Consumes the checked #11793 review-surface denominator (policy/review-surfaces.toml
plus its byte-deterministic generated projection) as the advisory context taxonomy
and computes one typed terminal verdict per governed surface row:

  candidate changes no governed surface       -> PASS_NOT_APPLICABLE
  every governed row carries current evidence -> PASS_CURRENT_REVIEW
  any governed row lacks current evidence     -> typed FAIL_*/NOT_PROVEN_* (non-green)

This evaluator is trusted base/default-branch code. Candidate-tree material is
consumed only as bounded data; nothing from the candidate tree is executed. A
CODEOWNERS match, approving review state, resolved threads, bot comment, green
check, or generated packet alone never satisfies the review proposition: evidence
must be an agent_review_packet.v1 document (#10881 closed contract) bound to the
exact evaluated head, carrying the manifest row's review profile, an independent
adversarial challenge role with required lens coverage, a first-falsifier
negative-control audit with established-criterion evidence, and typed predecessor
dispositions wherever the row declares a predecessor exit.

Result vocabulary (issue #11795, closed):

  PASS_NOT_APPLICABLE                no governed surface changed
  PASS_CURRENT_REVIEW                every governed row's evidence is current
  FAIL_REVIEW_MISSING                governed row has no reviewer packet
  FAIL_REVIEW_STALE_HEAD             packet binds another head
  FAIL_REVIEW_PROFILE_MISMATCH       packet profile/lens/role coverage mismatch
  FAIL_DENOMINATOR_INCOMPLETE        strict denominator validation failed
  FAIL_FIRST_FALSIFIER_MISSING       negative-control audit absent or vacuous
  FAIL_ARTIFACT_REVIEW_INCOMPLETE    packet violates the closed packet contract
  FAIL_PREDECESSOR_REVIEW_INCOMPLETE predecessor exit undispositioned
  FAIL_CLAIM_CEILING_EXCEEDED        evidence claims a different repository subject
  FAIL_CONTROLLER_RELATION           contradictory ownership / duplicate authority
  NOT_PROVEN_GITHUB                  input identity/bounds could not be established
  NOT_PROVEN_SUBJECT                 supplied artifact cannot bind a review subject
  INSTRUMENT_FAILURE                 the evaluator itself is broken

Exit codes mirror .github/workflows/semantic-close-containment.yml:
  0 - pass (PASS_NOT_APPLICABLE or PASS_CURRENT_REVIEW)
  1 - typed review failure (FAIL_*)
  3 - not-proven/instrument failure (NOT_PROVEN_*, INSTRUMENT_FAILURE)
A pass means only that the review evidence required by the governed surface is
current and internally consistent; product behavior, merge readiness, semantic
issue closure, and live policy remain separate. No threshold here authorizes
required enforcement; promotion belongs to #11796.

Denominator boundary: the full detector-set totality walk stays owned by
scripts/ci/validate_review_surfaces.py (--strict); this evaluator re-derives the
same typed checks in-process over any supplied tree (base and, optionally, the
candidate tree as data) so a denominator-invalidated input can never publish a
valid-looking context. Evidence consumption for trusted_workflow_run rows lands
with the #11701/#11703 handoff integration and reports NOT_PROVEN_GITHUB until
then; checked_projection rows are satisfiable today.

Usage:
  scripts/ci/authority_transfer_review.py --root PATH
      [--candidate-root PATH] [--repository OWNER/NAME] [--pr-number N]
      [--base-sha SHA] [--head-sha SHA]
      [--changed-list PATH | --changed-file PATH]...
      [--packet PATH]... [--max-changed-files N]
      [--receipt PATH] [--summary PATH] [--self-test]

With --self-test the evaluator replays internal expected-verdict fixtures and
exits non-zero on any drift, so a broken instrument never masquerades as a pass.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path
from typing import Any

_HERE = Path(__file__).resolve().parent
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))

import validate_review_surfaces as vrs  # noqa: E402

DEFAULT_MANIFEST = Path("policy/review-surfaces.toml")
DEFAULT_PROJECTION = Path("docs/policy/REVIEW_SURFACES.md")

PASS_NOT_APPLICABLE = "PASS_NOT_APPLICABLE"
PASS_CURRENT_REVIEW = "PASS_CURRENT_REVIEW"
FAIL_REVIEW_MISSING = "FAIL_REVIEW_MISSING"
FAIL_REVIEW_STALE_HEAD = "FAIL_REVIEW_STALE_HEAD"
FAIL_REVIEW_PROFILE_MISMATCH = "FAIL_REVIEW_PROFILE_MISMATCH"
FAIL_DENOMINATOR_INCOMPLETE = "FAIL_DENOMINATOR_INCOMPLETE"
FAIL_FIRST_FALSIFIER_MISSING = "FAIL_FIRST_FALSIFIER_MISSING"
FAIL_ARTIFACT_REVIEW_INCOMPLETE = "FAIL_ARTIFACT_REVIEW_INCOMPLETE"
FAIL_PREDECESSOR_REVIEW_INCOMPLETE = "FAIL_PREDECESSOR_REVIEW_INCOMPLETE"
FAIL_CLAIM_CEILING_EXCEEDED = "FAIL_CLAIM_CEILING_EXCEEDED"
FAIL_CONTROLLER_RELATION = "FAIL_CONTROLLER_RELATION"
NOT_PROVEN_GITHUB = "NOT_PROVEN_GITHUB"
NOT_PROVEN_SUBJECT = "NOT_PROVEN_SUBJECT"
INSTRUMENT_FAILURE = "INSTRUMENT_FAILURE"

# Worst-first severity order used to aggregate one terminal result.
SEVERITY_ORDER = (
    INSTRUMENT_FAILURE,
    FAIL_DENOMINATOR_INCOMPLETE,
    NOT_PROVEN_GITHUB,
    NOT_PROVEN_SUBJECT,
    FAIL_CLAIM_CEILING_EXCEEDED,
    FAIL_CONTROLLER_RELATION,
    FAIL_PREDECESSOR_REVIEW_INCOMPLETE,
    FAIL_FIRST_FALSIFIER_MISSING,
    FAIL_ARTIFACT_REVIEW_INCOMPLETE,
    FAIL_REVIEW_PROFILE_MISMATCH,
    FAIL_REVIEW_STALE_HEAD,
    FAIL_REVIEW_MISSING,
    PASS_CURRENT_REVIEW,
    PASS_NOT_APPLICABLE,
)

EXIT_PASS = 0
EXIT_TYPED_FAILURE = 1
EXIT_NOT_PROVEN = 3

STATUS_BOUNDARY = (
    "Advisory context only: a pass means the review evidence required by the "
    "governed surface is current and internally consistent. Product behavior, "
    "merge readiness, semantic issue closure, and live policy remain separate."
)

REQUIRED_PACKET_SECTIONS = (
    "packet_id",
    "subject",
    "challenge",
    "lenses",
    "negative_controls",
    "old_paths",
    "obligations",
    "roles",
    "lifecycle",
)

PACKET_TOP_LEVEL_KEYS = REQUIRED_PACKET_SECTIONS + (
    "schema",
    "schema_version",
    "metadata",
)

NEGATIVE_CONTROL_CRITERIA = (
    "exists",
    "red_before_or_mutation_evidence",
    "passes_only_intended_implementation",
    "correct_subject_and_generation",
    "independent_expectation_source",
    "alternate_subject_exclusion",
)

OLD_PATH_DISPOSITIONS = (
    "removed",
    "unreachable",
    "compatibility_projection",
    "historical_salvage_only",
    "still_live_independent",
    "unexpected_duplicate",
)

DEFAULT_MAX_CHANGED_FILES = 20000


def sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def worst(result_a: str, result_b: str) -> str:
    if result_a == result_b:
        return result_a
    return (
        result_a
        if SEVERITY_ORDER.index(result_a) <= SEVERITY_ORDER.index(result_b)
        else result_b
    )


def aggregate(results: list[str]) -> str:
    if not results:
        return PASS_NOT_APPLICABLE
    current = results[0]
    for result in results[1:]:
        current = worst(current, result)
    return current


# ---------------------------------------------------------------------------
# Denominator validation (re-derived from the #11793 checked contract)
# ---------------------------------------------------------------------------


def load_manifest_document(root: Path) -> dict[str, Any]:
    manifest_path = root / DEFAULT_MANIFEST
    try:
        with manifest_path.open("rb") as handle:
            return tomllib.load(handle)
    except FileNotFoundError:
        raise ValueError(f"{DEFAULT_MANIFEST.as_posix()}: manifest_unreadable (missing)")
    except OSError as error:
        raise ValueError(f"{DEFAULT_MANIFEST.as_posix()}: manifest_unreadable ({error})")
    except tomllib.TOMLDecodeError:
        raise ValueError(f"{DEFAULT_MANIFEST.as_posix()}: toml_parse_error")


def validate_denominator_tree(root: Path) -> list[str]:
    """Run the full typed #11793 invariant set in-process against one tree.

    Mirrors scripts/ci/validate_review_surfaces.py main(): closed vocabularies,
    mandatory fields, routing dispositions, platform-validated owners, binding
    conflicts, sensitive-path detector totality, self-surface binding, residue
    shape, CODEOWNERS owners, and byte-exact generated projection."""
    issues: list[str] = []
    try:
        doc = load_manifest_document(root)
    except ValueError as error:
        return [str(error)]
    if not doc:
        # An emptied manifest parses to {} and would otherwise report zero
        # issues, publishing a green context for a candidate that removed
        # the entire denominator. Fail closed instead (#11793).
        issues.append(f"{DEFAULT_MANIFEST.as_posix()}: denominator_incomplete (empty manifest)")
        return issues
    vrs.validate_header(doc, issues)
    vrs.validate_profiles(doc.get("profile"), issues)
    valid_owners = vrs.validate_identities(doc.get("code_owner_identity"), issues)
    surfaces = doc.get("surface") if isinstance(doc.get("surface"), dict) else {}
    bindings = vrs.collect_surface_bindings(doc, root, issues, valid_owners)
    vrs.check_binding_conflicts(bindings, issues)
    sensitive_files = vrs.detect_sensitive_files(root, issues)
    vrs.check_coverage(sensitive_files, bindings, issues)
    vrs.check_self_surface(surfaces, bindings, issues)
    vrs.validate_residue(doc.get("residue"), issues)
    codeowners_rows = vrs.parse_codeowners(root / vrs.DEFAULT_CODEOWNERS, issues)
    vrs.check_codeowners_owners(codeowners_rows, valid_owners, issues)
    try:
        rendered = vrs.render_projection(doc)
    except Exception as error:  # noqa: BLE001 - typed below as instrument drift
        issues.append(f"projection_render_failed ({error.__class__.__name__})")
        rendered = None
    if rendered is not None:
        projection_path = root / DEFAULT_PROJECTION
        try:
            committed = projection_path.read_text(encoding="utf-8")
        except OSError:
            issues.append(
                f"{vrs.DEFAULT_PROJECTION}: projection_stale (generated projection missing)"
            )
            committed = ""
        if committed != rendered:
            issues.append(
                f"{vrs.DEFAULT_PROJECTION}: projection_stale (regenerated projection "
                f"differs from the committed file)"
            )
    return sorted(set(issues))


# ---------------------------------------------------------------------------
# Changed-path matching (reuses the #11793 binding semantics verbatim)
# ---------------------------------------------------------------------------


def read_changed_files(inputs: dict[str, Any]) -> tuple[list[str], bool]:
    raw_paths: list[str] = []
    listed: Path | None = inputs["changed_list"]
    if listed is not None:
        try:
            # Strict decode: a replacement character would let a path the
            # evaluator could not actually read be scored as an ordinary
            # non-governed file, publishing a definite verdict over an input
            # whose identity was never established. Undecodable bytes are an
            # input-identity failure (NOT_PROVEN_GITHUB), not a silent miss.
            raw_paths.extend(listed.read_text(encoding="utf-8").splitlines())
        except OSError as error:
            raise ValueError(f"changed_list_unreadable ({error})")
        except UnicodeDecodeError as error:
            raise ValueError(f"changed_list_undecodable ({error})")
    raw_paths.extend(inputs["changed_files"])
    normalized: list[str] = []
    seen: set[str] = set()
    truncated = False
    for raw in raw_paths:
        candidate = vrs.normalize(raw)
        if not candidate or candidate in seen:
            continue
        seen.add(candidate)
        if len(normalized) >= inputs["max_changed_files"]:
            truncated = True
            break
        normalized.append(candidate)
    return sorted(normalized), truncated


def governed_rows(
    doc: dict[str, Any],
    changed_files: list[str],
) -> tuple[list[dict[str, Any]], bool]:
    """Match changed files against surface bindings using vrs.binding_covers;
    contradictory ownership over one changed path is observed, never assumed."""
    surfaces = doc.get("surface") if isinstance(doc.get("surface"), dict) else {}
    bindings: list[tuple[str, str]] = []
    for surface_id in sorted(surfaces):
        body = surfaces[surface_id]
        if not isinstance(body, dict):
            continue
        paths = body.get("paths")
        if not isinstance(paths, list):
            continue
        for raw_path in paths:
            if isinstance(raw_path, str) and raw_path.strip():
                bindings.append((surface_id, vrs.normalize(raw_path)))
    claims_by_file: dict[str, set[str]] = {}
    for file_rel in changed_files:
        for surface_id, pattern in bindings:
            if vrs.binding_covers(pattern, file_rel):
                claims_by_file.setdefault(file_rel, set()).add(surface_id)
    overlap_detected = any(len(owners) > 1 for owners in claims_by_file.values())
    matched_by_surface: dict[str, list[str]] = {}
    for file_rel in sorted(claims_by_file):
        for surface_id in sorted(claims_by_file[file_rel]):
            matched_by_surface.setdefault(surface_id, []).append(file_rel)
    profiles = doc.get("profile") if isinstance(doc.get("profile"), dict) else {}
    rows: list[dict[str, Any]] = []
    for surface_id in sorted(matched_by_surface):
        body = surfaces[surface_id]
        profile_name = str(body.get("review_profile", ""))
        profile_body = profiles.get(profile_name)
        profile_body = profile_body if isinstance(profile_body, dict) else {}
        lenses = profile_body.get("lenses")
        rows.append(
            {
                "surface_id": surface_id,
                "family": str(body.get("family", "")),
                "conflict_key": str(body.get("conflict_key", "")),
                "risk_class": str(body.get("risk_class", "")),
                "review_profile": profile_name,
                "required_evidence": str(body.get("required_evidence", "")),
                "required_lenses": [
                    str(item) for item in lenses
                ] if isinstance(lenses, list) else [],
                "predecessor_exit": str(body.get("predecessor_exit", "")),
                "matched_paths": sorted(matched_by_surface[surface_id]),
            }
        )
    return rows, overlap_detected


# ---------------------------------------------------------------------------
# Reviewer-packet classification (agent_review_packet.v1, exact-head)
# ---------------------------------------------------------------------------

def _packet_fail(
    path_label: str,
    digest: str,
    verdict: str,
    reason: str,
    profile: str = "",
) -> dict[str, Any]:
    return {
        "path": path_label,
        "sha256": digest,
        "verdict": verdict,
        "reason": reason,
        "covered_surfaces": [],
        "covered_refs": [],
        "profile": profile,
        "head_binding": "absent",
        "repo_subject": "",
        "seam_dispositions": [],
        "lens_state": {},
        "required_roles": [],
    }


def packet_field(packet: dict[str, Any], *keys: str) -> Any:
    cursor: Any = packet
    for key in keys:
        if not isinstance(cursor, dict) or key not in cursor:
            return None
        cursor = cursor[key]
    return cursor


def load_packet(path_label: str, path: Path, evaluated_head: str) -> dict[str, Any]:
    try:
        payload = path.read_bytes()
    except OSError as error:
        packet = _packet_fail(path_label, "", NOT_PROVEN_SUBJECT, f"packet_unreadable ({error.__class__.__name__})")
        return packet
    digest = sha256_bytes(payload)
    try:
        packet_obj = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return _packet_fail(path_label, digest, FAIL_ARTIFACT_REVIEW_INCOMPLETE, "packet_not_decodable_json_object")
    if not isinstance(packet_obj, dict):
        return _packet_fail(path_label, digest, FAIL_ARTIFACT_REVIEW_INCOMPLETE, "packet_not_json_object")
    return validate_packet(path_label, digest, packet_obj, evaluated_head)


def validate_packet(
    path_label: str,
    digest: str,
    packet: dict[str, Any],
    evaluated_head: str,
) -> dict[str, Any]:
    """Structural exact-head validation against the closed agent_review_packet.v1
    contract (#10881). Canonical projections stay with the #10881 machinery; this
    checks the fields the advisory context depends on, failing closed."""

    def fail(verdict: str, reason: str) -> dict[str, Any]:
        return _packet_fail(
            path_label,
            digest,
            verdict,
            reason,
            profile=str(packet_field(packet, "subject", "programme", "profile") or ""),
        )

    if packet.get("schema") != "agent_review_packet.v1" or packet.get("schema_version") != 1:
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "unknown_packet_generation")
    unknown = sorted(set(packet) - set(PACKET_TOP_LEVEL_KEYS))
    if unknown:
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, f"unknown_packet_field ({unknown[0]})")
    missing_sections = [key for key in REQUIRED_PACKET_SECTIONS if key not in packet]
    if missing_sections:
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, f"missing_packet_section ({missing_sections[0]})")

    repository = packet_field(packet, "subject", "repository")
    if not isinstance(repository, dict):
        return fail(NOT_PROVEN_SUBJECT, "subject_repository_missing")
    identity_fields = {key: repository.get(key) for key in ("name", "base", "head", "tree", "diff")}
    if not all(isinstance(value, str) and value.strip() for value in identity_fields.values()):
        return fail(NOT_PROVEN_SUBJECT, "subject_identity_incomplete")

    programme_profile = packet_field(packet, "subject", "programme", "profile")
    if not isinstance(programme_profile, str) or not programme_profile.strip():
        return fail(NOT_PROVEN_SUBJECT, "review_profile_unbound")

    builder_contract = packet_field(packet, "subject", "builder_packet", "contract")
    builder_digest = packet_field(packet, "subject", "builder_packet", "digest")
    if not all(
        isinstance(value, str) and value.strip() for value in (builder_contract, builder_digest)
    ):
        return fail(NOT_PROVEN_SUBJECT, "builder_identity_unbound")

    authorities = packet_field(packet, "subject", "changed", "authorities")
    if not isinstance(authorities, list) or not authorities:
        return fail(NOT_PROVEN_SUBJECT, "changed_authorities_empty")
    covered_refs: list[str] = []
    for authority in authorities:
        if not isinstance(authority, dict):
            continue
        ref = authority.get("ref")
        subject_text = authority.get("subject")
        if (
            isinstance(ref, str)
            and ref.strip()
            and isinstance(subject_text, str)
            and subject_text.strip()
        ):
            covered_refs.append(ref)

    roles = packet.get("roles")
    if not isinstance(roles, list) or not roles:
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "roles_missing")
    required_roles: list[str] = []
    for role_row in roles:
        if not isinstance(role_row, dict):
            return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "malformed_role_row")
        role_name = role_row.get("role")
        if role_name not in vrs.ROLES:
            return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, f"invented_role ({role_name!r})")
        if role_row.get("required") is True and isinstance(role_name, str):
            required_roles.append(role_name)
    if not required_roles:
        return fail(FAIL_REVIEW_PROFILE_MISMATCH, "no_required_role")
    if vrs.MANDATORY_ROLE not in required_roles:
        return fail(FAIL_REVIEW_PROFILE_MISMATCH, "missing_independent_challenge")

    lenses = packet.get("lenses")
    if not isinstance(lenses, list) or not lenses:
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "lenses_missing")
    lens_state: dict[str, str] = {}
    for lens_row in lenses:
        if not isinstance(lens_row, dict):
            return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "malformed_lens_row")
        lens_name = lens_row.get("lens")
        applicability = lens_row.get("applicability")
        if lens_name not in vrs.LENSES or applicability not in ("required", "not_applicable"):
            return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "invented_lens_or_applicability")
        lens_state[str(lens_name)] = str(applicability)

    challenge = packet.get("challenge")
    if not isinstance(challenge, dict):
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "challenge_missing")
    falsifiers = challenge.get("falsifiers")
    if not isinstance(falsifiers, list) or not falsifiers:
        return fail(FAIL_FIRST_FALSIFIER_MISSING, "falsifiers_absent")
    falsifier_ids: list[str] = []
    for falsifier in falsifiers:
        if isinstance(falsifier, dict):
            falsifier_id = falsifier.get("id")
            if isinstance(falsifier_id, str) and falsifier_id.strip():
                falsifier_ids.append(falsifier_id)
    if not falsifier_ids:
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "falsifier_ids_unbound")
    stage_questions = challenge.get("stage_questions")
    if not isinstance(stage_questions, list) or not stage_questions:
        return fail(FAIL_FIRST_FALSIFIER_MISSING, "stage_questions_absent")

    negative_controls = packet.get("negative_controls")
    if not isinstance(negative_controls, list) or not negative_controls:
        return fail(FAIL_FIRST_FALSIFIER_MISSING, "negative_controls_absent")
    for control in negative_controls:
        if not isinstance(control, dict):
            return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "malformed_negative_control")
        if control.get("falsifier_id") not in falsifier_ids:
            return fail(FAIL_FIRST_FALSIFIER_MISSING, "negative_control_without_falsifier")
        checks = control.get("checks")
        if not isinstance(checks, dict):
            return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "negative_control_checks_missing")
        missing_criteria = [name for name in NEGATIVE_CONTROL_CRITERIA if name not in checks]
        if missing_criteria:
            return fail(
                FAIL_ARTIFACT_REVIEW_INCOMPLETE,
                f"negative_control_criterion_missing ({missing_criteria[0]})",
            )
        for name in NEGATIVE_CONTROL_CRITERIA:
            result_row = checks.get(name)
            if not isinstance(result_row, dict):
                return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, f"negative_control_result_missing ({name})")
            status_value = result_row.get("status")
            if status_value not in ("established", "not_established"):
                return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, f"negative_control_status_invalid ({name})")
            if status_value == "not_established":
                # Packet contract: every negative-control criterion must be
                # established with evidence; not_established is a finding,
                # never a pass.
                return fail(
                    FAIL_ARTIFACT_REVIEW_INCOMPLETE,
                    f"negative_control_criterion_unestablished ({name})",
                )
            evidence_value = result_row.get("evidence")
            if status_value == "established" and not (
                isinstance(evidence_value, str) and evidence_value.strip()
            ):
                return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, f"established_without_evidence ({name})")

    obligations = packet.get("obligations")
    if not isinstance(obligations, dict):
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "obligations_missing")
    tests_mutations = obligations.get("tests_mutations")
    if not isinstance(tests_mutations, list) or not tests_mutations:
        return fail(FAIL_FIRST_FALSIFIER_MISSING, "no_test_mutation_obligation")

    old_paths = packet.get("old_paths")
    if not isinstance(old_paths, list):
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "old_paths_malformed")
    seam_dispositions: list[tuple[str, str]] = []
    for seam_row in old_paths:
        if not isinstance(seam_row, dict):
            return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "malformed_old_path_row")
        seam = seam_row.get("seam")
        disposition = seam_row.get("disposition")
        if not (isinstance(seam, str) and seam.strip()):
            return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "old_path_seam_unbound")
        if disposition not in OLD_PATH_DISPOSITIONS:
            return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "old_path_disposition_unknown")
        seam_dispositions.append((str(seam), str(disposition)))

    lifecycle = packet.get("lifecycle")
    if not isinstance(lifecycle, dict) or not isinstance(
        lifecycle.get("graceful_cleanup_claimed"), bool
    ):
        return fail(FAIL_ARTIFACT_REVIEW_INCOMPLETE, "lifecycle_unbound")

    return {
        "path": path_label,
        "sha256": digest,
        "verdict": PASS_CURRENT_REVIEW,
        "reason": "packet_structurally_current",
        "covered_refs": covered_refs,
        "covered_surfaces": [],
        "profile": programme_profile,
        "repo_subject": str(identity_fields["name"]),
        "head": str(identity_fields["head"]),
        "head_binding": "current" if identity_fields["head"] == evaluated_head else "stale",
        "required_roles": sorted(set(required_roles)),
        "lens_state": lens_state,
        "seam_dispositions": seam_dispositions,
    }


# ---------------------------------------------------------------------------
# Evaluation
# ---------------------------------------------------------------------------


def resolve_covered_surfaces(
    packet: dict[str, Any], governed: list[dict[str, Any]]
) -> list[str]:
    refs = set(packet.get("covered_refs", []))
    covered = []
    for row in governed:
        if row["surface_id"] in refs or row["conflict_key"] in refs:
            covered.append(row["surface_id"])
    return sorted(covered)


def classify_packets(
    packets: list[dict[str, Any]],
    governed: list[dict[str, Any]],
    profiles: dict[str, Any],
    repository: str,
) -> None:
    """Finalize each packet's verdict before any row aggregates it."""
    for packet in packets:
        if packet["verdict"] != PASS_CURRENT_REVIEW:
            continue
        if repository and packet["repo_subject"] != repository:
            packet["verdict"] = FAIL_CLAIM_CEILING_EXCEEDED
            packet["reason"] = "packet_subject_repository_mismatch"
            continue
        if packet["head_binding"] == "stale":
            packet["verdict"] = FAIL_REVIEW_STALE_HEAD
            packet["reason"] = "packet_head_differs_from_evaluated_head"
            continue
        if packet["profile"] not in profiles:
            packet["verdict"] = FAIL_REVIEW_PROFILE_MISMATCH
            packet["reason"] = "packet_profile_not_in_manifest"
            continue
        packet["covered_surfaces"] = resolve_covered_surfaces(packet, governed)
        for surface_id in packet["covered_surfaces"]:
            if not _packet_matches_surface_profile(packet, governed, surface_id):
                packet["verdict"] = worst(packet["verdict"], FAIL_REVIEW_PROFILE_MISMATCH)
                packet["reason"] = "packet_profile_differs_from_surface_profile"


def _packet_matches_surface_profile(
    packet: dict[str, Any], governed: list[dict[str, Any]], surface_id: str
) -> bool:
    for row in governed:
        if row["surface_id"] != surface_id:
            continue
        if row["review_profile"] and packet["profile"] != row["review_profile"]:
            return False
        for lens in row["required_lenses"]:
            if packet["lens_state"].get(lens) != "required":
                return False
    return True


def evaluate_governed_row(
    row: dict[str, Any], packets: list[dict[str, Any]], denominator_valid: bool
) -> str:
    evidence_kind = row["required_evidence"]
    if evidence_kind == "checked_projection":
        return PASS_CURRENT_REVIEW if denominator_valid else FAIL_DENOMINATOR_INCOMPLETE
    if evidence_kind == "trusted_workflow_run":
        return NOT_PROVEN_GITHUB
    if evidence_kind != "current_head_reviewer_packet":
        return FAIL_REVIEW_PROFILE_MISMATCH

    covering: list[dict[str, Any]] = []
    for packet in packets:
        if packet["verdict"] in (FAIL_ARTIFACT_REVIEW_INCOMPLETE, NOT_PROVEN_SUBJECT):
            continue
        if row["surface_id"] not in packet["covered_surfaces"]:
            continue
        covering.append(packet)
    if not covering:
        return FAIL_REVIEW_MISSING

    row_result = covering[0]["verdict"]
    for packet in covering[1:]:
        row_result = worst(row_result, packet["verdict"])

    if row["predecessor_exit"]:
        acknowledged = False
        duplicate_found = False
        for packet in covering:
            for _seam, disposition in packet["seam_dispositions"]:
                if disposition == "unexpected_duplicate":
                    duplicate_found = True
                else:
                    acknowledged = True
        if duplicate_found:
            row_result = worst(row_result, FAIL_CONTROLLER_RELATION)
        elif not acknowledged:
            row_result = worst(row_result, FAIL_PREDECESSOR_REVIEW_INCOMPLETE)
    return row_result


def evaluate(inputs: dict[str, Any]) -> dict[str, Any]:
    """Compute the deterministic advisory verdict document. Same inputs yield
    byte-identical receipts; no clock, hostname, or absolute path enters it."""
    root: Path = inputs["root"]
    evaluated_head = inputs["head_sha"]

    global_results: list[str] = []

    manifest_bytes = b""
    projection_bytes = b""
    try:
        manifest_bytes = (root / DEFAULT_MANIFEST).read_bytes()
        projection_bytes = (root / DEFAULT_PROJECTION).read_bytes()
    except OSError:
        pass

    base_issues = validate_denominator_tree(root)
    base_strict_pass = not base_issues
    if not base_strict_pass:
        global_results.append(FAIL_DENOMINATOR_INCOMPLETE)

    candidate_root: Path | None = inputs["candidate_root"]
    candidate_checked = candidate_root is not None
    candidate_issues: list[str] = []
    candidate_strict_pass: bool | None = None
    if candidate_root is not None:
        candidate_issues = validate_denominator_tree(candidate_root)
        candidate_strict_pass = not candidate_issues
        if not candidate_strict_pass:
            global_results.append(FAIL_DENOMINATOR_INCOMPLETE)

    denominator_valid = (
        base_strict_pass and (candidate_strict_pass is None or candidate_strict_pass)
    )

    try:
        doc = load_manifest_document(root)
    except ValueError:
        doc = {}

    changed_files: list[str] = []
    truncated = False
    governed: list[dict[str, Any]] = []
    overlap_detected = False
    if doc:
        try:
            changed_files, truncated = read_changed_files(inputs)
        except ValueError as error:
            global_results.append(NOT_PROVEN_GITHUB)
            changed_files = []
            truncated = True
            del error
        if changed_files or not truncated:
            governed, overlap_detected = governed_rows(doc, changed_files)
        if truncated:
            global_results.append(NOT_PROVEN_GITHUB)
        if overlap_detected:
            global_results.append(FAIL_CONTROLLER_RELATION)

    packets = [
        load_packet(label, entry["path"], evaluated_head)
        for label, entry in ((entry["label"], entry) for entry in inputs["packets"])
    ]

    profiles = doc.get("profile") if isinstance(doc.get("profile"), dict) else {}
    classify_packets(packets, governed, profiles, inputs["repository"])
    for packet in packets:
        global_results.append(packet["verdict"])

    verdict_rows: list[dict[str, Any]] = []
    for row in governed:
        row_result = evaluate_governed_row(row, packets, denominator_valid)
        verdict_rows.append(
            {
                "surface_id": row["surface_id"],
                "result": row_result,
                "matched_paths": row["matched_paths"],
            }
        )
        global_results.append(row_result)

    result = aggregate(global_results)
    return {
        "schema_version": 1,
        "policy": "authority-transfer-review",
        "issue": "11795",
        "status_boundary": STATUS_BOUNDARY,
        "result": result,
        "repository": inputs["repository"],
        "pr_number": inputs["pr_number"],
        "base_sha": inputs["base_sha"],
        "evaluated_head_sha": evaluated_head,
        "inputs": {
            "changed_file_count": len(changed_files),
            "changed_files_truncated": truncated,
            "max_changed_files": inputs["max_changed_files"],
            "packet_count": len(packets),
        },
        "denominator": {
            "manifest_path": DEFAULT_MANIFEST.as_posix(),
            "manifest_sha256": sha256_bytes(manifest_bytes) if manifest_bytes else "",
            "projection_path": DEFAULT_PROJECTION.as_posix(),
            "projection_sha256": sha256_bytes(projection_bytes) if projection_bytes else "",
            "validator_script": "scripts/ci/validate_review_surfaces.py",
            "base_tree_strict_pass": base_strict_pass,
            "base_tree_issues": base_issues[:50],
            "candidate_tree_checked": candidate_checked,
            "candidate_tree_strict_pass": candidate_strict_pass,
            "candidate_tree_issues": candidate_issues[:50],
        },
        "governed_rows": [
            {
                "surface_id": row["surface_id"],
                "family": row["family"],
                "risk_class": row["risk_class"],
                "review_profile": row["review_profile"],
                "required_evidence": row["required_evidence"],
                "matched_paths": row["matched_paths"],
            }
            for row in governed
        ],
        "packets": [_public_packet_view(packet) for packet in packets],
        "verdicts": verdict_rows,
    }


def _public_packet_view(packet: dict[str, Any]) -> dict[str, Any]:
    return {
        "path": packet["path"],
        "sha256": packet["sha256"],
        "verdict": packet["verdict"],
        "reason": packet["reason"],
        "covered_surfaces": packet["covered_surfaces"],
        "profile": packet["profile"],
        "head_binding": packet["head_binding"],
    }


def render_receipt(receipt: dict[str, Any]) -> str:
    return json.dumps(receipt, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def render_summary(receipt: dict[str, Any]) -> str:
    denominator = receipt["denominator"]
    lines = [
        "# Authority Transfer Review (advisory)",
        "",
        f"- Result: `{receipt['result']}`",
        f"- Repository: `{receipt['repository'] or 'n/a'}`",
        f"- PR: {receipt['pr_number'] if receipt['pr_number'] is not None else 'n/a'}",
        f"- Evaluated head: `{receipt['evaluated_head_sha'] or 'n/a'}`",
        "- Manifest: "
        f"`{denominator.get('manifest_path', 'n/a')}"
        f" @ {(denominator.get('manifest_sha256') or 'n/a')[:12]}`",
        f"- Base denominator strict pass: `{denominator.get('base_tree_strict_pass')}`",
        "- Candidate denominator checked: "
        f"`{denominator.get('candidate_tree_checked')}`",
        f"- Governed rows: {len(receipt['governed_rows'])}",
        f"- Packets evaluated: {receipt['inputs']['packet_count']}",
        f"- Changed files: {receipt['inputs']['changed_file_count']}"
        + (" (bounded list truncated)" if receipt["inputs"]["changed_files_truncated"] else ""),
        "",
        "| Surface | Verdict | Matched paths |",
        "| --- | --- | --- |",
    ]
    for row in receipt["verdicts"]:
        paths = ", ".join(f"`{p}`" for p in row["matched_paths"]) or "(none)"
        lines.append(f"| `{row['surface_id']}` | `{row['result']}` | {paths} |")
    if not receipt["verdicts"]:
        lines.append("| (none) | PASS_NOT_APPLICABLE | |")
    lines += [
        "",
        STATUS_BOUNDARY,
        "",
        "Result vocabulary and evidence contract: issue #11795. Denominator: #11793.",
        "Required enforcement is owned by #11796 and is not armed by this check.",
    ]
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# CLI and instrument self-test
# ---------------------------------------------------------------------------


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Advisory exact-head Authority Transfer Review evaluator (#11795)."
    )
    parser.add_argument("--root", type=Path, default=None)
    parser.add_argument("--candidate-root", type=Path, default=None)
    parser.add_argument("--repository", default="")
    parser.add_argument("--pr-number", type=int, default=None)
    parser.add_argument("--base-sha", default="")
    parser.add_argument("--head-sha", default="")
    parser.add_argument("--changed-list", type=Path, default=None)
    parser.add_argument("--changed-file", action="append", default=[])
    parser.add_argument("--packet", action="append", default=[])
    parser.add_argument("--max-changed-files", type=int, default=DEFAULT_MAX_CHANGED_FILES)
    parser.add_argument("--receipt", type=Path, default=None)
    parser.add_argument("--summary", type=Path, default=None)
    parser.add_argument("--self-test", action="store_true")
    return parser


def _fixture_manifest_text(catalog_predecessor_exit: str = "") -> str:
    def surface(
        surface_id: str,
        authority: str,
        controller: str,
        conflict_key: str,
        risk_class: str,
        evidence: str,
        falsifier: str,
        paths: list[str],
        predecessor_exit: str = "",
        profile: str = "semantic_close_authority",
    ) -> str:
        lines = [
            "",
            f"[surface.{surface_id}]",
            'family = "semantic_issue_completion"',
            f'authority = "{authority}"',
            f'controller = "{controller}"',
            f'conflict_key = "{conflict_key}"',
            f'risk_class = "{risk_class}"',
            f'review_profile = "{profile}"',
            f'required_evidence = "{evidence}"',
            f'first_falsifier = "{falsifier}"',
            'enforcement_successor = "#11796"',
        ]
        if predecessor_exit:
            lines.append(f'predecessor_exit = "{predecessor_exit}"')
        lines.append(
            "code_owner_route = { kind = \"not_proven\", resolution_owner = \"#11796\", note = \"deferred\" }"
        )
        lines.append("paths = [")
        lines.extend(f'  "{path}",' for path in paths)
        lines.append("]")
        return "\n".join(lines)

    body = "\n".join(
        [
            "schema_version = 1",
            'policy = "review-surfaces"',
            'owner = "EffortlessMetrics"',
            'status = "advisory"',
            'updated = "2026-08-25"',
            'issue = "11793"',
            'classification_rule = "One row per authority."',
            'enforcement_boundary = "Advisory metadata only."',
            'successor_consumption = "#11795 consumes the projection."',
            'projection_doc = "docs/policy/REVIEW_SURFACES.md"',
            'validator_script = "scripts/ci/validate_review_surfaces.py"',
            'validator_test = "scripts/ci/test_validate_review_surfaces.py"',
            "",
            "[families]",
            'semantic_issue_completion = "Close contracts."',
            'configuration_authority = "Configuration adapters."',
            'executable_policy_and_public_migration = "Executable policy."',
            "",
            "[profile.semantic_close_authority]",
            'fresh_direction = "Challenge false closes."',
            "lenses = [\"subject_evidence_identity\", \"lifecycle_currentness_concurrency\", \"spec_test_docs_consistency\"]",
            'required_roles = ["adversarial_challenger"]',
            'packet_contract = "schemas/agent_review_packet.v1.schema.json"',
            'handoff_authority = "#11701"',
            "",
            "[profile.public_api_or_retirement_authority]",
            'fresh_direction = "Challenge denominator movement."',
            "lenses = [\"release_external_boundary\", \"architecture_authority_duplication\", \"subject_evidence_identity\"]",
            'required_roles = ["adversarial_challenger"]',
            'packet_contract = "schemas/agent_review_packet.v1.schema.json"',
            'handoff_authority = "#10881"',
            "",
            "[code_owner_identity.EffortlessSteven]",
            'kind = "user"',
            'status = "valid"',
            'permission = "admin"',
            'validation_method = "api"',
            'evidence_date = "2026-08-25"',
        ]
    )
    body += "\n"
    body += surface(
        "manifest_self",
        "This manifest.",
        "#11793",
        "authority_review.manifest",
        "live_repository_policy_control",
        "checked_projection",
        "The manifest omits itself.",
        [
            "policy/review-surfaces.toml",
            "scripts/ci/validate_review_surfaces.py",
            "scripts/ci/test_validate_review_surfaces.py",
            "docs/policy/REVIEW_SURFACES.md",
            ".github/CODEOWNERS",
        ],
    )
    body += surface(
        "close_policy",
        "Close-proof policy.",
        "#10168",
        "close.contract",
        "semantic_control",
        "current_head_reviewer_packet",
        "A weaker close rule passes.",
        ["docs/agents/CLOSE_PROOF_POLICY.md", "xtask/src/tasks/pr_close_proof.rs"],
    )
    body += surface(
        "close_evaluator",
        "Containment evaluator.",
        "#10413",
        "close.evaluator",
        "semantic_control",
        "current_head_reviewer_packet",
        "An evaluator file escapes the denominator.",
        [
            "xtask/src/bin/semantic-close-containment.rs",
            "xtask/src/close_proof/**",
        ],
    )
    body += surface(
        "close_contract_fixtures",
        "Issue-contract fixtures.",
        "#10380",
        "close.contract",
        "semantic_control",
        "current_head_reviewer_packet",
        "A fixture flip relaxes a constraint.",
        [".ci/close-proof-contract/**"],
    )
    body += surface(
        "containment_fixtures",
        "Containment fixtures.",
        "#10413",
        "close.workflow",
        "semantic_control",
        "current_head_reviewer_packet",
        "An unproven close classifies as valid.",
        [".ci/semantic-close-containment/**"],
    )
    body += surface(
        "trusted_close_workflow",
        "Trusted containment workflow.",
        "#10413",
        "close.workflow",
        "live_repository_policy_control",
        "trusted_workflow_run",
        "The workflow points at a mutable ref.",
        [".github/workflows/semantic-close-containment.yml"],
    )
    body += surface(
        "template_fields",
        "PR-template fields.",
        "#10384",
        "close.contract",
        "semantic_control",
        "current_head_reviewer_packet",
        "A template field vanishes.",
        [".github/PULL_REQUEST_TEMPLATE.md"],
    )
    body += surface(
        "settings_schema",
        "Public settings schema.",
        "#10385",
        "config.public_schema",
        "public_surface_control",
        "current_head_reviewer_packet",
        "Schema drifts from authority rows.",
        [
            "schemas/perllsp-settings.schema.json",
            "docs/reference/CONFIGURATION_SCHEMA.md",
        ],
        profile="public_api_or_retirement_authority",
    )
    body += surface(
        "ripr_suppression",
        "RIPR suppression policy.",
        "docs/ci/ripr.md",
        "policy.ripr_suppression",
        "executable_policy_control",
        "current_head_reviewer_packet",
        "A suppression broadens without review.",
        ["policy/ripr-suppressions.toml"],
    )
    body += surface(
        "ripr_pipeline",
        "RIPR receipt pipeline.",
        "docs/ci/ripr.md",
        "policy.ripr_receipts",
        "executable_policy_control",
        "current_head_reviewer_packet",
        "The receipt schema loosens.",
        [
            "schemas/ripr-perl-facts-v1.schema.json",
            "ripr.toml",
            "xtask/src/tasks/ripr_evidence.rs",
            "scripts/ci/ripr_summary.py",
            ".github/workflows/ripr.yml",
        ],
    )
    body += surface(
        "api_baseline",
        "Public-API ratchet.",
        "#4497",
        "policy.public_api_baseline",
        "public_surface_control",
        "current_head_reviewer_packet",
        "The ratchet certifies a widening.",
        [
            ".ci/public-api-baselines/**",
            "xtask/tests/public_api_ratchet_tests.rs",
        ],
    )
    body += surface(
        "receipt_retirement",
        "Review-receipt retirement.",
        "#6060",
        "policy.retirement_ledger",
        "executable_policy_control",
        "trusted_workflow_run",
        "Retirement criteria loosen.",
        [".github/workflows/review-receipt-retirement.yml"],
    )
    body += surface(
        "review_ledger",
        "PR review ledger.",
        "#6060",
        "policy.review_ledger",
        "executable_policy_control",
        "current_head_reviewer_packet",
        "Ledger schema widens dishonestly.",
        ["docs/agents/pr-ledger.schema.json", "xtask/src/tasks/pr_ledger.rs"],
    )
    body += surface(
        "repo_settings",
        "Repository-managed settings.",
        "#11793",
        "policy.repo_settings",
        "live_repository_policy_control",
        "current_head_reviewer_packet",
        "Protection blocks appear without profile.",
        [".github/settings.yaml"],
    )
    body += surface(
        "packet_contract",
        "Shared review-packet contract.",
        "#10881",
        "authority_review.reviewer_packet",
        "live_repository_policy_control",
        "current_head_reviewer_packet",
        "An invented review kind enters.",
        [
            "schemas/agent_review_packet.v1.schema.json",
            "schemas/agent_review_finding.v1.schema.json",
            "schemas/stage_closure_projection.v1.schema.json",
            "xtask/src/tasks/agent_review_packet.rs",
            "fixtures/agent_review_packet/**",
        ],
    )
    body += surface(
        "authority_catalog",
        "Configuration catalog.",
        "#10790",
        "config.authority_catalog",
        "configuration_control",
        "current_head_reviewer_packet",
        "An unregistered leaf slips through.",
        [
            "src/authority/**",
            "crates/perl-lsp-rs-core/src/configuration_authority/**",
        ],
        predecessor_exit=catalog_predecessor_exit,
    )
    body += "\n".join(
        [
            "",
            "[residue.deferred_store]",
            'authority = "Accepted configuration store."',
            'parent_issue = "#7057"',
            'reason = "Not landed on current main."',
            'resolution_owner = "#7057"',
            "",
        ]
    )
    return body


def _write_fixture_root(base: Path) -> None:
    """Minimal but complete tree: every detector target the #11793 validator
    demands exists and every detector path is bound, so the full in-process
    invariant set can pass hermetically."""
    for rel in vrs.DETECTOR_FILES:
        target = base / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text("", encoding="utf-8")
    for rel in vrs.DETECTOR_DIRS:
        directory = base / rel
        directory.mkdir(parents=True, exist_ok=True)
        marker = directory / ".fixture-marker"
        marker.write_text("", encoding="utf-8")
    authority_dir = base / "src" / "authority" / "nested"
    authority_dir.mkdir(parents=True, exist_ok=True)
    (authority_dir / "leaf.rs").write_text("catalog", encoding="utf-8")
    manifest_path = base / DEFAULT_MANIFEST
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(_fixture_manifest_text(), encoding="utf-8", newline="\n")
    (base / ".github/CODEOWNERS").write_text("* @EffortlessSteven\n", encoding="utf-8")
    doc = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    rendered = vrs.render_projection(doc)
    projection_path = base / DEFAULT_PROJECTION
    projection_path.parent.mkdir(parents=True, exist_ok=True)
    projection_path.write_text(rendered, encoding="utf-8", newline="\n")


def self_test() -> int:
    """Replay internal expected-verdict fixtures; any drift fails the instrument."""
    import tempfile

    failures: list[str] = []

    def expect(label: str, actual: object, expected: object) -> None:
        if actual != expected:
            failures.append(f"{label}: expected {expected!r}, got {actual!r}")

    repository = "EffortlessMetrics/perl-lsp-swarm"
    head = "a" * 40
    stale_head = "b" * 40

    with tempfile.TemporaryDirectory(prefix="atr-selftest-") as tmp:
        base = Path(tmp)
        _write_fixture_root(base)

        def make_inputs(changed: list[str], packets: list[Path], **overrides: Any) -> dict[str, Any]:
            return {
                "root": base,
                "candidate_root": overrides.get("candidate_root"),
                "repository": overrides.get("repository", repository),
                "pr_number": 1,
                "base_sha": "c" * 40,
                "head_sha": overrides.get("head_sha", head),
                "changed_list": None,
                "changed_files": changed,
                "packets": [{"label": p.name, "path": p} for p in packets],
                "max_changed_files": overrides.get("max_changed_files", 100),
            }

        packets_dir = base / "fixtures-packets"
        packets_dir.mkdir(exist_ok=True)

        def write_packet(name: str, body: dict[str, Any]) -> Path:
            target = packets_dir / name
            target.write_text(json.dumps(body), encoding="utf-8", newline="\n")
            return target

        def packet_body(
            profile: str,
            head_value: str,
            repo: str = repository,
            authorities: list[dict[str, str]] | None = None,
        ) -> dict[str, Any]:
            return {
                "schema": "agent_review_packet.v1",
                "schema_version": 1,
                "packet_id": "self-test-packet",
                "subject": {
                    "repository": {
                        "name": repo,
                        "base": "main",
                        "head": head_value,
                        "tree": "tree-identity",
                        "diff": "diff-digest",
                    },
                    "programme": {
                        "name": "authority-transfer",
                        "stage": "advisory-preflight",
                        "proposition": "Governed change carries current review.",
                        "profile": profile,
                    },
                    "owning_issue": "#11795",
                    "builder_packet": {
                        "contract": "agent_implementation_packet.v1",
                        "digest": "digest-1",
                    },
                    "changed": {
                        "authorities": authorities
                        or [
                            {
                                "ref": "config.authority_catalog",
                                "subject": "src/authority/catalog.rs",
                            }
                        ],
                        "evidence": [{"kind": "receipt", "identity": "receipt-1"}],
                        "migrated_seams": [],
                    },
                },
                "challenge": {
                    "primary_proposition": "The governed change is reviewed.",
                    "falsifiers": [
                        {"id": "F1", "stage": "review", "statement": "Unregistered leaf passes."}
                    ],
                    "stage_questions": [
                        {"id": "Q1", "question": "What re-derives the leaf parity check?"}
                    ],
                },
                "lenses": [
                    {"lens": lens, "applicability": "required"}
                    for lens in vrs.LENSES
                ],
                "negative_controls": [
                    {
                        "falsifier_id": "F1",
                        "checks": {
                            name: {"status": "established", "evidence": f"{name}-evidence"}
                            for name in NEGATIVE_CONTROL_CRITERIA
                        },
                    }
                ],
                "old_paths": [],
                "obligations": {
                    "spec_ledger_ids": [],
                    "fixture_expectation_manifests": [],
                    "tests_mutations": [{"ref": "test_leaf_parity", "identity": "digest-2"}],
                    "generated_artifacts": [],
                    "docs_projections": [],
                    "change_fragments": [],
                },
                "roles": [
                    {
                        "role": "adversarial_challenger",
                        "required": True,
                        "obligation": "Re-derive the leaf-parity invariant independently.",
                    }
                ],
                "lifecycle": {"graceful_cleanup_claimed": False},
            }

        # 1. Not-applicable PR takes the cheap deterministic route.
        receipt = evaluate(make_inputs(["crates/other/src/lib.rs"], []))
        expect("not_applicable", receipt["result"], PASS_NOT_APPLICABLE)

        # 2. Governed change without a packet fails typed-missing, bound to the exact head.
        receipt = evaluate(make_inputs(["src/authority/catalog.rs"], []))
        expect("missing_packet", receipt["result"], FAIL_REVIEW_MISSING)
        expect("exact_head_bound", receipt["evaluated_head_sha"], head)
        expect(
            "governed_row_recorded",
            [row["surface_id"] for row in receipt["governed_rows"]],
            ["authority_catalog"],
        )

        # 3. Current packet passes and two consecutive computations are byte-identical.
        good = write_packet("good.json", packet_body("semantic_close_authority", head))
        first = evaluate(make_inputs(["src/authority/catalog.rs"], [good]))
        second = evaluate(make_inputs(["src/authority/catalog.rs"], [good]))
        expect("determinism", render_receipt(first), render_receipt(second))
        expect("current_review_pass", first["result"], PASS_CURRENT_REVIEW)

        # 4. A packet bound to the previous head never looks valid.
        stale = write_packet("stale.json", packet_body("semantic_close_authority", stale_head))
        expect(
            "stale_head",
            evaluate(make_inputs(["src/authority/catalog.rs"], [stale]))["result"],
            FAIL_REVIEW_STALE_HEAD,
        )

        # 5. Profile mismatch fails.
        wrong_profile = write_packet(
            "wrong-profile.json", packet_body("live_repository_policy_authority", head)
        )
        expect(
            "profile_mismatch",
            evaluate(make_inputs(["src/authority/catalog.rs"], [wrong_profile]))["result"],
            FAIL_REVIEW_PROFILE_MISMATCH,
        )

        # 6. Builder-self-review-only roles cannot satisfy the review proposition.
        builder_only = packet_body("semantic_close_authority", head)
        builder_only["roles"] = [
            {"role": "builder_self_review", "required": True, "obligation": "Self-checked."}
        ]
        expect(
            "builder_only_rejected",
            evaluate(
                make_inputs(["src/authority/catalog.rs"], [write_packet("builder-only.json", builder_only)])
            )["result"],
            FAIL_REVIEW_PROFILE_MISMATCH,
        )

        # 7. Claim ceiling: evidence claiming another repository subject fails.
        other_repo = write_packet(
            "other-repo.json",
            packet_body("semantic_close_authority", head, repo="Elsewhere/other"),
        )
        expect(
            "claim_ceiling",
            evaluate(make_inputs(["src/authority/catalog.rs"], [other_repo]))["result"],
            FAIL_CLAIM_CEILING_EXCEEDED,
        )

        # 8. Missing negative controls fail the first-falsifier class.
        no_controls = packet_body("semantic_close_authority", head)
        no_controls["negative_controls"] = []
        expect(
            "first_falsifier_missing",
            evaluate(
                make_inputs(
                    ["src/authority/catalog.rs"], [write_packet("no-controls.json", no_controls)]
                )
            )["result"],
            FAIL_FIRST_FALSIFIER_MISSING,
        )

        # 9. Established-without-evidence criterion fails artifact review.
        unevidenced = packet_body("semantic_close_authority", head)
        unevidenced["negative_controls"][0]["checks"]["exists"] = {"status": "established"}
        expect(
            "artifact_incomplete",
            evaluate(
                make_inputs(
                    ["src/authority/catalog.rs"], [write_packet("unevidenced.json", unevidenced)]
                )
            )["result"],
            FAIL_ARTIFACT_REVIEW_INCOMPLETE,
        )

        # 10. Malformed packet is unusable evidence, never a pass.
        malformed = packets_dir / "malformed.json"
        malformed.write_text("{not json", encoding="utf-8")
        expect(
            "artifact_malformed",
            evaluate(make_inputs(["src/authority/catalog.rs"], [malformed]))["result"],
            FAIL_ARTIFACT_REVIEW_INCOMPLETE,
        )

        # 11. Predecessor exit without disposition fails; unexpected duplicate is controller relation.
        manifest_path = base / DEFAULT_MANIFEST
        saved_manifest = manifest_path.read_text(encoding="utf-8")
        manifest_with_exit = _fixture_manifest_text(
            catalog_predecessor_exit="Old catalog retired here."
        )
        manifest_path.write_text(manifest_with_exit, encoding="utf-8", newline="\n")
        rendered = vrs.render_projection(tomllib.loads(manifest_with_exit))
        (base / DEFAULT_PROJECTION).write_text(rendered, encoding="utf-8", newline="\n")
        expect(
            "predecessor_missing",
            evaluate(make_inputs(["src/authority/catalog.rs"], [good]))["result"],
            FAIL_PREDECESSOR_REVIEW_INCOMPLETE,
        )
        dup_packet = packet_body("semantic_close_authority", head)
        dup_packet["old_paths"] = [
            {"seam": "old catalog", "disposition": "unexpected_duplicate"}
        ]
        expect(
            "controller_relation",
            evaluate(
                make_inputs(
                    ["src/authority/catalog.rs"], [write_packet("dup.json", dup_packet)]
                )
            )["result"],
            FAIL_CONTROLLER_RELATION,
        )
        ok_packet = packet_body("semantic_close_authority", head)
        ok_packet["old_paths"] = [{"seam": "old catalog", "disposition": "removed"}]
        expect(
            "predecessor_acknowledged",
            evaluate(
                make_inputs(
                    ["src/authority/catalog.rs"], [write_packet("ok-seams.json", ok_packet)]
                )
            )["result"],
            PASS_CURRENT_REVIEW,
        )
        manifest_path.write_text(saved_manifest, encoding="utf-8", newline="\n")
        restored = vrs.render_projection(tomllib.loads(saved_manifest))
        (base / DEFAULT_PROJECTION).write_text(restored, encoding="utf-8", newline="\n")

        # 12. Broken denominator dominates and never passes.
        saved = manifest_path.read_text(encoding="utf-8")
        manifest_path.write_text("schema_version = 99\n", encoding="utf-8", newline="\n")
        broken = evaluate(make_inputs(["src/authority/catalog.rs"], [good]))
        manifest_path.write_text(saved, encoding="utf-8", newline="\n")
        expect("denominator_broken", broken["result"], FAIL_DENOMINATOR_INCOMPLETE)

        # 13. Projection drift fails closed.
        projection_path = base / DEFAULT_PROJECTION
        saved_projection = projection_path.read_text(encoding="utf-8")
        projection_path.write_text("hand-edited lie\n", encoding="utf-8", newline="\n")
        drifted = evaluate(make_inputs(["src/authority/catalog.rs"], [good]))
        projection_path.write_text(saved_projection, encoding="utf-8", newline="\n")
        expect("projection_drift", drifted["result"], FAIL_DENOMINATOR_INCOMPLETE)

        # 14. Bounded-input overflow is NOT_PROVEN_GITHUB, never a pass.
        overflow = evaluate(
            make_inputs(
                ["src/authority/catalog.rs"] + [f"filler/{i}.txt" for i in range(120)],
                [],
                max_changed_files=100,
            )
        )
        expect("bounded_overflow", overflow["result"], NOT_PROVEN_GITHUB)

        # 15. checked_projection row is satisfiable today when the denominator holds.
        receipt = evaluate(make_inputs(["docs/policy/REVIEW_SURFACES.md"], []))
        expect("checked_projection_satisfiable", receipt["result"], PASS_CURRENT_REVIEW)

    if failures:
        print(f"Authority Transfer Review self-test FAILED ({len(failures)}):")
        for failure in failures:
            print(f"  - {failure}")
        return EXIT_NOT_PROVEN
    print("Authority Transfer Review self-test passed.")
    return EXIT_PASS


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if args.root is None:
        parser.error("--root is required unless --self-test is given")
        return EXIT_NOT_PROVEN

    packets = [{"label": str(path), "path": path} for path in args.packet]
    inputs = {
        "root": args.root.resolve(),
        "candidate_root": args.candidate_root.resolve() if args.candidate_root else None,
        "repository": args.repository,
        "pr_number": args.pr_number,
        "base_sha": args.base_sha,
        "head_sha": args.head_sha,
        "changed_list": args.changed_list,
        "changed_files": list(args.changed_file),
        "packets": packets,
        "max_changed_files": max(1, min(args.max_changed_files, 100000)),
    }
    receipt = evaluate(inputs)
    rendered = render_receipt(receipt)
    if args.receipt is not None:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(rendered, encoding="utf-8", newline="\n")
    summary = render_summary(receipt)
    if args.summary is not None:
        args.summary.parent.mkdir(parents=True, exist_ok=True)
        args.summary.write_text(summary, encoding="utf-8", newline="\n")
    print(summary, end="")
    result = receipt["result"]
    if result in (PASS_NOT_APPLICABLE, PASS_CURRENT_REVIEW):
        return EXIT_PASS
    if result.startswith("FAIL_"):
        return EXIT_TYPED_FAILURE
    return EXIT_NOT_PROVEN


if __name__ == "__main__":
    sys.exit(main())
