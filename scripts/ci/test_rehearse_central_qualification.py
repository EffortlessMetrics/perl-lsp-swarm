"""Actual-mode process/order controls; executable and central API are synthetic.

The separately runnable cross-repository rehearsal uses the actual embedded
central code. These compiler-free tests do not claim canonical Rust execution.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

from scripts.ci import rehearse_central_qualification as rehearsal
from scripts.ci import candidate_readiness as admission


class FixtureCentral:
    """Explicit central boundary stub; no copied catalog, parser or verifier."""
    def __init__(self):
        owner = self
        class Helper:
            CAPTURE = {'capture_context': owner.capture}
            def fixture(self, directory):
                owner.directory = Path(directory)
                owner.packet = SimpleNamespace(plan={}, name='fixture-native-attachment',
                    subject=dict(repository='EffortlessMetrics/perl-lsp-swarm', sha='a' * 40,
                        run_id='123', run_attempt='2', profile='standard', event='workflow_dispatch',
                        workflow='fixture', workflow_ref='fixture/main', ref='refs/heads/main'),
                    run={'repository': {}, 'head_repository': {}}, producer={'steps': [{}]},
                    check={}, artifact={'workflow_run': {}}, evidence={},
                    needs={'standard': {'outputs': {}}}, context={}, write_zip=lambda: None)
                return owner.packet
            def environment(self, fixture):
                env = {'GITHUB_' + key.upper(): value for key, value in fixture.subject.items()
                       if key not in {'profile', 'event'}}
                env.update(GITHUB_EVENT_NAME='workflow_dispatch', GITHUB_JOB='standard',
                    EM_CI_SELECTED_PROFILE='standard', EM_CI_SELF_HOSTED='true')
                return env
            def imported(self, fixture, acquire_plan):
                owner.imports += 1
                assert acquire_plan() == fixture.plan
                target = owner.directory / 'imported'
                target.mkdir(mode=0o700)
                for name, raw in fixture.files.items(): (target / name).write_bytes(raw)
                (target / 'em-ci-job-context.json').write_text(json.dumps(fixture.context))
                return target
            def custody(self, *args): pass
        self.QualificationSubjectHooks = Helper
        self.imports = 0

    def capture(self, environment, *, acquire_plan):
        plan = acquire_plan()
        return dict(schema='em-ci-job-context.v1', repository=plan['repository'], sha=plan['control'],
            run_id=environment['GITHUB_RUN_ID'], run_attempt=environment['GITHUB_RUN_ATTEMPT'],
            profile='standard', job='standard', event='workflow_dispatch', ref=environment['GITHUB_REF'],
            workflow=environment['GITHUB_WORKFLOW'], workflow_ref=environment['GITHUB_WORKFLOW_REF'],
            admission_recorded_at=rehearsal.stamp(),
            qualification_subject={key: plan[key] for key in ('head', 'tree', 'base', 'control', 'request_key')})


class ActualRehearsalTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.workspace = self.root / 'H'
        self.workspace.mkdir()
        def git(*args):
            return subprocess.run(['git', '-C', str(self.workspace), *args],
                capture_output=True, text=True, check=True).stdout.strip()
        git('init', '-q'); git('config', 'user.name', 'Fixture'); git('config', 'user.email', 'fixture@example.invalid')
        (self.workspace / '.gitignore').write_text('target/\n')
        git('add', '.gitignore'); git('commit', '-qm', 'base')
        (self.workspace / 'source').write_text('candidate')
        git('add', 'source'); git('commit', '-qm', 'H')
        self.head = git('rev-parse', 'HEAD')
        self.warm = self.root / 'warm.json'
        self.receipt = {'schema_version': 'rust_small_proof.v1',
            'subject': {'git_sha': self.head, 'worktree_dirty': False}, 'result': 'success',
            'steps': [{'name': name, 'outcome': 'ok'} for name in ('owner-first', 'owner-second')]}
        self.warm.write_text(json.dumps(self.receipt))
        self.tool = self.root / 'xtask'
        self.trace = self.root / 'trace.jsonl'
        self.write_tool()
        self.central = FixtureCentral()
        self.output = self.root / 'output'
        self.control_head, self.control_dirty = 'a' * 40, False
        real_run = subprocess.run
        def control_read(argv, **kwargs):
            if argv[:4] == ['git', '-c', 'core.fsmonitor=false', 'rev-parse']:
                return subprocess.CompletedProcess(argv, 0, stdout=self.control_head + '\n', stderr='')
            if argv[:4] == ['git', '-c', 'core.fsmonitor=false', 'status']:
                return subprocess.CompletedProcess(argv, 0, stdout=' M changed\n' if self.control_dirty else '', stderr='')
            return real_run(argv, **kwargs)
        self.control_patch = mock.patch.object(admission.subprocess, 'run', side_effect=control_read)
        self.control_patch.start()
        self.addCleanup(self.control_patch.stop)

    def write_tool(self, *, verify_status=0, producer_status=0):
        receipt = {**self.receipt, 'result': 'success' if producer_status == 0 else 'product_failure'}
        self.tool.write_text('#!' + sys.executable + '\nimport os,json,sys\nfrom pathlib import Path\n'
            f'with Path({str(self.trace)!r}).open("a") as out: out.write(json.dumps({{"argv":sys.argv[1:],"cwd":os.getcwd(),"pythonpath":os.environ.get("PYTHONPATH"),"target":os.environ.get("CARGO_TARGET_DIR")}})+"\\n")\n'
            f'if "--verify-receipt" in sys.argv: raise SystemExit({verify_status})\n'
            'p=Path("target/receipts/rust-small-proof.json"); p.parent.mkdir(parents=True,exist_ok=True)\n'
            f'p.write_text({json.dumps(receipt)!r})\nraise SystemExit({producer_status})\n')
        self.tool.chmod(0o755)
        self.digest = hashlib.sha256(self.tool.read_bytes()).hexdigest()

    def run_actual(self, **updates):
        args = dict(xtask=self.tool, xtask_sha256=self.digest, control_sha='a' * 40,
                    workspace=self.workspace, warm_receipt=self.warm, output=self.output)
        args.update(updates)
        return rehearsal.run_actual(self.central, **args)

    def test_warm_verification_precedes_producer_then_current_run_verification(self):
        with mock.patch.dict(os.environ, PYTHONPATH='/control/S', CARGO_TARGET_DIR='/admitted/H-target'):
            report = self.run_actual()
        rows = [json.loads(line) for line in self.trace.read_text().splitlines()]
        producer_index = next(i for i, row in enumerate(rows) if row['argv'] ==
                              ['--worktree-root', str(self.workspace), 'rust-small-proof'])
        self.assertEqual(producer_index, 2)  # Detailed entry + actual Adapter.explain.
        self.assertTrue(all('--verify-receipt' in row['argv'] for row in rows[:producer_index]))
        self.assertIn('--verify-receipt', rows[-1]['argv'])
        self.assertNotEqual(rows[0]['argv'][-1], rows[-1]['argv'][-1])
        self.assertIsNone(rows[producer_index]['pythonpath'])
        self.assertEqual(rows[producer_index]['target'], '/admitted/H-target')
        self.assertEqual(rows[producer_index]['cwd'], str(self.workspace))
        self.assertEqual(report['qualification_result'], 'NOT_PROVEN')
        self.assertIsNone(report['check_run_id'])
        self.assertEqual(report['verified_qualification']['selected'], 2)
        self.assertEqual(self.central.imports, 1)
        self.assertEqual((self.output / 'warm/rust-small-proof.json').read_bytes(), self.warm.read_bytes())
        entry = json.loads((self.output / 'entry.json').read_bytes())
        self.assertEqual(entry['acquisition_mode'], 'local-fixture-reviews-and-native-api')
        self.assertFalse(entry['execution_authorized'])
        self.assertEqual(entry['adapter_explain']['status'], 'READY')
        self.assertEqual(entry['adapter_explain']['qualification_result'], 'NOT_PROVEN')

    def test_failed_warm_semantics_never_executes_producer_or_import(self):
        self.write_tool(verify_status=1)
        with self.assertRaisesRegex(ValueError, 'warm proof'):
            self.run_actual()
        rows = [json.loads(line) for line in self.trace.read_text().splitlines()]
        self.assertEqual(len(rows), 2)
        self.assertTrue(all('--verify-receipt' in row['argv'] for row in rows))
        self.assertEqual(self.central.imports, 0)
        self.assertFalse((self.output / 'qualification.log').exists())

    def test_actual_failure_is_retained_without_fabricating_successful_import(self):
        self.write_tool(producer_status=7)
        report = self.run_actual()
        self.assertEqual(report['producer_exit_status'], 7)
        self.assertEqual(report['qualification_result'], 'NOT_PROVEN')
        self.assertEqual(self.central.imports, 0)
        self.assertEqual(json.loads((self.output / 'rust-small-proof.json').read_bytes())['result'], 'product_failure')

    def test_wrong_or_dirty_control_refuses_at_existing_adapter_boundary(self):
        for head, dirty in (('b' * 40, False), ('a' * 40, True)):
            self.control_head, self.control_dirty = head, dirty
            with self.subTest(head=head, dirty=dirty), self.assertRaisesRegex(ValueError, 'adapter boundary'):
                self.run_actual()
            self.assertEqual(self.central.imports, 0)
            self.assertFalse((self.output / 'qualification.log').exists())
            import shutil
            shutil.rmtree(self.output)

    def test_tool_digest_and_dirty_checkout_refuse_before_producer(self):
        with self.assertRaisesRegex(ValueError, 'installer identity'):
            self.run_actual(xtask_sha256='0' * 64)
        (self.workspace / 'source').write_text('dirty')
        with self.assertRaisesRegex(ValueError, 'clean H'):
            self.run_actual()
        self.assertFalse(self.trace.exists())


if __name__ == '__main__': unittest.main()
