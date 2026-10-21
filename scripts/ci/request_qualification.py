#!/usr/bin/env python3
"""Staged exact-subject qualification adapter; never merges or writes checks.

The real production readiness join refuses while authoritative selection,
warm-proof provenance and applicability owners remain uninstalled. Tests inject
those boundaries and GitHub transport; no file,
environment flag or candidate-supplied JSON can claim eligibility.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import zipfile
from dataclasses import asdict, dataclass
from typing import Any, Callable
from urllib.parse import quote

WORKFLOW = '.github/workflows/qualification-rehearsal.yml'
PREFIX = 'qualification:'
MAX_BYTES = 1024 * 1024
OID = re.compile(r'[0-9a-f]{40}')
DIGEST = re.compile(r'[0-9a-f]{64}')
REPO = re.compile(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+')


class Refused(ValueError):
    pass


class Uncertain(RuntimeError):
    pass


def positive(value: Any) -> bool:
    return type(value) is int and value > 0


def decode(raw: bytes) -> Any:
    def pairs(rows):
        result = {}
        for key, value in rows:
            if key in result:
                raise Refused('duplicate JSON field')
            result[key] = value
        return result
    def reject_constant(_):
        raise Refused('nonfinite JSON')

    if len(raw) > MAX_BYTES:
        raise Refused('response exceeds adapter bound')
    return json.loads(raw, object_pairs_hook=pairs,
                      parse_constant=reject_constant)


class GitHub:
    """Existing gh authentication; the only mutation is workflow dispatch."""
    def _call(self, method: str, path: str, body: dict | None = None) -> bytes:
        if method not in {'GET', 'POST'} or (method == 'POST' and not re.fullmatch(r'repos/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/actions/workflows/[1-9][0-9]*/dispatches', path)):
            raise Refused('transport mutation is outside workflow dispatch')
        argv = ['gh', 'api', '--method', method, path]
        if body is not None:
            argv.extend(['--input', '-'])
        try:
            result = subprocess.run(argv, input=None if body is None else json.dumps(body).encode(),
                                    capture_output=True, timeout=30, check=False)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise Uncertain(f'{method} transport unavailable') from error
        if result.returncode:
            # Never expose token-bearing stderr or retry an uncertain dispatch.
            raise Uncertain(f'{method} GitHub operation failed')
        if len(result.stdout) > MAX_BYTES:
            raise Refused('response exceeds adapter bound')
        return result.stdout

    def get(self, path: str) -> Any:
        result = decode(self._call('GET', path))
        if not isinstance(result, dict):
            raise Refused('GitHub response is not an object')
        return result

    def get_bytes(self, path: str) -> bytes:
        return self._call('GET', path)

    def dispatch(self, path: str, body: dict) -> None:
        self._call('POST', path, body)


@dataclass(frozen=True)
class Subject:
    repository: str
    repository_id: int
    pr: int
    head: str
    base: str
    control: str
    portfolio_sha256: str
    policy_sha256: str
    app_id: int
    check_name: str

    def validate(self) -> None:
        if (not REPO.fullmatch(self.repository) or not positive(self.repository_id)
                or not positive(self.pr) or not positive(self.app_id)
                or any(not OID.fullmatch(x) for x in (self.head, self.base, self.control))
                or any(not DIGEST.fullmatch(x) for x in (self.portfolio_sha256, self.policy_sha256))
                or not self.check_name.strip()):
            raise Refused('invalid expected subject')


@dataclass(frozen=True)
class Eligibility:
    classification: str
    reason: str


def unavailable_eligibility(plan: dict) -> Eligibility:
    try:
        from .candidate_readiness import production
    except ImportError:
        try:
            from candidate_readiness import production
        except ImportError:
            from scripts.ci.candidate_readiness import production
    result = production(plan)
    return Eligibility(result['classification'], result['reason'])


class Adapter:
    def __init__(self, api: Any, subject: Subject,
                 eligibility: Callable[[dict], Eligibility] = unavailable_eligibility):
        subject.validate()
        self.api, self.subject, self.eligibility = api, subject, eligibility
        self.root = f'repos/{subject.repository}'

    def snapshot(self) -> dict:
        s = self.subject
        repo = self.api.get(self.root)
        if repo.get('id') != s.repository_id or repo.get('full_name') != s.repository:
            raise Refused('repository identity changed')
        branch = repo.get('default_branch')
        if not isinstance(branch, str) or not branch:
            raise Refused('default branch unavailable')
        pr = self.api.get(f'{self.root}/pulls/{s.pr}')
        if (pr.get('number') != s.pr or pr.get('state') != 'open'
                or pr.get('base', {}).get('repo', {}).get('id') != s.repository_id
                or pr.get('head', {}).get('sha') != s.head
                or pr.get('base', {}).get('sha') != s.base):
            raise Refused('candidate head/base or repository changed')
        ref = self.api.get(f'{self.root}/git/ref/heads/{quote(branch, safe="")}')
        if ref.get('object', {}).get('sha') != s.control:
            raise Refused('trusted default-branch control changed')
        commit = self.api.get(f'{self.root}/git/commits/{s.head}')
        if commit.get('sha') != s.head:
            raise Refused('candidate commit identity unavailable')
        tree = commit.get('tree', {}).get('sha')
        if not isinstance(tree, str) or not OID.fullmatch(tree):
            raise Refused('candidate tree unavailable')
        workflow = self.api.get(f'{self.root}/actions/workflows/{WORKFLOW.rsplit("/", 1)[1]}')
        if workflow.get('path') != WORKFLOW or workflow.get('state') != 'active' or not positive(workflow.get('id')):
            raise Refused('staged workflow identity unavailable')
        binding = {**asdict(s), 'tree': tree, 'workflow_id': workflow['id'], 'workflow_path': WORKFLOW}
        key = hashlib.sha256(json.dumps(binding, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        return {**binding, 'request_key': key, 'control_branch': branch}

    def eligible(self, plan: dict) -> Eligibility:
        decision = self.eligibility(plan)
        if not isinstance(decision, Eligibility) or decision.classification not in {'READY', 'NOT_READY', 'NOT_PROVEN', 'UNTRUSTED_REPLAY_REQUIRED'}:
            raise Refused('eligibility producer returned an unsupported decision')
        return decision

    def explain(self) -> dict:
        plan = self.snapshot()
        decision = self.eligible(plan)
        self.current(plan)
        return {'status': decision.classification, 'reason': decision.reason, 'subject': plan,
                'qualification_result': 'NOT_PROVEN', 'effects': []}

    def matches(self, run: dict, plan: dict, *, run_id: int | None = None) -> bool:
        return (positive(run.get('id')) and positive(run.get('run_attempt'))
                and (run_id is None or run.get('id') == run_id)
                and run.get('event') == 'workflow_dispatch'
                and run.get('repository', {}).get('id') == plan['repository_id']
                and run.get('workflow_id') == plan['workflow_id']
                and run.get('path') == plan['workflow_path']
                and run.get('head_sha') == plan['control']
                and run.get('display_title') == PREFIX + plan['request_key'])

    def runs(self, plan: dict) -> list[dict]:
        raw = self.api.get(f'{self.root}/actions/workflows/{plan["workflow_id"]}/runs?event=workflow_dispatch&head_sha={plan["control"]}&per_page=100')
        rows = raw.get('workflow_runs')
        if not isinstance(rows, list) or type(raw.get('total_count')) is not int or raw['total_count'] != len(rows):
            raise Refused('run inventory incomplete; narrow history requires an owner disposition')
        return [r for r in rows if isinstance(r, dict) and self.matches(r, plan)]

    def current(self, plan: dict) -> None:
        if self.snapshot() != plan:
            raise Refused('subject moved during operation')

    def accepted(self, run: dict, plan: dict) -> dict:
        observed = self.api.get(f'{self.root}/actions/runs/{run["id"]}')
        if not self.matches(observed, plan, run_id=run['id']) or observed['run_attempt'] != run['run_attempt']:
            raise Refused('run or attempt changed during read-back')
        self.current(plan)
        if observed.get('status') == 'completed':
            return {'status': 'NOT_PROVEN', 'qualification_result': 'NOT_PROVEN',
                    'subject': plan, 'run_id': run['id'], 'run_attempt': run['run_attempt'],
                    'control_conclusion': observed.get('conclusion'),
                    'next_action': 'inspect exact saved run/attempt; no automatic retry'}
        if observed.get('status') not in {'queued', 'in_progress', 'waiting', 'requested', 'pending'} or observed.get('conclusion') is not None:
            raise Refused('run status is unsupported or contradictory')
        return {'status': 'PR_IN_FLIGHT' , 'qualification_result': 'NOT_PROVEN',
                'subject': plan, 'run_id': run['id'], 'run_attempt': run['run_attempt']}

    def request(self, *, recover_only: bool = False) -> dict:
        plan = self.snapshot()
        decision = self.eligible(plan)
        if decision.classification != 'READY':
            return {'status': decision.classification, 'reason': decision.reason,
                    'subject': plan, 'qualification_result': 'NOT_PROVEN'}
        runs = self.runs(plan)
        if runs:
            return self.accepted(min(runs, key=lambda r: r['id']), plan)
        if recover_only:
            raise Uncertain('matching run not observed; recovery never dispatches')
        self.current(plan)
        inputs = {'request': json.dumps(plan, sort_keys=True, separators=(',', ':'))}
        try:
            self.api.dispatch(f'{self.root}/actions/workflows/{plan["workflow_id"]}/dispatches',
                              {'ref': plan['control_branch'], 'inputs': inputs})
        except Uncertain as error:
            return {'status': 'NOT_PROVEN', 'reason': str(error), 'subject': plan,
                    'qualification_result': 'NOT_PROVEN', 'dispatch_may_have_occurred': True,
                    'next_action': 'recover the same request key; do not blindly dispatch again'}
        self.current(plan)
        runs = self.runs(plan)
        if not runs:
            return {'status': 'NOT_PROVEN', 'reason': 'dispatch acknowledged without matching run read-back',
                    'subject': plan, 'qualification_result': 'NOT_PROVEN',
                    'next_action': 'recover; do not blindly dispatch again'}
        return self.accepted(min(runs, key=lambda r: r['id']), plan)

    def elect(self, run_id: int, attempt: int) -> dict:
        """Metadata-only duplicate election; not eligibility or product success."""
        plan = self.snapshot()
        runs = self.runs(plan)
        mine = next((r for r in runs if r['id'] == run_id and r['run_attempt'] == attempt), None)
        if mine is None:
            raise Refused('current run/attempt missing from complete inventory')
        canonical = min(runs, key=lambda r: r['id'])
        self.accepted(mine, plan)
        return {'status': 'NOT_PROVEN', 'qualification_result': 'NOT_PROVEN', 'subject': plan,
                'canonical_run_id': canonical['id'], 'run_id': run_id, 'run_attempt': attempt,
                'rehearsal_elected': canonical['id'] == run_id,
                'reason': 'metadata rehearsal only; no product or result publisher is wired'}

    def inspect(self, run_id: int, attempt: int) -> dict:
        if not positive(run_id) or not positive(attempt):
            raise Refused('invalid saved run identity')
        plan = self.snapshot()
        run = self.api.get(f'{self.root}/actions/runs/{run_id}')
        if not self.matches(run, plan, run_id=run_id) or run['run_attempt'] != attempt:
            raise Refused('saved run/attempt does not match this request')
        self.current(plan)
        if run.get('status') != 'completed':
            return {**self.accepted(run, plan), 'reason': 'matching run pending; no proof yet'}
        if run.get('conclusion') not in {'success', 'failure'}:
            return {'status': 'NOT_PROVEN', 'qualification_result': 'NOT_PROVEN',
                    'reason': 'control run did not complete successfully', 'subject': plan,
                    'control_conclusion': run.get('conclusion')}
        artifacts = self.api.get(f'{self.root}/actions/runs/{run_id}/artifacts?per_page=100')
        rows = artifacts.get('artifacts')
        if not isinstance(rows, list) or type(artifacts.get('total_count')) is not int or artifacts['total_count'] != len(rows):
            raise Refused('artifact inventory incomplete')
        name = f'qualification-result-{plan["request_key"]}-{attempt}'
        matches = [a for a in rows if isinstance(a, dict) and a.get('name') == name]
        if len(matches) != 1:
            raise Refused('one exact result artifact required')
        artifact = matches[0]
        origin = artifact.get('workflow_run', {})
        if (not positive(artifact.get('id')) or artifact.get('expired') is not False
                or origin.get('id') != run_id or origin.get('repository_id') != plan['repository_id']
                or origin.get('head_sha') != plan['control']):
            raise Refused('result artifact origin mismatch')
        raw = self.api.get_bytes(f'{self.root}/actions/artifacts/{artifact["id"]}/zip')
        if len(raw) > MAX_BYTES:
            raise Refused('artifact exceeds adapter bound')
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            if archive.namelist() != ['qualification-result.json']:
                raise Refused('result artifact must contain exactly one manifest')
            entry = archive.getinfo('qualification-result.json')
            if entry.file_size > MAX_BYTES:
                raise Refused('result manifest exceeds adapter bound')
            manifest = decode(archive.read(entry))
        binding = {key: plan[key] for key in ('repository', 'repository_id', 'pr', 'head', 'tree', 'base',
                   'control', 'portfolio_sha256', 'policy_sha256', 'request_key', 'workflow_id', 'workflow_path')}
        binding.update(schema_version=1, run_id=run_id, run_attempt=attempt)
        if not isinstance(manifest, dict) or any(type(manifest.get(k)) is not type(v) or manifest.get(k) != v for k, v in binding.items()):
            raise Refused('result manifest subject/run/attempt mismatch')
        outcome = manifest.get('result')
        if outcome == 'not_proven':
            return {'status': 'NOT_PROVEN', 'qualification_result': 'NOT_PROVEN',
                    'reason': 'trusted result contains no qualification verdict', 'subject': plan,
                    'control_conclusion': run.get('conclusion')}
        if outcome not in {'success', 'product_failure'}:
            raise Refused('trusted result omitted a supported verdict')
        selected, executed, failed = (manifest.get(k) for k in ('selected', 'executed', 'failed'))
        if (not positive(selected) or not positive(executed) or executed > selected
                or type(failed) is not int or failed < 0 or failed > executed):
            raise Refused('selected proof is missing, empty or contradictory')
        if outcome == 'success' and (executed != selected or failed != 0 or run.get('conclusion') != 'success'):
            raise Refused('successful qualification lacks complete successful execution')
        if outcome == 'product_failure' and failed == 0:
            raise Refused('product failure lacks failed execution evidence')
        check_id = manifest.get('check_run_id')
        if not positive(check_id):
            raise Refused('trusted result omitted reporting check identity')
        check = self.api.get(f'{self.root}/check-runs/{check_id}')
        expected_external = f'{plan["request_key"]}:{run_id}:{attempt}'
        expected_url = f'https://github.com/{plan["repository"]}/actions/runs/{run_id}/attempts/{attempt}'
        if (check.get('id') != check_id or check.get('head_sha') != plan['head']
                or check.get('app', {}).get('id') != plan['app_id'] or check.get('name') != plan['check_name']
                or check.get('external_id') != expected_external or check.get('details_url') != expected_url
                or check.get('status') != 'completed'
                or check.get('conclusion') != ('success' if outcome == 'success' else 'failure')):
            raise Refused('approved check/app/head/run/attempt association mismatch')
        self.current(plan)
        latest = self.api.get(f'{self.root}/actions/runs/{run_id}')
        if latest.get('run_attempt') != attempt or not self.matches(latest, plan, run_id=run_id) or latest.get('conclusion') != run.get('conclusion') or latest.get('status') != 'completed':
            raise Refused('result attempt changed during acquisition')
        if outcome == 'product_failure':
            return {'status': 'PRODUCT_OR_TEST_FAILURE', 'qualification_result': 'FAILURE',
                    'subject': plan, 'run_id': run_id, 'run_attempt': attempt, 'check_run_id': check_id}
        decision = self.eligible(plan)
        self.current(plan)
        return {'status': 'SUCCESS' if decision.classification == 'READY' else decision.classification,
                'qualification_result': 'SUCCESS', 'eligibility': decision.classification,
                'subject': plan, 'run_id': run_id, 'run_attempt': attempt,
                'check_run_id': check_id, 'reason': 'evidence only; separate final merge judgment required'}


def rehearse_event(api: Any, event: dict, native: dict) -> dict:
    """Verify dispatch data against native context; emit no product success."""
    if native.get('GITHUB_EVENT_NAME') != 'workflow_dispatch':
        raise Refused('rehearsal requires an explicit native dispatch')
    raw = event.get('inputs', {}).get('request')
    if not isinstance(raw, str):
        raise Refused('dispatch omitted request data')
    incoming = decode(raw.encode())
    if not isinstance(incoming, dict):
        raise Refused('dispatch request must be an object')
    subject = Subject(**{name: incoming[name] for name in Subject.__dataclass_fields__})
    if native.get('GITHUB_REPOSITORY') != subject.repository or native.get('GITHUB_SHA') != subject.control:
        raise Refused('native repository/control differs from request')
    run_id, attempt = int(native['GITHUB_RUN_ID']), int(native['GITHUB_RUN_ATTEMPT'])
    if not positive(run_id) or not positive(attempt):
        raise Refused('invalid native run identity')
    result = Adapter(api, subject).elect(run_id, attempt)
    plan = result['subject']
    if incoming != plan:
        raise Refused('request identity differs from native acquired subject')
    manifest = {key: plan[key] for key in ('repository', 'repository_id', 'pr', 'head', 'tree', 'base',
                'control', 'portfolio_sha256', 'policy_sha256', 'request_key', 'workflow_id', 'workflow_path')}
    manifest.update(schema_version=1, run_id=run_id, run_attempt=attempt,
                    result='not_proven', selected=0, executed=0, failed=0, check_run_id=None,
                    canonical_run_id=result['canonical_run_id'], rehearsal_elected=result['rehearsal_elected'])
    return manifest


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('operation', choices=['explain', 'request', 'recover', 'inspect', 'rehearse'])
    p.add_argument('--repository', required=True)
    for name in ('repository-id', 'pr', 'app-id'):
        p.add_argument('--' + name, required=True, type=int)
    for name in ('expected-head', 'expected-base', 'expected-control', 'portfolio-sha256', 'policy-sha256', 'check-name'):
        p.add_argument('--' + name, required=True)
    p.add_argument('--run-id', type=int)
    p.add_argument('--run-attempt', type=int)
    return p


def main(argv=None) -> int:
    arguments = sys.argv[1:] if argv is None else argv
    if arguments == ['rehearse-event']:
        try:
            event = decode(Path(os.environ['GITHUB_EVENT_PATH']).read_bytes())
            manifest = rehearse_event(GitHub(), event, dict(os.environ))
            destination = Path('rehearsal-result/qualification-result.json')
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(json.dumps(manifest, sort_keys=True) + '\n', encoding='utf-8')
            print(json.dumps({'status': 'NOT_PROVEN', 'qualification_result': 'NOT_PROVEN',
                              'reason': 'metadata rehearsal only', 'run_id': manifest['run_id'],
                              'run_attempt': manifest['run_attempt']}))
            return 0  # metadata operation completed; not product qualification
        except (Refused, Uncertain, OSError, ValueError, KeyError, TypeError, AttributeError):
            print(json.dumps({'status': 'NOT_PROVEN', 'reason': 'rehearsal refused; no product evidence'}))
            return 1
    args = parser().parse_args(arguments)
    try:
        s = Subject(args.repository, args.repository_id, args.pr, args.expected_head, args.expected_base,
                    args.expected_control, args.portfolio_sha256, args.policy_sha256, args.app_id, args.check_name)
        adapter = Adapter(GitHub(), s)
        if args.operation == 'explain':
            result = adapter.explain()
        elif args.operation in {'request', 'recover'}:
            result = adapter.request(recover_only=args.operation == 'recover')
        elif not positive(args.run_id) or not positive(args.run_attempt):
            raise Refused('inspect/rehearse requires saved run id and attempt')
        elif args.operation == 'inspect':
            result = adapter.inspect(args.run_id, args.run_attempt)
        else:
            result = adapter.elect(args.run_id, args.run_attempt)
    except (Refused, Uncertain, OSError, ValueError, KeyError, TypeError, AttributeError, RuntimeError, zipfile.BadZipFile) as error:
        result = {'status': 'NOT_PROVEN', 'qualification_result': 'NOT_PROVEN',
                  'reason': str(error) if isinstance(error, (Refused, Uncertain)) else 'malformed or unavailable input'}
    print(json.dumps(result, sort_keys=True))
    return 0 if result['status'] in {'SUCCESS', 'PR_IN_FLIGHT', 'READY'} else 1


if __name__ == '__main__':
    raise SystemExit(main())
