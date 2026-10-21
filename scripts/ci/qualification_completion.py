"""Prepare the existing result manifest from canonical proof and native readback.

No check is written here. Without an exact existing H/app check the manifest is
not_proven even when canonical product execution succeeded. Candidate JSON cannot
supply the typed canonical observation or certify a native publication.
"""
from .qualification_publication import VerifiedQualification, prepare
from .request_qualification import Refused, positive


def build(adapter, proof: VerifiedQualification, *, check_run_id=None):
    if not isinstance(proof, VerifiedQualification):
        raise Refused('completion requires canonical owner typed verification')
    prepared = prepare(adapter, proof.run_id, proof.run_attempt, proof)
    plan = prepared['subject']
    manifest = {key: plan[key] for key in ('repository', 'repository_id', 'pr', 'head', 'tree',
        'base', 'control', 'portfolio_sha256', 'policy_sha256', 'request_key', 'workflow_id', 'workflow_path')}
    manifest.update(schema_version=1, run_id=proof.run_id, run_attempt=proof.run_attempt,
        selected=proof.selected, executed=proof.executed, failed=proof.failed,
        result='not_proven', check_run_id=None,
        canonical_receipt_sha256=proof.canonical_receipt_sha256)
    if check_run_id is None:
        manifest['reason'] = 'canonical product evidence retained; exact H/app publication absent'
        return manifest
    if not positive(check_run_id):
        raise Refused('completion requires native check identity')
    check = adapter.api.get(f'{adapter.root}/check-runs/{check_run_id}')
    payload = prepared['payload']
    if (check.get('id') != check_run_id or check.get('app', {}).get('id') != prepared['app_id']
            or any(check.get(key) != payload[key] for key in
                   ('name', 'head_sha', 'external_id', 'details_url', 'status', 'conclusion'))):
        raise Refused('completion native H/app/check/run/attempt association mismatch')
    adapter.current(plan)
    run = adapter.api.get(f'{adapter.root}/actions/runs/{proof.run_id}')
    if (not adapter.matches(run, plan, run_id=proof.run_id) or run['run_attempt'] != proof.run_attempt
            or run.get('status') != 'in_progress' or run.get('conclusion') is not None):
        raise Refused('completion native control attempt moved during readback')
    manifest.update(result=proof.result, check_run_id=check_run_id)
    return manifest
