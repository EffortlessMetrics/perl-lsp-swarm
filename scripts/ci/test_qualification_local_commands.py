"""Binding regressions: native authentication and receipt production are fixtures.

No synthetic receipt is represented as actual Cargo or native Actions evidence.
"""
import copy
from dataclasses import replace
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from scripts.ci import qualification_local_commands as local
from scripts.ci import qualification_native_adapters as native
from scripts.ci import test_qualification_native_adapters as adapter_fixtures


class LocalCommandBindingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.plan = dict(head='1'*40,tree='2'*40,base='3'*40,control='4'*40,request_key='5'*64)
        self.prepush = dict(schema='pre_push_proof_plan.v1',posture='PROCEED',
            base_sha=self.plan['base'],head_sha=self.plan['head'],
            selected=[{'command':'cargo check -p perl-parser --locked'},
                      {'command':'cargo clippy -p perl-parser --all-targets --locked -- -D warnings'},
                      {'command':'cargo test -p perl-parser --all-targets --locked'}])
        self.context = dict(sha=self.plan['control'], qualification_subject=dict(self.plan),
            product_workspace='/producer/actual-H',proof_started_at='2026-10-17T12:00:00Z',
            proof_finished_at='2026-10-17T12:00:00Z')
        self.proof = dict(schema_version='command-proof-set.v1',result='SUCCESS', commands=[
            dict(id=f'pre-push-{i}',receipt=dict(schema_version='command-evidence.v1',argv=list(argv),
                cwd=self.context['product_workspace'],candidate_identity=self.plan['head'],
                started_at=f'2026-10-17T12:00:00.{i*200:03d}Z',
                ended_at=f'2026-10-17T12:00:00.{i*200+100:03d}Z', duration_ms=100,
                exit_code=0,termination='exit_code:0',stdout_reference=f'/producer/logs/{i}.stdout',
                stderr_reference=f'/producer/logs/{i}.stderr',result='SUCCESS'))
            for i,argv in enumerate(local.selected_argv(self.prepush,self.plan))])
        self.write()

    def write(self):
        (self.root/'em-ci-job-context.json').write_text(json.dumps(self.context))
        (self.root/local.MEMBER).write_text(json.dumps(self.proof))

    def bind(self):
        return local.bind_authenticated(self.root,self.plan,self.prepush)

    def test_subsecond_commands_bind_to_producer_workspace_and_exact_real_selection(self):
        proof=self.bind()
        self.assertEqual(proof.argv,local.selected_argv(self.prepush,self.plan))
        self.assertEqual(proof.receipt_sha256,hashlib.sha256((self.root/local.MEMBER).read_bytes()).hexdigest())

    def test_native_H_bootstrap_is_distinct_from_control_S_capture(self):
        self.context['sha']=self.plan['head'];del self.context['qualification_subject'];self.write()
        self.bind()
        self.context['qualification_subject']={};self.write()
        with self.assertRaises(ValueError):self.bind()
        self.context['sha']=self.plan['control'];del self.context['qualification_subject'];self.write()
        with self.assertRaises(ValueError):self.bind()

    def test_partial_wrong_argv_cwd_candidate_or_non_success_cannot_satisfy_selection(self):
        original=copy.deepcopy(self.proof)
        for changes in ({'argv':['cargo','test']},{'cwd':'/consumer/H'},
                        {'candidate_identity':'9'*40},{'exit_code':True},{'exit_code':7},
                        {'termination':'timeout'},{'result':'NOT_PROVEN'},
                        {'duration_ms':-1},{'stderr_reference':''},
                        {'ended_at':'2026-10-17T12:00:01.000Z'}):
            self.proof=copy.deepcopy(original);self.proof['commands'][0]['receipt'].update(changes);self.write()
            with self.subTest(changes=changes),self.assertRaises(ValueError):self.bind()
        self.proof=copy.deepcopy(original);self.proof['commands'].pop();self.write()
        with self.assertRaises(ValueError):self.bind()
        self.proof=copy.deepcopy(original);self.proof['commands'][0]=None;self.write()
        with self.assertRaises(ValueError):self.bind()

    def test_serial_order_context_scope_and_shell_selection_refuse(self):
        self.proof['commands'][1]['receipt']['started_at']='2026-10-17T12:00:00.050Z';self.write()
        with self.assertRaises(ValueError):self.bind()
        for command in ('cd crates && cargo test','cargo test && cargo check','cargo test; cargo check',
                        'cargo test\ncargo check','cargo test $(touch /tmp/forbidden)'):
            with self.subTest(command=command),self.assertRaises(ValueError):
                local.selected_argv(dict(self.prepush,selected=[{'command':command}]),self.plan)

    def test_linked_receipt_and_mutating_packet_refuse(self):
        path=self.root/local.MEMBER;path.rename(self.root/'other');path.symlink_to(self.root/'other')
        with self.assertRaises(ValueError):self.bind()
        path.unlink();path.write_bytes((self.root/'other').read_bytes())
        real=local.regular_bytes;calls=0
        def read(path,limit):
            nonlocal calls
            calls+=1
            if calls==3:path.write_bytes(path.read_bytes()+b' ')
            return real(path,limit)
        with mock.patch.object(local,'regular_bytes',side_effect=read),self.assertRaises(ValueError):self.bind()

    def test_producer_preserves_actual_failure_json_and_checks_process_outcome(self):
        workspace=self.root/'workspace';workspace.mkdir()
        canonical=mock.Mock(control_sha=self.plan['control'],workspace=workspace,verifier=self.root/'trusted-xtask')
        canonical.executable_identity.return_value=('immutable-tool',)
        self.context['product_workspace']=str(workspace);self.write()
        environment=dict(RUNNER_TEMP=str(self.root),GITHUB_WORKSPACE=str(workspace),
                         GITHUB_TOKEN='metadata-only',GIT_DIR='/wrong',CARGO_BUILD_JOBS='2')
        self.proof['result']='FAILURE';self.proof['commands'][0]['receipt'].update(result='FAILURE',exit_code=7,termination='exit_code:7')
        raw=json.dumps(self.proof).encode()
        calls=[]
        def execute(argv,**kwargs):
            calls.append((argv,kwargs['env']))
            kwargs['stdout'].write(raw)
            return mock.Mock(returncode=1)
        with mock.patch.object(native,'invoke',return_value=json.dumps(self.prepush).encode()), \
                mock.patch.object(local,'product_subject'),mock.patch.object(local.subprocess,'run',side_effect=execute):
            status,path=local.produce(canonical,environment,acquire_plan=lambda:self.plan,
                                     output=self.root/'execution',command_timeout_secs=60)
        self.assertEqual(status,1);self.assertEqual(path.read_bytes(),raw)
        argv,child=calls[0]
        self.assertEqual(argv[:5],[str(canonical.verifier),'--worktree-root',str(workspace),'command-evidence','proof-set'])
        self.assertNotIn('GITHUB_TOKEN',child);self.assertNotIn('GIT_DIR',child)
        self.assertEqual(child['CARGO_BUILD_JOBS'],'2')
        with mock.patch.object(native,'invoke',return_value=json.dumps(self.prepush).encode()), \
                mock.patch.object(local,'product_subject'),mock.patch.object(local.subprocess,'run',side_effect=lambda *a,**kw:(kw['stdout'].write(raw),mock.Mock(returncode=0))[1]):
            with self.assertRaisesRegex(ValueError,'aggregate outcome disagree'):
                local.produce(canonical,environment,acquire_plan=lambda:self.plan,
                              output=self.root/'contradictory',command_timeout_secs=60)
        self.assertFalse((self.root/'contradictory'/local.MEMBER).exists())

    def test_wrong_execution_workspace_refuses_before_planner_or_process(self):
        canonical=mock.Mock(control_sha=self.plan['control'],workspace=self.root/'executed')
        with mock.patch.object(native,'invoke') as planner,mock.patch.object(local.subprocess,'run') as process:
            with self.assertRaisesRegex(ValueError,'product workspace differ'):
                local.produce(canonical,{'RUNNER_TEMP':str(self.root),'GITHUB_WORKSPACE':str(self.root/'checked')},
                    acquire_plan=lambda:self.plan,output=self.root/'fresh',command_timeout_secs=10)
        planner.assert_not_called();process.assert_not_called()


