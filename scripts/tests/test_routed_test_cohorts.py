"""Staged producer/CLI controls and a checked-in Cargo runtime oracle fixture.

The resolver-2 fixture describes a real two-crate Cargo graph. A finite model
producer discriminates dropped -p selection through the owner interface here;
its outputs are explicitly synthetic. This suite checks fixture shape only; hosted
execution against pinned Cargo is still required.
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
try:
    import tomllib
except ModuleNotFoundError:
    try:
        import tomli as tomllib
    except ModuleNotFoundError as error:
        raise ImportError(
            "Producer proof tests require tomli on Python 3.10; "
            "install scripts/requirements.txt test dependencies"
        ) from error
import unittest
from unittest.mock import Mock, patch

import test_cargo_cohort as fixtures
import cargo_cohort as owner
import cargo_admitted as admitted

ROOT=Path(__file__).resolve().parents[2]
FIXTURE=Path(__file__).parent/'fixtures/routed-resolver-2'


@contextlib.contextmanager
def working_directory(path):
    previous = os.getcwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(previous)


@unittest.skipUnless(sys.platform=='linux', 'Linux ownership interface')
class ProducerControls(unittest.TestCase):
    def setUp(self):
        self.fixture=fixtures.PortableControls();self.fixture.setUp();self.addCleanup(self.fixture.doCleanups)
        self.env=dict(os.environ,CARGO_TARGET_DIR=str(self.fixture.paths['target']),CARGO_BUILD_BUILD_DIR=str(self.fixture.paths['build']),CARGO_HOME=str(self.fixture.paths['cargo_home']),TMPDIR=str(self.fixture.paths['temp']))

    def harness_producer(self, *, omission=False, duplicate=False, should_panic=False):
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
        if should_panic: body=body.replace("test cwd_and_feature ... ok", "test cwd_and_feature - should panic ... ok")
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

    def test_should_panic_display_suffix_preserves_inventory_identity(self):
        instance=self.fixture.owner()
        row=self.invoke(instance,self.harness_producer(should_panic=True),{str(self.fixture.exe):self.context(self.fixture.root)})
        self.assertEqual(row['status'],0)
        self.assertEqual(row['coverage'][0]['cases'],['cwd_and_feature','ignored'])
        self.assertFalse(self.fixture.exe.exists())

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
        with patch.object(owner,'children_capability',side_effect=owner.Denied('native children unavailable')), \
             patch.object(admitted,'resource_plan') as plan, \
             patch.object(Path,'mkdir',side_effect=AssertionError('refused route allocated a directory')) as allocation, \
             patch.object(admitted,'call_clippy') as product, \
             patch.object(admitted.subprocess,'Popen') as launch,contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(admitted.main(argv),75)
            plan.assert_not_called();allocation.assert_not_called()
            product.assert_not_called();launch.assert_not_called()

    def test_bootstrap_choice_and_scope_use_execution_worktree(self):
        legacy = ['run','--locked','-p','xtask','--bin','routed-test-cohorts','--']
        bootstrap = ['run','--locked','-p','routed-test-cohorts-bootstrap','--bin','routed-test-cohorts-bootstrap','--']
        suffix = ['-p','cohort-a','-p','cohort-b','--evidence-dir','/exact/evidence']
        other = self.fixture.root / 'other-wrapper'; (other / 'scripts').mkdir(parents=True)
        manifests = [root / 'crates/routed-test-cohorts-bootstrap/Cargo.toml' for root in (self.fixture.root, other)]
        for manifest in manifests:
            manifest.parent.mkdir(parents=True)
        self.fixture.marker.rmdir(); self.fixture.lease.rmdir()
        for present, direct in [(False,False),(True,False),(False,True),(True,True)]:
            with self.subTest(execution_present=present, explicit_bootstrap=direct):
                # The wrapper checkout has the opposite membership. Absolute
                # cross-checkout invocation must still use the execution root.
                for manifest, exists in zip(manifests, (present, not present)):
                    if manifest.exists(): manifest.unlink()
                    if exists: manifest.write_text('[package]\n')
                invocation = (bootstrap if direct else legacy) + suffix
                effective = (bootstrap if direct or present else legacy) + suffix
                output = io.StringIO()
                with patch.dict(os.environ, {}, clear=True), \
                     patch.object(admitted, '__file__', str(other / 'scripts/cargo_admitted.py')), \
                     patch.object(owner, 'children_capability', return_value='EXPLICIT INTERFACE DOUBLE'), \
                     patch.object(admitted, 'resource_plan', return_value=(self.fixture.root,self.fixture.slot,self.fixture.paths)), \
                     patch.object(admitted, 'check_capacity', return_value={'policy':'percent'}), \
                     patch.object(admitted, 'budget_scope', wraps=admitted.budget_scope) as scope, \
                     patch.object(Path, 'mkdir', side_effect=AssertionError('preflight allocated a directory')) as allocation, \
                     patch.object(admitted, 'call_clippy') as product, \
                     patch.object(admitted.subprocess, 'Popen') as launch, \
                     working_directory(self.fixture.root), contextlib.redirect_stderr(output):
                    self.assertEqual(admitted.main(['--routed-test-cohorts','--preflight',*invocation]),0)
                    allocation.assert_not_called();product.assert_not_called();launch.assert_not_called()
                    self.assertEqual(scope.call_args.args[0], effective)
                    receipt = json.loads(output.getvalue().split('cargo-admitted preflight: ',1)[1])
                    digest = hashlib.sha256(json.dumps(effective,ensure_ascii=True,separators=(',',':')).encode()).hexdigest()
                    self.assertEqual(receipt['scope']['cargo_argv_sha256'], digest)
                    self.assertEqual(receipt['scope']['worktree'], str(self.fixture.root))
                    self.assertFalse(receipt['lease_acquired'])
                    self.assertFalse(self.fixture.lease.exists())

    def test_staged_original_descriptor_overrides_fabricated_environment(self):
        self.fixture.marker.rmdir(); self.fixture.lease.rmdir()
        local_config = self.fixture.root / '.cargo/config.local.toml'
        local_config.parent.mkdir()
        fixture_env = {
            'CARGO_ADMITTED_COHORT_DESCRIPTOR': 'fabricated',
            'CARGO_CRATE_NAME': 'before-bootstrap',
            'LD_LIBRARY_PATH': '/private/cohort-loader',
            'CARGO_REGISTRIES_PRIVATE_TOKEN': 'do-not-copy',
        }
        for package, binary, configured in [
            ('xtask', 'routed-test-cohorts', False),
            ('xtask', 'routed-test-cohorts', True),
            ('routed-test-cohorts-bootstrap', 'routed-test-cohorts-bootstrap', False),
            ('routed-test-cohorts-bootstrap', 'routed-test-cohorts-bootstrap', True),
        ]:
            with self.subTest(bootstrap=package, local_config=configured):
                if local_config.exists(): local_config.unlink()
                if configured:
                    local_config.write_text('[env.CARGO_ADMITTED_COHORT_DESCRIPTOR]\n'
                                            'value = "fabricated-by-local-config"\n'
                                            'force = true\nrelative = true\n')
                argv = ['--routed-test-cohorts','run','--locked','-p',package,'--bin',binary,'--','-p','cohort-a']
                tree = Mock(); tree.finish.return_value = {'tree_settled':True,'cancelled':False}
                observed = {}
                output = io.StringIO()
                def product(command, env, lease, tree, operation):
                    descriptor = json.loads(env['CARGO_ADMITTED_COHORT_DESCRIPTOR'])
                    observed.update(descriptor=descriptor, command=command,
                                    lease_identity=list(admitted.directory_identity(lease)),
                                    marker_identity=list(admitted.directory_identity(Path(descriptor['lease_marker']))))
                    self.assertEqual(Path(descriptor['lease_marker']).parent, lease)
                    return 0
                with patch.dict(os.environ, fixture_env, clear=True), \
                     patch.object(owner, 'children_capability', return_value='EXPLICIT INTERFACE DOUBLE'), \
                     patch.object(admitted, 'resource_plan', return_value=(self.fixture.root,self.fixture.slot,self.fixture.paths)), \
                     patch.object(admitted, 'local_config_cli_args', wraps=admitted.local_config_cli_args) as local_args, \
                     patch.object(admitted, 'check_capacity', return_value={'policy':'percent'}), \
                     patch.object(admitted, 'ClippyTree', return_value=tree) as tree_factory, \
                     patch.object(admitted, 'call_clippy', side_effect=product) as launch, \
                     patch.object(admitted.subprocess, 'Popen', side_effect=AssertionError('unexpected real launch')), \
                     contextlib.redirect_stderr(output):
                    self.assertEqual(admitted.main(argv), 0)
                    launch.assert_called_once()
                    local_args.assert_called_once_with(self.fixture.root)
                    tree_factory.assert_called_once_with(child_discovery='proc-children')
                descriptor = observed['descriptor']
                self.assertEqual(descriptor['pid'], os.getpid())
                self.assertEqual(descriptor['worktree'], str(self.fixture.root))
                self.assertEqual(descriptor['child_discovery'], 'proc-children')
                self.assertEqual(descriptor['lease'], str(self.fixture.lease))
                self.assertEqual(descriptor['lease_identity'], observed['lease_identity'])
                self.assertEqual(descriptor['marker_identity'], observed['marker_identity'])
                runtime = descriptor['runtime_environment']
                self.assertEqual(runtime['CARGO_CRATE_NAME'], 'before-bootstrap')
                self.assertEqual(runtime['LD_LIBRARY_PATH'], '/private/cohort-loader')
                self.assertIsNone(runtime['CARGO_BIN_NAME'])
                self.assertNotIn('CARGO_REGISTRIES_PRIVATE_TOKEN', runtime)
                digest = hashlib.sha256(json.dumps(runtime, sort_keys=True).encode()).hexdigest()
                self.assertEqual(descriptor['runtime_environment_sha256'], digest)
                command = observed['command']
                # Cargo merges scalar fields left to right even when --config
                # mixes files and inline TOML. Parse the real conflicting file;
                # checking only explicit inline fields misses later overrides.
                settings = {}
                local_reads = 0
                for index, argument in enumerate(command):
                    if argument != '--config': continue
                    config_arg = command[index + 1]
                    if config_arg == str(local_config):
                        partial = tomllib.loads(local_config.read_text())
                        local_reads += 1
                    else:
                        partial = tomllib.loads(config_arg)
                    settings.update(partial.get('env', {}).get('CARGO_ADMITTED_COHORT_DESCRIPTOR', {}))
                self.assertEqual(local_reads, int(configured))
                self.assertEqual(command.count(str(local_config)), int(configured))
                self.assertEqual(set(settings), {'value', 'force', 'relative'})
                self.assertEqual(settings['value'], json.dumps(descriptor, sort_keys=True))
                self.assertIs(settings['force'], True)
                self.assertIs(settings['relative'], False)
                self.assertEqual(command[command.index('run') + 1:],
                                 ['--target-dir',str(self.fixture.paths['target']),*argv[2:]])
                receipt_line = next(line for line in output.getvalue().splitlines()
                                    if line.startswith('cargo-admitted resources: '))
                public_descriptor = json.loads(receipt_line.split(': ', 1)[1])
                self.assertNotIn('runtime_environment', public_descriptor)
                self.assertEqual(public_descriptor['runtime_environment_sha256'], digest)
                for private_value in fixture_env.values():
                    self.assertNotIn(private_value, output.getvalue())
                self.assertFalse(self.fixture.lease.exists())

    def test_ordinary_admission_does_not_inject_cohort_descriptor(self):
        self.fixture.marker.rmdir(); self.fixture.lease.rmdir()
        tree = Mock(); tree.finish.return_value = {'tree_settled':True,'cancelled':False}
        output = io.StringIO()
        with patch.dict(os.environ, {'CARGO_CRATE_NAME':'ordinary-context'}, clear=True), \
             patch.object(owner, 'children_capability') as cohort_capability, \
             patch.object(admitted, 'resource_plan', return_value=(self.fixture.root,self.fixture.slot,self.fixture.paths)), \
             patch.object(admitted, 'check_capacity', return_value={'policy':'percent'}), \
             patch.object(admitted, 'ClippyTree', return_value=tree), \
             patch.object(admitted, 'call_clippy', return_value=0) as product, \
             patch.object(admitted.subprocess, 'Popen', side_effect=AssertionError('unexpected real launch')), \
             contextlib.redirect_stderr(output):
            self.assertEqual(admitted.main(['check','--locked','-p','cohort-a','--lib']), 0)
            product.assert_called_once()
            cohort_capability.assert_not_called()
        command, env = product.call_args.args[:2]
        self.assertNotIn('CARGO_ADMITTED_COHORT_DESCRIPTOR', env)
        self.assertEqual(env['CARGO_CRATE_NAME'], 'ordinary-context')
        self.assertFalse(any('CARGO_ADMITTED_COHORT_DESCRIPTOR' in argument for argument in command))
        self.assertNotIn('runtime_environment', output.getvalue())
        self.assertFalse(self.fixture.lease.exists())


class SourceControls(unittest.TestCase):

    def test_cargo_binary_runtime_fixture_covers_enabled_disabled_and_target_kinds(self):
        manifest=tomllib.loads((FIXTURE/'b/Cargo.toml').read_text())
        bins={row['name']:row for row in manifest['bin']}
        self.assertEqual(bins['enabled_probe']['required-features'],['enabled-bin'])
        self.assertEqual(bins['unbuilt_probe']['required-features'],['unbuilt'])
        self.assertFalse(bins['enabled_probe']['test']);self.assertFalse(bins['unbuilt_probe']['test'])
        targets={row['name']:row for row in manifest['test']}
        self.assertEqual(targets['runtime_env']['required-features'],['enabled-bin'])
        benches={row['name']:row for row in manifest['bench']}
        self.assertFalse(benches['runtime_env']['harness'])
        self.assertEqual(benches['runtime_env']['required-features'],['enabled-bin'])
        integration=(FIXTURE/'b/tests/runtime_env.rs').read_text()
        benchmark=(FIXTURE/'b/benches/runtime_env.rs').read_text()
        for source in (integration,benchmark):
            self.assertIn('CARGO_BIN_EXE_enabled_probe',source)
            self.assertIn('CARGO_BIN_EXE_unbuilt_probe',source)
            self.assertIn('enabled_probe',source)
            self.assertIn('unbuilt_probe',source)

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
