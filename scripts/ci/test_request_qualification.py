"""Real adapter execution with bounded fake GitHub and eligibility boundaries."""
import copy
import importlib.util
import io
import json
from pathlib import Path
import sys
import os
import subprocess
import tempfile
from unittest import mock
import unittest
import zipfile

SCRIPT = Path(__file__).with_name('request_qualification.py')
spec = importlib.util.spec_from_file_location('request_qualification', SCRIPT)
adapter = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = adapter
spec.loader.exec_module(adapter)
H, B, S, TREE = 'a' * 40, 'b' * 40, 'c' * 40, 'd' * 40
SUBJECT = adapter.Subject('owner/repo', 7, 3, H, B, S, 'e' * 64, 'f' * 64, 15368, 'Qualified proof')


def ready(plan):
    return adapter.Eligibility('READY', 'injected reviewed producer; no future CI requirement')


class GitHub:
    def __init__(self):
        self.subject = SUBJECT
        self.rows = []
        self.calls = []
        self.dispatch_error = False
        self.ack_without_run = False
        self.after_dispatch = None
        self.manifest_changes = {}
        self.artifact_changes = {}
        self.check_changes = {}
        self.complete = False
        self.mutate_run = None

    def plan(self):
        return adapter.Adapter(self, self.subject, ready).snapshot()

    def run(self, number=21):
        p = self.plan()
        return {'id': number, 'run_attempt': 1, 'event': 'workflow_dispatch',
                'repository': {'id': 7}, 'workflow_id': 9, 'path': adapter.WORKFLOW,
                'head_sha': S, 'display_title': adapter.PREFIX + p['request_key'],
                'status': 'completed' if self.complete else 'queued',
                'conclusion': 'success' if self.complete else None}

    def manifest(self):
        p = self.plan()
        data = {key: p[key] for key in ('repository', 'repository_id', 'pr', 'head', 'tree', 'base',
                    'control', 'portfolio_sha256', 'policy_sha256', 'request_key', 'workflow_id', 'workflow_path')}
        data.update(schema_version=1, run_id=21, run_attempt=1, result='success',
                    selected=4, executed=4, failed=0, check_run_id=45)
        data.update(self.manifest_changes)
        return data

    def get(self, path):
        self.calls.append(('GET', path))
        root = 'repos/owner/repo'
        if path == root:
            return {'id': 7, 'full_name': 'owner/repo', 'default_branch': 'main'}
        if path == root + '/pulls/3':
            return {'number': 3, 'state': 'open', 'head': {'sha': self.subject.head},
                    'base': {'sha': self.subject.base, 'repo': {'id': 7}}}
        if path == root + '/git/ref/heads/main':
            return {'object': {'sha': self.subject.control}}
        if path.startswith(root + '/git/commits/'):
            return {'sha': path.rsplit('/', 1)[1], 'tree': {'sha': TREE}}
        if path == root + '/actions/workflows/qualification-rehearsal.yml':
            return {'id': 9, 'path': adapter.WORKFLOW, 'state': 'active'}
        if '/actions/workflows/9/runs?' in path:
            return {'total_count': len(self.rows), 'workflow_runs': copy.deepcopy(self.rows)}
        if path == root + '/actions/runs/21/artifacts?per_page=100':
            p = self.plan()
            row = {'id': 55, 'expired': False, 'name': 'qualification-result-' + p['request_key'] + '-1',
                   'workflow_run': {'id': 21, 'repository_id': 7, 'head_sha': S}}
            row.update(self.artifact_changes)
            return {'total_count': 1, 'artifacts': [row]}
        if path.startswith(root + '/actions/runs/'):
            run_id = int(path.rsplit('/', 1)[1])
            r = copy.deepcopy(next(row for row in self.rows if row['id'] == run_id))
            if self.mutate_run:
                self.mutate_run(r)
            return r
        if path == root + '/check-runs/45':
            p = self.plan()
            row = {'id': 45, 'head_sha': H, 'app': {'id': 15368}, 'name': SUBJECT.check_name,
                   'external_id': p['request_key'] + ':21:1',
                   'details_url': 'https://github.com/owner/repo/actions/runs/21/attempts/1',
                   'status': 'completed', 'conclusion': 'success'}
            row.update(self.check_changes)
            return row
        raise AssertionError('unexpected API operation: ' + path)

    def dispatch(self, path, body):
        self.calls.append(('POST', path, body))
        if not self.ack_without_run:
            self.rows.append(self.run())
        if self.after_dispatch:
            self.after_dispatch()
        if self.dispatch_error:
            raise adapter.Uncertain('POST response lost')

    def get_bytes(self, path):
        self.calls.append(('BYTES', path))
        assert path == 'repos/owner/repo/actions/artifacts/55/zip'
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as archive:
            archive.writestr('qualification-result.json', json.dumps(self.manifest()))
        return output.getvalue()


