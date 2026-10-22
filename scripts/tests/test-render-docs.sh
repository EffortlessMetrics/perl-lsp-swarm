#!/usr/bin/env bash
# Exercise the complete production renderer in disposable receipt/doc fixtures.
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
for tool in python3 jq rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "test-render-docs: required test dependency missing: $tool" >&2
    exit 1
  fi
done

python3 - "${SCRIPT_DIR}/../render-docs.sh" <<'PYTHON'
import json
import os
from pathlib import Path
import sys
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(sys.argv[1]).read_bytes()

class RenderStatus(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='render docs status ')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for directory in ('scripts', 'docs', 'artifacts', 'bin'):
            (self.root/directory).mkdir()
        self.script = self.root/'scripts/render-docs.sh'
        self.script.write_bytes(SOURCE)
        self.state = self.root/'artifacts/state.json'
        self.state.write_text(json.dumps({'version':'1.2.3','tests':{'passed':7,'failed':2,'ignored':1,'active_tests':9,'total_all_tests':10,'pass_rate_active':'77.8','pass_rate_total':'70.0'},'docs':{'missing_docs':3}}))
        self.doc = self.root/'docs/example.md'
        self.doc.write_text('{{version}} {{test_passed}} {{tests.passed}} {{test_failed}} {{tests.failed}} {{test_ignored}} {{tests.ignored}} {{test_active}} {{tests.active_tests}} {{test_total}} {{tests.total_all_tests}} {{pass_rate_active}} {{tests.pass_rate_active}} {{pass_rate_total}} {{tests.pass_rate_total}} {{missing_docs}} {{docs.missing_docs}}\n')
        self.env = dict(os.environ)
        self.env['PATH'] = str(self.root/'bin') + ':' + self.env['PATH']

    def stub(self, name, text):
        tool = self.root/'bin'/name
        tool.write_text('#!/bin/sh\n'+text+'\n')
        tool.chmod(0o755)

    def run_renderer(self, caller=None):
        command = [shutil.which('bash'),str(self.script)] if caller is None else [shutil.which('bash'),'-c',caller,'_',str(self.script)]
        return subprocess.run(command, env=self.env, cwd=self.root, text=True, capture_output=True, timeout=5)

    def assert_failure(self, result, code=None):
        self.assertNotEqual(result.returncode, 0, result.stdout+result.stderr)
        if code is not None:
            self.assertEqual(result.returncode, code)
        self.assertNotIn('All tokens resolved successfully', result.stdout)
        self.assertNotIn('Documentation Rendering Complete', result.stdout)

    def test_valid_all_token_forms(self):
        result = self.run_renderer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('All tokens resolved successfully', result.stdout)
        self.assertEqual((self.root/'tmp/docs/example.md').read_text(), '1.2.3 7 7 2 2 1 1 9 9 10 10 77.8 77.8 70.0 70.0 3 3\n')

    def test_real_unknown_token(self):
        self.doc.write_text('{{unknown_schema_token}}\n')
        self.assert_failure(self.run_renderer(), 1)

    def test_silent_validator_error(self):
        self.stub('rg','echo validator-failure >&2\nexit 2')
        result = self.run_renderer()
        self.assert_failure(result, 2)
        self.assertIn('validator-failure',result.stderr)

    def test_partial_output_validator_error(self):
        self.stub('rg','echo partial-output\necho validator-failure >&2\nexit 2')
        self.assert_failure(self.run_renderer(),2)

    def test_nonstandard_validator_error(self):
        self.stub('rg','exit 17')
        self.assert_failure(self.run_renderer(),17)

    def test_missing_validator_preserves_warning_policy(self):
        for command in ('dirname','jq','sed','mkdir','find','basename','grep'):
            (self.root/'bin'/command).symlink_to(shutil.which(command))
        self.env['PATH']=str(self.root/'bin')
        result=self.run_renderer()
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('=== Validating Token Resolution ===',result.stdout)
        self.assertIn('Warning: ripgrep not found, skipping token validation',result.stdout)
        self.assertNotIn('All tokens resolved successfully',result.stdout)

    def test_root_markdown_valid(self):
        (self.root/'README.md').write_text('Root version {{version}}\n')
        result=self.run_renderer()
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual((self.root/'tmp/README.md').read_text(),'Root version 1.2.3\n')

    def test_root_markdown_unresolved(self):
        (self.root/'README.md').write_text('{{unknown_root_schema_token}}\n')
        self.assert_failure(self.run_renderer(),1)

    def test_missing_input(self):
        self.state.unlink()
        self.assert_failure(self.run_renderer(),1)

    def test_malformed_input(self):
        self.state.write_text('{bad json')
        result=self.run_renderer()
        self.assert_failure(result)
        self.assertIn('parse error',result.stderr)

    def test_input_subprocess_status(self):
        self.stub('jq','exit 17')
        self.assert_failure(self.run_renderer(),17)

    def test_find_pipeline_status(self):
        self.stub('find','exit 19')
        self.assert_failure(self.run_renderer(),19)

    def test_render_subprocess_status(self):
        self.stub('sed','exit 23')
        self.assert_failure(self.run_renderer(),23)

    def test_command_substitution_status(self):
        self.stub('rg','exit 17')
        result=self.run_renderer('output=$(bash "$1"); status=$?; printf "%s\\n" "$output"; exit "$status"')
        self.assert_failure(result,17)

    def test_pipefail_caller_status(self):
        self.stub('rg','exit 17')
        self.assert_failure(self.run_renderer('set -o pipefail; bash "$1" | cat; exit "$?"'),17)

    def test_caller_without_pipefail_retains_producer_status(self):
        self.stub('rg','exit 17')
        result=self.run_renderer('set +o pipefail; bash "$1" | cat; status=("${PIPESTATUS[@]}"); printf "PRODUCER=%s CONSUMER=%s\\n" "${status[0]}" "${status[1]}"; exit "${status[1]}"')
        self.assertEqual(result.returncode,0)
        self.assertIn('PRODUCER=17 CONSUMER=0',result.stdout)
        self.assertNotIn('All tokens resolved successfully',result.stdout)

if __name__ == '__main__':
    unittest.main(argv=[sys.argv[0]], verbosity=2)

PYTHON
