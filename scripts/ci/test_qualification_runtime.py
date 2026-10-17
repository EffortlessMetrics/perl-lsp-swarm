"""Compose actual H/S owner functions with bounded native/semantic test ports.

The native importer and Rust semantic verifier are explicit synthetic boundaries
here; central tests separately execute its actual authenticated import code.
"""
from dataclasses import replace
import hashlib
import json
from pathlib import Path
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