class AdapterTests(unittest.TestCase):
    def setUp(self):
        self.api = GitHub()
        self.adapter = adapter.Adapter(self.api, SUBJECT, ready)

    def complete(self):
        self.api.complete = True
        self.api.rows = [self.api.run()]

    def test_explain_is_read_only_and_does_not_require_future_ci(self):
        result = self.adapter.explain()
        self.assertEqual(result['status'], 'READY')
        self.assertEqual(result['qualification_result'], 'NOT_PROVEN')
        self.assertTrue(all(row[0] == 'GET' for row in self.api.calls))
        self.assertFalse(any('check-runs' in row[1] for row in self.api.calls))

    def test_production_eligibility_refuses_without_any_dispatch(self):
        result = adapter.Adapter(self.api, SUBJECT).request()
        self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertIn('#10057', result['reason'])
        self.assertFalse(any(row[0] == 'POST' for row in self.api.calls))

    def test_json_or_pass_string_cannot_assert_eligibility(self):
        for value in (True, 'READY', {'classification': 'READY'}):
            with self.subTest(value=value), self.assertRaises(adapter.Refused):
                adapter.Adapter(self.api, SUBJECT, lambda _: value).request()

    def test_explicit_request_reads_back_exact_run_and_attempt(self):
        result = self.adapter.request()
        self.assertEqual((result['status'], result['run_id'], result['run_attempt']), ('PR_IN_FLIGHT', 21, 1))
        posts = [r for r in self.api.calls if r[0] == 'POST']
        self.assertEqual(len(posts), 1)
        self.assertTrue(posts[0][1].endswith('/workflows/9/dispatches'))
        self.assertEqual(json.loads(posts[0][2]['inputs']['request'])['head'], H)
        self.assertEqual(result['qualification_result'], 'NOT_PROVEN')

    def test_duplicate_request_reuses_existing_transaction(self):
        first = self.adapter.request()
        second = self.adapter.request()
        self.assertEqual(first, second)
        self.assertEqual(sum(c[0] == 'POST' for c in self.api.calls), 1)

    def test_dispatch_ack_without_readback_is_not_in_flight(self):
        self.api.ack_without_run = True
        self.assertEqual(self.adapter.request()['status'], 'NOT_PROVEN')

    def test_lost_response_recovers_without_a_second_dispatch(self):
        self.api.dispatch_error = True
        result = self.adapter.request()
        self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertTrue(result['dispatch_may_have_occurred'])
        self.api.dispatch_error = False
        self.assertEqual(self.adapter.request(recover_only=True)['run_id'], 21)
        self.assertEqual(sum(c[0] == 'POST' for c in self.api.calls), 1)

    def test_recover_without_run_has_no_effect(self):
        with self.assertRaises(adapter.Uncertain):
            self.adapter.request(recover_only=True)
        self.assertFalse(any(c[0] == 'POST' for c in self.api.calls))

    def test_h2_before_or_after_dispatch_never_inherits_request(self):
        from dataclasses import replace
        self.api.subject = replace(SUBJECT, head='1' * 40)
        with self.assertRaises(adapter.Refused):
            self.adapter.request()
        self.assertFalse(any(c[0] == 'POST' for c in self.api.calls))
        self.api.subject = SUBJECT
        self.api.after_dispatch = lambda: setattr(self.api, 'subject', replace(SUBJECT, head='1' * 40))
        with self.assertRaises(adapter.Refused):
            self.adapter.request()

    def test_control_base_and_tree_bind_request_key(self):
        from dataclasses import replace
        original = self.adapter.snapshot()['request_key']
        for field, value in (('base', '2' * 40), ('control', '3' * 40), ('portfolio_sha256', '4' * 64),
                             ('policy_sha256', '5' * 64)):
            changed = replace(SUBJECT, **{field: value})
            self.api.subject = changed
            self.assertNotEqual(adapter.Adapter(self.api, changed, ready).snapshot()['request_key'], original)

    def test_wrong_workflow_control_event_or_repository_is_not_reused(self):
        good = self.api.run()
        for field, value in (('workflow_id', 10), ('head_sha', H), ('event', 'pull_request'),
                             ('repository', {'id': 8}), ('path', '.github/workflows/other.yml')):
            with self.subTest(field=field):
                row = dict(good, **{field: value})
                self.assertFalse(self.adapter.matches(row, self.adapter.snapshot()))

    def test_attempt_change_during_readback_refuses(self):
        self.api.rows = [self.api.run()]
        self.api.mutate_run = lambda row: row.update(run_attempt=2)
        with self.assertRaises(adapter.Refused):
            self.adapter.request()

    def test_duplicate_control_runs_elect_one_metadata_owner(self):
        self.api.rows = [self.api.run(22), self.api.run(21)]
        self.assertTrue(self.adapter.elect(21, 1)['rehearsal_elected'])
        self.assertFalse(self.adapter.elect(22, 1)['rehearsal_elected'])
        self.assertEqual(self.adapter.request()['run_id'], 21)
        self.assertFalse(any(c[0] == 'POST' for c in self.api.calls))

    def test_successful_exact_authenticated_result_is_evidence_not_merge(self):
        self.complete()
        result = self.adapter.inspect(21, 1)
        self.assertEqual(result['status'], 'SUCCESS')
        self.assertEqual(result['check_run_id'], 45)
        self.assertIn('separate final merge judgment', result['reason'])
        self.assertTrue(all(c[0] in {'GET', 'BYTES'} for c in self.api.calls))
        self.assertFalse(any('/merge' in c[1] or '/statuses' in c[1] for c in self.api.calls))

    def test_new_adverse_review_blocks_even_same_head_after_valid_evidence(self):
        self.complete()
        current = lambda _: adapter.Eligibility('NOT_READY', 'new material finding')
        result = adapter.Adapter(self.api, SUBJECT, current).inspect(21, 1)
        self.assertEqual(result['status'], 'NOT_READY')
        self.assertEqual(result['qualification_result'], 'SUCCESS')

    def test_wrong_result_run_attempt_head_base_control_and_portfolio_refuse(self):
        self.complete()
        for field, value in (('run_id', 22), ('run_attempt', 2), ('head', S), ('base', H), ('control', H),
                             ('tree', B), ('portfolio_sha256', '0' * 64), ('repository_id', 8)):
            with self.subTest(field=field), self.assertRaises(adapter.Refused):
                self.api.manifest_changes = {field: value}
                self.adapter.inspect(21, 1)

    def test_empty_partial_failed_or_bool_execution_never_qualifies(self):
        self.complete()
        for changes in ({'selected': 0, 'executed': 0}, {'executed': 3}, {'failed': 1},
                        {'selected': True, 'executed': True}, {'failed': False}):
            with self.subTest(changes=changes), self.assertRaises(adapter.Refused):
                self.api.manifest_changes = changes
                self.adapter.inspect(21, 1)

    def test_wrong_check_app_head_run_or_attempt_refuse(self):
        self.complete()
        for field, value in (('app', {'id': 1}), ('head_sha', S), ('name', 'Other'),
                             ('external_id', 'foreign'), ('conclusion', 'failure'),
                             ('details_url', 'https://github.com/owner/repo/actions/runs/21/attempts/2')):
            with self.subTest(field=field), self.assertRaises(adapter.Refused):
                self.api.check_changes = {field: value}
                self.adapter.inspect(21, 1)

    def test_foreign_or_expired_artifact_refuses(self):
        self.complete()
        for changes in ({'expired': True}, {'workflow_run': {'id': 22, 'repository_id': 7, 'head_sha': S}}):
            with self.subTest(changes=changes), self.assertRaises(adapter.Refused):
                self.api.artifact_changes = changes
                self.adapter.inspect(21, 1)

    def test_terminal_duplicates_are_not_reported_as_pending(self):
        for conclusion in ('success', 'failure', 'cancelled', 'timed_out'):
            with self.subTest(conclusion=conclusion):
                self.complete()
                self.api.rows[0]['conclusion'] = conclusion
                result = self.adapter.request()
                self.assertEqual(result['status'], 'NOT_PROVEN')
                self.assertEqual(result['control_conclusion'], conclusion)
                self.assertEqual(result['next_action'], 'inspect exact saved run/attempt; no automatic retry')
                self.assertFalse(any(c[0] == 'POST' for c in self.api.calls))

    def test_head_movement_during_final_eligibility_cannot_report_success(self):
        from dataclasses import replace
        self.complete()
        def moving(plan):
            self.api.subject = replace(SUBJECT, head='1' * 40)
            return ready(plan)
        with self.assertRaises(adapter.Refused):
            adapter.Adapter(self.api, SUBJECT, moving).inspect(21, 1)

    def test_metadata_rehearsal_cannot_be_product_success(self):
        self.complete()
        self.api.manifest_changes = {'result': 'not_proven', 'selected': 0, 'executed': 0}
        result = self.adapter.inspect(21, 1)
        self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertEqual(result['qualification_result'], 'NOT_PROVEN')

    def test_unknown_run_status_is_refused(self):
        self.api.rows = [self.api.run()]
        self.api.rows[0]['status'] = 'invented'
        with self.assertRaises(adapter.Refused):
            self.adapter.request()

    def test_authenticated_product_failure_is_preserved(self):
        self.complete()
        self.api.rows[0]['conclusion'] = 'failure'
        self.api.manifest_changes = {'result': 'product_failure', 'selected': 4, 'executed': 3, 'failed': 1}
        self.api.check_changes = {'conclusion': 'failure'}
        result = self.adapter.inspect(21, 1)
        self.assertEqual(result['status'], 'PRODUCT_OR_TEST_FAILURE')
        self.assertEqual(result['qualification_result'], 'FAILURE')

    def test_cancelled_control_is_not_a_product_failure(self):
        self.complete()
        self.api.rows[0]['conclusion'] = 'cancelled'
        result = self.adapter.inspect(21, 1)
        self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertEqual(result['control_conclusion'], 'cancelled')
        self.assertFalse(any(c[0] == 'BYTES' for c in self.api.calls))

    def test_rehearsal_native_identity_and_result_round_trip(self):
        self.complete()
        plan = self.api.plan()
        native = {'GITHUB_EVENT_NAME': 'workflow_dispatch', 'GITHUB_REPOSITORY': 'owner/repo',
                  'GITHUB_SHA': S, 'GITHUB_RUN_ID': '21', 'GITHUB_RUN_ATTEMPT': '1'}
        manifest = adapter.rehearse_event(self.api, {'inputs': {'request': json.dumps(plan)}}, native)
        self.assertEqual((manifest['run_id'], manifest['run_attempt'], manifest['result']), (21, 1, 'not_proven'))
        self.api.manifest_changes = manifest
        self.assertEqual(self.adapter.inspect(21, 1)['qualification_result'], 'NOT_PROVEN')
        for field, value in (('GITHUB_SHA', H), ('GITHUB_RUN_ID', '99'), ('GITHUB_RUN_ATTEMPT', '2'),
                             ('GITHUB_EVENT_NAME', 'pull_request')):
            with self.subTest(field=field), self.assertRaises(adapter.Refused):
                adapter.rehearse_event(self.api, {'inputs': {'request': json.dumps(plan)}}, dict(native, **{field: value}))
        with self.assertRaises(adapter.Refused):
            adapter.rehearse_event(self.api, {'inputs': {'request': json.dumps(dict(plan, run_id=99))}}, native)


