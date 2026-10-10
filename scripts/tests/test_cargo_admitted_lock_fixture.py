"""Finite borrowed-guard union eligibility and instrument falsifiers."""
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import Mock, patch

import test_cargo_admitted_nested as support

ROW = 'xtask-lock-union-fixture'
TEST_ROW = 'xtask-lock-union-test'
TEST_NAME = ('tasks::check_lint_policy::tests::lock_partition::'
             'the_two_rows_jointly_cover_every_borrowed_guard_discard')
ARGV = ('clippy', '--offline', '--quiet', '--lib', '--no-deps', '--message-format=json',
        '--', '--force-warn', 'let_underscore_lock', '--force-warn', 'clippy::let_underscore_lock')


class LockContractTests(unittest.TestCase):
    def test_fixed_lock_keeps_observed_native_cargo_serialization(self):
        # Native Cargo 1.95.0 rewrote only the header/list formatting in the
        # first source-bound run. Exact measured bytes prevent that mutation
        # without relaxing the owner's pre/post input identity checks.
        lock = (Path(__file__).resolve().parents[2] /
                '.spec/17479-nested-admission/lock-union-fixture/Cargo.lock')
        self.assertEqual(hashlib.sha256(lock.read_bytes()).hexdigest(),
                         'ff2a47a9ba4466ff072f8b5c2e61193fc9d3dee0c659a1eb27612f1bc309a5d3')

    def test_exact_finite_union_command(self):
        self.assertEqual(support.a.NESTED_COMMANDS.get(ROW), ARGV)

    def test_exact_one_owning_test(self):
        self.assertEqual(support.a.NESTED_COMMANDS.get(TEST_ROW),
                         ('test', '-p', 'xtask', '--bin', 'xtask', '--locked',
                          '--message-format=json', TEST_NAME,
                          '--', '--exact', '--test-threads=1', '--color', 'never'))

    def test_exact_repaired_control_can_reuse_same_harness(self):
        self.assertEqual(support.a.NESTED_COMMANDS.get('xtask-preparation-control-test'),
                         ('test', '-p', 'xtask', '--bin', 'xtask', '--locked',
                          '--message-format=json',
                          'tasks::gates::tests::failed_preparation_prevents_runtime_in_every_tier_and_keeps_backstops',
                          '--', '--exact', '--test-threads=1', '--color', 'never'))


