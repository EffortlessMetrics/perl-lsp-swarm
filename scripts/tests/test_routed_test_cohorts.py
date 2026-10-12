"""Staged producer/CLI controls. No Cargo execution or resolver qualification.

The resolver-2 fixture describes a real two-crate Cargo graph. A finite model
producer discriminates dropped -p selection through the owner interface here;
its outputs are explicitly synthetic. The real graph still needs native Cargo.
"""
import contextlib
import hashlib
import io
import json
import os
import re
import subprocess
from pathlib import Path
import shutil
import sys
import tomllib
import unittest
from unittest.mock import Mock, patch

import test_cargo_cohort as fixtures
import cargo_cohort as owner
import cargo_admitted as admitted

ROOT=Path(__file__).resolve().parents[2]
FIXTURE=Path(__file__).parent/'fixtures/routed-resolver-2'


@unittest.skipUnless(sys.platform=='linux', 'Linux ownership interface')
class ProducerControls(unittest.TestCase):
    def setUp(self):
        self.fixture=fixtures.PortableControls();self.fixture.setUp();self.addCleanup(self.fixture.doCleanups)
        self.env=dict(os.environ,CARGO_TARGET_DIR=str(self.fixture.paths['target']),CARGO_BUILD_BUILD_DIR=str(self.fixture.paths['build']),CARGO_HOME=str(self.fixture.paths['cargo_home']),TMPDIR=str(self.fixture.paths['temp']))

    def harness_producer(self, *, omission=False, duplicate=False):
        body='''import os,sys
if '--list' in sys.argv:
 print('ignored: test')
 if '--ignored' not in sys.argv: print('cwd_and_feature: test')
 sys.exit(0)
assert os.getcwd()==os.environ['CARGO_MANIFEST_DIR'], 'package cwd changed'
assert os.environ['FEATURE_RECEIPT']=='activated', 'feature context changed'
print('running 2 tests')
print('test cwd_and_feature ... ok')
print('test ignored ... ignored')
print('test result: ok. 1 passed; 0 failed; 1 ignored')
'''
        if omission: body=body.replace("print('test cwd_and_feature ... ok')",'pass')
        if duplicate: body=body.replace("print('test cwd_and_feature ... ok')", "print('test cwd_and_feature ... ok')\nprint('test cwd_and_feature ... ok')")
        body='#!'+sys.executable+'\n'+body
        return self.fixture.producer(mutation='p.write_text('+repr(body)+');p.chmod(0o700)')

    def invoke(self, instance, producer, consumers):
        with patch.object(owner,'children_capability',return_value='EXPLICIT INTERFACE DOUBLE'),patch.object(owner,'ClippyTree',fixtures.PortableTree):
            return instance.run(producer,consumers,self.env)

    def context(self, cwd):
        return {'args':[], 'cwd':str(cwd),'env':{'CARGO_MANIFEST_DIR':str(cwd),'FEATURE_RECEIPT':'activated'},'verify_harness':True}

    def test_package_cwd_and_exact_case_ignored_inventory(self):
        package=self.fixture.root/'package';package.mkdir()
        instance=self.fixture.owner()
        row=self.invoke(instance,self.harness_producer(),{str(self.fixture.exe):self.context(package)})
        self.assertEqual(row['coverage'][0]['cases'],['cwd_and_feature','ignored'])
        self.assertEqual(row['coverage'][0]['ignored'],['ignored'])
        self.assertFalse(self.fixture.exe.exists())
        authorization=json.loads(instance.receipt_path.read_text().splitlines()[0])
        self.assertEqual(authorization['coverage'],row['coverage'])

    def test_wrong_cwd_is_real_consumer_failure_and_retains_executable(self):
        package=self.fixture.root/'package';package.mkdir()
        specification=self.context(package);specification['cwd']=str(self.fixture.root)
        instance=self.fixture.owner()
        row=self.invoke(instance,self.harness_producer(),{str(self.fixture.exe):specification})
        self.assertNotEqual(row['status'],0);self.assertTrue(self.fixture.exe.exists());self.assertEqual(row['reclaimed'],[])

    def test_successful_harness_omission_or_duplicate_refuses_cleanup(self):
        for omission,duplicate in [(True,False),(False,True)]:
            with self.subTest(omission=omission,duplicate=duplicate):
                if self.fixture.exe.exists():self.fixture.exe.unlink()
                instance=self.fixture.owner()
                with self.assertRaises(owner.Denied):self.invoke(instance,self.harness_producer(omission=omission,duplicate=duplicate),{str(self.fixture.exe):self.context(self.fixture.root)})
                self.assertTrue(self.fixture.exe.exists());self.assertFalse(instance.receipt_path.exists())

    def test_observation_has_no_cleanup_or_consumer_authority(self):
        instance=self.fixture.owner(expected_targets=[])
        with patch.object(owner,'children_capability',return_value='EXPLICIT INTERFACE DOUBLE'),patch.object(owner,'ClippyTree',fixtures.PortableTree):
            row=instance.observe(self.fixture.producer(),self.env)
        self.assertEqual(row['status'],0);self.assertEqual(row['consumed'],[]);self.assertEqual(row['reclaimed'],[])
        self.assertTrue(self.fixture.exe.exists())

    def test_two_crate_model_dropping_activating_package_rejected(self):
        graph=self.fixture.root/'graph';shutil.copytree(FIXTURE,graph)
        workspace=tomllib.loads((graph/'Cargo.toml').read_text())
        activation=tomllib.loads((graph/'b/Cargo.toml').read_text())['dev-dependencies']['cohort-a']['features']
        self.assertEqual(workspace['workspace']['resolver'],'2');self.assertEqual(activation,['activated'])
        target=dict(fixtures.TARGET,target=dict(fixtures.TARGET['target'],src_path=str(graph/'a/tests/shared.rs')),features=activation)
        for packages in [('cohort-a','cohort-b'),('cohort-a',)]:
            with self.subTest(packages=packages):
                if self.fixture.exe.exists():self.fixture.exe.unlink()
                instance=self.fixture.owner(expected_targets=[target])
                # A synthetic finite feature model, not an executed Cargo oracle.
                command=self.fixture.producer()
                actual=dict(target,features=activation if 'cohort-b' in packages else [])
                command[2]=command[2][:command[2].index('print(json.dumps')]+ 'print('+repr(json.dumps(dict(actual,reason='compiler-artifact',executable=str(self.fixture.exe),fresh=False)))+')\n'
                if len(packages)==2:
                    row=self.invoke(instance,command,{str(self.fixture.exe):[]});self.assertEqual(row['status'],0)
                else:
                    with self.assertRaises(owner.Denied):self.invoke(instance,command,{str(self.fixture.exe):[]})
                    self.assertTrue(self.fixture.exe.exists())


    def test_bootstrap_runtime_fields_are_restored_without_inventing_crate_name(self):
        instance=self.fixture.owner()
        instance.descriptor['runtime_environment']={'CARGO_CRATE_NAME':None,'CARGO_BIN_NAME':None,'LD_LIBRARY_PATH':'/original/loader'}
        instance.descriptor['runtime_environment_sha256']=hashlib.sha256(json.dumps(instance.descriptor['runtime_environment'],sort_keys=True).encode()).hexdigest()
        body='#!'+sys.executable+'\nimport os\nassert "CARGO_CRATE_NAME" not in os.environ\nassert "CARGO_BIN_NAME" not in os.environ\nassert os.environ["LD_LIBRARY_PATH"]=="/original/loader"\n'
        producer=self.fixture.producer(mutation='import os\nassert \"CARGO_CRATE_NAME\" not in os.environ\nassert os.environ[\"LD_LIBRARY_PATH\"]==\"/original/loader\"\np.write_text('+repr(body)+');p.chmod(0o700)')
        with patch.dict(self.env,{'CARGO_CRATE_NAME':'xtask','CARGO_BIN_NAME':'routed-test-cohorts','LD_LIBRARY_PATH':'/bootstrap/loader'}):
            row=self.invoke(instance,producer,{str(self.fixture.exe):[]})
        self.assertEqual(row['status'],0)

    def test_pinned_public_copy_replacement_refuses_cleanup(self):
        public=self.fixture.root/'public-proof';public.write_bytes(b'public')
        subject={'sha256':hashlib.sha256(public.read_bytes()).hexdigest(),'file_identity':list(owner.facts(public.stat()))}
        instance=self.fixture.owner(pinned=[public],pinned_subjects={str(public):subject})
        body='#!'+sys.executable+'\nfrom pathlib import Path\np=Path('+repr(str(public))+')\np.rename(p.with_name("public-preserved"))\np.write_bytes(b"public")\n'
        producer=self.fixture.producer(mutation='p.write_text('+repr(body)+');p.chmod(0o700)')
        with self.assertRaises(owner.Denied):self.invoke(instance,producer,{str(self.fixture.exe):[]})
        self.assertTrue(self.fixture.exe.exists());self.assertTrue(public.exists())


    def test_runtime_descriptor_cannot_expand_to_resource_environment(self):
        self.fixture.descriptor['runtime_environment']={'CARGO_TARGET_DIR':'/foreign'}
        self.fixture.descriptor['runtime_environment_sha256']=hashlib.sha256(json.dumps(self.fixture.descriptor['runtime_environment'],sort_keys=True).encode()).hexdigest()
        with patch.object(owner.subprocess,'Popen') as launch:
            with self.assertRaises(owner.Denied):self.fixture.owner()
            launch.assert_not_called()

    def test_consumer_cannot_override_native_resource_or_owner_context(self):
        instance=self.fixture.owner()
        specification={'args':[],'cwd':str(self.fixture.root),'env':{'CARGO_TARGET_DIR':'/foreign'}}
        with self.assertRaises(owner.Denied):self.invoke(instance,self.fixture.producer(),{str(self.fixture.exe):specification})
        self.assertTrue(self.fixture.exe.exists())

    def test_build_script_context_difference_refuses_consumer_and_cleanup(self):
        instance=self.fixture.owner()
        specification={'args':[],'cwd':str(self.fixture.root),'env':{},'build_scripts':[{'package_id':'fixture','env':[['CUSTOM','expected']],'linked_paths':[],'out_dir':'/expected'}]}
        with self.assertRaises(owner.Denied):self.invoke(instance,self.fixture.producer(),{str(self.fixture.exe):specification})
        self.assertTrue(self.fixture.exe.exists())


    def test_public_copy_handles_partial_writes_and_destination_refusal(self):
        source=(ROOT/'xtask/src/bin/routed-test-cohorts.rs').read_text()
        program=re.search(r'let copy_program = r#"(.*?)"#;',source,re.S).group(1)
        original=self.fixture.root/'original-product';original.write_bytes(b'public executable'*10000);original.chmod(0o700)
        copied=self.fixture.root/'copied-product'
        real_write=os.write
        def partial(fd,data):return real_write(fd,data[:7])
        with patch.object(sys,'argv',['copy',str(original),str(copied)]),patch.object(os,'write',side_effect=partial),contextlib.redirect_stdout(io.StringIO()) as output:
            exec(compile(program,'native-public-copy','exec'),{})
        receipt=json.loads(output.getvalue())
        self.assertEqual(copied.read_bytes(),original.read_bytes())
        self.assertEqual(receipt['sha256'],hashlib.sha256(original.read_bytes()).hexdigest())
        self.assertEqual(receipt['copy_identity'],list(owner.facts(copied.stat())))
        before=set(Path('/proc/self/fd').iterdir())
        with patch.object(sys,'argv',['copy',str(original),str(copied)]):
            with self.assertRaises(FileExistsError):exec(compile(program,'native-public-copy','exec'),{})
        after=set(Path('/proc/self/fd').iterdir())
        self.assertEqual(before,after, 'destination refusal must close source observation')
        self.assertEqual(copied.read_bytes(),original.read_bytes())

    def test_staged_bootstrap_refuses_missing_native_capability_before_cargo(self):
        argv=['--routed-test-cohorts','run','--locked','-p','xtask','--bin','routed-test-cohorts','--','-p','cohort-a']
        with patch.object(owner,'children_capability',side_effect=owner.Denied('native children unavailable')),patch.object(admitted.subprocess,'Popen') as launch,contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(admitted.main(argv),75);launch.assert_not_called()

    def test_staged_original_descriptor_overrides_fabricated_environment(self):
        argv=['--routed-test-cohorts','run','--locked','-p','xtask','--bin','routed-test-cohorts','--','-p','cohort-a']
        self.fixture.marker.rmdir();self.fixture.lease.rmdir()
        tree=Mock();tree.finish.return_value={'tree_settled':True,'cancelled':False}
        observed={}
        def product(command,env,lease,tree,operation):
            observed['descriptor']=json.loads(env['CARGO_ADMITTED_COHORT_DESCRIPTOR']);observed['command']=command
            return 0
        fixture_env={'MIN_FREE_GB':'.001','MAX_USED_PCT':'100','CARGO_ADMITTED_COHORT_DESCRIPTOR':'fabricated','CARGO_CRATE_NAME':'before-bootstrap','CARGO_REGISTRIES_PRIVATE_TOKEN':'do-not-copy'}
        with patch.dict(os.environ,fixture_env,clear=True),patch.object(owner,'children_capability',return_value='EXPLICIT INTERFACE DOUBLE'),patch.object(admitted,'resource_plan',return_value=(self.fixture.root,self.fixture.slot,self.fixture.paths)),patch.object(admitted,'ClippyTree',return_value=tree),patch.object(admitted,'call_clippy',side_effect=product),contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(admitted.main(argv),0)
        descriptor=observed['descriptor'];self.assertEqual(descriptor['pid'],os.getpid());self.assertEqual(descriptor['worktree'],str(self.fixture.root))
        self.assertIn('lease_identity',descriptor);self.assertIn('marker_identity',descriptor)
        self.assertEqual(descriptor['runtime_environment']['CARGO_CRATE_NAME'],'before-bootstrap')
        self.assertNotIn('CARGO_REGISTRIES_PRIVATE_TOKEN',descriptor['runtime_environment'])
        self.assertTrue(any('env.CARGO_ADMITTED_COHORT_DESCRIPTOR.force=true'==argument for argument in observed['command']))


