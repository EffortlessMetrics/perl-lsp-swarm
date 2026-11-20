"""CLI path controls; Linux execution is not native Windows qualification."""
from pathlib import Path
import json
import os
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(os.environ.get('RECEIPT_PATH_SUBJECT', str(Path(__file__).with_name('verify_gate_receipt_freshness.py')))).resolve()
SUBJECT = 'a' * 40


class ReceiptFreshnessPathTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="receipt paths ' λ ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.directories = [self.root / name for name in
                            ("receipt space", "résumé λ", "extra ' one", "extra 二 two")]
        for directory in self.directories:
            directory.mkdir()
        self.paths = [directory / "résumé ' 二.json" for directory in self.directories]
        for index in range(4):
            self.write(index)

    def write(self, index, sha=SUBJECT):
        payload = ({'schema_version': 'ci_gate_shard.v1', 'subject_sha': sha}
                   if index == 1 else {'metadata': {'git_sha': sha}})
        self.paths[index].write_text(json.dumps(payload), encoding='utf-8')

    def run_cli(self, relative=False):
        directories = [path.relative_to(self.root) if relative else path
                       for path in self.directories]
        args = [sys.executable, str(SCRIPT), '--subject-sha', SUBJECT,
                '--receipt-dir', str(directories[0]), '--summary-dir', str(directories[1])]
        for directory in directories[2:]:
            args.extend(['--extra-dir', str(directory)])
        return subprocess.run(args, cwd=self.root, capture_output=True,
                              encoding='utf-8', timeout=10, check=False,
                              env={**os.environ, 'PYTHONIOENCODING': 'utf-8'})

    def test_fresh_absolute_paths(self):
        result = self.run_cli()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')

    def test_fresh_relative_paths(self):
        result = self.run_cli(relative=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')

    def test_each_directory_refuses_stale_then_recovers(self):
        for relative in (False, True):
            for index in range(4):
                with self.subTest(relative=relative, directory=index):
                    self.write(index, 'b' * 40)
                    result = self.run_cli(relative)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    expected = self.paths[index].relative_to(self.root) if relative else self.paths[index]
                    self.assertIn(str(expected), result.stderr)
                    self.assertIn('binds subject', result.stderr)
                    self.assertIn('1 unbound or foreign receipt', result.stderr)
                    self.write(index)
                    recovered = self.run_cli(relative)
                    self.assertEqual(recovered.returncode, 0, recovered.stderr)

    def test_each_directory_reports_unreadable_json(self):
        for index in range(4):
            with self.subTest(directory=index):
                self.paths[index].write_bytes(b'\xff')
                result = self.run_cli()
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn(str(self.paths[index]), result.stderr)
                self.assertIn('unreadable:', result.stderr)
                self.write(index)

    def test_all_four_paths_are_reported(self):
        for index in range(4):
            self.write(index, 'b' * 40)
        result = self.run_cli()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('4 unbound or foreign receipt', result.stderr)
        for path in self.paths:
            self.assertIn(str(path), result.stderr)


if __name__ == '__main__':
    unittest.main()
