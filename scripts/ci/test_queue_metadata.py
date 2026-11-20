"""Executable controls for shared queue metadata compilation and failure evidence."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import importlib.util
import time
import unittest
import yaml

ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts/ci/run_queue_metadata.py"

class QueueMetadata(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-qm", "base"], check=True)
        self.base = subprocess.check_output(["git", "-C", str(self.root), "rev-parse", "HEAD"], text=True).strip()
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-qm", "group"], check=True)
        self.sha = subprocess.check_output(["git", "-C", str(self.root), "rev-parse", "HEAD"], text=True).strip()
        self.event = self.root / "event.json"
        self.event.write_text(json.dumps({"action":"checks_requested", "merge_group":{"head_sha":self.sha, "base_sha":self.base}}))
        self.env = dict(os.environ, PATH=str(self.bin)+os.pathsep+os.environ["PATH"], GITHUB_EVENT_NAME="merge_group", GITHUB_EVENT_PATH=str(self.event), GITHUB_SHA=self.sha, GITHUB_RUN_ID="123", GITHUB_RUN_ATTEMPT="2", GITHUB_REPOSITORY="EffortlessMetrics/perl-lsp-swarm", GITHUB_WORKFLOW_REF="EffortlessMetrics/perl-lsp-swarm/.github/workflows/workflow-trigger-lint.yml@refs/heads/test", GITHUB_WORKFLOW_SHA=self.sha, GITHUB_JOB="queue-metadata", RUNNER_TEMP=str(self.root / "temp"), CALL_LOG=str(self.root / "calls"))
        Path(self.env["RUNNER_TEMP"]).mkdir()
        # The real production wrapper runs these instrumented commands, not a copy.
        fake = """#!/usr/bin/env python3
import json, os, pathlib, sys
name=pathlib.Path(sys.argv[0]).name
args=sys.argv[1:]
with open(os.environ['CALL_LOG'],'a') as f: f.write(json.dumps([name]+args)+'\\n')
if name=='cargo':
    out=pathlib.Path('target/debug'); out.mkdir(parents=True,exist_ok=True)
    for binary in ['xtask','action-pin-provenance']:
        dest=out/binary; dest.write_text(pathlib.Path(sys.argv[0]).read_text()); dest.chmod(0o755)
if '--receipt' in args and os.environ.get('OMIT_RECEIPT') != (args[0] if args else name):
    pathlib.Path(args[args.index('--receipt')+1]).write_text('{}')
