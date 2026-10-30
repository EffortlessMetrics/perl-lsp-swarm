"""Execute the generated-PR shell consumers with a recording GitHub adapter.

No network, credentials, product builds or hosted dispatch are used. YAML is read
with the installed PyYAML loader; production catalog authority remains unchanged.
"""
from pathlib import Path
import json
import os
import subprocess
import tempfile
import unittest
import yaml
from scripts.ci.test_staged_qualification_workflow_cutover import staged_tree

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = {
    'post-merge-status.yml': ('automation/post-merge-status', ['pr-title-check.yml']),
    'aggregate-ci-lane-history.yml': ('automation/ci-lane-history',
                                      ['pr-title-check.yml', 'validate-ci-lane-history.yml']),
}
STUB = '''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
log = pathlib.Path(os.environ['GH_SPY_LOG'])
with log.open('a') as out:
    out.write(json.dumps(args) + '\\n')
if args[:2] == ['pr', 'view']:
    if os.environ.get('FAIL_LOOKUP') == '1':
        sys.exit(1)
    count = sum(1 for line in log.read_text().splitlines() if json.loads(line)[:2] == ['pr', 'view'])
    head = 'b' * 40 if count < int(os.environ.get('MOVE_AT', '999')) else 'c' * 40
    print(json.dumps({'baseRefOid': 'a' * 40, 'headRefOid': head}))
    sys.exit(0)
if args[:2] == ['workflow', 'run']:
    sys.exit(1 if args[2] == os.environ.get('FAIL_WORKFLOW') else 0)
sys.exit(99)
'''


def dispatch_step(path):
    workflow = yaml.load(path.read_text(), Loader=yaml.BaseLoader)
    return next(step for job in workflow['jobs'].values() for step in job.get('steps', [])
                if step.get('name') == 'Raise CI on the generated PR')


class GeneratedMetadataDispatchTests(unittest.TestCase):
    def setUp(self):
        # Exercise the unapplied coordinated leaf, preserving today's required
        # product dispatches in tracked workflows until native cutover is ready.
        self.root = self.enterContext(staged_tree())

    def run_step(self, workflow, *, source_root=None, **overrides):
        step = dispatch_step((source_root or self.root) / '.github/workflows' / workflow)
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            (directory / 'gh').write_text(STUB)
            (directory / 'gh').chmod(0o755)
            log = directory / 'calls.jsonl'
            branch, _ = WORKFLOWS[workflow]
            # Isolated environment contains only fixture identities; no real token.
            env = dict(PATH=str(directory) + os.pathsep + os.environ['PATH'],
                       GH_SPY_LOG=str(log), BRANCH=branch, PR_NUMBER='123',
                       CREATED_HEAD_SHA='b' * 40, BASE_SHA='a' * 40,
                       GENERATED_HEAD_SHA='b' * 40, **overrides)
            run = subprocess.run(['bash', '-c', step['run']], env=env,
                                 capture_output=True, text=True, timeout=10)
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            return run, calls

    def test_installed_generation_retains_required_dispatch_and_exact_ci_inputs(self):
        for workflow, (_, metadata) in WORKFLOWS.items():
            with self.subTest(workflow=workflow):
                run, calls = self.run_step(workflow, source_root=ROOT)
                self.assertEqual(run.returncode, 0, run.stderr)
                dispatches = [call for call in calls if call[:2] == ['workflow', 'run']]
                self.assertEqual([call[2] for call in dispatches],
                                 ['ci.yml', 'em-ci-routed-rust.yml', 'ripr.yml', *metadata])
                self.assertEqual(dispatches[0][-4:], ['-f', 'base_sha=' + 'a' * 40,
                                                    '-f', 'head_sha=' + 'b' * 40])

    def test_publication_dispatches_exact_metadata_set_without_product(self):
        for workflow, (branch, expected) in WORKFLOWS.items():
            with self.subTest(workflow=workflow):
                run, calls = self.run_step(workflow)
                self.assertEqual(run.returncode, 0, run.stderr)
                dispatches = [call for call in calls if call[:2] == ['workflow', 'run']]
                self.assertEqual(dispatches, [['workflow', 'run', item, '--ref', branch]
                                             for item in expected])
                self.assertTrue(all(call[:2] in (['workflow', 'run'], ['pr', 'view']) for call in calls))

    def test_dispatch_failure_stays_red_and_attempts_remaining_metadata(self):
        for workflow, (_, expected) in WORKFLOWS.items():
            with self.subTest(workflow=workflow):
                run, calls = self.run_step(workflow, FAIL_WORKFLOW=expected[0])
                self.assertEqual(run.returncode, 1)
                self.assertEqual([call[2] for call in calls if call[:2] == ['workflow', 'run']], expected)

    def test_lane_history_lookup_failure_and_head_movement_stay_red(self):
        for options in ({'FAIL_LOOKUP': '1'}, {'MOVE_AT': '1'}, {'MOVE_AT': '2'}):
            with self.subTest(options=options):
                run, calls = self.run_step('aggregate-ci-lane-history.yml', **options)
                self.assertEqual(run.returncode, 1)
                self.assertEqual([call[2] for call in calls if call[:2] == ['workflow', 'run']],
                                 WORKFLOWS['aggregate-ci-lane-history.yml'][1])

    def test_publication_conditions_preserve_refusal_guards(self):
        for workflow in WORKFLOWS:
            condition = dispatch_step(self.root / '.github/workflows' / workflow)['if']
            self.assertIn("steps.create-pr.outputs.pull-request-number != '0'", condition)
        self.assertIn("steps.freshness.outputs.publish == 'true'",
                      dispatch_step(self.root / '.github/workflows/post-merge-status.yml')['if'])


if __name__ == '__main__':
    unittest.main()
