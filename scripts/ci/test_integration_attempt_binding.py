"""Execute actual integration identity shells; no Cargo, network or hosted proof."""
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import yaml

ROOT = Path(__file__).resolve().parents[2]
SCALARS = ('draft-pr-check', 'preflight-latest-check', 'conflict-markers',
           'check-all-targets', 'ux-tests')
SHARDS = ('meta', 'foundation', 'parser_stack', 'analysis', 'lsp', 'support', 'corpus', 'policy')
IDENTITY = dict(subject_sha='a'*40, run_id='1234', run_attempt='2')

class IntegrationIdentity(unittest.TestCase):
    def setUp(self):
        self.jobs = yaml.load((ROOT/'.github/workflows/ci.yml').read_text(), Loader=yaml.BaseLoader)['jobs']
        self.temp = self.enterContext(tempfile.TemporaryDirectory())
        self.directory = Path(self.temp)/'integration-proofs'
        self.directory.mkdir()
        self.env = dict(os.environ, RUNNER_TEMP=self.temp, GITHUB_EVENT_NAME='merge_group',
                        GITHUB_SHA=IDENTITY['subject_sha'], MERGE_GROUP_SHA=IDENTITY['subject_sha'],
                        GITHUB_RUN_ID=IDENTITY['run_id'], GITHUB_RUN_ATTEMPT=IDENTITY['run_attempt'],
                        COMPILE_RESULT='success', INTEGRATION_RESULT='success')
        self.needs = {name: dict(result='success', outputs=dict(IDENTITY)) for name in SCALARS}
        self.needs['merge-gate-shards'] = dict(result='success', outputs={})
        for prefix in ('COMPILE', 'INTEGRATION'):
            for suffix, value in zip(('SHA', 'RUN_ID', 'RUN_ATTEMPT'), IDENTITY.values()):
                self.env[f'{prefix}_PROOF_{suffix}'] = value
        for shard in SHARDS:
            (self.directory/f'integration-{shard}.txt').write_text(self.payload())

    def payload(self, **changes):
        return ''.join(f'{key}={value}\n' for key,value in dict(IDENTITY, **changes).items())

    def run_step(self, job, name=None, env=None):
        step = next(step for step in self.jobs[job]['steps'] if not name or step.get('name') == name)
        values = dict(self.env, NEEDS_JSON=json.dumps(self.needs))
        if env: values.update(env)
        return subprocess.run(['bash', '--noprofile', '--norc', '-c', step['run']],
                              env=values, capture_output=True, text=True, timeout=5)

    def final(self, **env): return self.run_step('integration-required', env=env)
    def aggregate(self): return self.run_step('merge-gate', 'Verify every selected producer identity')

    def test_positive_all_exact_current_attempt(self):
        self.assertEqual(self.aggregate().returncode, 0)
        self.assertEqual(self.final().returncode, 0)

    def test_finalizer_rejects_missing_wrong_g_run_and_attempt_for_both_producers(self):
        for prefix in ('COMPILE','INTEGRATION'):
            for suffix, wrong in (('SHA','b'*40), ('RUN_ID','5678'), ('RUN_ATTEMPT','1')):
                for value in ('', wrong):
                    with self.subTest(prefix=prefix,suffix=suffix,value=value):
                        self.assertNotEqual(self.final(**{f'{prefix}_PROOF_{suffix}':value}).returncode,0)
        for key,value in (('MERGE_GROUP_SHA',''),
                          ('GITHUB_SHA','b'*40),('GITHUB_RUN_ID',''),('GITHUB_RUN_ATTEMPT','0')):
            self.assertNotEqual(self.final(**{key:value}).returncode,0)

    def test_every_scalar_refuses_stale_or_absent_identity_even_when_successful(self):
        original=deepcopy(self.needs)
        for name in SCALARS:
            for key,wrong in (('subject_sha','b'*40),('run_id','5678'),('run_attempt','1')):
                for value in ('',wrong):
                    self.needs=deepcopy(original)
                    self.needs[name]['outputs'][key]=value
                    with self.subTest(name=name,key=key,value=value):
                        self.assertNotEqual(self.aggregate().returncode,0)
            for result in ('skipped','failure','cancelled',''):
                self.needs=deepcopy(original);self.needs[name]['result']=result
                self.assertNotEqual(self.aggregate().returncode,0)
        self.needs=deepcopy(original)
        del self.needs['ux-tests']
        self.assertNotEqual(self.aggregate().returncode,0)

    def test_every_matrix_member_must_have_current_attempt_identity(self):
        for name in SHARDS:
            path=self.directory/f'integration-{name}.txt'
            for contents in ('',self.payload(run_attempt='1'),self.payload(run_id='5678'),self.payload(subject_sha='b'*40)):
                path.write_text(contents)
                with self.subTest(name=name,contents=contents):self.assertNotEqual(self.aggregate().returncode,0)
            path.unlink();self.assertNotEqual(self.aggregate().returncode,0)
            path.write_text(self.payload())
        (self.directory/'integration-unselected.txt').write_text(self.payload())
        self.assertNotEqual(self.aggregate().returncode,0)

    def test_original_gate_still_requires_every_selected_result_and_rejects_g_noop(self):
        from scripts.ci.evaluate_ci_gate import evaluate
        self.needs['draft-pr-check']['outputs']['run_ci']='true'
        self.needs['preflight-latest-check']['outputs']['is_latest']='true'
        self.assertEqual(evaluate(self.needs,event_name='merge_group').status,'success')
        for name in self.needs:
            original=self.needs[name]['result']
            for result in ('failure','skipped','cancelled',''):
                self.needs[name]['result']=result
                self.assertNotIn(evaluate(self.needs,event_name='merge_group').status, ('success','scoped_noop'))
            self.needs[name]['result']=original
        self.needs['draft-pr-check']['outputs']['run_ci']='false'
        self.assertNotEqual(evaluate(self.needs,event_name='merge_group').status,'success')

    def test_actual_success_stamp_verifies_checkout_and_identity_before_emitting(self):
        repo=Path(self.temp)/'repo';repo.mkdir()
        env=dict(self.env)
        for key in subprocess.check_output(['git','rev-parse','--local-env-vars'],text=True).split():
            env.pop(key,None)
        def git(*args):
            return subprocess.check_output(['git','-C',str(repo),'-c','core.hooksPath=',
                '-c','commit.gpgsign=false','-c','user.name=Fixture','-c','user.email=fixture@example.invalid',
                *args],env=env,text=True).strip()
        git('init','-q','--template=');git('commit','--allow-empty','-qm','fixture')
        sha=git('rev-parse','HEAD')
        for name in (*SCALARS,'merge-gate','merge-gate-shards'):
            stamp=next(s for s in self.jobs[name]['steps'] if s.get('name')=='Bind successful native integration producer')
            output=Path(self.temp)/'output'
            env.update(GITHUB_SHA=sha,MERGE_GROUP_SHA=sha,GITHUB_OUTPUT=str(output),SHARD_NAME='meta')
            for changes in ({},dict(GITHUB_SHA='b'*40),dict(MERGE_GROUP_SHA='b'*40,GITHUB_SHA='b'*40),
                            dict(GITHUB_RUN_ID=''),dict(GITHUB_RUN_ATTEMPT='0'),dict(GITHUB_EVENT_NAME='pull_request')):
                output.write_text('')
                result=subprocess.run(['bash','-c',stamp['run']],cwd=repo,env=dict(env,**changes),
                                      capture_output=True,text=True)
                # Both metadata guards lack native checkouts; neither is product proof.
                valid=not changes or (name in ('draft-pr-check','preflight-latest-check') and len(changes)==2)
                self.assertEqual(result.returncode==0,valid,(name,changes,result.stderr))
                if not valid:self.assertEqual(output.read_text(),'')
                elif name!='merge-gate-shards':
                    self.assertEqual(output.read_text(),self.payload(subject_sha=dict(env,**changes)['MERGE_GROUP_SHA']))

    def test_native_metadata_guards_run_without_checkout(self):
        clean=Path(self.temp)/'clean-runner';clean.mkdir()
        env=dict(self.env,GITHUB_OUTPUT=str(clean/'outputs'))
        for key in subprocess.check_output(['git','rev-parse','--local-env-vars'],text=True).split():
            env.pop(key,None)
        env['GIT_CEILING_DIRECTORIES']=str(clean.parent)
        self.assertNotEqual(subprocess.run(['git','rev-parse','HEAD'],cwd=clean,env=env,
                                          capture_output=True).returncode,0)
        for name in ('draft-pr-check','preflight-latest-check'):
            for event in ('merge_group','workflow_dispatch'):
                env.update(GITHUB_EVENT_NAME=event,EVENT_NAME=event,CURRENT=IDENTITY['subject_sha'])
                Path(env['GITHUB_OUTPUT']).write_text('')
                job=self.jobs[name]
                guard=next(step for step in job['steps'] if step.get('id') in ('guard','check'))
                script=guard['run'].replace('${{ github.event_name }}',event)
                # Other expressions occur only in the unselected PR branch;
                # substitute data as Actions does before shell parsing.
                import re
                script=re.sub(r'\$\{\{.*?\}\}', '', script)
                actual=subprocess.run(['bash','-c',script],cwd=clean,env=env,capture_output=True,text=True)
                self.assertEqual(actual.returncode,0,actual.stderr)
                self.assertIn('run_ci=true' if name=='draft-pr-check' else 'is_latest=true',
                              Path(env['GITHUB_OUTPUT']).read_text())
                stamp=next(step for step in job['steps'] if step.get('id')=='subject')
                self.assertEqual(stamp['if'], "success() && github.event_name == 'merge_group'")
                if event=='merge_group':
                    actual=subprocess.run(['bash','-c',stamp['run']],cwd=clean,env=env,capture_output=True,text=True)
                    self.assertEqual(actual.returncode,0,actual.stderr)
                    self.assertIn(self.payload(),Path(env['GITHUB_OUTPUT']).read_text())

    def test_dispatch_retains_diagnostics_without_native_identity_or_artifacts(self):
        for key in list(self.env):
            if '_PROOF_' in key or key=='MERGE_GROUP_SHA':self.env.pop(key)
        self.env['GITHUB_EVENT_NAME']='workflow_dispatch'
        self.assertEqual(self.final().returncode,0)
        for field in ('COMPILE_RESULT','INTEGRATION_RESULT'):
            for value in ('failure','skipped','cancelled',''):
                self.assertNotEqual(self.final(**{field:value}).returncode,0)
        for name in (*SCALARS,'merge-gate','merge-gate-shards'):
            for step in self.jobs[name]['steps']:
                if step.get('name') in ('Bind successful native integration producer',
                    'Preserve successful shard identity','Download current-attempt shard identities',
                    'Verify every selected producer identity'):
                    self.assertEqual(step['if'], "success() && github.event_name == 'merge_group'")
        # Dispatch uses the unchanged fail-closed selected dependency evaluator.
        from scripts.ci.evaluate_ci_gate import evaluate
        for proof in self.needs.values():proof['outputs']={}
        self.needs['draft-pr-check']['outputs']['run_ci']='true'
        self.needs['preflight-latest-check']['outputs']['is_latest']='true'
        self.assertEqual(evaluate(self.needs,event_name='workflow_dispatch').status,'success')
        self.needs['merge-gate-shards']['result']='failure'
        self.assertNotIn(evaluate(self.needs,event_name='workflow_dispatch').status,('success','scoped_noop'))

    def test_success_only_producer_wiring_and_complete_matrix_names(self):
        for name in (*SCALARS,'merge-gate','merge-gate-shards'):
            producer=next(s for s in self.jobs[name]['steps'] if s.get('name')=='Bind successful native integration producer')
            self.assertEqual(producer['if'], "success() && github.event_name == 'merge_group'")
            self.assertNotIn('continue-on-error',producer)
            self.assertIn('GITHUB_RUN_ATTEMPT',producer['run'])
            if name not in ('draft-pr-check','preflight-latest-check'):self.assertIn('git rev-parse HEAD',producer['run'])
            if name != 'merge-gate-shards':
                for key in IDENTITY:self.assertEqual(self.jobs[name]['outputs'][key],'${{ steps.subject.outputs.'+key+' }}')
        self.assertEqual(tuple(x['name'] for x in self.jobs['merge-gate-shards']['strategy']['matrix']['include']),SHARDS)
        upload=self.jobs['merge-gate-shards']['steps'][-1]
        self.assertEqual(upload['if'], "success() && github.event_name == 'merge_group'")
        self.assertEqual(upload['with']['name'],'integration-${{ github.sha }}-${{ github.run_id }}-${{ github.run_attempt }}-${{ matrix.name }}')
        self.assertEqual(upload['with']['if-no-files-found'],'error')
        names=[s.get('name') for s in self.jobs['merge-gate']['steps']]
        self.assertLess(names.index('Gate summary'),names.index('Verify every selected producer identity'))
        self.assertLess(names.index('Verify every selected producer identity'),names.index('Bind successful native integration producer'))
        for prefix,job in (('COMPILE','check-all-targets'),('INTEGRATION','merge-gate')):
            for suffix,key in zip(('SHA','RUN_ID','RUN_ATTEMPT'),IDENTITY):
                self.assertEqual(self.jobs['integration-required']['steps'][0]['env'][prefix+'_PROOF_'+suffix], '${{ needs.'+job+'.outputs.'+key+' }}')

if __name__ == '__main__': unittest.main()
