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
SEMVER = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$")
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
PRIVATE_REF = re.compile(
    r"(?:(?:^|repo:)[A-Za-z]:[\\/]|^/tmp/|^/home/|\.codex(?:[\\/]|$)|worktrees?(?:[\\/]|$))",
    re.IGNORECASE,
)
GITHUB_REF = re.compile(r"^https://github\.com/EffortlessMetrics/perl-lsp-swarm/(?:issues|pull|actions/runs)/[0-9]+(?:[#/?].*)?$")
REPOSITORY_REF = re.compile(r"^repo:[^@\s]+@[0-9a-f]{40}$")


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
        if kind.startswith("github_"):
            _require(GITHUB_REF.fullmatch(reference) is not None, f"{name}.ref must be a durable repository GitHub reference")
        else:
            _require(REPOSITORY_REF.fullmatch(reference) is not None, f"{name}.ref must bind a repository path to an exact SHA")
        digest = _string(data["digest"], f"{name}.digest")
        _require(DIGEST.fullmatch(digest) is not None, f"{name}.digest must be sha256:<64 lowercase hex>")
        return cls(kind, reference, digest)


@dataclass(frozen=True)
class ImplementationContribution:
    implementation_pr: int
    merged_sha: str
    claim_ids: tuple[str, ...]
    relation: str
    evidence: DurableEvidence

    @classmethod
    def parse(cls, value: Any, name: str) -> "ImplementationContribution":
        data = _object(value, name)
        _exact_keys(data, name, {"implementation_pr", "merged_sha", "claim_ids", "relation", "evidence"})
        relation = _string(data["relation"], f"{name}.relation")
        _require(relation in {"implements", "contributes", "supersedes", "absorbs"}, f"{name}.relation is invalid")
        return cls(
            _positive_int(data["implementation_pr"], f"{name}.implementation_pr"),
            _sha(data["merged_sha"], f"{name}.merged_sha"),
            _unique_strings(data["claim_ids"], f"{name}.claim_ids", nonempty=True, identifiers=True),
            relation,
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
        _require(
            not (proof_class in {"mechanism", "fixture"} and claim_scope != "repository_mechanism"),
            f"{name} cannot represent mechanism or fixture evidence as product behavior",
        )
        return cls(
            _identifier(data["id"], f"{name}.id"),
            _unique_strings(data["claim_ids"], f"{name}.claim_ids", nonempty=True, identifiers=True),
            _sha(data["subject_sha"], f"{name}.subject_sha"),
            status,
            proof_class,
            claim_scope,
            DurableEvidence.parse(data["evidence"], f"{name}.evidence"),
        )


@dataclass(frozen=True)
class BlockerCloseoutV1:
    release: str
    blocker_id: str
    controller_issue: int
    controller_claim_ids: tuple[str, ...]
    status: str
    observed_main_sha: str
    contributions: tuple[ImplementationContribution, ...]
    review_status: str
    reviewed_head: str
    unresolved_findings: int
    observations: tuple[ProofObservation, ...]


def _parse_evidence_array(value: Any, name: str) -> tuple[DurableEvidence, ...]:
    _require(isinstance(value, list), f"{name} must be an array")
    return tuple(DurableEvidence.parse(item, f"{name}[{index}]") for index, item in enumerate(value))


def _all_evidence(packet: dict[str, Any]) -> Iterable[DurableEvidence]:
    controller = _object(packet["semantic_controller"], "semantic_controller")
    yield DurableEvidence.parse(controller["evidence"], "semantic_controller.evidence")
    for index, item in enumerate(packet["implementation_contributions"]):
        yield DurableEvidence.parse(_object(item, f"implementation_contributions[{index}]")["evidence"], f"implementation_contributions[{index}].evidence")
    review = _object(packet["review"], "review")
    yield DurableEvidence.parse(review["current_head_synthesis"], "review.current_head_synthesis")
    yield from _parse_evidence_array(review["finding_refs"], "review.finding_refs")
    proof = _object(packet["proof"], "proof")
    for index, item in enumerate(proof["observations"]):
        yield DurableEvidence.parse(_object(item, f"proof.observations[{index}]")["evidence"], f"proof.observations[{index}].evidence")
    yield from _parse_evidence_array(packet["follow_ups"], "follow_ups")
    for index, item in enumerate(packet["shared_implementation_relations"]):
        yield DurableEvidence.parse(_object(item, f"shared_implementation_relations[{index}]")["evidence"], f"shared_implementation_relations[{index}].evidence")


def validate_blocker_closeout(packet_value: Any, is_ancestor: Callable[[str, str], bool]) -> BlockerCloseoutV1:
    """Validate and return the normalized packet model.

    ``is_ancestor(merged_sha, observed_main_sha)`` must answer from the exact
    repository object graph. A false or instrument-failed answer rejects the
    packet; reachability is never inferred from issue or PR lifecycle state.
    """

    packet = _object(packet_value, "packet")
    root_keys = {
        "schema_version", "release", "blocker_id", "controller_issue", "semantic_controller",
        "status", "observed_main_sha", "implementation_prs", "merged_shas",
        "implementation_contributions", "review", "proof", "claim_effect", "follow_ups",
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
    _exact_keys(controller, "semantic_controller", {"issue", "claim_ids", "evidence"})
    _require(_positive_int(controller["issue"], "semantic_controller.issue") == controller_issue, "controller_issue contradicts semantic_controller.issue")
    controller_claims = _unique_strings(controller["claim_ids"], "semantic_controller.claim_ids", nonempty=True, identifiers=True)
    DurableEvidence.parse(controller["evidence"], "semantic_controller.evidence")

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
        try:
            reachable = is_ancestor(contribution.merged_sha, observed_main_sha)
        except Exception as error:  # instrument failure must fail closed
            raise ValueError(f"merged SHA reachability is not proven: {error}") from error
        _require(reachable, f"merged SHA {contribution.merged_sha} is not reachable from observed_main_sha")

    review = _object(packet["review"], "review")
    _exact_keys(review, "review", {"current_head_synthesis", "reviewed_head", "status", "unresolved_material_findings", "finding_refs"})
    DurableEvidence.parse(review["current_head_synthesis"], "review.current_head_synthesis")
    reviewed_head = _sha(review["reviewed_head"], "review.reviewed_head")
    review_status = _string(review["status"], "review.status")
    _require(review_status in {"current", "stale", "not_proven"}, "review.status is invalid")
    unresolved_findings = review["unresolved_material_findings"]
    _require(type(unresolved_findings) is int and unresolved_findings >= 0, "review.unresolved_material_findings must be a non-negative integer")
    finding_refs = _parse_evidence_array(review["finding_refs"], "review.finding_refs")
    _require(len(finding_refs) >= unresolved_findings, "unresolved material findings require durable finding references")
    if review_status == "current":
        _require(reviewed_head == observed_main_sha, "current review refers to a superseded candidate head")

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
    for observation in observations:
        _require(set(observation.claim_ids) <= set(controller_claims), "proof observation names a claim outside the semantic controller")
        if observation.status == "passed":
            _require(observation.subject_sha == observed_main_sha, "passed proof is cross-subject")

    claim_effect = _object(packet["claim_effect"], "claim_effect")
    _exact_keys(claim_effect, "claim_effect", {"preserves", "narrows", "limitations"})
    preserves = _unique_strings(claim_effect["preserves"], "claim_effect.preserves", identifiers=True)
    narrows = _unique_strings(claim_effect["narrows"], "claim_effect.narrows", identifiers=True)
    limitations = _unique_strings(claim_effect["limitations"], "claim_effect.limitations")
    _require(not (set(preserves) & set(narrows)), "claim_effect cannot both preserve and narrow one claim")
    _require(set(preserves) | set(narrows) <= set(controller_claims), "claim_effect names a claim outside the semantic controller")
    if status == "resolved":
        _require(review_status == "current", "resolved requires current review")
        _require(unresolved_findings == 0, "resolved cannot retain unresolved material findings")
        _require(not not_proven and all(item.status == "passed" for item in observations), "resolved requires every required proof to pass")
        _require(set(preserves) == set(controller_claims) and not narrows, "resolved must preserve every semantic-controller claim")
    elif status == "bounded_limitation":
        _require(review_status == "current", "bounded_limitation requires current review")
        _require(unresolved_findings == 0, "bounded_limitation cannot retain unresolved material findings")
        _require(all(item.status == "passed" for item in observations), "bounded_limitation requires every required proof to pass within its narrowed claim")
        _require(bool(narrows) and bool(limitations), "bounded_limitation requires exact narrowed claims and limitations")
        _require(set(preserves) | set(narrows) == set(controller_claims), "bounded_limitation must state the effect on every semantic-controller claim")
    elif status == "blocked":
        _require(any(item.status == "failed" for item in observations) or unresolved_findings > 0, "blocked requires a failed proof or unresolved material finding")
    else:
        _require(bool(not_proven) or review_status in {"stale", "not_proven"}, "not_proven requires explicit missing, stale, or instrument-failed evidence")

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
        release, blocker_id, controller_issue, controller_claims, status,
        observed_main_sha, contributions, review_status, reviewed_head,
        unresolved_findings, observations,
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
