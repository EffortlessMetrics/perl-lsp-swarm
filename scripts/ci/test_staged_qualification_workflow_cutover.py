"""Exercise the unapplied queue perimeter through the native workflow catalog.

This is source rehearsal. Neither patch application nor source metadata admits
product execution or satisfies a native required context.
"""
from contextlib import contextmanager
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest
import yaml

from scripts.ci import validate_gate_enforcement_contract as catalog
from scripts.ci import test_request_qualification as adapter_fixture

ROOT = Path(__file__).resolve().parents[2]
PATCH = ROOT / '.ci/staged/17903-root-triggers.patch'
POLICY = '.ci/policies/required-checks.toml'
POST_LANDING = ('.github/workflows/docs-deploy.yml',)
RETIRED = '.github/workflows/main-history-event.yml'
ARCHIVED = '.ci/retired-workflows/main-history-event.yml'
GENERATORS = ('.github/workflows/aggregate-ci-lane-history.yml',
              '.github/workflows/post-merge-status.yml')
PROTECTED = {
    'Compile All Targets (bit-rot guard)': ('.github/workflows/ci.yml', 'integration-required'),
    'Conflict marker check': ('.github/workflows/ci.yml', 'conflict-markers'),
    'validate-title': ('.github/workflows/pr-title-check.yml', 'validate-title'),
    'Perl LSP Rust Small Result': ('.github/workflows/em-ci-routed-rust.yml', 'rust-small-result'),
    'ripr+ New Gap Gate': ('.github/workflows/ripr.yml', 'ripr'),
}


def git(root, *args):
    env = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
    return subprocess.run(['git', '-C', str(root), *args], env=env, check=True,
                          capture_output=True, text=True).stdout


@contextmanager
def staged_tree():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        changed_paths = [line.removeprefix('--- a/') for line in PATCH.read_text().splitlines()
                         if line.startswith('--- a/')]
        paths = {*catalog.read_workflow_catalog(ROOT), POLICY, *changed_paths}
        for name in sorted(paths):
            target = root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, target)
        git(root, 'init', '-q', '--template=')
        git(root, 'add', '.')
        git(root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
            'commit', '-qm', 'source rehearsal')
        git(root, 'apply', '--check', str(PATCH))
        git(root, 'apply', str(PATCH))
        yield root


