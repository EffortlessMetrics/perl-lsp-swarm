"""Compose actual H/S owner functions with bounded native/semantic test ports.

The native importer and Rust semantic verifier are explicit synthetic boundaries
here; central tests separately execute its actual authenticated import code.
"""
from dataclasses import replace
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest import mock

from scripts.ci import candidate_readiness as admission
from scripts.ci import qualification_runtime as runtime
from scripts.ci.request_qualification import Adapter, Subject
from scripts.ci import test_rust_small_subject_bridge as bridge_fixture


class NativeAPI:
    def __init__(self, bridge):
        self.bridge, self.calls = bridge, []
        self.subject = Subject(bridge.plan['repository'], 7, 3, bridge.plan['head'],
            bridge.plan['base'], bridge.plan['control'], 'f' * 64, '1' * 64,
            15368, 'Perl LSP Rust Small Result')
        self.tree = bridge.plan['tree']
        self.row = None
        self.extra_rows = []

    def get(self, path):
        self.calls.append(('GET', path))
        base = 'repos/' + self.subject.repository
        if path == base:
            return dict(id=7, full_name=self.subject.repository, default_branch='main')
        if path == base + '/pulls/3':
            return dict(number=3, state='open', head=dict(sha=self.subject.head),
                        base=dict(sha=self.subject.base, repo=dict(id=7)))
        if path == base + '/git/ref/heads/main':
            return dict(object=dict(sha=self.subject.control))
        if path == base + '/git/commits/' + self.subject.head:
            return dict(sha=self.subject.head, tree=dict(sha=self.tree))
        if path == base + '/actions/workflows/qualification-rehearsal.yml':
            return dict(id=9, path='.github/workflows/qualification-rehearsal.yml', state='active')
        if path == base + '/actions/runs/123':
            return dict(self.row)
        if path.startswith(base + '/actions/workflows/9/runs?'):
            rows = [dict(self.row), *self.extra_rows]
            return dict(total_count=len(rows), workflow_runs=rows)
        raise AssertionError('unexpected native read ' + path)