class TransportAndEntryTests(unittest.TestCase):
    def test_native_binary_transport_and_literal_request_data(self):
        github = adapter.GitHub()
        completed = subprocess.CompletedProcess([], 0, stdout=b'\xffZIP', stderr=b'')
        with mock.patch.object(adapter.subprocess, 'run', return_value=completed) as run:
            self.assertEqual(github.get_bytes('repos/owner/repo/actions/artifacts/55/zip'), b'\xffZIP')
            self.assertNotIn('text', run.call_args.kwargs)
        data = {'request': '`echo secret`; $(exit 1)'}
        with mock.patch.object(adapter.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'', b'')) as run:
            github.dispatch('repos/owner/repo/actions/workflows/9/dispatches', data)
            self.assertEqual(json.loads(run.call_args.kwargs['input']), data)
            self.assertNotIn('shell', run.call_args.kwargs)
        for path in ('repos/owner/repo/pulls/3/merge', 'repos/owner/repo/check-runs',
                     'repos/owner/repo/actions/runs/21/rerun'):
            with self.subTest(path=path), self.assertRaises(adapter.Refused):
                github.dispatch(path, {})

    def test_transport_error_is_bounded_private_and_never_retried(self):
        result = subprocess.CompletedProcess([], 1, b'', b'token=private-secret')
        with mock.patch.object(adapter.subprocess, 'run', return_value=result) as run:
            with self.assertRaises(adapter.Uncertain) as error:
                adapter.GitHub().dispatch('repos/owner/repo/actions/workflows/9/dispatches', {})
            self.assertEqual(run.call_count, 1)
            self.assertNotIn('private-secret', str(error.exception))
        with mock.patch.object(adapter.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'[]', b'')):
            with self.assertRaises(adapter.Refused):
                adapter.GitHub().get('repos/owner/repo')

    def test_real_cli_refuses_production_request_despite_environment_pass_strings(self):
        api = GitHub()
        paths = ['repos/owner/repo', 'repos/owner/repo/pulls/3', 'repos/owner/repo/git/ref/heads/main',
                 'repos/owner/repo/git/commits/' + H,
                 'repos/owner/repo/actions/workflows/qualification-rehearsal.yml']
        payloads = {path: api.get(path) for path in paths}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = root / 'responses.json'
            fixture.write_text(json.dumps(payloads))
            sentinel = root / 'mutation'
            gh = root / 'gh'
            gh.write_text('#!' + sys.executable + '\nimport json,os,sys\n'
                          'if sys.argv[3] != "GET":\n'
                          ' open(os.environ["MUTATION"],"w").write("unexpected")\n'
                          ' sys.exit(99)\n'
                          'print(json.dumps(json.load(open(os.environ["RESPONSES"]))[sys.argv[4]]))\n')
            gh.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'],
                       RESPONSES=str(fixture), MUTATION=str(sentinel), CANDIDATE_READY='true')
            args = [sys.executable, '-B', str(SCRIPT), 'request', '--repository', 'owner/repo',
                    '--repository-id', '7', '--pr', '3', '--app-id', '15368', '--check-name', 'Qualified proof',
                    '--expected-head', H, '--expected-base', B, '--expected-control', S,
                    '--portfolio-sha256', 'e' * 64, '--policy-sha256', 'f' * 64]
            result = subprocess.run(args, capture_output=True, text=True, env=env, timeout=10)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn('#10057', json.loads(result.stdout)['reason'])
            self.assertFalse(sentinel.exists())

    def test_native_cli_rehearsal_and_existing_policy_diagnostics_never_qualify(self):
        api = GitHub()
        api.rows = [api.run()]
        plan = api.plan()
        root_repo = SCRIPT.parents[2]
        paths = ['repos/owner/repo', 'repos/owner/repo/pulls/3',
                 'repos/owner/repo/git/ref/heads/main', 'repos/owner/repo/git/commits/' + H,
                 'repos/owner/repo/actions/workflows/qualification-rehearsal.yml',
                 'repos/owner/repo/actions/workflows/9/runs?event=workflow_dispatch&head_sha=' + S + '&per_page=100',
                 'repos/owner/repo/actions/runs/21']
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            responses = root / 'responses.json'
            responses.write_text(json.dumps({path: api.get(path) for path in paths}))
            event = root / 'event.json'
            event.write_text(json.dumps({'inputs': {'request': json.dumps(plan)}}))
            gh = root / 'gh'
            gh.write_text('#!' + sys.executable + '\nimport json,os,sys\n'
                          'assert sys.argv[3] == "GET", "rehearsal attempted mutation"\n'
                          'print(json.dumps(json.load(open(os.environ["RESPONSES"]))[sys.argv[4]]))\n')
            gh.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'],
                       RESPONSES=str(responses), GITHUB_EVENT_PATH=str(event),
                       GITHUB_EVENT_NAME='workflow_dispatch', GITHUB_REPOSITORY='owner/repo',
                       GITHUB_SHA=S, GITHUB_RUN_ID='21', GITHUB_RUN_ATTEMPT='1')
            result = subprocess.run([sys.executable, '-B', str(SCRIPT), 'rehearse-event'],
                                    cwd=root, env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(json.loads(result.stdout)['qualification_result'], 'NOT_PROVEN')
            manifest_path = root / 'rehearsal-result/qualification-result.json'
            before = manifest_path.read_bytes()
            manifest = json.loads(before)
            self.assertEqual((manifest['run_id'], manifest['run_attempt']), (21, 1))
            self.assertEqual((manifest['selected'], manifest['executed'], manifest['failed']), (0, 0, 0))
            receipt = root / 'gate-enforcement-contract.json'
            check = subprocess.run([sys.executable, '-B', str(SCRIPT.with_name('validate_gate_enforcement_contract.py')),
                                    '--root', str(root_repo), '--receipt', str(receipt)],
                                   capture_output=True, text=True, timeout=20)
            self.assertEqual(check.returncode, 0, check.stdout + check.stderr)
            report = json.loads(receipt.read_bytes())
            self.assertEqual(report['status'], 'SUCCESS')
            self.assertEqual(report['live_enforcement_status'], 'NOT_PROVEN')
            self.assertEqual(manifest_path.read_bytes(), before)

    def test_rehearsal_workflow_has_only_explicit_dispatch_entry(self):
        contract_path = SCRIPT.with_name('validate_gate_enforcement_contract.py')
        spec = importlib.util.spec_from_file_location('qualification_contract', contract_path)
        contract = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(contract)
        root = SCRIPT.parents[2]
        workflow = (root / adapter.WORKFLOW).read_text()
        events, _ = contract._parse_events(workflow.splitlines())
        self.assertEqual(events, frozenset({'workflow_dispatch'}))
        self.assertIn('python3 -B scripts/ci/request_qualification.py rehearse-event', workflow)
        self.assertIn('cancel-in-progress: false', workflow)
        self.assertNotIn('checks: write', workflow)
        self.assertNotIn('contents: write', workflow)


if __name__ == '__main__':
    unittest.main()
