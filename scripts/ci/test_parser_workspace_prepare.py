"""Consumer/production-owner composition controls; no product compilation."""
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

prepare = load('prepare', ROOT / 'scripts/ci/parser_workspace_prepare.py')
fixture = load('owner_fixture', ROOT / 'scripts/tests/test_cargo_admitted_nested.py')
EXPECTED = {
    'parser': ('parser-check', 'parser-build', 'parser-clippy', 'parser-lib'),
    'dap': ('dap-lsp-build', 'dap-core-build', 'dap-bin-build', 'dap-clippy'),
}

class PreparationControls(unittest.TestCase):
    def setUp(self):
        # Reuse the existing owner's exact-root/lease/tool fixture, not another
        # validator. Only source/config/tool observations are mocked there;
        # membership, snapshot equality, identities and argv rendering are real.
        self.subject = fixture.NestedTests()
        self.subject.setUp()
        self.addCleanup(self.subject.doCleanups)
        self.enterContext(patch.object(prepare, 'nested_command', fixture.a.nested_command))
        self.enterContext(patch('sys.stdout', io.StringIO()))
        self.enterContext(patch('sys.stderr', io.StringIO()))

    def run_mode(self, mode='parser', invoke=None, env=None):
        return prepare.run(self.subject.env if env is None else env,
                           invoke or (lambda args, **kw: subprocess.CompletedProcess(args, 0, '', '')), mode)

    def test_both_modes_use_exact_owner_rows_and_lints(self):
        for mode, rows in EXPECTED.items():
            calls = []
            def invoke(args, **kwargs):
                calls.append((args, kwargs))
                return subprocess.CompletedProcess(args, 0, '', '')
            self.assertEqual(self.run_mode(mode, invoke), 0)
            self.assertEqual(len(calls), 4)
            for (cmd, kw), row in zip(calls, rows):
                expected, env, cwd = self.subject.command(row)
                self.assertEqual(cmd, expected)
                self.assertEqual(kw['env'], env)
                self.assertEqual(kw['cwd'], cwd)
                self.assertEqual(kw['encoding'], 'utf-8')
                self.assertEqual(kw['errors'], 'replace')
            parser_lint = list(fixture.a.NESTED_COMMANDS['parser-clippy'])
            dap_lint = list(fixture.a.NESTED_COMMANDS['dap-clippy'])
            self.assertEqual(parser_lint, ['clippy','--package','perl-parser','--locked','--offline','--','-D','warnings'])
            self.assertEqual(dap_lint, ['clippy','-p','perl-dap','--lib','--locked','--','-D','warnings','-A','clippy::wildcard_imports'])

    def test_every_failure_stops_later_stages(self):
        for mode in EXPECTED:
            for failed in range(4):
                calls = []
                def invoke(args, **kw):
                    calls.append(args)
                    return subprocess.CompletedProcess(args, 101 if len(calls)==failed+1 else 0, '', 'failure')
                self.assertEqual(self.run_mode(mode, invoke), 101)
                self.assertEqual(len(calls), failed+1)

    def test_missing_plan_cannot_launch(self):
        with self.assertRaises(fixture.a.Denied):
            self.run_mode(invoke=lambda *a, **kw: self.fail('must refuse before launch'), env={})

    def test_absent_row_stops_before_any_child(self):
        self.subject.plan['request']['rows'].remove('parser-clippy')
        self.subject.save()
        calls=[]
        def invoke(args, **kw):
            calls.append(args)
            return subprocess.CompletedProcess(args,0,'','')
        with self.assertRaises(fixture.a.Denied):
            self.run_mode(invoke=invoke)
        self.assertEqual(len(calls),0)

    def test_original_marker_replacement_cannot_launch(self):
        self.subject.marker.rename(self.subject.marker.with_name('retained-original'))
        self.subject.marker.mkdir()
        with self.assertRaises(fixture.a.Denied):
            self.run_mode(invoke=lambda *a, **kw: self.fail('must refuse before launch'))

    def test_ownership_loss_is_checked_after_each_success(self):
        # Losing ownership after the LAST command must also refuse; otherwise
        # deleting the post-validation survives ordinary next-stage controls.
        for failed in (0, 3):
            with self.subTest(stage=failed):
                calls=[]
                original=self.subject.marker.with_name('retained-original')
                def invoke(args, **kw):
                    calls.append(args)
                    if len(calls)==failed+1:
                        self.subject.marker.rename(original)
                        self.subject.marker.mkdir()
                    return subprocess.CompletedProcess(args,0,'','')
                try:
                    with self.assertRaises(fixture.a.Denied):
                        self.run_mode(invoke=invoke)
                    self.assertEqual(len(calls),failed+1)
                finally:
                    self.subject.marker.rmdir()
                    original.rename(self.subject.marker)

    def test_source_environment_and_sibling_roots_refuse_before_launch(self):
        for key in ('CARGO_TARGET_DIR','CARGO_BUILD_BUILD_DIR','CARGO_PROFILE_DEV_DEBUG'):
            with self.subTest(key=key), self.assertRaises(fixture.a.Denied):
                self.run_mode(env=self.subject.env | {key:'changed'}, invoke=lambda *a, **kw:self.fail('must refuse'))
        self.subject.plan['source']={'head':'other-source'}
        self.subject.save()
        with self.assertRaises(fixture.a.Denied):
            self.run_mode(invoke=lambda *a, **kw:self.fail('must refuse'))

    def test_parser_checks_keep_original_diagnostics_and_warning_anchor(self):
        for diagnostic in ('cannot find value `signature`','failed to resolve: could not find `tower_lsp`'):
            self.assertEqual(self.run_mode(invoke=lambda args,**kw: subprocess.CompletedProcess(args,0,diagnostic,'')),1)
        calls=[]
        def invoke(args, **kw):
            calls.append(args)
            return subprocess.CompletedProcess(args,0,'','warning: issue\n --> crates\\perl-parser\\src\\lib.rs:1\n' if len(calls)==2 else '')
        self.assertEqual(self.run_mode(invoke=invoke),1)
        self.assertEqual(len(calls),2)
        self.assertFalse(prepare.warnings_anchored_in_parser('warning: issue\n --> crates/other/src/lib.rs:1'))

    def test_signal_status_is_failure(self):
        self.assertEqual(self.run_mode(invoke=lambda args,**kw:subprocess.CompletedProcess(args,-15,'','')),1)

    def test_required_wiring_and_nonzero_guard_preserved(self):
        runner=(ROOT/'xtask/src/tasks/gates.rs').read_text()
        self.assertIn('routed_preparation::RUNTIME | routed_preparation::PARSER',runner)
        self.assertIn('routed_runtime_evidence::validate',runner)
        policy=(ROOT/'.ci/gate-policy.yaml').read_text()
        dap=policy.split('  - name: dap_workspace_prepare\n',1)[1].split('  - name:',1)[0]
        for field in ('command: python scripts/ci/parser_workspace_prepare.py --dap','required: true','timeout_seconds: 1500','retry_count: 0'):
            self.assertIn(field,dap)

if __name__ == '__main__':
    unittest.main()
