"""Real adapter/process joins; native transport and installed xtask are synthetic."""
from contextlib import contextmanager
from dataclasses import replace
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from scripts.ci import qualification_native_adapters as native
from scripts.ci import test_qualification_runtime as runtime
from scripts.ci.rust_small_canonical_owner import CanonicalOwner


class NativeOwnersTests(unittest.TestCase):
    def setUp(self):
        self.fixture = runtime.RuntimeCompositionTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.control = self.root / 'control'
        (self.control / '.ci/policies').mkdir(parents=True)
        self.policy = b'[[checks]]\nname="required"\nrequired=true\nruleset_integration_id=15368\n'
        (self.control / '.ci/policies/required-checks.toml').write_bytes(self.policy)
        subject = replace(self.fixture.api.subject, policy_sha256=hashlib.sha256(self.policy).hexdigest())
        self.fixture.api.subject = self.fixture.adapter.subject = subject
        self.plan = self.fixture.adapter.snapshot()
        self.tool = self.root / 'xtask'
        self.trace = self.root / 'trace.jsonl'
        self.prepush = dict(schema='pre_push_proof_plan.v1',base_sha=self.plan['base'],head_sha=self.plan['head'],
            posture='PROCEED', selected=[{'command':'cargo a'}], deferred=[{'command':'mandatory-later'}])
        self.scope = dict(schema_version=2,base=self.plan['base'],head_sha=self.plan['head'],selected_lanes=['existing'])
        self.tool.write_text('#!' + sys.executable + '\nimport sys,json\nfrom pathlib import Path\n'
            f'with Path({str(self.trace)!r}).open("a") as f: f.write(json.dumps(sys.argv[1:])+"\\n")\n'
            f'if "pre-push-plan" in sys.argv: print({json.dumps(self.prepush)!r})\n'
            f'elif "ci-scope" in sys.argv: print({json.dumps(self.scope)!r})\n')
        self.tool.chmod(0o755)
        self.verifier = CanonicalOwner(verifier=self.tool,verifier_sha256=hashlib.sha256(self.tool.read_bytes()).hexdigest(),
            control_sha=self.plan['control'],workspace=self.fixture.bridge.fixture.workspace)
        self.packet = self.root / 'warm'
        self.packet.mkdir()
        self.context = dict(schema='em-ci-job-context.v1',profile='standard',repository=self.plan['repository'],
            sha=self.plan['head'],run_id='122',run_attempt='1',producer_check_run_id=200)
        self.receipt = dict(schema_version='rust_small_proof.v1',subject={'git_sha':self.plan['head'],'worktree_dirty':False},
            result='success',steps=[dict(name='canonical-a',argv=['cargo','a'],outcome='ok'),
                                    dict(name='canonical-b',argv=['git','b'],outcome='ok')])
        self.write_packet()
        self.run = dict(id=122,run_attempt=1,repository={'id':self.plan['repository_id']},
                        head_sha=self.plan['head'],status='completed',conclusion='success')
        self.check = dict(id=200,head_sha=self.plan['head'],app={'id':15368},status='completed',conclusion='success')
        self.jobs = [{'check_run_url':f'https://api.github.com/{self.fixture.adapter.root}/check-runs/200',
                      'head_sha':self.plan['head'],'status':'completed','conclusion':'success'}]
        self.checks = []
        self.legacy = []
        self.original_get = self.fixture.api.get
        self.patch = mock.patch.object(self.fixture.api,'get',side_effect=self.get)
        self.bytes_patch = mock.patch.object(self.fixture.api,'get_bytes',create=True,side_effect=lambda path:json.dumps(self.legacy).encode())
        self.bytes_patch.start()
        self.addCleanup(self.bytes_patch.stop)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        self.imports = []
        self.owners = native.NativeEntryOwners(self.fixture.adapter,self.fixture.owners.environment,
            canonical_owner=self.verifier,control_root=self.control,native_warm_import=self.import_warm,
            warm_run_id=122,warm_run_attempt=1)
        _, portfolio, _ = self.owners.portfolio(self.plan)
        subject = replace(subject,portfolio_sha256=portfolio)
        self.fixture.api.subject = self.fixture.adapter.subject = subject
        self.plan = self.fixture.adapter.snapshot()
        self.trace.unlink()
        self.imports.clear()

    def write_packet(self):
        (self.packet/'em-ci-job-context.json').write_text(json.dumps(self.context))
        (self.packet/'rust-small-proof.json').write_text(json.dumps(self.receipt))

    def import_warm(self,run,attempt,*,acquire_plan):
        self.assertEqual((run,attempt),(122,1))
        self.assertEqual(acquire_plan(),self.plan)
        self.imports.append((run,attempt))
        return self.packet

    def get(self,path):
        if path.endswith('/actions/runs/122'): return dict(self.run)
        if path.endswith('/check-runs/200'): return dict(self.check)
        if '/attempts/1/jobs?' in path: return {'total_count':len(self.jobs),'jobs':self.jobs}
        if '/status?per_page' in path: return {'sha':self.plan['head'],'total_count':len(self.legacy),'statuses':self.legacy}
        if '/check-runs?filter=all' in path: return {'total_count':len(self.checks),'check_runs':self.checks}
        return self.original_get(path)

    def row(self,id=1,conclusion='failure',status='completed',name='required',app=15368):
        return dict(id=id,name=name,head_sha=self.plan['head'],app={'id':app},status=status,
                    conclusion=conclusion,html_url=f'https://github.com/example/job/{id}')

    def test_existing_planners_and_verified_canonical_vocabulary_preserve_deferred_work(self):
        selected = self.owners.selection(self.plan)
        self.assertEqual(selected.selected,('canonical-a','canonical-b'))
        self.assertEqual(selected.integration_obligations,('required','deferred:mandatory-later'))
        proof = self.owners.entry_committed(self.plan,selected)
        self.assertEqual(proof.disposition,'verified')
        self.assertIn('/runs/122/attempts/1#job-200',proof.producer_reference)
        calls = [json.loads(line) for line in self.trace.read_text().splitlines()]
        self.assertTrue(all(row[:2]==['--worktree-root',str(self.verifier.workspace)] for row in calls))
        self.assertTrue(any('pre-push-plan' in row for row in calls))
        self.assertTrue(any('ci-scope' in row for row in calls))
        self.assertTrue(all(run==122 for run,_ in self.imports))

    def test_selected_local_command_without_canonical_execution_refuses(self):
        actual=native.invoke
        def invoke(owner,*arguments):
            if arguments[0]=='pre-push-plan':
                return json.dumps(dict(self.prepush,selected=[{'command':'cargo xtask workflow-policy-lint'}])).encode()
            return actual(owner,*arguments)
        with mock.patch.object(native,'invoke',side_effect=invoke),self.assertRaisesRegex(ValueError,'selected local command'):
            self.owners.portfolio(self.plan)

    def test_actual_package_planner_commands_are_not_implied_by_workspace_rust_proof(self):
        # Native pre_push_plan::add_rust_package_proof selects these commands.
        # Rust Small has no equivalent Clippy/all-target package execution fact.
        selected=[{'command':'cargo check -p perl-parser --locked'},
                  {'command':'cargo clippy -p perl-parser --all-targets --locked -- -D warnings'},
                  {'command':'cargo test -p perl-parser --all-targets --locked'}]
        actual=native.invoke
        def invoke(owner,*arguments):
            if arguments[0]=='pre-push-plan':return json.dumps(dict(self.prepush,selected=selected)).encode()
            return actual(owner,*arguments)
        with mock.patch.object(native,'invoke',side_effect=invoke),self.assertRaisesRegex(ValueError,'selected local command'):
            self.owners.portfolio(self.plan)

    def test_self_run_including_prior_attempt_never_imports(self):
        self.owners.environment['GITHUB_RUN_ID']='122'
        with self.assertRaises(ValueError): self.owners.selection(self.plan)
        self.assertEqual(self.imports,[])

    def test_foreign_attempt_app_head_or_unfinished_warm_producer_refuses(self):
        for object,changes in ((self.run,{'run_attempt':2}),(self.run,{'status':'in_progress'}),
                               (self.check,{'head_sha':'0'*40}),(self.check,{'app':{'id':9}})):
            original=dict(object);object.update(changes)
            with self.subTest(changes=changes),self.assertRaises(ValueError): self.owners.selection(self.plan)
            object.clear();object.update(original)

    def test_same_head_app_check_from_another_native_run_cannot_authenticate_warm_proof(self):
        self.jobs[0]['check_run_url']='https://api.github.com/foreign/check-runs/200'
        with self.assertRaises(ValueError):self.owners.warm(self.plan)

    def test_policy_portfolio_and_selected_commands_cannot_be_relabelled(self):
        with self.assertRaises(ValueError): self.owners.selection(dict(self.plan,portfolio_sha256='0'*64))
        path=self.control/'.ci/policies/required-checks.toml';path.write_bytes(self.policy+b'# changed')
        with self.assertRaises(ValueError): self.owners.selection(self.plan)
        path.write_bytes(self.policy)
        selected=self.owners.selection(self.plan)
        with self.assertRaises(ValueError): self.owners.entry_committed(self.plan,replace(selected,selected=('bounded-local',)))

    def test_warm_run_or_packet_mutation_during_actual_verifier_process_refuses(self):
        real=native.subprocess.run
        def run(*args,**kwargs):
            result=real(*args,**kwargs)
            self.run['run_attempt']=2
            return result
        with mock.patch.object(native.subprocess,'run',side_effect=run),self.assertRaises(ValueError):
            self.owners.warm(self.plan)
        self.run['run_attempt']=1
        path=self.packet/'rust-small-proof.json';raw=path.read_bytes()
        def run(*args,**kwargs):
            result=real(*args,**kwargs);path.write_bytes(raw+b' ');path.write_bytes(raw);return result
        with mock.patch.object(native.subprocess,'run',side_effect=run),self.assertRaises(ValueError):
            self.owners.warm(self.plan)

    def test_known_failure_survives_pending_replacement_until_real_success(self):
        self.checks=[self.row(),self.row(2,None,'in_progress')]
        self.assertEqual(self.owners.applicability(self.plan)[0].disposition,'unresolved_failure')
        self.checks.append(self.row(3,'success'))
        self.assertEqual(self.owners.applicability(self.plan)[0].disposition,'repaired')
        self.checks=[]
        self.assertEqual(self.owners.applicability(self.plan)[0].disposition,'absent')

    def test_neutral_cancelled_or_other_app_success_cannot_erase_a_known_failure(self):
        for conclusion in ('neutral','skipped','cancelled'):
            self.checks=[self.row(name='advisory'),self.row(2,conclusion,name='advisory')]
            self.assertIn('unresolved_failure',[row.disposition for row in self.owners.applicability(self.plan)])
        self.checks=[self.row(name='advisory'),self.row(2,'success',name='advisory',app=9)]
        self.assertIn('unresolved_failure',[row.disposition for row in self.owners.applicability(self.plan)])
        self.checks=[]
        self.legacy=[{'id':1,'creator':{'id':7},'state':'error','context':'external','url':'https://api.github.com/status/1'},
                     {'id':2,'creator':{'id':7},'state':'pending','context':'external','url':'https://api.github.com/status/2'}]
        self.assertIn('unresolved_failure',[row.disposition for row in self.owners.applicability(self.plan)])

    def test_wrong_source_app_and_truncated_result_inventory_never_clear_failure(self):
        self.checks=[self.row(app=9)]
        with self.assertRaises(ValueError): self.owners.applicability(self.plan)
        self.checks=[self.row(id=i) for i in range(1,101)]
        with self.assertRaises(ValueError): self.owners.applicability(self.plan)

    def test_actual_runtime_one_shot_capture_and_execution_within_lease(self):
        fixture=runtime.RuntimeCompositionTests();fixture.setUp()
        try:
            fixture.fresh_producer_packet()
            trace=[]
            event={'inputs':{'request':json.dumps(fixture.plan)}}
            def capture(environment,*,acquire_plan):
                self.assertNotIn('capture',trace);trace.append('capture')
                return {'qualification_subject':acquire_plan()}
            @contextmanager
            def allocation(plan,selected):
                trace.append('acquire')
                try:yield
                finally:trace.append('release')
            def producer(environment,*,acquire_plan):
                self.assertEqual(trace,['capture','acquire']);trace.append('producer');return 0
            with mock.patch.object(runtime.admission,'production',return_value={'classification':'READY'}):
                result=native.allocate_and_execute(fixture.owners,event,capture,
                    allocation_owner=allocation,proof_owner=producer)
            self.assertEqual(result[0],0)
            self.assertEqual(trace,['capture','acquire','producer','release'])
            fixture.fresh_producer_packet();trace.clear()
            @contextmanager
            def moved_allocation(plan,selected):
                trace.append('acquire');fixture.api.row['run_attempt']=3
                try:yield
                finally:trace.append('release')
            with mock.patch.object(runtime.admission,'production',return_value={'classification':'READY'}),self.assertRaises(ValueError):
                native.allocate_and_execute(fixture.owners,event,capture,
                    allocation_owner=moved_allocation,proof_owner=lambda *args,**kw:self.fail('stale execution'))
            self.assertEqual(trace,['capture','acquire','release'])
        finally:fixture.doCleanups()

    def test_allocator_lease_wraps_execution_and_rechecks_entry_after_wait(self):
        trace=[]
        owners=mock.Mock()
        owners.enter_native.return_value={'status':'READY_FOR_ALLOCATION'}
        owners.acquire.return_value=self.plan
        owners.selection_owner.return_value=self.owners.selection(self.plan)
        owners.execute_proof.side_effect=lambda producer:trace.append('execute')
        @contextmanager
        def allocation(plan,selected):
            self.assertEqual(plan,self.plan);trace.append('acquire')
            try:yield
            finally:trace.append('release')
        native.allocate_and_execute(owners,{},lambda:None,allocation_owner=allocation,proof_owner=lambda:None)
        self.assertEqual(trace,['acquire','execute','release'])
        self.assertEqual(owners.enter_native.call_count,2)
        owners.enter_native.side_effect=[{'status':'READY_FOR_ALLOCATION'},{'status':'NOT_PROVEN'}]
        with self.assertRaises(ValueError):
            native.allocate_and_execute(owners,{},lambda:None,allocation_owner=allocation,proof_owner=lambda:None)
        self.assertEqual(trace[-2:],['acquire','release'])
        self.assertEqual(owners.execute_proof.call_count,1)


if __name__=='__main__':unittest.main()
