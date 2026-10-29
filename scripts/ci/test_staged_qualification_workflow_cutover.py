"""Exercise the unapplied trigger diff using the native workflow catalog.

This is source rehearsal. Neither patch application nor adapter metadata admits
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

from scripts.ci import validate_gate_enforcement_contract as catalog
from scripts.ci import test_request_qualification as adapter_fixture

ROOT = Path(__file__).resolve().parents[2]
PATCH = ROOT / '.ci/staged/17903-root-triggers.patch'
ROOTS = ('.github/workflows/ci.yml', '.github/workflows/ripr.yml',
         '.github/workflows/em-ci-routed-rust.yml')
POLICY = '.ci/policies/required-checks.toml'
DISPATCH_PATHS = ('.github/workflows/aggregate-ci-lane-history.yml',
                  '.github/workflows/post-merge-status.yml',
                  'xtask/tests/lane_history_ci_dispatch_contract.rs',
                  'xtask/tests/post_merge_status_regen_tests.rs')
ORDINARY = {'pull_request', 'pull_request_target', 'merge_group', 'push'}


def git(root, *args):
    env = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
    return subprocess.run(['git', '-C', str(root), *args], env=env, check=True,
                          capture_output=True, text=True).stdout


@contextmanager
def staged_tree():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        for name in (*[path.relative_to(ROOT).as_posix() for path in
                       sorted((ROOT / '.github/workflows').glob('*.y*ml'))], POLICY,
                     *[path for path in DISPATCH_PATHS if path.startswith('xtask/')]):
            target = root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, target)
        git(root, 'init', '-q', '--template=')
        git(root, 'add', '.')
        git(root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
            'commit', '-qm', 'source rehearsal')
        git(root, 'apply', '--unidiff-zero', '--check', str(PATCH))
        git(root, 'apply', '--unidiff-zero', str(PATCH))
        yield root


class StagedTriggerCutoverTests(unittest.TestCase):
    def assert_closed(self, root):
        for path in ROOTS:
            self.assertFalse(catalog.read_workflow(root, path).events & ORDINARY, path)

    def test_real_patch_closes_ordinary_roots_using_native_catalog(self):
        with staged_tree() as root:
            for path in ROOTS:
                before = catalog.read_workflow(ROOT, path)
                after = catalog.read_workflow(root, path)
                self.assertTrue(before.events & ORDINARY, path)
                self.assertFalse(after.events & ORDINARY, path)
                self.assertIn('workflow_dispatch', after.events)
                # No product command, result job, formatter, conflict marker or
                # artifact obligation disappears under a trigger cleanup.
                self.assertEqual(before.jobs, after.jobs, path)
                self.assertEqual((ROOT / path).read_text().split('jobs:\n', 1)[1],
                                 (root / path).read_text().split('jobs:\n', 1)[1])
            self.assertEqual(catalog.read_workflow(root, ROOTS[0]).events,
                             frozenset({'workflow_dispatch'}))
            self.assertEqual(catalog.read_workflow(root, ROOTS[2]).events,
                             frozenset({'workflow_dispatch'}))

    def test_root_policy_rollback_restores_exact_original_bytes(self):
        with staged_tree() as root:
            git(root, 'apply', '--unidiff-zero', '--reverse', '--check', str(PATCH))
            git(root, 'apply', '--unidiff-zero', '--reverse', str(PATCH))
            for path in (*ROOTS, POLICY, *DISPATCH_PATHS):
                self.assertEqual((root / path).read_bytes(), (ROOT / path).read_bytes(), path)
            self.assertEqual(git(root, 'diff', '--exit-code'), '')

    def test_reintroduced_ready_or_synchronize_outside_ci_is_detected(self):
        for path in ROOTS[1:]:
            with self.subTest(path=path), staged_tree() as root:
                target = root / path
                target.write_text(target.read_text().replace('on:\n',
                    'on:\n  pull_request:\n    types: [ready_for_review, synchronize]\n', 1))
                with self.assertRaises(AssertionError):
                    self.assert_closed(root)

    def test_cache_schedule_and_result_metadata_are_retained_not_exempted(self):
        with staged_tree() as root:
            workflow = catalog.read_workflow(root, ROOTS[1])
            self.assertEqual(workflow.events, frozenset({'workflow_dispatch', 'schedule'}))
            self.assertIn('seed-cache', workflow.jobs)
            self.assertIn("github.event_name != 'schedule'", workflow.jobs['route-ripr'].condition)
            self.assertIn("github.event_name == 'schedule'", workflow.jobs['seed-cache'].condition)
            # Native diagnostic intentionally still refuses this mixed cache/
            # qualification root. Preserve that honest NOT_PROVEN boundary.
            findings = catalog.qualification_source_findings({ROOTS[1]: workflow}, [ROOTS[1]])
            self.assertIn('qualification_automatic_entry', {finding.code for finding in findings})

    def test_policy_changes_only_event_projection_keeps_mandatory_producers(self):
        before = tomllib.loads((ROOT / POLICY).read_text())
        with staged_tree() as root:
            after = tomllib.loads((root / POLICY).read_text())
        self.assertEqual(before.keys(), after.keys())
        for key in before:
            if key != 'checks':
                self.assertEqual(before[key], after[key])
        self.assertEqual(len(before['checks']), len(after['checks']))
        for left, right in zip(before['checks'], after['checks']):
            if left.get('workflow') in ROOTS:
                events = [event for event in left['events'] if event not in ORDINARY]
                expected = {**left, 'events': events or ['workflow_dispatch']}
                self.assertEqual(expected, right)
            else:
                self.assertEqual(left, right)

    def test_projected_native_contract_remains_structurally_valid(self):
        with staged_tree() as root:
            git(root, 'add', '.')
            git(root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                'commit', '-qm', 'projected source only')
            result = catalog.validate(root, root / POLICY)
            self.assertEqual(result['status'], 'SUCCESS', result['findings'])
            self.assertEqual(result['live_enforcement_status'], 'NOT_PROVEN')

    def test_actual_adapter_refuses_product_admission_before_dispatch(self):
        api = adapter_fixture.GitHub()
        adapter = adapter_fixture.adapter.Adapter(api, adapter_fixture.SUBJECT)
        result = adapter.request()
        self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertEqual(result['qualification_result'], 'NOT_PROVEN')
        self.assertNotEqual(result['subject']['control'], result['subject']['head'])
        self.assertEqual(result['subject']['workflow_path'],
                         '.github/workflows/qualification-rehearsal.yml')
        self.assertTrue(api.calls)
        self.assertTrue(all(method == 'GET' for method, *_ in api.calls))
        self.assertIn('#10057', result['reason'])


if __name__ == '__main__':
    unittest.main()
