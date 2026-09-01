#!/usr/bin/env python3
"""Validate one blocker_closeout.v1 terminal packet.

The validator owns packet-local structure, identity, proof, review, and claim laws.
It checks merged-commit reachability against a caller-supplied Git repository, but
does not discover the release denominator, query GitHub, or decide a freeze.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable


SCHEMA_VERSION = "blocker_closeout.v1"
SHA40 = re.compile(r"^[0-9a-f]{40}$")
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
IDENTIFIER = re.compile(r"^[a-z0-9]+(?:[._-][a-z0-9]+)*$")
SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(?:0|[1-9A-Za-z][0-9A-Za-z-]*)(?:\.(?:0|[1-9A-Za-z][0-9A-Za-z-]*))*)?$")
TERMINAL_STATUSES = {"resolved", "bounded_limitation", "blocked", "not_proven"}
PROOF_STATUSES = {
    "passed",
    "failed",
    "not_proven",
    "stale",
    "skipped",
    "cancelled",
    "timed_out",
    "instrument_failed",
    "malformed",
}
EPISTEMIC_GAPS = PROOF_STATUSES - {"passed", "failed"}
PROOF_CLASSES = {
    "product_test",
    "installed_acceptance",
    "workflow",
    "review",
    "repository_receipt",
    "mechanism",
    "fixture",
}
CLAIM_SCOPES = {"repository_mechanism", "product_behavior", "installed_user_behavior"}
EVIDENCE_KINDS = {
    "github_issue",
    "github_issue_comment",
    "github_pull",
    "github_review",
    "github_check",
    "repository_blob",
    "repository_receipt",
}
PROOF_AUTHORITY_MATRIX = {
    ("product_test", "product_behavior"): {"github_check", "repository_receipt"},
    ("installed_acceptance", "installed_user_behavior"): {"github_check", "repository_receipt"},
    ("workflow", "repository_mechanism"): {"github_check", "repository_receipt"},
    ("review", "repository_mechanism"): {"github_review"},
    ("repository_receipt", "repository_mechanism"): {"repository_receipt"},
    ("mechanism", "repository_mechanism"): {"github_check", "repository_blob", "repository_receipt"},
    ("fixture", "repository_mechanism"): {"repository_blob", "repository_receipt"},
}
PRIVATE_REF = re.compile(
    r"(?:(?:^|repo:)[A-Za-z]:[\\/]|^/tmp/|^/home/|\.codex(?:[\\/]|$)|worktrees?(?:[\\/]|$)|^repo:(?:/|[A-Za-z]:[\\/]))",
    re.IGNORECASE,
)
ISSUE_REF = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/issues/([0-9]+)$")
ISSUE_COMMENT_REF = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/issues/([0-9]+)#issuecomment-[0-9]+$")
PULL_REF = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/pull/([0-9]+)$")
PULL_REVIEW_REF = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/pull/([0-9]+)#pullrequestreview-[0-9]+$")
CHECK_REF = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/[0-9]+/job/[0-9]+$")
REPOSITORY_REF = re.compile(r"^repo:[^@\s]+@([0-9a-f]{40})$")
EVIDENCE_REF_PATTERNS = {
    "github_issue": ISSUE_REF,
    "github_issue_comment": ISSUE_COMMENT_REF,
    "github_pull": PULL_REF,
    "github_review": PULL_REVIEW_REF,
    "github_check": CHECK_REF,
    "repository_blob": REPOSITORY_REF,
    "repository_receipt": REPOSITORY_REF,
}


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def _object(value: Any, name: str) -> dict[str, Any]:
    _require(isinstance(value, dict), f"{name} must be an object")
    return value


def _exact_keys(value: dict[str, Any], name: str, keys: set[str]) -> None:
    missing = sorted(keys - value.keys())
    extra = sorted(value.keys() - keys)
    if missing:
        raise ValueError(f"{name} is missing required field {missing[0]}")
    if extra:
        raise ValueError(f"{name} has unexpected field {extra[0]}")


def _string(value: Any, name: str) -> str:
    _require(isinstance(value, str) and value.strip() == value and bool(value), f"{name} must be a non-empty trimmed string")
    return value


def _positive_int(value: Any, name: str) -> int:
    _require(type(value) is int and value > 0, f"{name} must be a positive integer")
    return value


def _sha(value: Any, name: str) -> str:
    text = _string(value, name)
    _require(SHA40.fullmatch(text) is not None, f"{name} must be a lowercase 40-character SHA")
    return text


def _identifier(value: Any, name: str) -> str:
    text = _string(value, name)
    _require(IDENTIFIER.fullmatch(text) is not None, f"{name} must be a stable identifier")
    return text


def _unique_strings(value: Any, name: str, *, nonempty: bool = False, identifiers: bool = False) -> tuple[str, ...]:
    _require(isinstance(value, list), f"{name} must be an array")
    if nonempty:
        _require(bool(value), f"{name} must be non-empty")
    parsed = tuple(_identifier(item, f"{name}[]") if identifiers else _string(item, f"{name}[]") for item in value)
    _require(len(parsed) == len(set(parsed)), f"{name} must be unique")
    _require(list(parsed) == sorted(parsed), f"{name} must be sorted")
    return parsed


@dataclass(frozen=True)
class DurableEvidence:
    kind: str
    ref: str
    digest: str

    @classmethod
    def parse(cls, value: Any, name: str) -> "DurableEvidence":
        data = _object(value, name)
        _exact_keys(data, name, {"kind", "ref", "digest"})
        kind = _string(data["kind"], f"{name}.kind")
        _require(kind in EVIDENCE_KINDS, f"{name}.kind is invalid")
        reference = _string(data["ref"], f"{name}.ref")
        _require(PRIVATE_REF.search(reference) is None, f"{name}.ref must not contain a private or worktree path")
        _require(
            EVIDENCE_REF_PATTERNS[kind].fullmatch(reference) is not None,
            f"{name}.ref does not match exact grammar for {kind}",
        )
        digest = _string(data["digest"], f"{name}.digest")
        _require(DIGEST.fullmatch(digest) is not None, f"{name}.digest must be sha256:<64 lowercase hex>")
        return cls(kind, reference, digest)


@dataclass(frozen=True)
class ControllerClaim:
    id: str
    proof_class: str
    claim_scope: str

    @classmethod
    def parse(cls, value: Any, name: str) -> "ControllerClaim":
        data = _object(value, name)
        _exact_keys(data, name, {"id", "proof_class", "claim_scope"})
        proof_class = _string(data["proof_class"], f"{name}.proof_class")
        claim_scope = _string(data["claim_scope"], f"{name}.claim_scope")
        _require(proof_class in PROOF_CLASSES, f"{name}.proof_class is invalid")
        _require(claim_scope in CLAIM_SCOPES, f"{name}.claim_scope is invalid")
        _require(
            (proof_class, claim_scope) in PROOF_AUTHORITY_MATRIX,
            f"{name} proof_class and claim_scope are not an admitted controller claim contract",
        )
        return cls(_identifier(data["id"], f"{name}.id"), proof_class, claim_scope)


@dataclass(frozen=True)
class ImplementationContribution:
    implementation_pr: int
    candidate_head_sha: str
    merged_sha: str
    claim_ids: tuple[str, ...]
    relation: str
    evidence: DurableEvidence

    @classmethod
    def parse(cls, value: Any, name: str) -> "ImplementationContribution":
        data = _object(value, name)
        _exact_keys(data, name, {"implementation_pr", "candidate_head_sha", "merged_sha", "claim_ids", "relation", "evidence"})
        relation = _string(data["relation"], f"{name}.relation")
        _require(relation in {"implements", "contributes", "supersedes", "absorbs"}, f"{name}.relation is invalid")
        return cls(
            _positive_int(data["implementation_pr"], f"{name}.implementation_pr"),
            _sha(data["candidate_head_sha"], f"{name}.candidate_head_sha"),
            _sha(data["merged_sha"], f"{name}.merged_sha"),
            _unique_strings(data["claim_ids"], f"{name}.claim_ids", nonempty=True, identifiers=True),
            relation,
            DurableEvidence.parse(data["evidence"], f"{name}.evidence"),
        )


@dataclass(frozen=True)
class LandedIntegration:
    implementation_pr: int
    candidate_head_sha: str
    landed_sha: str
    kind: str
    evidence: DurableEvidence

    @classmethod
    def parse(cls, value: Any, name: str) -> "LandedIntegration":
        data = _object(value, name)
        _exact_keys(data, name, {"implementation_pr", "candidate_head_sha", "landed_sha", "kind", "evidence"})
        kind = _string(data["kind"], f"{name}.kind")
        _require(kind in {"squash", "merge", "fast_forward"}, f"{name}.kind is invalid")
        return cls(
            _positive_int(data["implementation_pr"], f"{name}.implementation_pr"),
            _sha(data["candidate_head_sha"], f"{name}.candidate_head_sha"),
            _sha(data["landed_sha"], f"{name}.landed_sha"),
            kind,
            DurableEvidence.parse(data["evidence"], f"{name}.evidence"),
        )


@dataclass(frozen=True)
class ProofObservation:
    id: str
    claim_ids: tuple[str, ...]
    subject_sha: str
    status: str
    proof_class: str
    claim_scope: str
    evidence: DurableEvidence

    @classmethod
    def parse(cls, value: Any, name: str) -> "ProofObservation":
        data = _object(value, name)
        _exact_keys(data, name, {"id", "claim_ids", "subject_sha", "status", "proof_class", "claim_scope", "evidence"})
        status = _string(data["status"], f"{name}.status")
        proof_class = _string(data["proof_class"], f"{name}.proof_class")
        claim_scope = _string(data["claim_scope"], f"{name}.claim_scope")
        _require(status in PROOF_STATUSES, f"{name}.status is invalid")
        _require(proof_class in PROOF_CLASSES, f"{name}.proof_class is invalid")
        _require(claim_scope in CLAIM_SCOPES, f"{name}.claim_scope is invalid")
        evidence = DurableEvidence.parse(data["evidence"], f"{name}.evidence")
        admitted_evidence = PROOF_AUTHORITY_MATRIX.get((proof_class, claim_scope))
        _require(
            admitted_evidence is not None and evidence.kind in admitted_evidence,
            f"{name} proof_class/claim_scope/evidence kind is not admitted by the proof authority matrix",
        )
        subject_sha = _sha(data["subject_sha"], f"{name}.subject_sha")
        if evidence.kind == "repository_receipt":
            receipt_match = REPOSITORY_REF.fullmatch(evidence.ref)
            _require(
                receipt_match is not None and receipt_match.group(1) == subject_sha,
                f"{name} repository receipt subject SHA does not match proof observation subject_sha",
            )
        return cls(
            _identifier(data["id"], f"{name}.id"),
            _unique_strings(data["claim_ids"], f"{name}.claim_ids", nonempty=True, identifiers=True),
            subject_sha,
            status,
            proof_class,
            claim_scope,
            evidence,
        )


@dataclass(frozen=True)
class ReviewAuthority:
    authority_kind: str
    authority_number: int | None
    evidence: DurableEvidence
    reviewed_head: str
    status: str
    unresolved_findings: int


@dataclass(frozen=True)
class BlockerCloseoutV1:
    release: str
    blocker_id: str
    controller_issue: int
    controller_claims: tuple[ControllerClaim, ...]
    controller_evidence: DurableEvidence
    status: str
    observed_main_sha: str
    contributions: tuple[ImplementationContribution, ...]
    landed_integrations: tuple[LandedIntegration, ...]
    reviews: tuple[ReviewAuthority, ...]
    observations: tuple[ProofObservation, ...]


def _parse_evidence_array(value: Any, name: str) -> tuple[DurableEvidence, ...]:
    _require(isinstance(value, list), f"{name} must be an array")
    return tuple(DurableEvidence.parse(item, f"{name}[{index}]") for index, item in enumerate(value))


def _all_evidence(packet: dict[str, Any]) -> Iterable[DurableEvidence]:
    controller = _object(packet["semantic_controller"], "semantic_controller")
    yield DurableEvidence.parse(controller["evidence"], "semantic_controller.evidence")
    for index, item in enumerate(packet["implementation_contributions"]):
        yield DurableEvidence.parse(_object(item, f"implementation_contributions[{index}]")["evidence"], f"implementation_contributions[{index}].evidence")
    for index, item in enumerate(packet["landed_integrations"]):
        yield DurableEvidence.parse(_object(item, f"landed_integrations[{index}]")["evidence"], f"landed_integrations[{index}].evidence")
    for index, item in enumerate(packet["reviews"]):
        review = _object(item, f"reviews[{index}]")
        yield DurableEvidence.parse(review["current_head_synthesis"], f"reviews[{index}].current_head_synthesis")
        yield from _parse_evidence_array(review["finding_refs"], f"reviews[{index}].finding_refs")
    proof = _object(packet["proof"], "proof")
    for index, item in enumerate(proof["observations"]):
        yield DurableEvidence.parse(_object(item, f"proof.observations[{index}]")["evidence"], f"proof.observations[{index}].evidence")
    yield from _parse_evidence_array(packet["follow_ups"], "follow_ups")
    for index, item in enumerate(packet["shared_implementation_relations"]):
        yield DurableEvidence.parse(_object(item, f"shared_implementation_relations[{index}]")["evidence"], f"shared_implementation_relations[{index}].evidence")


def validate_blocker_closeout(packet_value: Any, is_ancestor: Callable[[str, str], bool]) -> BlockerCloseoutV1:
    """Validate and return the normalized packet model.

    ``is_ancestor(ancestor_sha, descendant_sha)`` must answer from the exact
    repository object graph for merge integration and landed-main relations.
    A false or instrument-failed answer rejects the packet; reachability is
    never inferred from issue or PR lifecycle state.
    """

    packet = _object(packet_value, "packet")
    root_keys = {
        "schema_version", "release", "blocker_id", "controller_issue", "semantic_controller",
        "status", "observed_main_sha", "implementation_prs", "merged_shas",
        "implementation_contributions", "landed_integrations", "reviews", "proof", "claim_effect", "follow_ups",
        "invalidation_paths", "shared_implementation_relations",
    }
    _exact_keys(packet, "packet", root_keys)
    _require(packet["schema_version"] == SCHEMA_VERSION, "schema_version is invalid")
    release = _string(packet["release"], "release")
    _require(SEMVER.fullmatch(release) is not None, "release must be a semantic version without choosing one in the validator")
    blocker_id = _identifier(packet["blocker_id"], "blocker_id")
    controller_issue = _positive_int(packet["controller_issue"], "controller_issue")
    status = _string(packet["status"], "status")
    _require(status in TERMINAL_STATUSES, "status is invalid")
    observed_main_sha = _sha(packet["observed_main_sha"], "observed_main_sha")

    controller = _object(packet["semantic_controller"], "semantic_controller")
    _exact_keys(controller, "semantic_controller", {"issue", "claims", "evidence"})
    _require(_positive_int(controller["issue"], "semantic_controller.issue") == controller_issue, "controller_issue contradicts semantic_controller.issue")
    raw_controller_claims = controller["claims"]
    _require(isinstance(raw_controller_claims, list) and raw_controller_claims, "semantic_controller.claims must be a non-empty array")
    controller_claim_contracts = tuple(
        ControllerClaim.parse(item, f"semantic_controller.claims[{index}]")
        for index, item in enumerate(raw_controller_claims)
    )
    controller_claims = tuple(item.id for item in controller_claim_contracts)
    _require(len(controller_claims) == len(set(controller_claims)), "semantic_controller claim ids must be unique")
    _require(list(controller_claims) == sorted(controller_claims), "semantic_controller claims must be sorted by id")
    controller_evidence = DurableEvidence.parse(controller["evidence"], "semantic_controller.evidence")
    _require(
        controller_evidence.kind in {"github_issue", "github_issue_comment"},
        "semantic_controller.evidence must be issue authority",
    )
    controller_pattern = ISSUE_REF if controller_evidence.kind == "github_issue" else ISSUE_COMMENT_REF
    controller_match = controller_pattern.fullmatch(controller_evidence.ref)
    _require(
        controller_match is not None and int(controller_match.group(1)) == controller_issue,
        "semantic_controller.evidence ref does not match controller_issue",
    )

    raw_prs = packet["implementation_prs"]
    _require(isinstance(raw_prs, list), "implementation_prs must be an array")
    implementation_prs = tuple(_positive_int(value, "implementation_prs[]") for value in raw_prs)
    _require(len(implementation_prs) == len(set(implementation_prs)), "implementation_prs must be unique")
    _require(list(implementation_prs) == sorted(implementation_prs), "implementation_prs must be sorted")
    raw_shas = packet["merged_shas"]
    _require(isinstance(raw_shas, list), "merged_shas must be an array")
    merged_shas = tuple(_sha(value, "merged_shas[]") for value in raw_shas)
    _require(len(merged_shas) == len(set(merged_shas)), "merged_shas must be unique")
    _require(list(merged_shas) == sorted(merged_shas), "merged_shas must be sorted")

    raw_contributions = packet["implementation_contributions"]
    _require(isinstance(raw_contributions, list), "implementation_contributions must be an array")
    contributions = tuple(ImplementationContribution.parse(item, f"implementation_contributions[{index}]") for index, item in enumerate(raw_contributions))
    contribution_prs = tuple(item.implementation_pr for item in contributions)
    contribution_shas = tuple(item.merged_sha for item in contributions)
    _require(len(contribution_prs) == len(set(contribution_prs)), "implementation contribution PRs cannot be double-counted")
    _require(len(contribution_shas) == len(set(contribution_shas)), "implementation contribution SHAs cannot be double-counted")
    _require(tuple(sorted(contribution_prs)) == implementation_prs, "implementation_prs contradict implementation_contributions")
    _require(tuple(sorted(contribution_shas)) == merged_shas, "merged_shas contradict implementation_contributions")
    for contribution in contributions:
        _require(set(contribution.claim_ids) <= set(controller_claims), "implementation contribution names a claim outside the semantic controller")
        _require(contribution.evidence.kind == "github_pull", "implementation contribution evidence must be pull-request authority")
        contribution_match = PULL_REF.fullmatch(contribution.evidence.ref)
        _require(
            contribution_match is not None and int(contribution_match.group(1)) == contribution.implementation_pr,
            "implementation contribution evidence ref does not match implementation_pr",
        )

    raw_integrations = packet["landed_integrations"]
    _require(isinstance(raw_integrations, list), "landed_integrations must be an array")
    integrations = tuple(LandedIntegration.parse(item, f"landed_integrations[{index}]") for index, item in enumerate(raw_integrations))
    integration_prs = tuple(item.implementation_pr for item in integrations)
    _require(len(integration_prs) == len(set(integration_prs)), "landed integration PRs cannot be double-counted")
    _require(tuple(sorted(integration_prs)) == implementation_prs, "landed_integrations must exactly cover implementation contributions")
    contributions_by_pr = {item.implementation_pr: item for item in contributions}
    for integration in integrations:
        contribution = contributions_by_pr[integration.implementation_pr]
        _require(
            integration.candidate_head_sha == contribution.candidate_head_sha,
            "landed integration candidate head contradicts implementation contribution",
        )
        _require(
            integration.landed_sha == contribution.merged_sha,
            "landed integration SHA contradicts implementation contribution",
        )
        _require(integration.evidence.kind == "repository_receipt", "landed integration requires repository-receipt authority")
        receipt_match = REPOSITORY_REF.fullmatch(integration.evidence.ref)
        _require(
            receipt_match is not None and receipt_match.group(1) == integration.landed_sha,
            "landed integration receipt SHA does not match landed_sha",
        )
        if integration.kind == "squash":
            _require(integration.candidate_head_sha != integration.landed_sha, "squash integration must distinguish candidate head from landed SHA")
        elif integration.kind == "merge":
            try:
                candidate_reachable = is_ancestor(integration.candidate_head_sha, integration.landed_sha)
            except Exception as error:  # instrument failure must fail closed
                raise ValueError(f"merge candidate ancestry is not proven: {error}") from error
            _require(
                candidate_reachable,
                f"merge candidate head {integration.candidate_head_sha} is not reachable from landed SHA {integration.landed_sha}",
            )
        elif integration.kind == "fast_forward":
            _require(integration.candidate_head_sha == integration.landed_sha, "fast-forward integration requires candidate head to equal landed SHA")
        try:
            reachable = is_ancestor(integration.landed_sha, observed_main_sha)
        except Exception as error:  # instrument failure must fail closed
            raise ValueError(f"landed SHA reachability is not proven: {error}") from error
        _require(reachable, f"landed SHA {integration.landed_sha} is not reachable from observed_main_sha")

    raw_reviews = packet["reviews"]
    _require(isinstance(raw_reviews, list) and raw_reviews, "reviews must be a non-empty array")
    reviews: list[ReviewAuthority] = []
    review_keys: list[tuple[str, int | None]] = []
    for index, raw_review in enumerate(raw_reviews):
        name = f"reviews[{index}]"
        review = _object(raw_review, name)
        _exact_keys(
            review,
            name,
            {"authority_kind", "authority_number", "current_head_synthesis", "reviewed_head", "status", "unresolved_material_findings", "finding_refs"},
        )
        authority_kind = _string(review["authority_kind"], f"{name}.authority_kind")
        _require(authority_kind in {"implementation_pr", "semantic_controller", "landed_tree"}, f"{name}.authority_kind is invalid")
        evidence = DurableEvidence.parse(review["current_head_synthesis"], f"{name}.current_head_synthesis")
        reviewed_head = _sha(review["reviewed_head"], f"{name}.reviewed_head")
        review_status = _string(review["status"], f"{name}.status")
        _require(review_status in {"current", "stale", "not_proven"}, f"{name}.status is invalid")
        if authority_kind == "implementation_pr":
            authority_number = _positive_int(review["authority_number"], f"{name}.authority_number")
            _require(authority_number in implementation_prs, "review authority is not a modeled implementation PR")
            review_match = PULL_REVIEW_REF.fullmatch(evidence.ref)
            _require(
                evidence.kind == "github_review"
                and review_match is not None
                and int(review_match.group(1)) == authority_number,
                "review synthesis evidence ref does not match implementation PR authority",
            )
            if review_status == "current":
                _require(
                    reviewed_head == contributions_by_pr[authority_number].candidate_head_sha,
                    "current PR review head does not match modeled candidate head",
                )
        elif authority_kind == "semantic_controller":
            authority_number = _positive_int(review["authority_number"], f"{name}.authority_number")
            _require(authority_number == controller_issue, "review authority is not the semantic controller")
            review_pattern = ISSUE_REF if evidence.kind == "github_issue" else ISSUE_COMMENT_REF
            review_match = review_pattern.fullmatch(evidence.ref)
            _require(
                evidence.kind in {"github_issue", "github_issue_comment"}
                and review_match is not None
                and int(review_match.group(1)) == controller_issue,
                "review synthesis evidence ref does not match semantic-controller authority",
            )
            if review_status == "current":
                _require(reviewed_head == observed_main_sha, "current semantic-controller review refers to a superseded landed head")
        else:
            _require(review["authority_number"] is None, "landed-tree review authority_number must be null")
            authority_number = None
            _require(evidence.kind == "repository_receipt", "landed-tree review requires repository-receipt authority")
            receipt_match = REPOSITORY_REF.fullmatch(evidence.ref)
            _require(
                receipt_match is not None and receipt_match.group(1) == reviewed_head,
                "landed-tree review receipt SHA does not match reviewed_head",
            )
            if review_status == "current":
                _require(reviewed_head == observed_main_sha, "current landed-tree review refers to a superseded landed head")
        unresolved_findings = review["unresolved_material_findings"]
        _require(type(unresolved_findings) is int and unresolved_findings >= 0, f"{name}.unresolved_material_findings must be a non-negative integer")
        finding_refs = _parse_evidence_array(review["finding_refs"], f"{name}.finding_refs")
        _require(len(finding_refs) >= unresolved_findings, "unresolved material findings require durable finding references")
        reviews.append(ReviewAuthority(authority_kind, authority_number, evidence, reviewed_head, review_status, unresolved_findings))
        review_keys.append((authority_kind, authority_number))
    _require(len(review_keys) == len(set(review_keys)), "review authorities cannot be double-counted")
    current_landed_review = any(item.authority_kind == "landed_tree" and item.status == "current" for item in reviews)
    current_candidate_reviews = {
        item.authority_number
        for item in reviews
        if item.authority_kind == "implementation_pr" and item.status == "current"
    }
    every_candidate_reviewed = bool(implementation_prs) and current_candidate_reviews == set(implementation_prs)
    total_unresolved_findings = sum(item.unresolved_findings for item in reviews)
    any_current_review = current_landed_review or every_candidate_reviewed

    proof = _object(packet["proof"], "proof")
    _exact_keys(proof, "proof", {"required", "passed", "not_proven", "observations"})
    required = _unique_strings(proof["required"], "proof.required", nonempty=True, identifiers=True)
    passed = _unique_strings(proof["passed"], "proof.passed", identifiers=True)
    not_proven = _unique_strings(proof["not_proven"], "proof.not_proven", identifiers=True)
    raw_observations = proof["observations"]
    _require(isinstance(raw_observations, list) and raw_observations, "proof.observations must be non-empty")
    observations = tuple(ProofObservation.parse(item, f"proof.observations[{index}]") for index, item in enumerate(raw_observations))
    observation_ids = tuple(item.id for item in observations)
    _require(len(observation_ids) == len(set(observation_ids)), "proof observation ids must be unique")
    _require(tuple(sorted(observation_ids)) == required, "proof.required must exactly match proof.observations")
    expected_passed = tuple(sorted(item.id for item in observations if item.status == "passed"))
    expected_not_proven = tuple(sorted(item.id for item in observations if item.status in EPISTEMIC_GAPS))
    _require(passed == expected_passed, "proof.passed contradicts observation statuses")
    _require(not_proven == expected_not_proven, "proof.not_proven contradicts observation statuses")
    claim_contract_by_id = {item.id: item for item in controller_claim_contracts}
    for observation in observations:
        _require(set(observation.claim_ids) <= set(controller_claims), "proof observation names a claim outside the semantic controller")
        for claim_id in observation.claim_ids:
            contract = claim_contract_by_id[claim_id]
            _require(
                observation.proof_class == contract.proof_class and observation.claim_scope == contract.claim_scope,
                f"proof observation for {claim_id} contradicts the semantic-controller claim contract",
            )
        if observation.status == "passed":
            _require(observation.subject_sha == observed_main_sha, "passed proof is cross-subject")

    claim_effect = _object(packet["claim_effect"], "claim_effect")
    _exact_keys(claim_effect, "claim_effect", {"preserves", "narrows", "limitations"})
    preserves = _unique_strings(claim_effect["preserves"], "claim_effect.preserves", identifiers=True)
    narrows = _unique_strings(claim_effect["narrows"], "claim_effect.narrows", identifiers=True)
    limitations = _unique_strings(claim_effect["limitations"], "claim_effect.limitations")
    _require(not (set(preserves) & set(narrows)), "claim_effect cannot both preserve and narrow one claim")
    _require(set(preserves) | set(narrows) <= set(controller_claims), "claim_effect names a claim outside the semantic controller")
    passed_claim_coverage = {
        claim_id
        for observation in observations
        if observation.status == "passed"
        for claim_id in observation.claim_ids
    }
    if status == "resolved":
        _require(any_current_review, "resolved requires every candidate exact-head review or one current landed-tree cumulative review")
        _require(total_unresolved_findings == 0, "resolved cannot retain unresolved material findings")
        _require(not not_proven and all(item.status == "passed" for item in observations), "resolved requires every required proof to pass")
        _require(set(preserves) == set(controller_claims) and not narrows, "resolved must preserve every semantic-controller claim")
        _require(set(preserves) <= passed_claim_coverage, "resolved preserved claim lacks observation proof coverage")
    elif status == "bounded_limitation":
        _require(any_current_review, "bounded_limitation requires every candidate exact-head review or one current landed-tree cumulative review")
        _require(total_unresolved_findings == 0, "bounded_limitation cannot retain unresolved material findings")
        _require(all(item.status == "passed" for item in observations), "bounded_limitation requires every required proof to pass within its narrowed claim")
        _require(bool(narrows) and bool(limitations), "bounded_limitation requires exact narrowed claims and limitations")
        _require(set(preserves) | set(narrows) == set(controller_claims), "bounded_limitation must state the effect on every semantic-controller claim")
        _require(set(preserves) | set(narrows) <= passed_claim_coverage, "bounded_limitation claim lacks observation proof coverage")
    elif status == "blocked":
        # Blocked packets report a decisive failed observation or an unresolved
        # finding. They do not claim terminal proof closure, so preserved-claim
        # coverage is intentionally not promoted into a closure requirement.
        _require(any(item.status == "failed" for item in observations) or total_unresolved_findings > 0, "blocked requires a failed proof or unresolved material finding")
    else:
        # NOT_PROVEN is epistemic: missing/currentness/instrument gaps stay
        # explicit and are never converted into failed or closure evidence.
        _require(
            bool(not_proven) or any(item.status in {"stale", "not_proven"} for item in reviews),
            "not_proven requires explicit missing, stale, or instrument-failed evidence",
        )

    _parse_evidence_array(packet["follow_ups"], "follow_ups")
    _unique_strings(packet["invalidation_paths"], "invalidation_paths", nonempty=True)

    shared = packet["shared_implementation_relations"]
    _require(isinstance(shared, list), "shared_implementation_relations must be an array")
    shared_keys: list[tuple[int, str]] = []
    by_pr = {item.implementation_pr: item for item in contributions}
    for index, raw_relation in enumerate(shared):
        name = f"shared_implementation_relations[{index}]"
        relation = _object(raw_relation, name)
        _exact_keys(relation, name, {"implementation_pr", "other_blocker_id", "relation", "claim_ids", "evidence"})
        implementation_pr = _positive_int(relation["implementation_pr"], f"{name}.implementation_pr")
        other_blocker = _identifier(relation["other_blocker_id"], f"{name}.other_blocker_id")
        _require(other_blocker != blocker_id, f"{name}.other_blocker_id must name a different blocker")
        _require(implementation_pr in by_pr, f"{name}.implementation_pr is not a packet contribution")
        relation_kind = _string(relation["relation"], f"{name}.relation")
        _require(relation_kind in {"shared", "absorbed_by", "supersedes", "dependency"}, f"{name}.relation is invalid")
        relation_claims = _unique_strings(relation["claim_ids"], f"{name}.claim_ids", nonempty=True, identifiers=True)
        _require(set(relation_claims) <= set(by_pr[implementation_pr].claim_ids), f"{name}.claim_ids exceed the contribution")
        DurableEvidence.parse(relation["evidence"], f"{name}.evidence")
        shared_keys.append((implementation_pr, other_blocker))
    _require(len(shared_keys) == len(set(shared_keys)), "shared implementation relations cannot be double-counted")

    evidence_by_ref: dict[str, str] = {}
    for evidence in _all_evidence(packet):
        previous = evidence_by_ref.setdefault(evidence.ref, evidence.digest)
        _require(previous == evidence.digest, f"contradictory digest for evidence ref {evidence.ref}")

    return BlockerCloseoutV1(
        release, blocker_id, controller_issue, controller_claim_contracts,
        controller_evidence, status, observed_main_sha, contributions, integrations,
        tuple(reviews), observations,
    )


def _git_ancestor_checker(repository: Path) -> Callable[[str, str], bool]:
    def is_ancestor(ancestor: str, descendant: str) -> bool:
        process = subprocess.run(
            ["git", "merge-base", "--is-ancestor", ancestor, descendant],
            cwd=repository,
            check=False,
            capture_output=True,
            text=True,
            timeout=15,
        )
        if process.returncode == 0:
            return True
        if process.returncode == 1:
            return False
        detail = process.stderr.strip() or process.stdout.strip() or f"exit {process.returncode}"
        raise ValueError(f"git merge-base failed: {detail}")

    return is_ancestor


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packet", required=True, type=Path)
    parser.add_argument("--repository", type=Path, default=Path("."))
    args = parser.parse_args(argv)
    try:
        packet = json.loads(args.packet.read_text(encoding="utf-8"))
        validate_blocker_closeout(packet, _git_ancestor_checker(args.repository.resolve()))
    except (OSError, json.JSONDecodeError, ValueError, subprocess.SubprocessError) as error:
        print(f"blocker-closeout: invalid: {error}", file=sys.stderr)
        return 1
    print(f"blocker-closeout: valid ({packet['blocker_id']}: {packet['status']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
