"""Inactive H-bound publisher preparation. No HTTP mutation or token acquisition.

Only trusted control code may pass a VerifiedQualification after canonical
semantic verification and native artifact import. This module accepts no JSON
proof/readiness override and does not turn diagnostic metadata into proof.
"""
from dataclasses import dataclass
import re

REPOSITORY = 'EffortlessMetrics/perl-lsp-swarm'
CONTEXT = 'Perl LSP Rust Small Result'
APP_ID = 15368
SHA256 = re.compile(r'[0-9a-f]{64}')


@dataclass(frozen=True)
class VerifiedQualification:
    request_key: str
    head: str
    tree: str
    base: str
    control: str
    portfolio_sha256: str
    policy_sha256: str
    run_id: int
    run_attempt: int
    selected: int
    executed: int
    failed: int
    result: str
    canonical_receipt_sha256: str
    authenticated_producer_reference: str


def prepare(adapter, run_id: int, run_attempt: int, proof: VerifiedQualification) -> dict:
    """Prepare one exact existing Rust Small context; perform read-only reacquisition.

    The future privileged control job must obtain `proof` from the actual
    canonical import owner, never deserialize this type from candidate data.
    No permission decision, publisher workflow or write path is installed here.
    """
    from .request_qualification import Refused, positive
    if not isinstance(proof, VerifiedQualification):
        raise Refused('publisher requires canonical owner typed verification')
    plan = adapter.snapshot()
    if plan['repository'] != REPOSITORY or plan['app_id'] != APP_ID or plan['check_name'] != CONTEXT:
        raise Refused('publication outside scoped Actions app/Rust Small context')
    if not positive(run_id) or not positive(run_attempt):
        raise Refused('publisher lacks native run identity')
    for key in ('request_key', 'head', 'tree', 'base', 'control', 'portfolio_sha256', 'policy_sha256'):
        if getattr(proof, key) != plan[key]:
            raise Refused('publisher proof differs from acquired subject')
    if (not positive(proof.run_id) or not positive(proof.run_attempt)
            or proof.run_id != run_id or proof.run_attempt != run_attempt
            or not SHA256.fullmatch(proof.canonical_receipt_sha256)
            or not proof.authenticated_producer_reference):
        raise Refused('publisher lacks exact authenticated canonical proof')
    run = adapter.api.get(f'{adapter.root}/actions/runs/{run_id}')
    if not adapter.matches(run, plan, run_id=run_id) or run['run_attempt'] != run_attempt:
        raise Refused('publisher native control run/attempt mismatch')
    if run.get('status') != 'in_progress' or run.get('conclusion') is not None:
        raise Refused('publisher must belong to the live native control attempt')
    if (not positive(proof.selected) or not positive(proof.executed)
            or proof.executed > proof.selected or type(proof.failed) is not int
            or proof.failed < 0 or proof.failed > proof.executed):
        raise Refused('publisher execution missing or contradictory')
    if proof.result == 'success':
        if proof.executed != proof.selected or proof.failed:
            raise Refused('publisher cannot report partial/failed execution as success')
        conclusion = 'success'
    elif proof.result == 'product_failure' and proof.failed > 0:
        conclusion = 'failure'
    else:
        raise Refused('publisher cannot certify rehearsal/cancellation/incomplete proof')
    adapter.current(plan)
    latest = adapter.api.get(f'{adapter.root}/actions/runs/{run_id}')
    if (not adapter.matches(latest, plan, run_id=run_id) or latest['run_attempt'] != run_attempt
            or latest.get('status') != 'in_progress' or latest.get('conclusion') is not None):
        raise Refused('publisher native control attempt moved during preparation')
    external = f'{plan["request_key"]}:{run_id}:{run_attempt}'
    url = f'https://github.com/{plan["repository"]}/actions/runs/{run_id}/attempts/{run_attempt}'
    return {'status': 'PREPARED_NOT_PUBLISHED', 'app_id': APP_ID, 'subject': plan,
        'required_permission': 'checks: write', 'method': 'POST',
        'endpoint': f'{adapter.root}/check-runs',
        'payload': {'name': CONTEXT, 'head_sha': plan['head'], 'external_id': external,
            'details_url': url, 'status': 'completed', 'conclusion': conclusion,
            'output': {'title': 'Canonical Rust Small ' + conclusion,
                'summary': 'Verified canonical receipt SHA256 ' + proof.canonical_receipt_sha256}},
        'effects': [], 'published': False}
