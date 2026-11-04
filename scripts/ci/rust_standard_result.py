#!/usr/bin/env python3
"""Run the existing consumer refusal after validating central selected proof."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
if __package__:
    from .rust_small_evidence import validate_proof_evidence, unique_object
    from .export_rust_small_receipt import regular_bytes, MAX_RECEIPT_BYTES
    from .rust_small_subject import product_subject
else:
    from rust_small_evidence import validate_proof_evidence, unique_object
    from export_rust_small_receipt import regular_bytes, MAX_RECEIPT_BYTES
    from rust_small_subject import product_subject


def imported_receipt(environment, *, proof_sha=None):
    """Require actual imported product bytes after immutable central validation.

    Central owns native authentication/import and Rust semantic verification;
    these policy checks bind its imported product result to this policy subject.
    """
    if environment.get('EM_CI_LANE') != 'perl-rust-small':
        raise ValueError('canonical Perl import missing')
    root = Path(environment.get('EM_CI_ARTIFACT_DIR', ''))
    if not root.is_absolute():
        raise ValueError('canonical imported directory missing')
    raw, _ = regular_bytes(root / 'rust-small-proof.json', MAX_RECEIPT_BYTES)
    receipt = json.loads(raw, object_pairs_hook=unique_object)
    if receipt.get('schema_version') != 'rust_small_proof.v1' or receipt.get('result') != 'success':
        raise ValueError('canonical product receipt did not succeed')
    subject = receipt.get('subject', {})
    if subject.get('git_sha') != (proof_sha or environment['GITHUB_SHA']) or subject.get('worktree_dirty') is not False:
        raise ValueError('canonical imported source mismatch')


def policy_environment(environment, *, acquire_plan=None):
    if environment.get('EM_CI_SELECTED_PROOF_RESULT') != 'success':
        raise ValueError('selected proof did not succeed')
    mapped = dict(environment)
    acquired = None
    if acquire_plan is not None:
        context_bytes, _ = regular_bytes(Path(environment['RUNNER_TEMP']) / 'em-ci-job-context.json', 16384)
        context = json.loads(context_bytes, object_pairs_hook=unique_object)
        acquired = product_subject(environment, context, acquire_plan)
    for target, source in [('EXPECTED_REPOSITORY', 'GITHUB_REPOSITORY'), ('EXPECTED_SHA', 'GITHUB_SHA'),
                           ('EXPECTED_RUN_ID', 'GITHUB_RUN_ID'), ('EXPECTED_RUN_ATTEMPT', 'GITHUB_RUN_ATTEMPT')]:
        mapped[target] = environment.get(source, '')
    if acquired is not None:
        mapped['EXPECTED_SHA'] = acquired['head']
    validate_proof_evidence(mapped)
    imported_receipt(environment, proof_sha=acquired['head'] if acquired is not None else None)
    event_name = environment.get('GITHUB_EVENT_NAME')
    if event_name not in ('pull_request', 'merge_group', 'workflow_dispatch', 'push'):
        raise ValueError('unsupported event')
    event = json.loads(Path(environment['GITHUB_EVENT_PATH']).read_text())
    candidate = tree = ''
    if event_name == 'pull_request':
        pr = event['pull_request']
        if pr.get('draft') is not False:
            raise ValueError('draft or ambiguous PR cannot execute result policy')
        candidate, tree = pr['head']['sha'], environment['GITHUB_SHA']
    elif event_name == 'workflow_dispatch':
        if acquired is not None:
            candidate, tree = acquired['head'], acquired['control']
        else:
            inputs = event.get('inputs', {})
            if inputs.get('pre_merge_admission') != 'true' or inputs.get('reviewed_sha') != environment['GITHUB_SHA']:
                raise ValueError('dispatch lacks exact reviewed integration admission')
            candidate = tree = inputs['reviewed_sha']
    elif event_name == 'merge_group':
        candidate = tree = event['merge_group']['head_sha']
    if event_name in ('pull_request', 'merge_group', 'workflow_dispatch') and any(not re.fullmatch('[0-9a-f]{40}', value) for value in (candidate, tree)):
        raise ValueError('event subject is absent')
    if acquired is not None and product_subject(environment, context, acquire_plan) != acquired:
        raise ValueError('acquired subject changed during result policy')
    return dict(mapped, REPOSITORY=environment['GITHUB_REPOSITORY'], EVENT_NAME=event_name,
                CANDIDATE_SHA=candidate, EFFECTIVE_WORKFLOW_TREE=tree)


def main():
    try:
        environment = policy_environment(os.environ)
        return subprocess.run(['bash', '.ci/rust-main-red-probe.sh'], env=environment, check=False).returncode
    except (ValueError, KeyError, TypeError, OSError):
        print('::error::Rust Small result policy NOT_PROVEN', file=sys.stderr)
        return 1

if __name__ == '__main__':
    raise SystemExit(main())