class RuntimeCompositionTests(unittest.TestCase):
    def setUp(self):
        self.bridge = bridge_fixture.SubjectBridgeTests()
        self.bridge.setUp()
        self.addCleanup(self.bridge.doCleanups)
        self.api = NativeAPI(self.bridge)
        self.adapter = Adapter(self.api, self.api.subject)
        self.plan = self.adapter.snapshot()
        self.bridge.plan = self.plan
        dispatcher_ref = (f'{self.plan["repository"]}/{self.plan["workflow_path"]}'
                          f'@refs/heads/{self.plan["control_branch"]}')
        self.bridge.env.update(GITHUB_WORKFLOW='Qualification Adapter Rehearsal',
            GITHUB_WORKFLOW_REF=dispatcher_ref, GITHUB_REF='refs/heads/' + self.plan['control_branch'])
        self.bridge.context.update(workflow=self.bridge.env['GITHUB_WORKFLOW'],
            workflow_ref=dispatcher_ref, event='workflow_dispatch', ref=self.bridge.env['GITHUB_REF'])
        self.bridge.context['qualification_subject'] = {key: self.plan[key]
            for key in ('head', 'tree', 'base', 'control', 'request_key')}
        self.bridge.context['producer_check_run_id'] = 201
        self.bridge.write_context()
        self.api.row = dict(id=123, run_attempt=2, event='workflow_dispatch',
            repository=dict(id=7), workflow_id=9, path=self.plan['workflow_path'],
            head_sha=self.plan['control'], display_title='qualification:' + self.plan['request_key'],
            status='in_progress', conclusion=None)
        self.selected = admission.Selection(self.plan['request_key'], self.plan['portfolio_sha256'],
            self.plan['policy_sha256'], ('cargo-current',), ('Rust Small', 'RIPR'), 'existing selector')
        self.imports = []
        self.owners = runtime.RustSmallOwners(self.adapter, self.bridge.env,
            selection_owner=lambda plan: self.selected, native_import=self.native_import,
            canonical_owner=self.canonical_owner, applicability_owner=lambda plan: ())
        self.owners.export()
        self.packet = self.bridge.fixture.root
        (self.packet / 'em-ci-job-context.json').write_text(json.dumps(self.bridge.context))

    def native_import(self, *, acquire_plan):
        self.assertEqual(acquire_plan(), self.plan)
        self.imports.append('native authenticated import boundary')
        return self.bridge.fixture.root

    def canonical_owner(self, receipt, plan, selected):
        self.assertEqual(receipt, self.bridge.fixture.root / 'rust-small-proof.json')
        digest = hashlib.sha256(receipt.read_bytes()).hexdigest()
        reference = f'https://github.com/{plan["repository"]}/actions/runs/123/attempts/2#job-201'
        return admission.CommittedProof(plan['request_key'], 'verified', selected.selected, (),
                                        digest, reference)

    def fresh_producer_packet(self):
        for name in ('rust-small-proof.json', 'perl-rust-small-export.json', 'em-ci-job-context.json'):
            (self.packet / name).unlink(missing_ok=True)

    def test_execute_bound_producer_then_actual_export_preserves_native_control(self):
        self.fresh_producer_packet()
        self.owners.environment.update(GH_TOKEN='metadata-only', GIT_DIR='/wrong')
        calls = []
        def producer(environment, *, acquire_plan):
            calls.append(acquire_plan())
            self.assertNotIn('GH_TOKEN', environment)
            self.assertNotIn('GIT_DIR', environment)
            self.assertEqual(environment['GITHUB_SHA'], self.plan['control'])
            self.assertNotEqual(environment['GITHUB_SHA'], self.plan['head'])
            # Real child invocation, synthetic semantic producer. Its fixture
            # receipt already exists at the exact H checkout.
            return subprocess.run([sys.executable, '-c', 'raise SystemExit(0)'],
                                  env=environment, check=False).returncode
        status, manifest = self.owners.execute_proof(producer)
        self.assertEqual((status, manifest['result']), (0, 'success'))
        self.assertEqual(manifest['sha'], self.plan['control'])
        self.assertEqual(calls, [self.plan])
        self.assertEqual(json.loads((self.packet / 'rust-small-proof.json').read_bytes())
                         ['subject']['git_sha'], self.plan['head'])
        self.assertTrue(all(method == 'GET' for method, _ in self.api.calls))

    def test_producer_failure_stays_failure_and_signals_cannot_export(self):
        self.fresh_producer_packet()
        self.bridge.receipt['result'] = 'product_failure'
        self.bridge.write_receipt()
        status, manifest = self.owners.execute_proof(lambda env, **kw: 7)
        self.assertEqual((status, manifest['result']), (7, 'product_failure'))
        self.fresh_producer_packet()
        with self.assertRaises(ValueError):
            self.owners.execute_proof(lambda env, **kw: -9)
        self.assertFalse((self.packet / 'rust-small-proof.json').exists())

    def test_producer_wrong_checkout_context_or_request_never_exports(self):
        self.fresh_producer_packet()
        source = self.bridge.workspace / 'source.txt'
        source.write_text('dirty candidate')
        called = []
        with self.assertRaises(ValueError):
            self.owners.execute_proof(lambda *args, **kw: called.append(1))
        self.assertEqual(called, [])
        self.bridge.git('checkout', '--', 'source.txt')
        def change_context(environment, **kw):
            self.bridge.fixture.context_path.write_text('{}')
            return 0
        with self.assertRaises(ValueError):
            self.owners.execute_proof(change_context)
        self.assertFalse((self.packet / 'rust-small-proof.json').exists())
        self.bridge.write_context()
        def move_request(environment, **kw):
            self.api.subject = replace(self.api.subject, base='0' * 40)
            return 0
        with self.assertRaises(ValueError):
            self.owners.execute_proof(move_request)
        self.assertFalse((self.packet / 'rust-small-proof.json').exists())

    def test_zero_process_status_cannot_contradict_product_failure(self):
        self.fresh_producer_packet()
        self.bridge.receipt['result'] = 'product_failure'
        self.bridge.write_receipt()
        with self.assertRaises(ValueError):
            self.owners.execute_proof(lambda environment, **kw: 0)
        self.assertEqual(list(self.packet.iterdir()), [])
        self.bridge.receipt['result'] = 'success'
        self.bridge.write_receipt()
        with self.assertRaises(ValueError):
            self.owners.execute_proof(lambda environment, **kw: 7)
        self.assertEqual(list(self.packet.iterdir()), [])

    def test_import_owner_callback_feeds_existing_admission_join(self):
        authorities = admission.ExistingControlAuthorities(Path('/unused'),
            selection_owner=lambda plan: self.selected, committed_owner=self.owners.committed_proof,
            applicability_owner=lambda plan: ())
        review = lambda plan: admission.Review(plan['request_key'], 'REVIEW_CURRENT', 'canonical', '')
        native = lambda plan: admission.Review(plan['request_key'], 'CONVERGED', 'canonical', '')
        with mock.patch.object(authorities, 'semantic_review', review), \
             mock.patch.object(authorities, 'native_review', native):
            result = admission.evaluate(self.plan, authorities)
        self.assertEqual(result['classification'], 'READY')
        self.assertFalse(result['execution_authorized'])
        self.assertFalse(result['merge_authorized'])
        self.assertTrue(self.imports)

    def test_prepare_h_check_keeps_s_native_custody_and_performs_no_write(self):
        result = self.owners.prepare_publication(123, 2)
        self.assertEqual(result['payload']['head_sha'], self.plan['head'])
        self.assertNotEqual(result['payload']['head_sha'], self.plan['control'])
        self.assertEqual(result['payload']['external_id'], self.plan['request_key'] + ':123:2')
        self.assertFalse(result['published'])
        self.assertEqual(result['effects'], [])
        self.assertTrue(all(method == 'GET' for method, _ in self.api.calls))
        mapped = self.owners.result_environment()
        self.assertEqual(mapped['CANDIDATE_SHA'], self.plan['head'])
        self.assertEqual(mapped['EFFECTIVE_WORKFLOW_TREE'], self.plan['control'])

    def test_selection_policy_and_portfolio_movement_refuse_before_import(self):
        for changed in (replace(self.selected, portfolio_sha256='0' * 64),
                        replace(self.selected, policy_sha256='0' * 64),
                        replace(self.selected, selected=()),
                        replace(self.selected, selected=('cargo-current', 'cargo-current'))):
            self.owners.selection_owner = lambda plan: changed
            with self.subTest(selection=changed), self.assertRaises(ValueError):
                self.owners.prepare_publication(123, 2)
        self.assertEqual(self.imports, [])

    def test_foreign_native_dispatcher_refuses_before_current_run_import(self):
        self.owners.environment['GITHUB_WORKFLOW_REF'] = (
            self.plan['repository'] + '/.github/workflows/ci.yml@refs/heads/' + self.plan['control_branch'])
        with mock.patch.object(self.owners, 'native_import', wraps=self.native_import) as imported:
            with self.assertRaisesRegex(ValueError, 'native dispatcher workflow'):
                self.owners.prepare_publication(123, 2)
            imported.assert_not_called()

    def test_imported_workflow_event_and_ref_must_match_native_dispatcher(self):
        context_path = self.packet / 'em-ci-job-context.json'
        for field, value in (('workflow_ref', self.plan['repository'] + '/.github/workflows/ci.yml@refs/heads/main'),
                             ('workflow', 'CI definition'), ('event', 'pull_request'), ('ref', 'refs/heads/other')):
            with self.subTest(field=field):
                context_path.write_text(json.dumps({**self.bridge.context, field: value}))
                with mock.patch.object(self.owners, 'canonical_owner', wraps=self.canonical_owner) as canonical:
                    with self.assertRaisesRegex(ValueError, 'native dispatcher workflow/event/ref'):
                        self.owners.prepare_publication(123, 2)
                    canonical.assert_not_called()
        context_path.write_text(json.dumps(self.bridge.context))

    def test_missing_capture_and_wrong_imported_origin_refuse(self):
        for change in ({'qualification_subject': {}}, {'sha': self.plan['head']},
                       {'run_attempt': '3'}, {'producer_check_run_id': None}):
            with self.subTest(change=change):
                (self.packet / 'em-ci-job-context.json').write_text(json.dumps({**self.bridge.context, **change}))
                with self.assertRaises(ValueError):
                    self.owners.prepare_publication(123, 2)
        (self.packet / 'em-ci-job-context.json').write_text(json.dumps(self.bridge.context))

    def test_json_exit_zero_wrong_commands_and_unauthenticated_proof_refuse(self):
        original = self.canonical_owner(self.packet / 'rust-small-proof.json', self.plan, self.selected)
        for proof in ({'result': 'success', 'exit_code': 0},
                      replace(original, executed=('other-command',)),
                      replace(original, canonical_receipt_sha256='0' * 64),
                      replace(original, producer_reference='candidate-authored'),
                      replace(original, executed=()), replace(original, failed=('cargo-current',))):
            self.owners.canonical_owner = lambda *args: proof
            with self.subTest(proof=proof), self.assertRaises(ValueError):
                self.owners.prepare_publication(123, 2)

    def test_receipt_context_and_request_movement_during_verification_refuse(self):
        receipt = self.packet / 'rust-small-proof.json'
        before = receipt.read_bytes()
        context = self.packet / 'em-ci-job-context.json'
        context_before = context.read_bytes()
        original = self.canonical_owner(receipt, self.plan, self.selected)
        for target in (receipt, context):
            def mutate(*args):
                target.write_bytes(target.read_bytes() + b' ')
                return original
            self.owners.canonical_owner = mutate
            with self.subTest(target=target), self.assertRaises(ValueError):
                self.owners.prepare_publication(123, 2)
            receipt.write_bytes(before)
            context.write_bytes(context_before)
        def move(*args):
            self.api.subject = replace(self.api.subject, head='0' * 40)
            return original
        self.owners.canonical_owner = move
        with self.assertRaises(ValueError):
            self.owners.prepare_publication(123, 2)

    def test_entry_uses_distinct_warm_owner_and_never_current_run_import(self):
        self.assertEqual(self.owners.entry_committed_proof(self.plan, self.selected).disposition, 'unavailable')
        self.assertEqual(self.imports, [])
        warm = self.canonical_owner(self.packet / 'rust-small-proof.json', self.plan, self.selected)
        self.owners.entry_committed_owner = lambda plan, selected: warm
        def production(plan, **ports):
            self.assertEqual(ports['committed_owner'](plan, self.selected), warm)
            return {'classification': 'READY'}
        with mock.patch.object(runtime.admission, 'production', side_effect=production):
            self.assertEqual(self.owners.eligibility(self.plan).classification, 'READY')
        self.assertEqual(self.imports, [])
        self.owners.entry_committed_owner = self.owners.committed_proof
        with self.assertRaisesRegex(ValueError, 'current-run'):
            self.owners.entry_committed_proof(self.plan, self.selected)

    def test_verified_observation_reused_by_completion_without_reimport(self):
        from scripts.ci import qualification_completion as completion
        verified = self.owners.verified_qualification()
        self.assertEqual(len(self.imports), 1)
        with mock.patch.object(self.owners, 'committed_proof', side_effect=AssertionError('reimported')):
            manifest = completion.build(self.adapter, verified)
        self.assertEqual(manifest['result'], 'not_proven')
        self.assertIsNone(manifest['check_run_id'])
        self.assertEqual((manifest['selected'], manifest['executed'], manifest['failed']), (1, 1, 0))
        self.assertEqual(len(self.imports), 1)

    def test_native_entry_joins_existing_election_and_owner_ports_before_capture(self):
        event = {'inputs': {'request': json.dumps(self.plan)}}
        captured = []
        def capture(environment, *, acquire_plan):
            captured.append(acquire_plan())
            return {'qualification_subject': {key: self.plan[key] for key in
                    ('head', 'tree', 'base', 'control', 'request_key')}}
        def production(plan, **ports):
            self.assertEqual(plan, self.plan)
            self.assertEqual(ports['selection_owner'](plan), self.selected)
            self.assertEqual(ports['committed_owner'](plan, self.selected).disposition, 'unavailable')
            return {'classification': 'NOT_PROVEN', 'reason': 'distinct warm proof unavailable'}
        with mock.patch.object(runtime.admission, 'production', side_effect=production):
            refused = self.owners.enter_native(event, capture)
        self.assertEqual(refused['status'], 'NOT_PROVEN')
        self.assertEqual(captured, [])
        with mock.patch.object(runtime.admission, 'production', return_value={'classification': 'READY'}):
            result = self.owners.enter_native(event, capture)
        self.assertEqual(result['status'], 'READY_FOR_ALLOCATION')
        self.assertEqual(captured, [self.plan])
        self.assertEqual(result['result'], 'not_proven')
        self.assertEqual((result['selected'], result['executed'], result['failed']), (0, 0, 0))
        self.assertIsNone(result['check_run_id'])
        self.assertFalse(result['execution_authorized'])
        self.assertFalse(result['merge_authorized'])
        self.assertEqual(self.imports, [])
        self.assertTrue(all(method == 'GET' for method, _ in self.api.calls))

    def test_native_duplicate_or_wrong_dispatch_never_reaches_owner_admission(self):
        event = {'inputs': {'request': json.dumps(self.plan)}}
        self.api.extra_rows = [dict(self.api.row, id=122)]
        with mock.patch.object(self.owners, 'eligibility', side_effect=AssertionError('duplicate admitted')):
            result = self.owners.enter_native(event, None)
        self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertEqual(result['canonical_run_id'], 122)
        self.api.extra_rows = []
        with mock.patch.object(self.owners, 'eligibility', side_effect=AssertionError('bad request admitted')):
            with self.assertRaises(ValueError):
                self.owners.enter_native({'inputs': {'request': json.dumps(dict(self.plan, tree='0' * 40))}}, None)

    def test_native_attempt_or_subject_change_during_admission_never_captures(self):
        event = {'inputs': {'request': json.dumps(self.plan)}}
        def moved(plan):
            self.api.row['run_attempt'] = 3
            return runtime.Eligibility('READY', 'fixture')
        with mock.patch.object(self.owners, 'eligibility', side_effect=moved):
            with self.assertRaises(ValueError):
                self.owners.enter_native(event, lambda *args, **kw: self.fail('stale capture'))
        self.api.row['run_attempt'] = 2
        def capture(environment, **kw):
            self.api.row['status'] = 'completed'
            self.api.row['conclusion'] = 'success'
            return {}
        with mock.patch.object(self.owners, 'eligibility', return_value=runtime.Eligibility('READY', 'fixture')):
            with self.assertRaisesRegex(ValueError, 'in-progress'):
                self.owners.enter_native(event, capture)

    def test_other_run_at_entry_readback_refuses_before_owner_admission(self):
        original = self.api.get
        reads = 0
        def get(path):
            nonlocal reads
            row = original(path)
            if path.endswith('/actions/runs/123'):
                reads += 1
                if reads == 2:  # After metadata election, before owner admission.
                    return dict(row, id=124)
            return row
        with mock.patch.object(self.api, 'get', side_effect=get), mock.patch.object(
                self.owners, 'eligibility', return_value=runtime.Eligibility('READY', 'fixture')) as eligibility:
            with self.assertRaises(ValueError):
                self.owners.enter_native({'inputs': {'request': json.dumps(self.plan)}}, lambda *a, **kw: {})
            eligibility.assert_not_called()

    def test_native_late_duplicate_during_capture_refuses_stale_admission(self):
        event = {'inputs': {'request': json.dumps(self.plan)}}
        def capture(environment, **kw):
            self.api.extra_rows = [dict(self.api.row, id=122)]
            return {'qualification_subject': self.plan}
        with mock.patch.object(self.owners, 'eligibility', return_value=runtime.Eligibility('READY', 'fixture')):
            with self.assertRaisesRegex(ValueError, 'election changed during context capture'):
                self.owners.enter_native(event, capture)

    def test_canonical_failure_remains_failure_and_wrong_attempt_refuses(self):
        original = self.canonical_owner(self.packet / 'rust-small-proof.json', self.plan, self.selected)
        self.owners.canonical_owner = lambda *args: replace(original, disposition='failure', failed=('cargo-current',))
        result = self.owners.prepare_publication(123, 2)
        self.assertEqual(result['payload']['conclusion'], 'failure')
        self.assertFalse(result['published'])
        with self.assertRaises(ValueError):
            self.owners.prepare_publication(123, 3)


if __name__ == '__main__':
    unittest.main()