class SourceControls(unittest.TestCase):

    @unittest.skipUnless(sys.platform=='linux', 'native entrypoint refusal')
    def test_real_staged_entrypoint_refuses_before_bootstrap_on_missing_capability(self):
        try:owner.children_capability()
        except owner.Denied:pass
        else:self.skipTest('host supports discovery; this negative does not launch a Rust bootstrap')
        command=[sys.executable,str(ROOT/'scripts/cargo_admitted.py'),'--routed-test-cohorts','run','--locked','-p','xtask','--bin','routed-test-cohorts','--','-p','xtask']
        result=subprocess.run(command,cwd=ROOT,capture_output=True,text=True,timeout=5)
        self.assertEqual(result.returncode,75,result.stderr)
        self.assertIn('readable live-thread proc children',result.stderr)
        self.assertNotIn('cargo-admitted resources:',result.stderr)

    def test_gate_activation_remains_original_and_source_proof_is_wired(self):
        policy=(ROOT/'.ci/gate-policy.yaml').read_text()
        gate=policy.split('  - name: unit_routed_full\n',1)[1].split('\n  # ===',1)[0]
        self.assertIn('command: cargo build -p perllsp --locked && env PERL_LSP_BIN="$PWD/target/debug/perllsp" cargo test --locked --tests {package_args}',gate)
        workflow=(ROOT/'.github/workflows/ci-gate-self-tests.yml').read_text()
        for path in ('xtask/src/bin/routed-test-cohorts.rs','scripts/tests/test_routed_test_cohorts.py','scripts/tests/fixtures/routed-resolver-2/**'):
            self.assertIn("      - '"+path+"'",workflow)
        self.assertIn("run: python3 -m unittest discover -s scripts/tests -p 'test_routed_test_cohorts.py' -v",workflow)


if __name__=='__main__':unittest.main()
