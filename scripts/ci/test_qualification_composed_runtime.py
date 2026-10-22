"""Uninstalled compose_native -> execute_native proof, with synthetic authorities.

Git checkout, readiness join, adapter validation, subprocess and export are real.
API, reviews, warm/current import, capture, allocator and tool semantics are
fixtures: this does NOT authenticate a prior Actions run or prove native acceptance.
The existing adapter fixture supplies the receipt vocabulary; no second catalogue.
"""
from contextlib import contextmanager
from dataclasses import replace
import hashlib
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest import mock

from scripts.ci import candidate_readiness as admission
from scripts.ci import qualification_native_factory as factory
from scripts.ci import test_qualification_native_adapters as fixtures
from scripts.ci.rust_small_canonical_owner import CanonicalOwner


class ComposedRuntimeTests(unittest.TestCase):
    def setUp(self):
        # Existing disposable Git fixtures must not inherit an invoking Git hook's
        # index/worktree. This patch is scoped and restores the caller environment.
        environment = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        patch = mock.patch.dict(os.environ, environment, clear=True)
        patch.start()
        self.addCleanup(patch.stop)
        self.native = fixtures.NativeOwnersTests()
        self.native.setUp()
        self.addCleanup(self.native.doCleanups)
        self.runtime = self.native.fixture
        self.root = self.native.root
        self.control = self.native.control
        def git(*args):
            return subprocess.run(['git', '-C', str(self.control), *args],
                capture_output=True, text=True, check=True).stdout.strip()
        self.git = git
        git('init', '-q')
        git('config', 'user.name', 'Fixture')
        git('config', 'user.email', 'fixture@example.invalid')
        git('fetch', '-q', str(self.runtime.bridge.workspace), self.native.plan['head'])
        git('reset', '--mixed', 'FETCH_HEAD')
        git('checkout', '--', 'source.txt')
        git('add', '.ci/policies/required-checks.toml')
        git('commit', '-qm', 'synthetic trusted control S')
        subject = replace(self.runtime.api.subject, control=git('rev-parse', 'HEAD'))
        self.runtime.api.subject = self.runtime.adapter.subject = subject
        self.plan = self.runtime.adapter.snapshot()
        self.native.plan = self.runtime.plan = self.plan
        self.runtime.api.row.update(head_sha=self.plan['control'],
            display_title='qualification:' + self.plan['request_key'])
        self.environment = dict(self.runtime.owners.environment, GITHUB_SHA=self.plan['control'])
        self.workspace = self.root / 'composed-H'
        self.context = dict(self.runtime.bridge.context, sha=self.plan['control'],
            qualification_subject={key: self.plan[key] for key in
                ('head', 'tree', 'base', 'control', 'request_key')}, product_workspace=str(self.workspace))
        self.context.pop('proof_started_at', None)
        self.context_path = Path(self.environment['RUNNER_TEMP']) / 'em-ci-job-context.json'
        self.context_path.unlink()
        self.runtime.fresh_producer_packet()
        self.trace = []
        self.failure_marker = self.root / 'selected-proof-fails'
        # Extend the existing synthetic planner/verifier, using its receipt.
        with self.native.tool.open('a') as stream:
            stream.write('\nif sys.argv[3:]==["rust-small-proof"]:\n'
                f'    receipt={self.native.receipt!r}\n'
                f'    failed=Path({str(self.failure_marker)!r}).exists()\n'
                '    if failed:\n'
                '        receipt["result"]="product_failure"\n'
                '        receipt["steps"][0]["outcome"]="product_failure"\n'
                '    output=Path("target/receipts/rust-small-proof.json")\n'
                '    output.parent.mkdir(parents=True,exist_ok=True)\n'
                '    output.write_text(json.dumps(receipt))\n'
                '    print("synthetic selected proof failure" if failed else "synthetic selected proof success")\n'
                '    raise SystemExit(7 if failed else 0)\n')
        self.canonical = CanonicalOwner(verifier=self.native.tool,
            verifier_sha256=hashlib.sha256(self.native.tool.read_bytes()).hexdigest(),
            control_sha=self.plan['control'], workspace=self.workspace)
        patch = mock.patch.object(factory, 'SOURCE_ROOT', self.control)
        patch.start()
        self.addCleanup(patch.stop)
        self.owners = factory.compose_native(self.runtime.adapter, self.environment,
            canonical_owner=self.canonical, control_root=self.control,
            native_warm_import=self.native.import_warm, warm_run_id=122, warm_run_attempt=1,
            native_import=self.runtime.native_import)
        self.review_classification = 'REVIEW_CURRENT'
        # production() hardcodes the installed source root. Keep evaluate and all
        # composed ports real while supplying this test's clean disposable S.
        def readiness(plan, **ports):
            self.assertEqual(self.git('rev-parse', 'HEAD'), plan['control'])
            self.assertEqual(self.git('status', '--porcelain'), '')
            authorities = admission.ExistingControlAuthorities(self.control, **ports)
            with mock.patch.object(authorities, 'semantic_review', return_value=admission.Review(
                    plan['request_key'], self.review_classification, 'synthetic review', '')), \
                 mock.patch.object(authorities, 'native_review', return_value=admission.Review(
                    plan['request_key'], 'CONVERGED', 'synthetic review', '')):
                return admission.evaluate(plan, authorities)
        patch = mock.patch.object(admission, 'production', side_effect=readiness)
        patch.start()
        self.addCleanup(patch.stop)
        self.native.trace.unlink(missing_ok=True)
        self.log = self.root / 'producer.log'

    def capture(self, environment, *, acquire_plan):
        self.assertEqual(acquire_plan(), self.plan)
        self.assertEqual(environment['GITHUB_WORKSPACE'], str(self.workspace))
        self.trace.append('capture')
        return dict(self.context)

    @contextmanager
    def allocation(self, plan, selected):
        self.assertEqual(plan, self.plan)
        self.assertEqual(selected.selected, tuple(row['name'] for row in self.native.receipt['steps']))
        self.trace.append('allocate')
        try:
            yield
        finally:
            self.trace.append('release')

    def execute(self, allocation=None):
        return factory.execute_native(self.owners, {'inputs': {'request': json.dumps(self.plan)}},
            self.capture, allocation_owner=allocation or self.allocation, log_path=self.log)

    def assert_no_product(self):
        self.assertFalse(self.log.exists())
        self.assertEqual(list(self.runtime.packet.iterdir()), [])
        self.assertEqual(self.runtime.imports, [])

    def test_positive_real_composition_subprocess_and_export(self):
        status, manifest = self.execute()
        self.assertEqual((status, manifest['result']), (0, 'success'))
        self.assertEqual(self.trace, ['capture', 'allocate', 'release'])
        self.assertEqual((manifest['sha'], manifest['run_id'], manifest['run_attempt']),
            (self.plan['control'], '123', '2'))
        receipt = json.loads((self.runtime.packet / 'rust-small-proof.json').read_bytes())
        self.assertEqual(receipt['subject']['git_sha'], self.plan['head'])
        self.assertNotEqual(manifest['sha'], receipt['subject']['git_sha'])
        self.assertTrue(self.native.imports)  # distinct warm import used by real readiness
        self.assertEqual(self.runtime.imports, [])  # current import is a later policy job
        calls = [json.loads(line)[2:] for line in self.native.trace.read_text().splitlines()]
        self.assertIn(['rust-small-proof'], calls)
        self.assertTrue(any(row[0] == 'pre-push-plan' for row in calls))
        self.assertTrue(any(row[0] == 'ci-scope' for row in calls))
        self.assertEqual(self.git('status', '--porcelain'), '')

    def test_actual_readiness_refuses_before_capture_or_allocation(self):
        self.review_classification = 'NOT_READY'
        result = self.execute()
        self.assertNotEqual(result['status'], 'READY_FOR_ALLOCATION')
        self.assertEqual(self.trace, [])
        self.assertFalse(self.context_path.exists())
        self.assert_no_product()

    def test_stale_head_base_control_refuse_before_allocation(self):
        original = self.runtime.api.subject
        for field in ('head', 'base', 'control'):
            with self.subTest(field=field):
                self.runtime.api.subject = replace(original, **{field: '0' * 40})
                with self.assertRaises(ValueError):
                    self.execute()
                self.assertEqual(self.trace, [])
                self.assert_no_product()
        self.runtime.api.subject = original

    def test_attempt_drift_while_waiting_releases_without_product(self):
        @contextmanager
        def drift(plan, selected):
            with self.allocation(plan, selected):
                self.runtime.api.row['run_attempt'] = 3
                yield
        with self.assertRaises(ValueError):
            self.execute(drift)
        self.assertEqual(self.trace, ['capture', 'allocate', 'release'])
        self.assertNotIn('proof_started_at', json.loads(self.context_path.read_bytes()))
        self.assert_no_product()

    def test_failed_selected_subprocess_preserves_failed_export_and_release(self):
        self.failure_marker.touch()
        status, manifest = self.execute()
        self.assertEqual((status, manifest['result']), (7, 'product_failure'))
        self.assertEqual(self.trace, ['capture', 'allocate', 'release'])
        self.assertIn('synthetic selected proof failure', self.log.read_text())
        receipt = json.loads((self.runtime.packet / 'rust-small-proof.json').read_bytes())
        self.assertEqual(receipt['steps'][0]['outcome'], 'product_failure')
        self.assertEqual(receipt['subject']['git_sha'], self.plan['head'])


if __name__ == '__main__':
    unittest.main()
