#!/usr/bin/env python3
"""candidate_readiness.v1: join existing authorities, never execute proof or merge.

Typed observations are returned by trusted in-process authority adapters. This
module accepts no receipt JSON, readiness flag or command override. The default
control adapter acquires actual canonical review facts and refuses missing proof
interfaces; a central importer can supply already authenticated canonical proof.
"""
from __future__ import annotations

from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import subprocess
from typing import Protocol

DIGEST = re.compile(r'[0-9a-f]{64}')


@dataclass(frozen=True)
class Review:
    subject_key: str
    classification: str
    reference: str
    reason: str


@dataclass(frozen=True)
class Selection:
    subject_key: str
    portfolio_sha256: str
    policy_sha256: str
    selected: tuple[str, ...]
    integration_obligations: tuple[str, ...]
    reference: str
    inapplicable_reason: str = ''


@dataclass(frozen=True)
class CommittedProof:
    subject_key: str
    disposition: str  # verified, failure, unavailable, untrusted
    executed: tuple[str, ...]
    failed: tuple[str, ...]
    canonical_receipt_sha256: str
    producer_reference: str
    reason: str = ''


@dataclass(frozen=True)
class IntegrationFinding:
    subject_key: str
    disposition: str  # unresolved_failure, repaired, pending, absent
    reference: str


class Authorities(Protocol):
    def semantic_review(self, plan: dict) -> Review: ...
    def native_review(self, plan: dict) -> Review: ...
    def selection(self, plan: dict) -> Selection | None: ...
    def committed_proof(self, plan: dict, selected: Selection) -> CommittedProof: ...
    def integration_findings(self, plan: dict) -> tuple[IntegrationFinding, ...]: ...


def evaluate(plan: dict, authorities: Authorities) -> dict:
    """Join independently acquired owner decisions, preserving all bounded gaps.

    `authorities` is trusted control code, not a deserialized request object.
    Selection and execution semantics stay with their existing owners. Missing
    future integration checks are next-stage obligations, never entry proof.
    """
    key = plan['request_key']
    gaps = []
    observations = []
    obligations: tuple[str, ...] = ()

    def gap(code, classification='NOT_PROVEN', reference=''):
        gaps.append({'code': code, 'classification': classification, 'reference': reference})

    for name in ('semantic_review', 'native_review'):
        row = getattr(authorities, name)(plan)
        if not isinstance(row, Review) or row.subject_key != key or not row.reference:
            gap(name + '_unbound')
            continue
        observations.append({'authority': name, 'classification': row.classification,
                             'reference': row.reference, 'reason': row.reason})
        expected = 'REVIEW_CURRENT' if name == 'semantic_review' else 'CONVERGED'
        if row.classification != expected:
            gap(name, 'NOT_READY' if row.classification == 'NOT_READY' else 'NOT_PROVEN', row.reference)

    selected = authorities.selection(plan)
    selection_valid = isinstance(selected, Selection)
    if not selection_valid:
        gap('existing_selection_interface_unavailable')
    else:
        selection_valid = (selected.subject_key == key
            and selected.portfolio_sha256 == plan['portfolio_sha256']
            and selected.policy_sha256 == plan['policy_sha256']
            and bool(selected.reference)
            and isinstance(selected.selected, tuple)
            and len(set(selected.selected)) == len(selected.selected)
            and all(isinstance(x, str) and x for x in selected.selected)
            and isinstance(selected.integration_obligations, tuple)
            and bool(selected.integration_obligations)
            and len(set(selected.integration_obligations)) == len(selected.integration_obligations)
            and isinstance(selected.inapplicable_reason, str)
            and all(isinstance(x, str) and x for x in selected.integration_obligations))
        if not selection_valid:
            gap('selection_subject_policy_or_obligations_mismatch')
        else:
            obligations = selected.integration_obligations
            if not selected.selected:
                if not selected.inapplicable_reason:
                    gap('empty_selection_without_positive_inapplicability')
            elif selected.inapplicable_reason:
                gap('contradictory_selection_inapplicability')
            else:
                proof = authorities.committed_proof(plan, selected)
                if not isinstance(proof, CommittedProof) or proof.subject_key != key:
                    gap('committed_proof_subject_mismatch')
                elif proof.disposition == 'untrusted':
                    gap('committed_proof_requires_isolated_replay', 'UNTRUSTED_REPLAY_REQUIRED', proof.producer_reference)
                elif proof.disposition == 'failure':
                    gap('committed_proof_failed', 'NOT_READY', proof.producer_reference)
                elif proof.disposition != 'verified':
                    gap('committed_proof_unavailable', reference=proof.producer_reference)
                elif (not DIGEST.fullmatch(proof.canonical_receipt_sha256)
                      or not isinstance(proof.producer_reference, str) or not proof.producer_reference
                      or not isinstance(proof.executed, tuple) or proof.executed != selected.selected
                      or not isinstance(proof.failed, tuple) or proof.failed):
                    gap('committed_proof_incomplete_or_unauthenticated')
                else:
                    observations.append({'authority': 'committed_proof', 'classification': 'verified',
                        'reference': proof.producer_reference,
                        'canonical_receipt_sha256': proof.canonical_receipt_sha256})

    findings = authorities.integration_findings(plan)
    if not isinstance(findings, tuple):
        gap('integration_applicability_observation_unavailable')
        findings = ()
    for finding in findings:
        if not isinstance(finding, IntegrationFinding) or finding.subject_key != key or not finding.reference:
            gap('integration_finding_unbound')
        elif finding.disposition == 'unresolved_failure':
            gap('applicable_integration_failure', 'NOT_READY', finding.reference)
        elif finding.disposition not in {'repaired', 'pending', 'absent'}:
            gap('integration_finding_disposition_unknown', reference=finding.reference)

    priorities = ('NOT_READY', 'UNTRUSTED_REPLAY_REQUIRED', 'NOT_PROVEN')
    classification = next((state for state in priorities if any(g['classification'] == state for g in gaps)), 'READY')
    return {'schema_version': 'candidate_readiness.v1', 'classification': classification,
        'subject': dict(plan), 'observations': observations,
        'integration_obligations': list(obligations), 'gaps': gaps,
        'first_actionable_gap': gaps[0] if gaps else None,
        'execution_authorized': False, 'merge_authorized': False}


