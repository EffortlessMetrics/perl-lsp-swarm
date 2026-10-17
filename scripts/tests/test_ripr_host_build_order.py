"""Focused workflow-text and native Bash controls; mock Cargo is not receipt proof."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

DEFAULT = Path(__file__).resolve().parents[2] / '.github/workflows/ripr.yml'
WORKFLOW = Path(os.environ.get('WORKFLOW_UNDER_TEST', DEFAULT))
HOST_COMMAND = 'bash scripts/ci/observe-xtask-build.sh host-initial "$RIPR_FRESHNESS_HANDOFF/xtask-builds" -- cargo build -p xtask --locked'
ALWAYS = ['Adjudicate receipt cache exactness', 'Generate repo-wide RIPR+ baseline receipt',
          'Record exact RIPR badge producer', 'Generate review guidance',
          'Generate impacted evidence', 'Generate PR evidence summary',
          'Generate warning annotations', 'Audit suppression lifecycle',
          'Validate PR evidence contracts', 'Enforce new RIPR gap quality gate',
          'Append PR evidence summary', 'Record RIPR freshness handoff result']
CHECKS = [
    ['xtask', 'ripr-pr', '--base', 'origin/main', '--head', 'HEAD', '--pr-head', 'fixed-pr-head', '--check'],
    ['xtask', 'ripr-plus', '--receipt', 'target/receipts/quality/ripr-plus.json', '--check'],
    ['xtask', 'ripr-review-comments', '--base', 'origin/main', '--head', 'HEAD', '--pr-head', 'fixed-pr-head', '--check'],
    ['xtask', 'impacted-evidence', '--labels-csv', 'proof-label', '--check'],
    ['xtask', 'ripr-pr-summary', '--check'], ['xtask', 'ripr-annotations', '--check'],
]


def job(text):
    matches = list(re.finditer(r'^  ripr-github:\s*$', text, re.M))
    if len(matches) != 1:
        raise AssertionError('expected exactly one primary hosted job')
    start = matches[0].start()
    next_job = re.search(r'^  [A-Za-z0-9_-]+:\s*$', text[matches[0].end():], re.M)
    return text[start:matches[0].end() + next_job.start()] if next_job else text[start:]


def steps(text):
    text = job(text)
    matches = list(re.finditer(r'^      - name: (.+)$', text, re.M))
    result = {}
    for i, match in enumerate(matches):
        name = match.group(1)
        if name in result:
            raise AssertionError('duplicate step: ' + name)
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        result[name] = text[match.start():end]
    return result


def field(block, name, indent=8):
    matches = re.findall(r'^' + ' ' * indent + re.escape(name) + r': (.+)$', block, re.M)
    if len(matches) > 1:
        raise AssertionError('duplicate field: ' + name)
    return matches[0] if matches else None


def bash_run(block):
    lines = block.splitlines()
    start = lines.index('        run: |') + 1
    body = []
    for line in lines[start:]:
        if line.startswith('          '):
            body.append(line[10:])
        elif line.strip() and not line.lstrip().startswith('#'):
            break
    if not body:
        raise AssertionError('empty Bash run block')
    return '\n'.join(body) + '\n'


class OrderProof(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        text = WORKFLOW.read_text()
        cls.parts = steps(text)
        cls.primary_job = job(text).split('    steps:', 1)[0]

    def test_build_follows_container_before_cache_and_baseline(self):
        names = list(self.parts)
        self.assertLess(names.index('Generate PR evidence'), names.index('Build xtask'))
        self.assertLess(names.index('Build xtask'), names.index('Restore memoized ripr-plus receipt'))
        self.assertLess(names.index('Build xtask'), names.index('Generate repo-wide RIPR+ baseline receipt'))

    def test_prepare_creates_runner_owned_observations_before_container(self):
        names = list(self.parts)
        self.assertLess(names.index('Prepare RIPR freshness handoff'), names.index('Generate PR evidence'))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            env = os.environ.copy()
            env.pop('BASH_ENV', None)
            env.update(RUNNER_TEMP=str(root), GITHUB_ENV=str(root / 'github-env'),
                       GITHUB_RUN_ID='20', GITHUB_RUN_ATTEMPT='3')
            result = subprocess.run(['bash', '--noprofile', '--norc', '-e', '-o', 'pipefail', '-c',
                                     bash_run(self.parts['Prepare RIPR freshness handoff'])],
                                    cwd=root, env=env, text=True, capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            values = dict(line.split('=', 1) for line in (root / 'github-env').read_text().splitlines())
            observations = Path(values['RIPR_FRESHNESS_HANDOFF']) / 'xtask-builds'
            self.assertTrue(observations.is_dir())
            self.assertEqual(observations.stat().st_uid, os.getuid())
            self.assertTrue(os.access(observations, os.W_OK))
            self.assertRegex(values['RIPR_FRESHNESS_TOKEN'], r'^20/3/[0-9]+$')

    def test_package_wide_build_and_conditions(self):
        self.assertEqual(field(self.parts['Build xtask'], 'run'), HOST_COMMAND)
        self.assertIn(field(self.primary_job, 'continue-on-error', indent=4), (None, 'false'))
        mandatory = ['Build xtask', 'Run ripr doctor', 'Generate PR evidence',
                     'Restore memoized ripr-plus receipt'] + ALWAYS
        for name in mandatory:
            if name != 'Audit suppression lifecycle':
                self.assertIn(field(self.parts[name], 'continue-on-error'), (None, 'false'), name)
        for name in ['Build xtask', 'Run ripr doctor', 'Generate PR evidence', 'Restore memoized ripr-plus receipt']:
            self.assertIsNone(field(self.parts[name], 'if'), name)
        self.assertEqual(field(self.parts['Run ripr doctor'], 'run'), 'ripr doctor')
        for name in ALWAYS:
            self.assertEqual(field(self.parts[name], 'if'), 'always()', name)
        self.assertEqual(field(self.parts['Audit suppression lifecycle'], 'continue-on-error'), 'true')
        self.assertEqual(field(self.parts['Upload ripr PR evidence'], 'if'),
                         "always() && steps.ripr-freshness-result.outputs.ready == 'true'")
        self.assertEqual(field(self.parts['Save memoized ripr-plus receipt'], 'if'),
                         "success() && hashFiles('target/receipts/quality/.ripr-plus-fresh') != ''")

    def validate(self, token, fail_call=0):
        import json
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            handoff = root / 'handoff'
            handoff.mkdir()
            if token is not None:
                (handoff / 'clear-succeeded').write_text(token)
            cargo = root / 'cargo'
            cargo.write_text('#!/usr/bin/env python3\nimport json,os,sys\nfrom pathlib import Path\np=Path(os.environ["MOCK_CALLS"])\nn=len(p.read_text().splitlines())+1 if p.exists() else 1\nwith p.open("a") as f:f.write(json.dumps(sys.argv[1:])+"\\n")\nsys.exit(37 if n==int(os.environ["MOCK_FAIL_CALL"]) else 0)\n')
            cargo.chmod(0o700)
            env = os.environ.copy()
            env.pop('BASH_ENV', None)
            env.update(PATH=str(root) + os.pathsep + env['PATH'], RIPR_FRESHNESS_HANDOFF=str(handoff),
                       RIPR_FRESHNESS_TOKEN='current-token', BASE_REF='main', PR_HEAD_SHA='fixed-pr-head',
                       RIPR_BASE_SHA='origin/main', RIPR_EVALUATED_SHA='HEAD',
                       PR_LABELS='proof-label', MOCK_CALLS=str(root / 'calls.jsonl'), MOCK_FAIL_CALL=str(fail_call))
            script = bash_run(self.parts['Validate PR evidence contracts'])
            # The subject guard needs a real git checkout, so it cannot run
            # under this mock-cargo harness (temp cwd, no repository). Its
            # behavior is covered by scripts/ci/test_ripr_event_operands.py;
            # here an active guard command line is asserted as workflow
            # text and excluded from the executed script (#18043).
            guard = 'bash scripts/ci/require-ripr-subject.sh'
            lines = script.splitlines()
            self.assertTrue(
                any(line.strip() == guard for line in lines),
                'Validate block must invoke the subject guard as an active command',
            )
            script = '\n'.join(line for line in lines if line.strip() != guard) + '\n'
            result = subprocess.run(['bash', '--noprofile', '--norc', '-e', '-o', 'pipefail', '-c',
                                     script],
                                    cwd=root, env=env, text=True, capture_output=True, timeout=10)
            calls = [json.loads(line) for line in (root / 'calls.jsonl').read_text().splitlines()] if (root / 'calls.jsonl').exists() else []
            return result, calls

    def test_valid_marker_invokes_all_exact_checks(self):
        result, calls = self.validate('current-token')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, CHECKS)

    def test_missing_and_mismatched_marker_refuse_before_cargo(self):
        for token in [None, 'old-token']:
            with self.subTest(token=token):
                result, calls = self.validate(token)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(calls, [])
                self.assertIn('invalidation did not complete', result.stderr)

    def test_nonzero_cargo_check_stops_later_checks(self):
        for number in [1, 3, 6]:
            with self.subTest(number=number):
                result, calls = self.validate('current-token', number)
                self.assertEqual(result.returncode, 37)
                self.assertEqual(calls, CHECKS[:number])


if __name__ == '__main__':
    unittest.main(verbosity=2)
