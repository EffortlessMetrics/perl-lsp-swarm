"""Bind an acquired candidate to native control custody without granting admission.

The caller must supply the trusted in-process native plan acquirer. No dispatch
input, environment variable or candidate file enables this bridge. The central
owner must independently capture qualification_subject in its native job context;
the existing central interface lacks that capture and therefore refuses H != S.
"""
import os
from pathlib import Path
import re
import subprocess

SUBJECT_FIELDS = ('head', 'tree', 'base', 'control', 'request_key')


def product_subject(environment, context, acquire_plan):
    """Consume existing snapshot/custody interfaces; never evaluate readiness."""
    if not callable(acquire_plan):
        raise ValueError('trusted native plan acquirer missing')
    plan = acquire_plan()
    if not isinstance(plan, dict) or plan.get('repository') != environment.get('GITHUB_REPOSITORY'):
        raise ValueError('acquired repository mismatch')
    subject = {field: plan.get(field) for field in SUBJECT_FIELDS}
    for field, value in subject.items():
        width = 64 if field == 'request_key' else 40
        if not isinstance(value, str) or not re.fullmatch('[0-9a-f]{' + str(width) + '}', value) or value == '0' * width:
            raise ValueError('acquired subject identity missing')
    if environment.get('GITHUB_EVENT_NAME') != 'workflow_dispatch' or subject['control'] != environment.get('GITHUB_SHA'):
        raise ValueError('acquired control is not this native dispatch')
    expected = dict(schema='em-ci-job-context.v1', repository=plan['repository'],
                    sha=subject['control'], run_id=environment.get('GITHUB_RUN_ID'),
                    run_attempt=environment.get('GITHUB_RUN_ATTEMPT'), profile='standard')
    if (not isinstance(context, dict)
            or any(context.get(key) != value for key, value in expected.items())
            or context.get('qualification_subject') != subject):
        raise ValueError('central acquired subject capture missing or contradictory')
    for field in ('run_id', 'run_attempt'):
        if not isinstance(expected[field], str) or not re.fullmatch('[1-9][0-9]*', expected[field]):
            raise ValueError('native run identity missing')
    workspace = Path(environment.get('GITHUB_WORKSPACE', ''))
    if not workspace.is_absolute():
        raise ValueError('candidate checkout missing')
    # Hooks may leave repository overrides behind. Read only this checkout and
    # disable status helpers rather than executing a candidate fsmonitor hook.
    metadata_credentials = {'GH_TOKEN', 'GITHUB_TOKEN', 'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN'}
    git_environment = {key: value for key, value in os.environ.items()
                       if not key.startswith('GIT_') and key not in metadata_credentials}

    def git(*args):
        try:
            result = subprocess.run(['git', '-c', 'core.fsmonitor=false', '-C', str(workspace), *args],
                                    env=git_environment, capture_output=True, text=True,
                                    encoding='utf-8', timeout=10, check=False)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise ValueError('candidate checkout cannot be acquired') from error
        if result.returncode or len(result.stdout) > 1024 * 1024:
            raise ValueError('candidate checkout cannot be acquired')
        return result.stdout.strip()

    if (Path(git('rev-parse', '--show-toplevel')).resolve() != workspace.resolve()
            or git('rev-parse', 'HEAD') != subject['head']
            or git('rev-parse', 'HEAD^{tree}') != subject['tree']
            or git('status', '--porcelain', '--untracked-files=no')):
        raise ValueError('candidate checkout is not the exact clean acquired subject')
    return subject
