"""Finite fixture composition and diagnostic falsifiers; no Cargo compilation."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import Mock, patch

import test_cargo_admitted_nested as support

spec = importlib.util.spec_from_file_location('fixture_adapter', Path(os.environ.get('FIXTURE_ADAPTER_TEST_FILE', str(Path(__file__).resolve().parents[1] / 'ci/disallowed_fields_prepare.py'))))
f = importlib.util.module_from_spec(spec)
spec.loader.exec_module(f)
a = support.a


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.n = support.NestedTests()
        self.n.setUp()
        self.addCleanup(self.n.doCleanups)
        self.root = self.n.bind_disallowed_fixture()
        p = patch.object(f, 'owner', a)
        p.start(); self.addCleanup(p.stop)

    def output(self, code='clippy::disallowed_fields', level='error', terminal=False):
        events = [{'reason':'compiler-message','manifest_path':str(self.root/'Cargo.toml'),
                   'target':{'src_path':str(self.root/'src/lib.rs')},
                   'message':{'code':{'code':code},'level':level,
                              'spans':[{'is_primary':True,'file_name':'src/lib.rs','line_start':4}]}},
                  {'reason':'build-finished','success':terminal}]
        return ''.join(json.dumps(event)+'\n' for event in events)

    def invoke(self):
        return Mock(return_value=subprocess.CompletedProcess([],101,self.output(),''))

    def test_fixed_command_cwd_both_roots_and_configuration(self):
        cmd, env, cwd = self.n.command(a.DISALLOWED_FIXTURE_ROW)
        self.assertEqual(cwd,self.root)
        self.assertEqual(env['CARGO_TARGET_DIR'],str(self.root/'target'))
        self.assertEqual(env['CARGO_BUILD_BUILD_DIR'],str(self.root/'build'))
        self.assertEqual(env['CLIPPY_CONF_DIR'],str(self.root))
        self.assertEqual(env['RUSTC_WORKSPACE_WRAPPER'],self.n.tool['subjects']['clippy-driver']['path'])
        self.assertEqual(cmd[0],self.n.tool['subjects']['cargo-clippy']['path'])
        position=cmd.index('--target-dir')
        self.assertEqual(cmd[position+2:],list(a.NESTED_COMMANDS[a.DISALLOWED_FIXTURE_ROW])[1:])
        self.assertEqual(self.n.command(a.DISALLOWED_TEST_ROW)[1]['CARGO_ADMITTED_FIXTURE_PYTHON'],self.n.plan['disallowed_fields_fixture']['python']['path'])

    def test_live_expected_failure_and_unchanged_parent_controls(self):
        invoke=self.invoke()
        self.assertEqual(f.fixture(self.n.env,invoke),101)
        invoke.assert_called_once()
        self.assertEqual(self.n.env['CARGO_TARGET_DIR'],str(self.n.paths['target']))
        self.assertTrue(self.n.lock.exists())

    def test_missing_configuration_refuses_before_compiler(self):
        (self.root/'clippy.toml').unlink();invoke=self.invoke()
        with self.assertRaises((a.Denied,OSError)):f.fixture(self.n.env,invoke)
        invoke.assert_not_called()

    def test_oversized_generated_input_refuses_before_hashing(self):
        (self.root/'Cargo.toml').write_bytes(b'x'*(a.BUDGET_FILE_LIMIT+1))
        with patch.object(a,'file_subject') as observe,self.assertRaises(a.Denied):
            a.disallowed_fixture(self.n.worktree,self.n.paths)
        observe.assert_not_called()

    def test_changed_field_access_refuses_before_compiler(self):
        p=self.root/'src/lib.rs';p.write_text(p.read_text().replace('range.start','range.end'))
        invoke=self.invoke()
        with self.assertRaises(a.Denied):f.fixture(self.n.env,invoke)
        invoke.assert_not_called()

    def test_changed_manifest_lock_template_and_adapter_refuse(self):
        for item in [('files','Cargo.toml','generated'),('files','Cargo.lock','generated'),
                     ('files','src/lib.rs','template')]:
            subject=self.n.plan['disallowed_fields_fixture'][item[0]][item[1]][item[2]]
            p=Path(subject['path']);old=p.read_bytes();p.write_bytes(old+b'\n')
            with self.subTest(item=item),self.assertRaises(a.Denied):self.n.command(a.DISALLOWED_FIXTURE_ROW)
            p.write_bytes(old)
            # Restoration changes original file identity: retain a fresh test
            # snapshot solely for the next independent mutation case.
            self.n.plan['disallowed_fields_fixture']=a.disallowed_fixture(self.n.worktree,self.n.paths);self.n.save()
        p=Path(self.n.plan['disallowed_fields_fixture']['adapter']['path']);p.write_text('changed')
        with self.assertRaises(a.Denied):self.n.command(a.DISALLOWED_FIXTURE_ROW)

    def test_changed_or_replaced_output_roots_refuse(self):
        for name in ['target','build']:
            p=self.root/name;p.rename(self.root/(name+'-original'));p.mkdir()
            invoke=self.invoke()
            with self.subTest(name=name),self.assertRaises(a.Denied):f.fixture(self.n.env,invoke)
            invoke.assert_not_called()
            p.rmdir();(self.root/(name+'-original')).rename(p)
        self.n.plan['disallowed_fields_fixture']['directories']['target']['path']=str(self.n.paths['target']);self.n.save()
        with self.assertRaises(a.Denied):self.n.command(a.DISALLOWED_FIXTURE_ROW)

    def test_linked_or_replaced_cwd_refuses(self):
        original=self.root.with_name('retained-original-fixture');self.root.rename(original)
        self.root.symlink_to(original,target_is_directory=True)
        with self.assertRaises(a.Denied):self.n.command(a.DISALLOWED_FIXTURE_ROW)

    def test_missing_fixture_binding_and_row_membership_refuse(self):
        self.n.plan.pop('disallowed_fields_fixture');self.n.save()
        for row in [a.DISALLOWED_FIXTURE_ROW,a.DISALLOWED_TEST_ROW]:
            with self.subTest(row=row),self.assertRaises(a.Denied):self.n.command(row)
        self.n.plan['disallowed_fields_fixture']=a.disallowed_fixture(self.n.worktree,self.n.paths)
        self.n.plan['request']['rows'].remove(a.DISALLOWED_FIXTURE_ROW);self.n.save()
        with self.assertRaises(a.Denied):f.fixture(self.n.env,self.invoke())

    def test_native_plan_captures_fixture_and_requires_member_leaf(self):
        request=self.n.root/'request.json'
        request.write_text(json.dumps({'schema_version':1,'rows':[a.DISALLOWED_FIXTURE_ROW,a.DISALLOWED_TEST_ROW]}))
        parent={k:v for k,v in self.n.env.items() if k not in ['CARGO','RUSTC','RUSTDOC','CARGO_ADMITTED_RESOURCES']}
        with patch.object(a,'clippy_toolchain',return_value=self.n.tool):
            plan=a.nested_plan(request,parent,self.n.worktree,self.n.paths)
            self.assertEqual(plan['disallowed_fields_fixture'],self.n.plan['disallowed_fields_fixture'])
            request.write_text(json.dumps({'schema_version':1,'rows':[a.DISALLOWED_TEST_ROW]}))
            with self.assertRaises(a.Denied):a.nested_plan(request,parent,self.n.worktree,self.n.paths)

    def test_fixture_configuration_discovery_changes_refuse(self):
        with patch.object(a,'nested_configuration',wraps=support.ORIGINAL_CONFIGURATION):
            self.n.plan['configuration']=a.nested_configuration(self.n.worktree,self.n.paths,(self.root,));self.n.save()
            (self.root/'.cargo').mkdir();(self.root/'.cargo/config.toml').write_text('[build]\nrustflags=["--cap-lints=allow"]\n')
            with self.assertRaises(a.Denied):self.n.command(a.DISALLOWED_FIXTURE_ROW)

    def test_wrong_diagnostic_warning_and_success_are_rejected(self):
        for stdout,status in [(self.output(code='clippy::other'),101),(self.output(level='warning'),101),
                              (self.output(terminal=True),101),(self.output(),0),(self.output(),-9)]:
            with self.subTest(stdout=stdout,status=status),self.assertRaises(a.Denied):f.diagnostic_success(stdout,status,self.root)

    def test_corrupt_json_missing_terminal_duplicate_terminal_and_stale_origin_rejected(self):
        for stdout in [self.output()+'not-json\n',self.output().splitlines()[0]+'\n',
                       self.output()+json.dumps({'reason':'build-finished','success':False})+'\n',
                       self.output().replace(str(self.root/'Cargo.toml'),'/foreign/Cargo.toml'),
                       self.output().replace('src/lib.rs','src/foreign.rs'), self.output().replace('"line_start": 4','"line_start": 1'),
                       '\n'+self.output(),self.output()+'\n']:
            with self.subTest(stdout=stdout),self.assertRaises(a.Denied):f.diagnostic_success(stdout,101,self.root)

    def test_changed_inputs_during_compilation_refuse(self):
        def changed(*args,**kwargs):
            (self.root/'clippy.toml').write_text('disallowed-fields=[]\n')
            return subprocess.CompletedProcess([],101,self.output(),'')
        with self.assertRaises(a.Denied):f.fixture(self.n.env,changed)

    def test_owning_exact_named_test_and_missing_mode_membership(self):
        good='running 1 test\ntest '+a.DISALLOWED_TEST+' ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 99 filtered out; finished in 0.02s\n'
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,good,''))
        with patch.object(f,'capture_owning_artifact') as capture:
            self.assertEqual(f.owning_test(self.n.env,invoke),0)
            capture.assert_called_once_with(good,self.n.env)
        with patch.object(f,'capture_owning_artifact') as capture:
            for bad in [good.replace(a.DISALLOWED_TEST,'other'),good.replace('1 passed','0 passed'),good.replace('... ok','... ignored'),good.replace('running 1 test','running 0 tests')]:
                invoke.return_value=subprocess.CompletedProcess([],0,bad,'')
                with self.assertRaises(a.Denied):f.owning_test(self.n.env,invoke)
            capture.assert_not_called()
        self.n.plan['request']['rows'].remove(a.DISALLOWED_TEST_ROW);self.n.save();invoke.reset_mock()
        with self.assertRaises(a.Denied):f.owning_test(self.n.env,invoke)
        invoke.assert_not_called()

    def owning_artifact_output(self):
        path=self.n.paths['build']/'debug/deps/xtask-proof'
        path.parent.mkdir(parents=True,exist_ok=True);path.write_text('native test executable');path.chmod(0o755)
        event={'reason':'compiler-artifact','manifest_path':str(self.n.worktree/'xtask/Cargo.toml'),
               'target':{'name':'xtask','kind':['bin'],'src_path':str(self.n.worktree/'xtask/src/main.rs')},
               'profile':{'test':True},'executable':str(path),'filenames':[str(path)],'fresh':True}
        stdout=json.dumps(event)+'\n'+json.dumps({'reason':'build-finished','success':True})+'\n'
        return stdout,path

    def test_current_harness_artifact_copied_under_original_owner(self):
        stdout,path=self.owning_artifact_output();f.capture_owning_artifact(stdout,self.n.env)
        copy=self.n.paths['temp']/('disallowed-fields-owning-'+str(os.getpid()))
        self.assertEqual(copy.read_bytes(),path.read_bytes());self.assertEqual(copy.stat().st_mode&0o777,0o444)
        record=json.loads(copy.with_suffix('.json').read_text())
        self.assertEqual(record['artifact'],a.file_subject(path));self.assertTrue(self.n.lock.exists())
        self.assertTrue(record['copied_under_original_live_owner'])

    def test_missing_stale_duplicate_wrong_root_and_failed_harness_artifacts_refuse(self):
        stdout,path=self.owning_artifact_output()
        for bad in ['',stdout+stdout,stdout.replace('"success": true','"success": false'),
                    stdout.replace(str(path),str(self.n.paths['target']/'debug/xtask')),
                    stdout.replace('"test": true','"test": false')]:
            with self.subTest(stdout=bad),self.assertRaises((a.Denied,OSError)):f.capture_owning_artifact(bad,self.n.env)


if __name__ == '__main__':
    unittest.main()
