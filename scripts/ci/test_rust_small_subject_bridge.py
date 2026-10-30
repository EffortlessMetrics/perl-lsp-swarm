"""Candidate/control separation at the existing export and result-policy seams."""
import json
import os
import subprocess
import unittest
from unittest import mock

from scripts.ci import test_export_rust_small_receipt as fixtures
from scripts.ci.export_rust_small_receipt import export
from scripts.ci.rust_standard_result import policy_environment
from scripts.ci import rust_small_subject


class SubjectBridgeTests(unittest.TestCase):
    def setUp(self):
        fixture = fixtures.ExportTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        self.fixture = fixture
        self.env = dict(fixture.env)
        self.workspace = fixture.workspace
        self.git('init', '-q')
        self.git('config', 'user.name', 'Fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        (self.workspace / 'source.txt').write_text('base\n')
        self.git('add', 'source.txt')
        self.git('commit', '-qm', 'base')
        base = self.git('rev-parse', 'HEAD')
        (self.workspace / 'source.txt').write_text('candidate\n')
        self.git('commit', '-qam', 'candidate')
        self.plan = dict(repository=self.env['GITHUB_REPOSITORY'], head=self.git('rev-parse', 'HEAD'),
                         tree=self.git('rev-parse', 'HEAD^{tree}'), base=base,
                         control=self.env['GITHUB_SHA'], request_key='e' * 64)
        self.context = dict(fixture.context, qualification_subject={
            key: self.plan[key] for key in ('head', 'tree', 'base', 'control', 'request_key')})
        self.write_context()
        self.receipt = dict(fixture.receipt, subject=dict(git_sha=self.plan['head'], worktree_dirty=False))
        self.write_receipt()
        self.event = fixture.base / 'event.json'
        # Dispatch JSON cannot assert admission, including legacy admission flags.
        self.event.write_text(json.dumps({'inputs': {'request': json.dumps(self.plan)}}))
        self.env.update(GITHUB_EVENT_PATH=str(self.event), EM_CI_LANE='perl-rust-small',
                        EM_CI_SELECTED_PROOF_RESULT='success')
        self.evidence = dict(schema='em-ci-rust-result.v1', repository=self.plan['repository'],
                             sha=self.plan['head'], run_id='123', run_attempt='2', profile='standard',
                             result='success', proof_outcome='success', route='self_hosted',
                             infrastructure_failure='', proof_entered_at='2026-10-02T00:00:00Z')
        self.env['EM_CI_EVIDENCE'] = json.dumps(self.evidence)

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.workspace), *args], text=True).strip()

    def write_context(self):
        self.fixture.context_path.write_text(json.dumps(self.context))

    def write_receipt(self):
        self.fixture.source.write_text(json.dumps(self.receipt))

    def acquire(self):
        return dict(self.plan)

    def test_export_keeps_native_custody_and_exact_candidate_bytes(self):
        before = self.fixture.source.read_bytes()
        result = export(self.env, acquire_plan=self.acquire)
        self.assertEqual(result['sha'], self.plan['control'])
        self.assertEqual((result['run_id'], result['run_attempt']), ('123', '2'))
        self.assertEqual((self.fixture.root / 'rust-small-proof.json').read_bytes(), before)
        self.assertNotEqual(self.plan['head'], result['sha'])

    def test_result_policy_maps_candidate_proof_separately_from_control(self):
        export(self.env, acquire_plan=self.acquire)
        result = policy_environment(self.env, acquire_plan=self.acquire)
        self.assertEqual(result['EXPECTED_SHA'], self.plan['head'])
        self.assertEqual(result['CANDIDATE_SHA'], self.plan['head'])
        self.assertEqual(result['EFFECTIVE_WORKFLOW_TREE'], self.plan['control'])
        self.assertEqual(result['GITHUB_SHA'], self.plan['control'])

    def test_request_json_and_environment_do_not_activate_bridge(self):
        self.env.update(QUALIFICATION_READY='true', QUALIFICATION_HEAD=self.plan['head'])
        with self.assertRaises(ValueError):
            export(self.env)
        with self.assertRaises((ValueError, OSError)):
            policy_environment(self.env)

    def test_legacy_central_context_cannot_authorize_separate_candidate(self):
        del self.context['qualification_subject']
        self.write_context()
        with self.assertRaisesRegex(ValueError, 'central.*subject'):
            export(self.env, acquire_plan=self.acquire)
        self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_each_acquired_subject_field_must_match_central_capture(self):
        for field in ('head', 'tree', 'base', 'control', 'request_key'):
            with self.subTest(field=field):
                captured = dict(self.context, qualification_subject={
                    **self.context['qualification_subject'], field: 'f' * len(self.plan[field])})
                self.fixture.context_path.write_text(json.dumps(captured))
                with self.assertRaises(ValueError):
                    export(self.env, acquire_plan=self.acquire)
                self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_native_repository_run_attempt_and_control_cannot_be_relabelled(self):
        for field in ('repository', 'run_id', 'run_attempt', 'sha'):
            with self.subTest(field=field):
                self.fixture.context_path.write_text(json.dumps({**self.context, field: 'foreign'}))
                with self.assertRaises(ValueError):
                    export(self.env, acquire_plan=self.acquire)

    def test_wrong_candidate_checkout_cannot_export(self):
        self.git('checkout', '-q', self.plan['base'])
        with self.assertRaises(ValueError):
            export(self.env, acquire_plan=self.acquire)
        self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_wrong_candidate_tree_cannot_export(self):
        self.plan['tree'] = 'f' * 40
        self.context['qualification_subject']['tree'] = self.plan['tree']
        self.write_context()
        with self.assertRaises(ValueError):
            export(self.env, acquire_plan=self.acquire)
        self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_tracked_checkout_dirt_cannot_export(self):
        (self.workspace / 'source.txt').write_text('uncommitted\n')
        with self.assertRaises(ValueError):
            export(self.env, acquire_plan=self.acquire)
        self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_control_only_evidence_cannot_become_candidate_proof(self):
        export(self.env, acquire_plan=self.acquire)
        self.env['EM_CI_EVIDENCE'] = json.dumps({**self.evidence, 'sha': self.plan['control']})
        with self.assertRaises(ValueError):
            policy_environment(self.env, acquire_plan=self.acquire)

    def test_plan_data_is_not_a_native_acquirer(self):
        with self.assertRaises(ValueError):
            export(self.env, acquire_plan=self.plan)

    def test_fresh_acquisition_cannot_silently_change_during_export(self):
        calls = []
        def moving():
            calls.append(1)
            return dict(self.plan, base='f' * 40) if len(calls) > 1 else dict(self.plan)
        with self.assertRaises(ValueError):
            export(self.env, acquire_plan=moving)
        self.assertEqual(len(calls), 2)
        self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_capture_cannot_hide_duplicate_subject_fields(self):
        raw = json.dumps(self.context)
        self.fixture.context_path.write_text(raw[:-1] + ',"qualification_subject":' +
                                             json.dumps(self.context['qualification_subject']) + '}')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            export(self.env, acquire_plan=self.acquire)

    def test_git_inspection_cannot_receive_metadata_credentials_or_git_overrides(self):
        secrets = {name: 'metadata-secret' for name in (
            'GH_TOKEN', 'GITHUB_TOKEN', 'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN')}
        injected = {**secrets, 'GIT_DIR': '/unrelated/git', 'GIT_CONFIG_COUNT': '1'}
        real_run = subprocess.run
        inspected = []
        def observe(argv, **kwargs):
            inspected.append(kwargs['env'])
            self.assertTrue(all(name not in kwargs['env'] for name in injected))
            self.assertIn('core.fsmonitor=false', argv)
            return real_run(argv, **kwargs)
        def acquire():
            self.assertTrue(all(os.environ[name] == value for name, value in secrets.items()))
            return dict(self.plan)
        with mock.patch.dict(os.environ, injected), \
             mock.patch.object(rust_small_subject.subprocess, 'run', side_effect=observe):
            export(self.env, acquire_plan=acquire)
        self.assertGreaterEqual(len(inspected), 4)


if __name__ == '__main__':
    unittest.main()