class ExistingControlAuthorities:
    """Live canonical review adapters; remaining ports must use existing owners.

    Runs fixed read-only authority commands from the exact clean control checkout.
    No Cargo compilation or product execution. This intentionally has no way to
    turn a local/caller receipt into authenticated committed evidence.
    """
    def __init__(self, root: Path, *, selection_owner=None, committed_owner=None, applicability_owner=None):
        for owner in (selection_owner, committed_owner, applicability_owner):
            if owner is not None and not callable(owner):
                raise ValueError('owner interface must be trusted in-process code')
        self.root = root
        self.selection_owner = selection_owner
        self.committed_owner = committed_owner
        self.applicability_owner = applicability_owner

    def command(self, argv: list[str]) -> dict:
        if any(name in os.environ for name in ('SEMANTIC_REVIEW_TEST_FIXTURE', 'CONVERGENCE_TEST_FIXTURE_DIR')):
            raise ValueError('fixture environment cannot supply live admission')
        environment = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        result = subprocess.run(argv, cwd=self.root, env=environment, capture_output=True, text=True, timeout=60, check=False)
        if result.returncode not in (0, 1):
            raise ValueError('canonical authority instrument failed')
        if len(result.stdout) > 1024 * 1024:
            raise ValueError('authority response exceeds bound')
        # Both existing review authorities emit one JSON object on stdout.
        raw = result.stdout
        if argv[0] == 'bash' and '\n{' in raw:
            raw = raw[raw.index('\n{') + 1:]
        payload = json.loads(raw)
        if not isinstance(payload, dict):
            raise ValueError('authority omitted an object')
        payload['_authority_exit_code'] = result.returncode
        return payload

    def semantic_review(self, plan):
        row = self.command(['python3', '-B', 'scripts/ci/check-pr-semantic-review-currentness.py',
            str(plan['pr']), plan['repository'], '--root', str(self.root), '--expected-head', plan['head']])
        live = row.get('acquisition_mode') == 'gh_live' and row.get('current_head') == plan['head'] and row.get('_authority_exit_code') == 0
        classification = row.get('classification') if live else 'NOT_PROVEN'
        return Review(plan['request_key'], classification, 'check-pr-semantic-review-currentness.py', row.get('reason', 'missing review'))

    def native_review(self, plan):
        row = self.command(['bash', 'scripts/ci/check-pr-review-convergence', str(plan['pr']), plan['repository']])
        live = row.get('wrapper_schema_version') == 'pr_review_convergence.v2' and row.get('headRefOid') == plan['head']
        # The final-integration wrapper also blocks draft state. Admission
        # consumes its native finding/request facts, not that lifecycle gate.
        # Semantic currentness remains entirely with the separate owner above.
        counts = ('unresolved_active', 'unresolved_outdated', 'unresolved_total', 'resolved_without_disposition')
        complete = (all(type(row.get(k)) is int and row[k] >= 0 for k in counts)
                    and isinstance(row.get('pending_reviewers'), list)
                    and isinstance(row.get('current_change_requests'), list)
                    and 'review_decision' in row
                    and row['unresolved_total'] == row['unresolved_active'] + row['unresolved_outdated'])
        classification = 'NOT_PROVEN'
        if live and complete:
            if row['current_change_requests'] or row['review_decision'] == 'CHANGES_REQUESTED' or row['unresolved_total'] or row['resolved_without_disposition']:
                classification = 'NOT_READY'
            elif not row['pending_reviewers']:
                classification = 'CONVERGED'
        return Review(plan['request_key'], classification, 'check-pr-review-convergence', 'native current review/finding observation')

    def selection(self, plan):
        # pre_push_plan/ci_scope own selection; their complete authenticated
        # adapter is supplied by central composition, never copied here.
        return self.selection_owner(plan) if self.selection_owner is not None else None

    def committed_proof(self, plan, selected):
        if self.committed_owner is not None:
            return self.committed_owner(plan, selected)
        return CommittedProof(plan['request_key'], 'unavailable', (), (), '', '',
                              'existing canonical import owner has not supplied this committed proof')

    def integration_findings(self, plan):
        # No missing context is converted into an adverse verdict or a pass.
        # Acquisition of applicable existing failures must be supplied too.
        if self.applicability_owner is not None:
            return self.applicability_owner(plan)
        return (IntegrationFinding(plan['request_key'], 'unavailable', 'existing required-result applicability owner'),)