class LockFixtureTests(unittest.TestCase):
    def setUp(self):
        self.n=support.NestedTests();self.n.setUp();self.addCleanup(self.n.doCleanups)
        self.root=self.n.bind_lock_fixture();self.a=support.a
        path=Path(os.environ.get('LOCK_ADAPTER_TEST_FILE',str(Path(__file__).resolve().parents[1]/'ci/lock_union_prepare.py')))
        spec=importlib.util.spec_from_file_location('lock_adapter',path)
        self.f=importlib.util.module_from_spec(spec);spec.loader.exec_module(self.f)
        for module in [self.f,self.f.shared]:
            p=patch.object(module,'owner',self.a);p.start();self.addCleanup(p.stop)

    def events(self):
        lines=(self.root/'src/lib.rs').read_text().splitlines()
        expected=[('let _ = dropped_std_mutex.lock();','let_underscore_lock'),
                  ('let _ = dropped_std_rwlock.read();','let_underscore_lock'),
                  ('let _ = dropped_pl_mutex.lock();','clippy::let_underscore_lock'),
                  ('let _ = dropped_pl_rwlock.write();','clippy::let_underscore_lock')]
        events=[]
        for statement,lint in expected:
            number=next(i+1 for i,line in enumerate(lines) if line.strip()==statement)
            events.append({'reason':'compiler-message','manifest_path':str(self.root/'Cargo.toml'),
                           'target':{'src_path':str(self.root/'src/lib.rs')},
                           'message':{'level':'warning','code':{'code':lint},
                                      'spans':[{'is_primary':True,'file_name':'src/lib.rs','line_start':number,
                                                'text':[{'text':lines[number-1]}]}]}})
        return events+[{'reason':'build-finished','success':True}]

    def output(self,events=None):return ''.join(json.dumps(e)+'\n' for e in (self.events() if events is None else events))

    def test_live_zero_exit_and_four_current_positive_sites(self):
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,self.output(),''))
        self.assertEqual(self.f.fixture(self.n.env,invoke),0);invoke.assert_called_once()

    def test_exact_command_and_private_roots(self):
        command,env,cwd=self.n.command(ROW);self.assertEqual(cwd,self.root)
        self.assertEqual(command[0],self.n.tool['subjects']['cargo-clippy']['path'])
        self.assertEqual(command[command.index('--target-dir')+2:],list(ARGV)[1:])
        self.assertEqual(env['CARGO_TARGET_DIR'],str(self.root/'target'))
        self.assertEqual(env['CARGO_BUILD_BUILD_DIR'],str(self.root/'build'))
        self.assertEqual(env['CLIPPY_CONF_DIR'],str(self.root))
        self.assertEqual(self.n.command(TEST_ROW)[1]['CARGO_ADMITTED_LOCK_PYTHON'],self.n.plan['lock_union_fixture']['python']['path'])

    def test_missing_swapped_duplicate_warning_or_stale_span_refuses(self):
        for kind in ['missing','swapped','duplicate','held','level','foreign','text','line','span']:
            events=self.events()
            if kind=='missing':events.pop(0)
            elif kind=='swapped':events[0]['message']['code']['code']='clippy::let_underscore_lock'
            elif kind=='duplicate':events.insert(0,events[0])
            elif kind=='held':
                import copy
                lines=(self.root/'src/lib.rs').read_text().splitlines();i=next(i for i,l in enumerate(lines) if 'let mut guard = held_std_mutex' in l)
                extra=copy.deepcopy(events[0]);extra['message']['spans'][0].update(line_start=i+1,text=[{'text':lines[i]}]);events.insert(-1,extra)
            elif kind=='level':events[0]['message']['level']='error'
            elif kind=='foreign':events[0]['manifest_path']='another/Cargo.toml'
            elif kind=='text':events[0]['message']['spans'][0]['text'][0]['text']='stale'
            elif kind=='line':events[0]['message']['spans'][0]['line_start']=999
            elif kind=='span':events[0]['message']['spans']=[]
            with self.subTest(kind=kind),self.assertRaises(self.a.Denied):self.f.diagnostic_success(self.output(events),0,self.root)

    def test_failed_corrupt_missing_duplicate_or_false_terminal_refuses(self):
        for code in [1,101,-9]:
            with self.subTest(code=code),self.assertRaises(self.a.Denied):self.f.diagnostic_success(self.output(),code,self.root)
        for output in ['not-json\n','[]\n',self.output(self.events()[:-1]),self.output()+self.output(),
                       self.output(self.events()[:-1]+[{'reason':'build-finished','success':False}]),self.output().replace('\n','\n\n',1)]:
            with self.subTest(output=output[:30]),self.assertRaises((self.a.Denied,ValueError)):self.f.diagnostic_success(output,0,self.root)

    def test_changed_inputs_config_adapter_and_both_roots_refuse(self):
        paths=[self.root/'Cargo.toml',self.root/'Cargo.lock',self.root/'clippy.toml',self.root/'src/lib.rs',
               Path(self.n.plan['lock_union_fixture']['adapter']['path']),
               Path(self.n.plan['lock_union_fixture']['support_adapter']['path'])]
        for p in paths:
            old=p.read_bytes();p.write_bytes(old+b'\n');invoke=Mock(return_value=subprocess.CompletedProcess([],0,self.output(),''))
            with self.subTest(path=p),self.assertRaises(self.a.Denied):self.f.fixture(self.n.env,invoke)
            invoke.assert_not_called();p.write_bytes(old)
            self.n.plan['lock_union_fixture']=self.a.lock_union_fixture(self.n.worktree,self.n.paths);self.n.save()
        for name in ['target','build']:
            p=self.root/name;p.rename(self.root/(name+'-old'));p.mkdir();invoke=Mock(return_value=subprocess.CompletedProcess([],0,self.output(),''))
            with self.subTest(root=name),self.assertRaises(self.a.Denied):self.f.fixture(self.n.env,invoke)
            invoke.assert_not_called();p.rmdir();(self.root/(name+'-old')).rename(p)

    def test_dependency_checksum_and_feature_provenance_refuses(self):
        template=self.n.worktree/'.spec/17479-nested-admission/lock-union-fixture'
        for filename,before,after in [('Cargo.toml','arc_lock','deadlock_detection'),
                                     ('Cargo.toml','=0.12.5','=0.12.4'),
                                     ('Cargo.lock','c4512299','deadbeef')]:
            p=template/filename;old=p.read_text();p.write_text(old.replace(before,after))
            with self.subTest(filename=filename,after=after),self.assertRaises(self.a.Denied):self.a.validate_lock_union_template(self.n.worktree)
            p.write_text(old)

    def test_missing_binding_membership_and_mutation_during_compile_refuse(self):
        binding=self.n.plan.pop('lock_union_fixture');self.n.save();invoke=Mock()
        with self.assertRaises(self.a.Denied):self.f.fixture(self.n.env,invoke)
        invoke.assert_not_called();self.n.plan['lock_union_fixture']=binding;self.n.save()
        self.n.plan['request']['rows'].remove(ROW);self.n.save()
        with self.assertRaises(self.a.Denied):self.f.fixture(self.n.env,invoke)
        invoke.assert_not_called();self.n.plan['request']['rows'].append(ROW);self.n.save()
        output=self.output()
        def change(*args,**kwargs):
            (self.root/'src/lib.rs').write_text('changed');return subprocess.CompletedProcess([],0,output,'')
        with self.assertRaises(self.a.Denied):self.f.fixture(self.n.env,change)

    def test_plan_binds_fixture_and_requires_leaf_before_admission(self):
        request=self.n.root/'request.json'
        request.write_text(json.dumps({'schema_version':1,'rows':[ROW,TEST_ROW]}))
        parent={k:v for k,v in self.n.env.items() if k not in ['CARGO','RUSTC','RUSTDOC','CARGO_ADMITTED_RESOURCES']}
        with patch.object(self.a,'clippy_toolchain',return_value=self.n.tool):
            plan=self.a.nested_plan(request,parent,self.n.worktree,self.n.paths)
            self.assertEqual(plan['lock_union_fixture'],self.n.plan['lock_union_fixture'])
            request.write_text(json.dumps({'schema_version':1,'rows':[TEST_ROW]}))
            with self.assertRaises(self.a.Denied):self.a.nested_plan(request,parent,self.n.worktree,self.n.paths)

    def test_late_configuration_selector_refuses(self):
        with patch.object(self.a,'nested_configuration',wraps=support.ORIGINAL_CONFIGURATION):
            self.n.plan['configuration']=self.a.nested_configuration(self.n.worktree,self.n.paths,(self.root,));self.n.save()
            (self.root/'.cargo').mkdir();(self.root/'.cargo/config.toml').write_text('[build]\nrustflags=["--cap-lints=allow"]\n')
            invoke=Mock()
            with self.assertRaises(self.a.Denied):self.f.fixture(self.n.env,invoke)
            invoke.assert_not_called()

    def test_owning_exact_named_result_and_capture_parameters(self):
        name=TEST_NAME;good=f'running 1 test\ntest {name} ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4791 filtered out; finished in 0.93s\n'
        for output in [good,good.replace(name,'another_test'),good.replace('1 passed','0 passed'),good.replace('0 ignored','1 ignored'),'']:
            invoke=Mock(return_value=subprocess.CompletedProcess([],0,output,''))
            with patch.object(self.f.shared,'capture_owning_artifact') as capture:
                if output==good:
                    self.assertEqual(self.f.shared.owning_test(self.n.env,invoke,ROW,TEST_ROW,TEST_NAME,'lock-union-owning'),0)
                    capture.assert_called_once_with(good,self.n.env,TEST_ROW,TEST_NAME,'lock-union-owning')
                else:
                    with self.assertRaises(self.a.Denied):self.f.shared.owning_test(self.n.env,invoke,ROW,TEST_ROW,TEST_NAME,'lock-union-owning')
                    capture.assert_not_called()
