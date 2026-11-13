"""Compiler-free controls for the actual flake listing/replay boundary.

Synthetic libtest events and fake Cargo prove transport, not native selection.
The workflow and extractor remain the only implementation under test.
"""
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / '.github/workflows/flake-detection.yml').read_text()
SPEC = importlib.util.spec_from_file_location('flake_extractor', ROOT / '.ci/scripts/extract-failed-tests.py')
EXTRACTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXTRACTOR)


def observation(package, name, event='ok'):
    failed = event == 'failed'
    return [json.dumps(row) + '\n' for row in (
        {'type': 'run', 'package': package, 'target': 'lib'},
        {'type': 'suite', 'event': 'started', 'test_count': 1},
        {'type': 'test', 'event': event, 'name': name},
        {'type': 'suite', 'event': 'failed' if failed else 'ok',
         'passed': int(event == 'ok'), 'failed': int(failed), 'ignored': int(event == 'ignored')},
        {'type': 'run_end', 'package': package, 'exit_status': 101 if failed else 0},
    )]


def replay_function(source):
    match = re.search(r'^          run_exact_replay\(\) \{\n.*?^          \}', source, re.M | re.S)
    if match is None:
        raise AssertionError('actual workflow replay function missing')
    return textwrap.dedent(match.group())


class IdentityRoundTripTests(unittest.TestCase):
    def test_failed_listing_to_shell_argv_preserves_full_identities(self):
        selected = [('package-a', 'left::same'), ('package-a', 'right::same'),
                    ('package-b', 'left::same'),
                    ('package-with-hyphen', 'mod::雪 "quoted" \\ path $(false); * [case]')]
        lines = sum((observation(pkg, name, 'failed') for pkg, name in selected), [])
        receipt = EXTRACTOR.build_receipt(lines, 'fixture-head', 'fixture-toolchain')
        self.assertEqual(receipt['verdict'], 'COMPLETE_WITH_FAILURES')
        match = re.search(r"          import json\n          receipt = json.load\(open\(\"flake-receipt.json\"\)\).*?^          PY$", WORKFLOW, re.M | re.S)
        self.assertIsNotNone(match, 'actual workflow listing producer missing')
        listing = textwrap.dedent(match.group().rsplit('\n', 1)[0])
        loop = re.search(r"^          while IFS=.*read -r pkg test_name; do$", WORKFLOW, re.M)
        self.assertIsNotNone(loop, 'actual workflow listing reader missing')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'flake-receipt.json').write_text(json.dumps(receipt))
            result = subprocess.run([sys.executable, '-c', listing], cwd=root, capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            cargo = root / 'cargo'
            cargo.write_text('#!' + sys.executable + '\nimport json, sys\nwith open("argv.jsonl", "a") as f: f.write(json.dumps(sys.argv[1:]) + "\\n")\n')
            cargo.chmod(0o755)
            script = 'set -eu\nFILTER=(--no-default-features --features "alpha,beta")\n' + replay_function(WORKFLOW) + '\n' + loop.group().strip() + '\n run_exact_replay "$pkg" "$test_name" replay.jsonl\ndone < failed-tests.tsv\n'
            result = subprocess.run(['bash', '-c', script], cwd=root,
                                    env=dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH']),
                                    capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            commands = [json.loads(line) for line in (root / 'argv.jsonl').read_text().splitlines()]
            expected = [['test', '-p', pkg, '--lib', '--locked', '--no-default-features', '--features',
                         'alpha,beta', '--', '-Z', 'unstable-options', '--format', 'json', '--exact', name,
                         '--test-threads=1'] for pkg, name in selected]
            self.assertEqual(commands, expected)
            envelope = json.loads((root / 'replay.jsonl').read_text().splitlines()[0])
            self.assertEqual(envelope['command'], ['cargo'] + expected[-1])
            self.assertEqual(envelope['package'], selected[-1][0])

    def test_exact_pass_survives_json_escaping(self):
        for name in ('left::same', 'mod::雪 "quoted" \\ path', 'mod::name:with:colons'):
            with self.subTest(name=name):
                receipt = EXTRACTOR.build_receipt(observation('package-a', name), 'H', 'T', expect='package-a:' + name)
                self.assertEqual(receipt['verdict'], 'COMPLETE_PASS')
                self.assertEqual(receipt['passes'][0]['name'], name)

    def test_duplicate_short_names_do_not_satisfy_exact_replay(self):
        for pkg, name in (('package-b', 'left::same'), ('package-a', 'right::same'), ('package-a', 'same')):
            with self.subTest(package=pkg, name=name):
                receipt = EXTRACTOR.build_receipt(observation(pkg, name), 'H', 'T', expect='package-a:left::same')
                self.assertEqual(receipt['verdict'], 'NOT_PROVEN')
                self.assertNotEqual(receipt['exit_code'], 0)

    def test_ignored_exact_name_is_not_a_passing_replay(self):
        receipt = EXTRACTOR.build_receipt(observation('package-a', 'left::same', 'ignored'), 'H', 'T', expect='package-a:left::same')
        self.assertEqual(receipt['counts']['ignored'], 1)
        self.assertEqual(receipt['passes'], [])
        self.assertEqual(receipt['verdict'], 'NOT_PROVEN')

    def test_repeated_identity_cannot_become_single_replay(self):
        receipt = EXTRACTOR.build_receipt(observation('package-a', 'left::same') * 2, 'H', 'T', expect='package-a:left::same')
        self.assertEqual(len(receipt['passes']), 2)
        self.assertEqual(receipt['verdict'], 'NOT_PROVEN')


if __name__ == '__main__':
    unittest.main()