def production(plan: dict, *, selection_owner=None, committed_owner=None, applicability_owner=None):
    """Adapter callable; no injectable CLI JSON or environment pass string."""
    root = Path(__file__).resolve().parents[2]
    try:
        git_env = {k: v for k, v in os.environ.items() if k not in {'GH_TOKEN', 'GITHUB_TOKEN', 'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN'} and not k.startswith('GIT_')}
        head = subprocess.run(['git', '-c', 'core.fsmonitor=false', 'rev-parse', 'HEAD'], cwd=root, env=git_env, capture_output=True,
                              text=True, timeout=10, check=True).stdout.strip()
        if head != plan['control']:
            return {'classification': 'NOT_PROVEN', 'reason': '#10057 admission requires exact clean control source S'}
        status = subprocess.run(['git', '-c', 'core.fsmonitor=false', 'status', '--porcelain', '--untracked-files=all'],
                                cwd=root, env=git_env, capture_output=True, text=True, timeout=10, check=False)
        if status.returncode != 0 or status.stdout.strip():
            return {'classification': 'NOT_PROVEN', 'reason': '#10057 admission requires exact clean control source S'}
        result = evaluate(plan, ExistingControlAuthorities(root, selection_owner=selection_owner,
            committed_owner=committed_owner, applicability_owner=applicability_owner))
        reason = result['first_actionable_gap']
        return {**result, 'reason': '#10057 ' + (reason['code'] if reason else 'existing authority join complete; explicit request still required')}
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        return {'classification': 'NOT_PROVEN', 'reason': '#10057 existing authority acquisition unavailable'}