class StagedTriggerCutoverTests(unittest.TestCase):
    def assert_closed(self, root):
        findings = catalog.development_source_findings(
            catalog.read_workflow_catalog(root), post_landing_sources=POST_LANDING)
        self.assertEqual(findings, [])

    def test_whole_catalog_closes_development_roots_and_retains_actual_groups(self):
        before = catalog.read_workflow_catalog(ROOT)
        self.assertTrue(catalog.development_source_findings(before, post_landing_sources=POST_LANDING))
        with staged_tree() as root:
            after = catalog.read_workflow_catalog(root)
            self.assertEqual(set(before) - {RETIRED}, set(after))
            self.assert_closed(root)
            for path, original in before.items():
                if path == RETIRED:
                    self.assertEqual((ROOT / path).read_bytes(), (root / ARCHIVED).read_bytes())
                    continue
                changed = after[path]
                self.assertEqual('merge_group' in original.events, 'merge_group' in changed.events, path)
                if 'merge_group' in changed.events:
                    self.assertEqual(changed.merge_group_types, ('checks_requested',), path)
                if path == '.github/workflows/ci.yml':
                    self.assertEqual(set(original.jobs) | {'integration-required'}, set(changed.jobs), path)
                    left = yaml.load((ROOT / path).read_text(), Loader=yaml.BaseLoader)['jobs']
                    right = yaml.load((root / path).read_text(), Loader=yaml.BaseLoader)['jobs']
                    del right['integration-required']
                    right['check-all-targets']['name'] = left['check-all-targets']['name']
                    self.assertEqual(left, right)
                else:
                    self.assertEqual(original.jobs.keys(), changed.jobs.keys(), path)
                if path not in (*GENERATORS, '.github/workflows/ci.yml', '.github/workflows/em-ci-routed-rust.yml'):
                    self.assertEqual((ROOT / path).read_text().split('jobs:\n', 1)[1],
                                     (root / path).read_text().split('jobs:\n', 1)[1], path)

    def test_rollback_restores_every_changed_byte(self):
        with staged_tree() as root:
            git(root, 'apply', '--reverse', '--check', str(PATCH))
            git(root, 'apply', '--reverse', str(PATCH))
            self.assertEqual(git(root, 'diff', '--exit-code'), '')

    def test_reintroduced_metadata_or_product_subscription_is_detected(self):
        for path in ('.github/workflows/ripr.yml', '.github/workflows/pr-title-check.yml',
                     '.github/workflows/semantic-close-containment.yml'):
            with self.subTest(path=path), staged_tree() as root:
                target = root / path
                target.write_text(target.read_text().replace('on:\n',
                    'on:\n  pull_request_target:\n    types: [ready_for_review, synchronize, closed]\n', 1))
                with self.assertRaises(AssertionError):
                    self.assert_closed(root)

    def test_new_skipped_wrapper_is_detected_without_a_selected_workflow_list(self):
        with staged_tree() as root:
            (root / '.github/workflows/new-wrapper.yml').write_text(
                'name: new wrapper\non:\n  issue_comment:\n    types: [created]\njobs:\n'
                '  no-op:\n    if: false\n    runs-on: ubuntu-latest\n    steps:\n      - run: true\n')
            git(root, 'add', '.github/workflows/new-wrapper.yml')
            with self.assertRaises(AssertionError):
                self.assert_closed(root)

    def test_separate_schedules_releases_cache_and_post_landing_purposes_survive(self):
        before = catalog.read_workflow_catalog(ROOT)
        with staged_tree() as root:
            after = catalog.read_workflow_catalog(root)
            for path, workflow in before.items():
                if path == RETIRED:
                    continue
                retained = workflow.events - catalog.DEVELOPMENT_EVENTS - {'push'}
                self.assertTrue(retained <= after[path].events, path)
            for path in POST_LANDING:
                self.assertIn('push', after[path].events, path)
            ripr = after['.github/workflows/ripr.yml']
            self.assertIn('schedule', ripr.events)
            self.assertIn('seed-cache', ripr.jobs)
            self.assertIn("github.event_name == 'schedule'", ripr.jobs['seed-cache'].condition)
            self.assertIn('supersede-stale', after['.github/workflows/post-merge-status.yml'].jobs)
            self.assertEqual(after['.github/workflows/post-merge-status.yml'].events,
                             frozenset({'workflow_dispatch'}))
            self.assertNotIn(RETIRED, after)

    def test_policy_changes_only_event_projection_preserves_all_five_producers(self):
        before = tomllib.loads((ROOT / POLICY).read_text())
        with staged_tree() as root:
            after = tomllib.loads((root / POLICY).read_text())
            workflows = catalog.read_workflow_catalog(root)
        self.assertEqual(before.keys(), after.keys())
        for key in before:
            if key != 'checks':
                self.assertEqual(before[key], after[key])
        self.assertEqual(len(before['checks']), len(after['checks']))
        for left, right in zip(before['checks'], after['checks']):
            expected = dict(left)
            if left.get('workflow') in workflows and 'events' in left:
                events = [event for event in left['events'] if event not in catalog.DEVELOPMENT_EVENTS
                          and not (event == 'push' and left['workflow'] not in POST_LANDING)]
                expected['events'] = events or ['workflow_dispatch']
            if left['name'] == 'Compile All Targets (bit-rot guard)':
                expected['job'] = 'integration-required'
                expected['applicability'] = 'always-or-scoped-noop'
                expected['reason'] = right['reason']
                self.assertIn('Every selected shard and UX obligation', right['reason'])
            self.assertEqual(expected, right)
        for name, producer in PROTECTED.items():
            entry = next(check for check in after['checks'] if check['name'] == name)
            self.assertEqual((entry['workflow'], entry['job']), producer)
            self.assertIn(entry['job'], workflows[entry['workflow']].jobs)
        # The patch must not pretend title has a G-membership producer yet.
        self.assertNotIn('merge_group', workflows[PROTECTED['validate-title'][0]].events)

    def test_projected_contract_passes_source_only_and_keeps_native_ceiling(self):
        with staged_tree() as root:
            git(root, 'add', '.')
            git(root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                'commit', '-qm', 'projected source only')
            result = catalog.validate(root, root / POLICY, no_development_events=True,
                                      post_landing_sources=POST_LANDING)
            self.assertEqual(result['status'], 'SUCCESS', result['findings'])
            self.assertEqual(result['live_enforcement_status'], 'NOT_PROVEN')
            self.assertEqual(result['development_event_perimeter']['native_zero_run_status'], 'NOT_PROVEN')
            findings = catalog.qualification_source_findings(
                catalog.read_workflow_catalog(root), sorted({path for path, _ in PROTECTED.values()}),
                entry_event='merge_group')
            self.assertTrue(findings)

    def test_actual_adapter_still_refuses_product_admission_before_dispatch(self):
        api = adapter_fixture.GitHub()
        adapter = adapter_fixture.adapter.Adapter(api, adapter_fixture.SUBJECT)
        result = adapter.request()
        self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertEqual(result['qualification_result'], 'NOT_PROVEN')
        self.assertNotEqual(result['subject']['control'], result['subject']['head'])
        self.assertTrue(api.calls)
        self.assertTrue(all(method == 'GET' for method, *_ in api.calls))


