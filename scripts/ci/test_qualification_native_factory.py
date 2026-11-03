"""Native source factory joins with explicit API/review/tool/allocation fixtures."""
from contextlib import contextmanager
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from scripts.ci import qualification_native_factory as factory
from scripts.ci import test_qualification_runtime as fixtures
from scripts.ci.rust_small_canonical_owner import CanonicalOwner


class CheckoutTests(unittest.TestCase):
    def setUp(self):
        self.temporary=tempfile.TemporaryDirectory();self.addCleanup(self.temporary.cleanup)
        self.root=Path(self.temporary.name);self.control=self.root/'S';self.control.mkdir()
        self.workspace=self.root/'H'
        def git(*args):
            return subprocess.run(['git','-C',str(self.control),*args],capture_output=True,text=True,check=True).stdout.strip()
        self.git=git
        git('init','-q');git('config','user.name','Fixture');git('config','user.email','fixture@example.invalid')
        (self.control/'source').write_text('H');git('add','source');git('commit','-qm','H')
        self.head=git('rev-parse','HEAD');self.tree=git('rev-parse','HEAD^{tree}')
        (self.control/'source').write_text('S');git('commit','-qam','S')
        self.plan=dict(head=self.head,tree=self.tree,control=git('rev-parse','HEAD'),base=git('rev-parse','HEAD'))
        self.adapter=mock.Mock();self.adapter.snapshot.return_value=self.plan
        self.environment=dict(GITHUB_SHA=self.plan['control'])
        self.patch=mock.patch.object(factory,'SOURCE_ROOT',self.control);self.patch.start();self.addCleanup(self.patch.stop)

    def checkout(self):
        return factory.checkout_candidate(self.adapter,self.environment,control_root=self.control,workspace=self.workspace)

    def test_actual_detached_checkout_uses_H_and_preserves_clean_S_without_running_hook(self):
        hook=self.root/'hooks';hook.mkdir();marker=self.root/'hook-ran'
        (hook/'post-checkout').write_text('#!/bin/sh\ntouch '+str(marker)+'\n');(hook/'post-checkout').chmod(0o755)
        self.git('config','core.hooksPath',str(hook))
        self.assertEqual(self.checkout(),self.workspace)
        self.assertEqual((self.workspace/'source').read_text(),'H')
        self.assertEqual((self.control/'source').read_text(),'S')
        self.assertFalse(marker.exists())
        self.assertEqual(self.checkout(),self.workspace)  # Existing exact checkout is retained.
        (self.workspace/'source').write_text('changed')
        with self.assertRaises(ValueError):self.checkout()

    def test_candidate_attributes_cannot_run_configured_filters_before_allocation(self):
        # Add attributes to H and create a later control S. Smudge executes on
        # ordinary checkout; clean executes during status. Neither may run here.
        self.git('checkout','-q',self.head)
        (self.control/'.gitattributes').write_text('source filter=fixture\n')
        self.git('add','.gitattributes');self.git('commit','-qm','H attributes')
        self.plan.update(head=self.git('rev-parse','HEAD'),tree=self.git('rev-parse','HEAD^{tree}'))
        (self.control/'control-only').write_text('S')
        self.git('add','control-only');self.git('commit','-qm','S after attributes')
        self.plan['control']=self.git('rev-parse','HEAD');self.environment['GITHUB_SHA']=self.plan['control']
        marker=self.root/'filter-ran'
        self.git('config','filter.fixture.smudge','touch '+str(marker)+'; cat')
        self.git('config','filter.fixture.clean','touch '+str(marker)+'; cat')
        self.git('config','filter.fixture.required','true')
        self.checkout()
        self.assertEqual((self.workspace/'source').read_text(),'H')
        self.assertFalse(marker.exists())
        self.checkout();self.assertFalse(marker.exists())

    def test_shallow_control_fetches_missing_exact_H_without_moving_control_branch(self):
        # Remote H is a sibling/descendant absent from a single-branch shallow S.
        remote=self.root/'remote.git'
        subprocess.run(['git','clone','-q','--bare',str(self.control),str(remote)],check=True)
        (self.control/'source').write_text('new candidate')
        self.git('commit','-qam','remote H')
        head=self.git('rev-parse','HEAD');tree=self.git('rev-parse','HEAD^{tree}')
        self.git('push','-q',str(remote),'HEAD:refs/heads/candidate')
        self.git('branch','control',self.plan['control'])
        self.git('push','-q',str(remote),'control')
        shallow=self.root/'shallow'
        subprocess.run(['git','clone','-q','--depth','1','--single-branch','--branch','control',remote.as_uri(),str(shallow)],check=True)
        self.control=shallow
        self.plan.update(head=head,tree=tree)
        with mock.patch.object(factory,'SOURCE_ROOT',shallow):self.checkout()
        self.assertEqual((self.workspace/'source').read_text(),'new candidate')
        self.assertEqual(self.git('rev-parse','HEAD'),self.plan['control'])
        self.assertEqual(self.git('rev-parse','--is-shallow-repository'),'false')

    def test_changed_control_and_linked_checkout_refuse_before_product(self):
        (self.control/'source').write_text('dirty')
        with self.assertRaises(ValueError):self.checkout()
        self.assertFalse(self.workspace.exists())
        (self.control/'source').write_text('S')
        self.workspace.symlink_to(self.control,target_is_directory=True)
        with self.assertRaises(ValueError):self.checkout()

    def test_compose_binds_existing_owners_and_points_product_environment_at_H(self):
        tool=mock.Mock(workspace=self.workspace)
        owners=factory.compose_native(self.adapter,self.environment,canonical_owner=tool,
            control_root=self.control,native_warm_import=lambda *a,**k:None,warm_run_id=122,
            warm_run_attempt=1,native_import=lambda **k:None)
        self.assertEqual(owners.environment['GITHUB_WORKSPACE'],str(self.workspace))
        self.assertEqual(owners.selection_owner.__self__.canonical,tool)
        self.assertEqual(owners.entry_committed_owner.__self__,owners.selection_owner.__self__)
        self.assertEqual(self.adapter.eligibility,owners.eligibility)


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        self.fixture=fixtures.RuntimeCompositionTests();self.fixture.setUp();self.addCleanup(self.fixture.doCleanups)
        self.fixture.fresh_producer_packet()
        self.environment=self.fixture.owners.environment
        self.context=Path(self.environment['RUNNER_TEMP'])/'em-ci-job-context.json';self.context.unlink()
        self.root=Path(self.environment['RUNNER_TEMP']);self.log=self.root/'actual-producer.log'
        self.tool=self.root/'installed-xtask'
        self.tool.write_text('#!'+sys.executable+'\nimport os,sys,json\nfrom pathlib import Path\n'
            'assert "GITHUB_TOKEN" not in os.environ\nassert "GIT_DIR" not in os.environ\n'
            f'assert sys.argv[1:]==["--worktree-root",{self.environment["GITHUB_WORKSPACE"]!r},"rust-small-proof"]\n'
            f'Path("target/receipts/rust-small-proof.json").write_text({json.dumps(self.fixture.bridge.receipt)!r})\n')
        self.tool.chmod(0o755)
        self.fixture.owners.canonical_owner=CanonicalOwner(verifier=self.tool,
            verifier_sha256=hashlib.sha256(self.tool.read_bytes()).hexdigest(),
            control_sha=self.fixture.plan['control'],workspace=Path(self.environment['GITHUB_WORKSPACE']))
        self.environment.update(GITHUB_TOKEN='metadata-only',GIT_DIR='/wrong')
        self.trace=[]
        self.patch=mock.patch.object(fixtures.admission,'production',return_value={'classification':'READY'})
        self.patch.start();self.addCleanup(self.patch.stop)

    def capture(self,environment,*,acquire_plan):
        self.trace.append('capture')
        context=dict(self.fixture.bridge.context,product_workspace=environment['GITHUB_WORKSPACE'])
        context.pop('proof_started_at',None)
        self.assertEqual(acquire_plan(),self.fixture.plan)
        return context

    @contextmanager
    def allocation(self,plan,selection):
        self.trace.append('allocation')
        self.assertFalse(self.context.exists() and 'proof_started_at' in json.loads(self.context.read_bytes()))
        try:yield
        finally:self.trace.append('release')

    def run_native(self,**kwargs):
        return factory.execute_native(self.fixture.owners,{'inputs':{'request':json.dumps(self.fixture.plan)}},
            self.capture,allocation_owner=self.allocation,log_path=self.log,**kwargs)

    def test_native_entry_capture_real_process_private_export_and_lifetime(self):
        status,manifest=self.run_native()
        self.assertEqual(status,0);self.assertEqual(manifest['result'],'success')
        self.assertEqual(self.trace,['capture','allocation','release'])
        self.assertIn('proof_started_at',json.loads(self.context.read_bytes()))
        self.assertTrue(self.log.exists())
        self.assertEqual(json.loads((self.fixture.packet/'rust-small-proof.json').read_bytes())['subject']['git_sha'],self.fixture.plan['head'])

    def install_local_composition_fixture(self, *, status=0):
        """Real subprocess ports; planner/command/Rust semantics remain synthetic.

        This proves factory composition and byte custody, not Cargo execution,
        native import authentication, runner allocation or App publication.
        """
        workspace = Path(self.environment['GITHUB_WORKSPACE'])
        plan = self.fixture.plan
        output = self.root / 'local-proof'
        calls = self.root / 'installed-owner-calls.jsonl'
        prepush = dict(schema='pre_push_proof_plan.v1', posture='PROCEED',
            base_sha=plan['base'], head_sha=plan['head'],
            selected=[{'command': 'cargo check -p perl-parser --locked'}])
        # Deliberate formatting makes reserialization detectable at export.
        raw = (json.dumps(dict(schema_version='command-proof-set.v1',
            result='SUCCESS' if status == 0 else 'FAILURE',
            commands=[dict(id='pre-push-0', receipt=dict(
                schema_version='command-evidence.v1',
                argv=['cargo', 'check', '-p', 'perl-parser', '--locked'],
                cwd=str(workspace), candidate_identity=plan['head'],
                result='SUCCESS' if status == 0 else 'FAILURE', exit_code=status,
                termination=f'exit_code:{status}'))]), indent=2) + '\n').encode()
        self.tool.write_text('#!' + sys.executable + '\n' +
            'import json,os,sys\nfrom pathlib import Path\n' +
            f'workspace=Path({str(workspace)!r})\n' +
            f'calls=Path({str(calls)!r})\n' +
            f'output=Path({str(output)!r})\n' +
            f'plan={plan!r}\n' +
            'assert sys.argv[1:3]==["--worktree-root",str(workspace)]\n' +
            'assert Path.cwd()==workspace\n' +
            'assert not any(k.startswith("GIT_") or k in ' +
            '{"GH_TOKEN","GITHUB_TOKEN","GH_ENTERPRISE_TOKEN","GITHUB_ENTERPRISE_TOKEN"} for k in os.environ)\n' +
            'assert os.environ["RUSTUP_TOOLCHAIN"]=="1.95.0"\n' +
            'assert os.environ["RUSTUP_AUTO_INSTALL"]=="0"\n' +
            'arguments=sys.argv[3:]\n' +
            'with calls.open("a") as stream: stream.write(json.dumps(arguments)+"\\n")\n' +
            'if arguments[0]=="pre-push-plan":\n' +
            '    assert arguments==["pre-push-plan","--base",plan["base"],"--head",plan["head"],"--format","json"]\n' +
            f'    print({json.dumps(prepush)!r})\n' +
            'elif arguments[:2]==["command-evidence","proof-set"]:\n' +
            '    assert arguments==["command-evidence","proof-set","--spec",str(output/"spec.json"),"--json"]\n' +
            '    assert os.environ["GITHUB_SHA"]==plan["control"]\n' +
            '    assert os.environ["GITHUB_RUN_ID"]=="123" and os.environ["GITHUB_RUN_ATTEMPT"]=="2"\n' +
            '    context=json.loads((output.parent/"em-ci-job-context.json").read_bytes())\n' +
            '    assert context["qualification_subject"]["head"]==plan["head"] and "proof_started_at" in context\n' +
            '    spec=json.loads((output/"spec.json").read_bytes())\n' +
            '    assert spec=={"schema_version":"command-proof-set.v1","commands":[{"id":"pre-push-0",' +
            '"program":"cargo","args":["check","-p","perl-parser","--locked"],"cwd":str(workspace),' +
            '"candidate":plan["head"],"timeout_secs":10,"out_dir":str(output/"logs")}]}\n' +
            f'    sys.stdout.buffer.write({raw!r})\n    raise SystemExit({status})\n' +
            'else:\n    assert arguments==["rust-small-proof"]\n' +
            '    assert os.environ["GITHUB_SHA"]==plan["control"]\n' +
            f'    Path("target/receipts/rust-small-proof.json").write_text({json.dumps(self.fixture.bridge.receipt)!r})\n')
        self.fixture.owners.canonical_owner = CanonicalOwner(verifier=self.tool,
            verifier_sha256=hashlib.sha256(self.tool.read_bytes()).hexdigest(),
            control_sha=plan['control'], workspace=workspace)
        return output, calls, raw

    def test_real_local_owner_then_rust_exports_exact_sidecar_and_preserves_native_transport(self):
        output, calls, raw = self.install_local_composition_fixture()
        # Assert orchestration failures as proof failures, retaining owner stderr
        # in the diagnostic instead of mistaking a broken fixture for green proof.
        try:
            status, manifest = self.run_native(command_output=output, command_timeout_secs=10)
        except (OSError, ValueError) as error:
            stderr = output / 'owner-stderr.log'
            self.fail(f'local-to-Rust composition did not complete: {error}; '
                f'owner stderr: {stderr.read_text() if stderr.exists() else "not invoked"}')
        self.assertEqual((status, manifest['result']), (0, 'success'))
        self.assertEqual([json.loads(row)[0] for row in calls.read_text().splitlines()],
            ['pre-push-plan', 'command-evidence', 'rust-small-proof'])
        self.assertEqual(self.trace, ['capture', 'allocation', 'release'])
        self.assertEqual((output / 'command-proof-set.json').read_bytes(), raw)
        self.assertEqual(manifest.get('command_member'), dict(path='command-proof-set.json',
            bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest()))
        self.assertEqual((self.fixture.packet / 'command-proof-set.json').read_bytes(), raw)
        self.assertEqual({key: manifest[key] for key in ('sha', 'run_id', 'run_attempt')},
            dict(sha=self.fixture.plan['control'], run_id='123', run_attempt='2'))
        self.assertNotEqual(manifest['sha'], self.fixture.plan['head'])
        self.assertEqual(json.loads((self.fixture.packet / 'rust-small-proof.json').read_bytes())
            ['subject']['git_sha'], self.fixture.plan['head'])
        context = json.loads(self.context.read_bytes())
        self.assertEqual(context['qualification_subject'], {key: self.fixture.plan[key]
            for key in ('head', 'tree', 'base', 'control', 'request_key')})
        self.assertEqual((context['sha'], context['run_id'], context['run_attempt']),
            (self.fixture.plan['control'], '123', '2'))
        self.assertEqual(self.environment['GITHUB_TOKEN'], 'metadata-only')
        self.assertEqual(self.environment['GIT_DIR'], '/wrong')

    def test_real_nonzero_local_owner_preserves_failure_and_prevents_rust_export(self):
        output, calls, raw = self.install_local_composition_fixture(status=7)
        with self.assertRaisesRegex(ValueError, 'actual selected local command failed'):
            self.run_native(command_output=output, command_timeout_secs=10)
        self.assertEqual((output / 'command-proof-set.json').read_bytes(), raw)
        self.assertFalse((output / 'command-proof-set.json.partial').exists())
        self.assertEqual([json.loads(row)[0] for row in calls.read_text().splitlines()],
            ['pre-push-plan', 'command-evidence'])
        self.assertEqual(self.trace, ['capture', 'allocation', 'release'])
        self.assertFalse(self.log.exists())
        self.assertEqual(list(self.fixture.packet.iterdir()), [])

    def test_failed_local_owner_stops_before_rust_and_releases_actual_owner_port(self):
        with mock.patch('scripts.ci.qualification_local_commands.produce',return_value=(1,self.root/'failed.json')):
            with self.assertRaisesRegex(ValueError,'local command failed'):
                self.run_native(command_output=self.root/'local-proof',command_timeout_secs=10)
        self.assertEqual(self.trace,['capture','allocation','release'])
        self.assertFalse(self.log.exists())
        self.assertEqual(list(self.fixture.packet.iterdir()),[])

    def test_duplicate_native_run_never_captures_allocates_or_launches_product(self):
        self.fixture.api.extra_rows = [dict(self.fixture.api.row, id=122)]
        result = self.run_native()
        self.assertEqual(result['status'], 'NOT_PROVEN')
        self.assertEqual(result['canonical_run_id'], 122)
        self.assertFalse(result['rehearsal_elected'])
        self.assertEqual((result['selected'], result['executed'], result['failed']), (0, 0, 0))
        self.assertIsNone(result['check_run_id'])
        self.assertEqual(self.trace, [])
        self.assertFalse(self.context.exists())
        self.assertFalse(self.log.exists())
        self.assertEqual(list(self.fixture.packet.iterdir()), [])

    def test_duplicate_discovered_while_waiting_for_allocation_releases_without_product(self):
        @contextmanager
        def raced(plan, selection):
            self.trace.append('allocation')
            self.fixture.api.extra_rows = [dict(self.fixture.api.row, id=122)]
            try:
                yield
            finally:
                self.trace.append('release')
        with self.assertRaisesRegex(ValueError, 'admission moved while awaiting allocation'):
            factory.execute_native(self.fixture.owners,
                {'inputs': {'request': json.dumps(self.fixture.plan)}},
                self.capture, allocation_owner=raced, log_path=self.log)
        self.assertEqual(self.trace, ['capture', 'allocation', 'release'])
        self.assertNotIn('proof_started_at', json.loads(self.context.read_bytes()))
        self.assertFalse(self.log.exists())
        self.assertEqual(list(self.fixture.packet.iterdir()), [])

    def test_stale_attempt_during_allocation_never_starts_proof(self):
        @contextmanager
        def moved(plan,selection):
            self.fixture.api.row['run_attempt']=3
            try:yield
            finally:self.trace.append('release')
        with self.assertRaises(ValueError):
            factory.execute_native(self.fixture.owners,{'inputs':{'request':json.dumps(self.fixture.plan)}},
                self.capture,allocation_owner=moved,log_path=self.log)
        self.assertNotIn('proof_started_at',json.loads(self.context.read_bytes()))
        self.assertFalse(self.log.exists());self.assertIn('release',self.trace)


if __name__=='__main__':unittest.main()
