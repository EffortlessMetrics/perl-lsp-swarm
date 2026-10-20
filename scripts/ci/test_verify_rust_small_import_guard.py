"""Behavioral adapter guard tests; fake Cargo does not establish product proof."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
ADAPTER = ROOT / 'scripts/ci/verify_rust_small_import.sh'
TOKENS = ('GH_TOKEN', 'GITHUB_TOKEN', 'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN')


class ImportGuardTests(unittest.TestCase):
    def run_adapter(self, version='1.95.0', result='success', cargo_status=0):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            receipt = root / 'rust-small-proof.json'
            receipt.write_text(json.dumps({'result': result}))
            log = root / 'cargo.log'
            cargo = root / 'cargo'
            cargo.write_text('''#!/usr/bin/env python3
import json, os, sys
with open(os.environ['TEST_CARGO_LOG'], 'a') as out:
    out.write(json.dumps({'argv': sys.argv[1:], 'env': {key: os.environ.get(key) for key in ('GH_TOKEN', 'GITHUB_TOKEN', 'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN', 'RUSTUP_TOOLCHAIN', 'RUSTUP_AUTO_INSTALL', 'CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS')}}) + '\\n')
if sys.argv[1:] == ['--version']:
    print('cargo ' + os.environ['TEST_CARGO_VERSION'])
else:
    sys.exit(int(os.environ['TEST_CARGO_STATUS']))
''')
            cargo.chmod(0o755)
            env = os.environ.copy()
            env.update({key: 'synthetic-policy-token' for key in TOKENS})
            env.update(PATH=str(root) + os.pathsep + env['PATH'],
                       EM_CI_ARTIFACT_DIR=str(root), RUNNER_TEMP=str(root),
                       TEST_CARGO_LOG=str(log), TEST_CARGO_VERSION=version,
                       TEST_CARGO_STATUS=str(cargo_status), RUSTUP_TOOLCHAIN='wrong',
                       RUSTUP_AUTO_INSTALL='1')
            run = subprocess.run(['bash', str(ADAPTER)], cwd=ROOT, env=env,
                                 capture_output=True, text=True, timeout=15)
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return run, calls, str(receipt), str(root)

    def test_stale_cargo_refuses_before_verifier(self):
        run, calls, _, _ = self.run_adapter(version='1.75.0')
        self.assertEqual(run.returncode, 78, run.stderr)
        self.assertIn('cargo-toolchain-guard: REFUSED', run.stderr)
        self.assertEqual([call['argv'] for call in calls], [['--version']])

    def test_probe_and_verifier_share_sanitized_pinned_environment(self):
        run, calls, receipt, root = self.run_adapter()
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual([call['argv'] for call in calls], [
            ['--version'], ['run', '-p', 'xtask', '--locked', '--',
                            'rust-small-proof', '--verify-receipt', receipt]])
        for call in calls:
            for token in TOKENS:
                self.assertIsNone(call['env'][token])
            self.assertEqual(call['env']['RUSTUP_TOOLCHAIN'], '1.95.0')
            self.assertEqual(call['env']['RUSTUP_AUTO_INSTALL'], '0')
            self.assertEqual(call['env']['CARGO_BUILD_JOBS'], '2')
            self.assertEqual(call['env']['CARGO_TARGET_DIR'], root + '/perl-receipt-validator-target')

    def test_diagnostic_receipt_still_fails_policy(self):
        run, calls, _, _ = self.run_adapter(result='product_failure')
        self.assertEqual(run.returncode, 1, run.stderr)
        self.assertIn('diagnostic, not successful proof', run.stderr)
        self.assertEqual(len(calls), 2)

    def test_verifier_failure_is_preserved(self):
        run, calls, _, _ = self.run_adapter(cargo_status=42)
        self.assertEqual(run.returncode, 42, run.stderr)
        self.assertEqual(len(calls), 2)


if __name__ == '__main__':
    unittest.main()