class StagedIntegrationFanInTests(unittest.TestCase):
    def setUp(self):
        self.root = self.enterContext(staged_tree())
        self.ci = yaml.load((self.root / '.github/workflows/ci.yml').read_text(), Loader=yaml.BaseLoader)
        self.job = self.ci['jobs']['integration-required']
        self.step = self.job['steps'][0]

    def run_finalizer(self, compile_result='success', integration_result='success', *, source=None):
        return subprocess.run(['bash', '--noprofile', '--norc', '-c', source or self.step['run']],
                              env={'COMPILE_RESULT': compile_result, 'INTEGRATION_RESULT': integration_result},
                              capture_output=True, text=True, timeout=5)

    def test_existing_protected_name_depends_on_same_run_compile_and_selected_aggregate(self):
        self.assertEqual(self.job['name'], 'Compile All Targets (bit-rot guard)')
        self.assertEqual(self.job['needs'], ['check-all-targets', 'merge-gate'])
        self.assertEqual(self.job['if'], 'always()')
        self.assertEqual(self.step['env'], {
            'COMPILE_RESULT': '${{ needs.check-all-targets.result }}',
            'INTEGRATION_RESULT': '${{ needs.merge-gate.result }}',
        })
        self.assertEqual(sum(job.get('name') == self.job['name'] for job in self.ci['jobs'].values()), 1)
        self.assertEqual(self.ci['jobs']['check-all-targets']['name'], 'Compile All Targets execution')
        self.assertIn('merge-gate-shards', self.ci['jobs']['merge-gate']['needs'])
        self.assertIn('ux-tests', self.ci['jobs']['merge-gate']['needs'])

    def test_any_missing_skipped_failed_or_cancelled_dependency_is_not_green(self):
        self.assertEqual(self.run_finalizer().returncode, 0)
        for result in ('', 'skipped', 'failure', 'cancelled', 'neutral', 'timed_out', 'unknown'):
            for compile_result, integration_result in ((result, 'success'), ('success', result)):
                with self.subTest(compile=compile_result, integration=integration_result):
                    observed = self.run_finalizer(compile_result, integration_result)
                    self.assertNotEqual(observed.returncode, 0, observed.stdout)

    def test_old_green_compile_cannot_hide_a_selected_shard_failure(self):
        from scripts.ci import evaluate_ci_gate as gate
        needs = {name: {'result': 'success', 'outputs': {}} for name in gate.EXPECTED_DEPENDENCIES}
        needs['draft-pr-check']['outputs']['run_ci'] = 'true'
        needs['preflight-latest-check']['outputs']['is_latest'] = 'true'
        needs['merge-gate-shards']['result'] = 'failure'
        verdict = gate.evaluate(needs, event_name='merge_group')
        self.assertEqual(verdict.status, 'failure')
        self.assertNotEqual(self.run_finalizer('success', verdict.status).returncode, 0)
        mutant = self.step['run'].replace(' || [ "$INTEGRATION_RESULT" != success ]', '')
        self.assertEqual(self.run_finalizer('success', verdict.status, source=mutant).returncode, 0)

    def test_actual_g_rust_small_router_skip_stays_unproven(self):
        workflow = yaml.load((self.root / '.github/workflows/em-ci-routed-rust.yml').read_text(), Loader=yaml.BaseLoader)
        step = next(step for step in workflow['jobs']['rust-small-result']['steps']
                    if step.get('name') == 'Evaluate routed result')
        self.assertEqual(step['env']['EVENT_NAME'], '${{ github.event_name }}')
        with tempfile.TemporaryDirectory() as directory:
            env = {'PATH': os.environ['PATH'], 'ROUTE_RESULT': 'skipped', 'EVENT_NAME': 'merge_group',
                   'GITHUB_STEP_SUMMARY': str(Path(directory) / 'summary')}
            observed = subprocess.run(['bash', '--noprofile', '--norc', '-c', step['run']],
                                      env=env, capture_output=True, text=True, timeout=5)
            self.assertNotEqual(observed.returncode, 0)
            self.assertIn('Merge-group Rust Small route skipped: NOT_PROVEN', observed.stdout)
            # Unmodified production preimage really passed this same missing proof.
            before = yaml.load((ROOT / '.github/workflows/em-ci-routed-rust.yml').read_text(), Loader=yaml.BaseLoader)
            old_step = next(step for step in before['jobs']['rust-small-result']['steps']
                            if step.get('name') == 'Evaluate routed result')
            old = subprocess.run(['bash', '--noprofile', '--norc', '-c', old_step['run']],
                                 env=env, capture_output=True, text=True, timeout=5)
            self.assertEqual(old.returncode, 0, old.stderr)


if __name__ == '__main__':
    unittest.main()
