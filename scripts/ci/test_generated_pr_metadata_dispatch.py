"""Execute the real staged publication readback with a recording GitHub adapter.

No network, credentials, builds or hosted dispatch. PyYAML reads the production
step; the native catalog remains the event/policy owner.
"""
from pathlib import Path
import json
import os
import subprocess
import tempfile
import unittest
import yaml
from scripts.ci.test_staged_qualification_workflow_cutover import ROOT, preimage_tree, staged_tree

WORKFLOWS = ('post-merge-status.yml', 'aggregate-ci-lane-history.yml')
STUB = '''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
with pathlib.Path(os.environ['GH_SPY_LOG']).open('a', encoding='utf-8') as out:
    out.write(json.dumps(args) + '\\n')
if args[:2] == ['pr', 'view']:
    if os.environ.get('FAIL_LOOKUP') == '1':
        sys.exit(1)
    print(os.environ.get('OBSERVED', 'b' * 40 + '\\tOPEN'))
    sys.exit(0)
if args[:2] == ['workflow', 'run']:
    sys.exit(0)
sys.exit(99)
'''


def steps(path):
    workflow = yaml.load(path.read_text(encoding='utf-8'), Loader=yaml.BaseLoader)
    return [step for job in workflow['jobs'].values() for step in job.get('steps', [])]


def readback_step(path):
    return next(step for step in steps(path)
                if step.get('name') == 'Reconcile generated PR without CI')


class GeneratedPublicationTests(unittest.TestCase):
    def setUp(self):
        self.root = self.enterContext(staged_tree())

    def run_step(self, workflow, *, run_source=None, **overrides):
        step = readback_step(self.root / '.github/workflows' / workflow)
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            (directory / 'gh').write_text(STUB, encoding='utf-8')
            (directory / 'gh').chmod(0o755)
            log, summary = directory / 'calls.jsonl', directory / 'summary.md'
            env = dict(PATH=str(directory) + os.pathsep + os.environ['PATH'],
                       GH_SPY_LOG=str(log), GH_REPO='fixture/repo', PR_NUMBER='123',
                       CREATED_HEAD_SHA='b' * 40, GITHUB_STEP_SUMMARY=str(summary))
            env.update(overrides)
            result = subprocess.run(['bash', '-c', step['run'] if run_source is None else run_source],
                                    env=env, capture_output=True, text=True, encoding="utf-8", timeout=10)
            calls = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()] if log.exists() else []
            return result, calls, summary.read_text(encoding='utf-8') if summary.exists() else ''

    def assert_no_dispatch(self, calls):
        self.assertTrue(calls)
        self.assertTrue(all(call[:2] == ['pr', 'view'] for call in calls), calls)

    def test_real_publication_reads_exact_open_head_and_starts_no_ci(self):
        for workflow in WORKFLOWS:
            with self.subTest(workflow=workflow):
                result, calls, summary = self.run_step(workflow)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assert_no_dispatch(calls)
                self.assertEqual(calls, [['pr', 'view', '123', '--repo', 'fixture/repo',
                    '--json', 'headRefOid,state', '--jq', '[.headRefOid, .state] | @tsv']])
                self.assertIn('NOT_PROVEN', summary)
                self.assertIn('b' * 40, summary)

    def test_missing_moved_closed_or_unreadable_subject_stays_failed(self):
        for workflow in WORKFLOWS:
            for options in ({'FAIL_LOOKUP': '1'}, {'OBSERVED': 'c' * 40 + '\tOPEN'},
                            {'OBSERVED': 'b' * 40 + '\tCLOSED'}, {'OBSERVED': ''}):
                with self.subTest(workflow=workflow, options=options):
                    result, calls, summary = self.run_step(workflow, **options)
                    self.assertNotEqual(result.returncode, 0)
                    self.assert_no_dispatch(calls)
                    self.assertEqual(summary, '')

    def test_invalid_identity_refuses_before_native_read(self):
        for options in ({'PR_NUMBER': ''}, {'PR_NUMBER': '0'}, {'PR_NUMBER': '--help'},
                        {'CREATED_HEAD_SHA': ''}, {'CREATED_HEAD_SHA': 'not-a-sha'}):
            with self.subTest(options=options):
                result, calls, summary = self.run_step(WORKFLOWS[0], **options)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, [])
                self.assertEqual(summary, '')

    def test_retained_dispatch_and_ignored_head_mutants_are_observed(self):
        for workflow in WORKFLOWS:
            source = readback_step(self.root / '.github/workflows' / workflow)['run']
            result, calls, _ = self.run_step(workflow,
                run_source=source + '\ngh workflow run pr-title-check.yml --ref candidate\n')
            self.assertEqual(result.returncode, 0)
            with self.assertRaises(AssertionError):
                self.assert_no_dispatch(calls)
            mutant = source.replace('if [ "$observed" != "$expected" ]; then', 'if false; then')
            result, _, summary = self.run_step(workflow, run_source=mutant,
                                               OBSERVED='c' * 40 + '\tOPEN')
            self.assertEqual(result.returncode, 0)
            self.assertTrue(summary)
            result, _, _ = self.run_step(workflow, OBSERVED='c' * 40 + '\tOPEN')
            self.assertNotEqual(result.returncode, 0)

    def test_whole_generators_have_no_hidden_dispatch_and_keep_publication_guards(self):
        for workflow in WORKFLOWS:
            path = self.root / '.github/workflows' / workflow
            source_steps = steps(path)
            self.assertTrue(any(step.get('uses', '').startswith('peter-evans/create-pull-request@')
                                for step in source_steps))
            for step in source_steps:
                self.assertNotIn('gh workflow run', step.get('run', ''))
                self.assertNotIn('/dispatches', step.get('run', ''))
            condition = readback_step(path)['if']
            self.assertIn("steps.create-pr.outputs.pull-request-number != ''", condition)
            self.assertIn("steps.create-pr.outputs.pull-request-number != '0'", condition)
        self.assertIn("steps.freshness.outputs.publish == 'true'",
                      readback_step(self.root / '.github/workflows/post-merge-status.yml')['if'])
        with preimage_tree() as historical:
            for workflow in WORKFLOWS:
                self.assertTrue(any('gh workflow run' in step.get('run', '')
                                    for step in steps(historical / '.github/workflows' / workflow)))
                # The actual installed generator has the same bounded readback,
                # with no dependency on replaying development workflows.
                installed = ROOT / '.github/workflows' / workflow
                self.assertEqual(readback_step(installed), readback_step(self.root / '.github/workflows' / workflow))
                self.assertTrue(all('gh workflow run' not in step.get('run', '')
                                    and '/dispatches' not in step.get('run', '') for step in steps(installed)))


if __name__ == '__main__':
    unittest.main()
