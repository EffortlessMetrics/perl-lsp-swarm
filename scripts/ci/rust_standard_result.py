#!/usr/bin/env python3
"""Run the existing consumer refusal after validating central selected proof."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
if __package__:
    from .rust_small_evidence import validate_proof_evidence
else:
    from rust_small_evidence import validate_proof_evidence


def policy_environment(environment):
    if environment.get('EM_CI_SELECTED_PROOF_RESULT') != 'success':
        raise ValueError('selected proof did not succeed')
    mapped = dict(environment)
    for target, source in [('EXPECTED_REPOSITORY', 'GITHUB_REPOSITORY'), ('EXPECTED_SHA', 'GITHUB_SHA'),
                           ('EXPECTED_RUN_ID', 'GITHUB_RUN_ID'), ('EXPECTED_RUN_ATTEMPT', 'GITHUB_RUN_ATTEMPT')]:
        mapped[target] = environment.get(source, '')
    validate_proof_evidence(mapped)
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
    elif event_name == 'merge_group':
        candidate = tree = event['merge_group']['head_sha']
    if event_name in ('pull_request', 'merge_group') and any(not re.fullmatch('[0-9a-f]{40}', value) for value in (candidate, tree)):
        raise ValueError('event subject is absent')
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