key=args[0] if name=='xtask' else name
sys.exit(7 if os.environ.get('FAIL_CHECK')==key else 0)
"""
        for name in ("cargo", "python3"):
            path=self.bin/name
            # Fake python is invoked only for the pin regression; use real python shebang.
            path.write_text(fake.replace("#!/usr/bin/env python3", "#!"+os.sys.executable))
            path.chmod(0o755)

    def run_wrapper(self, **updates):
        self.env.update(updates)
        return subprocess.run([os.sys.executable, str(RUNNER)], cwd=self.root, env=self.env, text=True, capture_output=True, timeout=15)

    def calls(self):
        p=self.root/'calls'
        return [json.loads(x) for x in p.read_text().splitlines()] if p.exists() else []

    def records(self):
        return {p.stem:json.loads(p.read_text()) for p in (Path(self.env['RUNNER_TEMP'])/'queue-metadata'/self.sha/self.env['GITHUB_RUN_ID']/self.env['GITHUB_RUN_ATTEMPT']).glob('*.execution.json')}

    def test_one_build_preserves_every_command_and_bound_record(self):
        r=self.run_wrapper()
        self.assertEqual(r.returncode,0,r.stderr)
        calls=self.calls()
        self.assertEqual(sum(x[0]=='cargo' for x in calls),1)
        self.assertEqual(calls[1],['cargo','build','--locked','-p','xtask','--bin','xtask','--bin','action-pin-provenance'])
        self.assertEqual([x[0:2] for x in calls[2:]], [['xtask','workflow-trigger-lint'],['action-pin-provenance','--base'],['xtask','methodology-gate'],['xtask','check-agent-capabilities']])
        self.assertIn('scripts/ci/test_action_pin_setup_perl_sites_18049.py',calls[0])
        self.assertEqual(calls[3][calls[3].index('--base')+1],self.base)
        self.assertNotEqual(self.base,self.sha)
        self.assertEqual(calls[4][calls[4].index('--fixture')+1],str(self.event))
        records=self.records()
        self.assertEqual(len(records),6)
        for rec in records.values():
            self.assertEqual((rec['sha'],rec['run_id'],rec['run_attempt']),(self.sha,'123','2'))
            self.assertEqual(rec['status'],'success')

    def test_each_failure_stays_red_but_other_checks_execute(self):
        for check in ['python3','workflow-trigger-lint','action-pin-provenance','methodology-gate','check-agent-capabilities']:
            with self.subTest(check=check):
                self.setUp()
                r=self.run_wrapper(FAIL_CHECK=check)
                self.assertNotEqual(r.returncode,0)
                self.assertEqual(len(self.calls()),6)
                self.assertEqual(sum(x['status']=='failure' for x in self.records().values()),1)

    def test_build_failure_cannot_inherit_old_binary_or_success(self):
        r=self.run_wrapper(FAIL_CHECK='cargo')
        self.assertNotEqual(r.returncode,0)
        self.assertEqual(len(self.calls()),2)
        self.assertEqual(sum(x['status']=='not_run' for x in self.records().values()),4)

    def test_success_without_native_receipt_refuses(self):
        r=self.run_wrapper(OMIT_RECEIPT='workflow-trigger-lint')
        self.assertNotEqual(r.returncode,0)
        self.assertEqual(self.records()['workflow-trigger-lint.execution']['status'],'missing_receipt')

    def test_bad_or_missing_identity_refuses_before_commands(self):
        for change in [dict(GITHUB_EVENT_NAME='pull_request'),dict(GITHUB_SHA='f'*40),dict(GITHUB_RUN_ID=''),dict(GITHUB_RUN_ATTEMPT='0'),dict(GITHUB_WORKFLOW_SHA=''),dict(GITHUB_REPOSITORY=''),dict(GITHUB_JOB='')]:
            with self.subTest(change=change):
                self.setUp()
                r=self.run_wrapper(**change)
                self.assertNotEqual(r.returncode,0)
                self.assertEqual(self.calls(),[])

    def test_unrelated_base_refuses_before_commands(self):
        tree=subprocess.check_output(['git','-C',str(self.root),'rev-parse','HEAD^{tree}'],text=True).strip()
        other=subprocess.check_output(['git','-C',str(self.root),'-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit-tree',tree,'-m','unrelated'],text=True).strip()
        event=json.loads(self.event.read_text())
        event['merge_group']['base_sha']=other
        self.event.write_text(json.dumps(event))
        self.assertNotEqual(self.run_wrapper().returncode,0)
        self.assertEqual(self.calls(),[])

    def test_same_attempt_cannot_reuse_previous_receipts(self):
        self.assertEqual(self.run_wrapper().returncode,0)
        self.assertNotEqual(self.run_wrapper().returncode,0)
        self.assertEqual(len(self.calls()),6)

    def test_new_attempt_gets_distinct_records(self):
        self.assertEqual(self.run_wrapper().returncode,0)
        self.assertEqual(self.run_wrapper(GITHUB_RUN_ATTEMPT='3').returncode,0)
        self.assertEqual(len(self.calls()),12)
        self.assertTrue(all(x['run_attempt']=='3' for x in self.records().values()))
        self.assertTrue((Path(self.env['RUNNER_TEMP'])/'queue-metadata'/self.sha/'123'/'2').is_dir())

    def test_timeout_and_missing_program_are_not_success(self):
        spec=importlib.util.spec_from_file_location('queue_metadata_test_subject',RUNNER)
        module=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        self.assertEqual(module.execute(['/no/such/metadata-command'],1,self.env),('instrument_failure',None))
        start=time.monotonic()
        self.assertEqual(module.execute([os.sys.executable,'-c','import time; time.sleep(10)'],0.1,self.env),('timed_out',None))
        self.assertLess(time.monotonic()-start,3)

    def test_workflow_routes_one_shared_queue_job_without_non_rust(self):
        load=lambda name: yaml.safe_load((ROOT/'.github/workflows'/name).read_text())
        w=load('workflow-trigger-lint.yml')
        self.assertEqual(w['jobs']['lint']['if'], "github.event_name != 'merge_group'")
        q=w['jobs']['queue-metadata']
        self.assertEqual(q['if'], "github.event_name == 'merge_group'")
        self.assertEqual(q['runs-on'],'ubuntu-24.04')
        self.assertEqual(q['permissions'],{'contents':'read'})
        text=json.dumps(q)
        self.assertIn('run_queue_metadata.py',text)
        self.assertNotIn('secrets.',text)
        self.assertNotIn('non-rust',text)
        self.assertEqual(load('methodology-gate.yml')['jobs']['methodology-gate']['if'], "github.event_name != 'merge_group'")
        self.assertNotIn("github.event_name == 'merge_group'", load('agent-capability-gate.yml')['jobs']['route-agent-capability-gate']['if'])

if __name__ == '__main__': unittest.main()