class NativeLocalCompositionTests(unittest.TestCase):
    def test_actual_planner_portfolio_retains_separate_receipt_and_rebinds_at_entry(self):
        fixture=adapter_fixtures.NativeOwnersTests();fixture.setUp()
        try:
            selected=[{'command':'cargo check -p perl-parser --locked'},
                      {'command':'cargo clippy -p perl-parser --all-targets --locked -- -D warnings'},
                      {'command':'cargo test -p perl-parser --all-targets --locked'}]
            prepush=dict(fixture.prepush,selected=selected)
            binding=LocalCommandBindingTests();binding.setUp()
            try:
                binding.plan=fixture.plan;binding.prepush=prepush
                binding.context.update(sha=fixture.plan['head']);binding.context.pop('qualification_subject')
                for row in binding.proof['commands']:
                    row['receipt']['candidate_identity']=fixture.plan['head']
                binding.write()
                for member in (local.MEMBER,'em-ci-job-context.json'):
                    if member==local.MEMBER:
                        (fixture.packet/member).write_bytes((binding.root/member).read_bytes())
                fixture.context.update(product_workspace=binding.context['product_workspace'],
                    proof_started_at=binding.context['proof_started_at'],proof_finished_at=binding.context['proof_finished_at'])
                fixture.write_packet()
                actual=native.invoke
                def invoke(owner,*arguments):
                    return json.dumps(prepush).encode() if arguments[0]=='pre-push-plan' else actual(owner,*arguments)
                with mock.patch.object(native,'invoke',side_effect=invoke):
                    portfolio,sha,_=fixture.owners.portfolio(fixture.plan)
                    self.assertEqual(portfolio['local_commands']['argv'],[list(a) for a in local.selected_argv(prepush,fixture.plan)])
                    subject=replace(fixture.fixture.api.subject,portfolio_sha256=sha)
                    fixture.fixture.api.subject=fixture.fixture.adapter.subject=subject
                    fixture.plan=fixture.fixture.adapter.snapshot()
                    selection=fixture.owners.selection(fixture.plan)
                    self.assertEqual(selection.selected,('canonical-a','canonical-b'))
                    self.assertEqual(fixture.owners.entry_committed(fixture.plan,selection).disposition,'verified')
                    (fixture.packet/local.MEMBER).unlink()
                    with self.assertRaises(ValueError):fixture.owners.entry_committed(fixture.plan,selection)
            finally:binding.doCleanups()
        finally:fixture.doCleanups()


if __name__=='__main__':unittest.main()
