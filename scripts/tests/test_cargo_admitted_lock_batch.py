"""Finite five-test selection and independent lint measurement falsifiers."""
import inspect
import json
import subprocess
import unittest
from unittest.mock import Mock, patch

import test_cargo_admitted_lock_fixture as support

PREFIX = 'tasks::check_lint_policy::tests::lock_partition::'
NAMES = tuple(PREFIX + name for name in (
    'clippy_row_covers_parking_lot_guards_and_not_standard_library',
    'discards_outside_the_governed_forms_are_covered_by_neither_row',
    'explicit_drop_discards_are_covered_by_no_clippy_lint_at_any_level',
    'let_underscore_must_use_owns_owned_and_mapped_guard_discards',
    'rustc_row_covers_standard_library_guards_and_not_parking_lot',
))
BATCH = 'xtask-lock-remaining-test'
BASE = ('clippy', '--offline', '--quiet', '--lib', '--no-deps', '--message-format=json', '--')
FLAGS = {
    'xtask-lock-union-fixture': ('--force-warn', 'let_underscore_lock', '--force-warn', 'clippy::let_underscore_lock'),
    'xtask-lock-rustc-fixture': ('-A', 'clippy::let_underscore_lock', '--force-warn', 'let_underscore_lock'),
    'xtask-lock-clippy-fixture': ('-A', 'let_underscore_lock', '--force-warn', 'clippy::let_underscore_lock'),
    'xtask-lock-must-use-fixture': ('-A', 'let_underscore_lock', '-A', 'clippy::let_underscore_lock', '--force-warn', 'clippy::let_underscore_must_use'),
    'xtask-lock-sweep-fixture': ('--force-warn', 'let_underscore_lock', '--force-warn', 'clippy::let_underscore_lock',
                                '-W', 'clippy::all', '-W', 'clippy::pedantic', '-W', 'clippy::nursery', '-W', 'clippy::restriction'),
}
MODE_ROWS = dict(zip(('union','rustc','clippy','must-use','sweep'), FLAGS))
STD = ('let _ = dropped_std_mutex.lock();', 'let _ = dropped_std_rwlock.read();')
PL = ('let _ = dropped_pl_mutex.lock();', 'let _ = dropped_pl_rwlock.write();')
OWNED = ('let _ = dropped_arc_mutex.lock_arc();', 'let _ = dropped_arc_rwlock.write_arc();',
         'let _ = PlMutexGuard::map(dropped_mapped_pl_guard, |value| &mut value.0);')


class LockBatchTests(unittest.TestCase):
    def setUp(self):
        support.LockFixtureTests.setUp(self)
    def available(self):
        for row in FLAGS:
            self.assertIn(row, self.a.NESTED_COMMANDS)
        self.assertIn(BATCH, self.a.NESTED_COMMANDS)

    def findings(self, measurement):
        mappings = {
            'union': [(s,'let_underscore_lock') for s in STD]+[(s,'clippy::let_underscore_lock') for s in PL],
            'rustc': [(s,'let_underscore_lock') for s in STD],
            'clippy': [(s,'clippy::let_underscore_lock') for s in PL],
            'must-use': [(s,'clippy::let_underscore_must_use') for s in OWNED],
            'sweep': [(s,'let_underscore_lock') for s in STD]+[(s,'clippy::let_underscore_lock') for s in PL]+[(s,'clippy::let_underscore_must_use') for s in OWNED],
        }
        return [self.finding(statement,lint) for statement,lint in mappings[measurement]]

    def finding(self, statement, lint):
        lines=(self.root/'src/lib.rs').read_text().splitlines()
        number=next(i+1 for i,line in enumerate(lines) if line.strip()==statement)
        return {'reason':'compiler-message','manifest_path':str(self.root/'Cargo.toml'),
                'target':{'src_path':str(self.root/'src/lib.rs')},
                'message':{'code':{'code':lint},'level':'warning','spans':[{'is_primary':True,
                    'file_name':'src/lib.rs','line_start':number,'text':[{'text':lines[number-1]}]}]}}

    def measurement_output(self, mode, events=None):
        return ''.join(json.dumps(x)+'\n' for x in ((self.findings(mode) if events is None else events)+[{'reason':'build-finished','success':True}]))

    def test_batch_literal_five_exact_names_and_each_lint_tuple(self):
        self.available()
        self.assertEqual(tuple(self.a.LOCK_FIXTURE_ROWS),tuple(FLAGS))
        self.assertEqual(self.a.NESTED_COMMANDS[BATCH],
            ('test','-p','xtask','--bin','xtask','--locked','--message-format=json','--','--exact',
             *NAMES,'--test-threads=1','--color','never'))
        for row,flags in FLAGS.items():
            self.assertEqual(self.a.NESTED_COMMANDS[row],BASE+flags)
            command,child,cwd=self.n.command(row)
            self.assertEqual(cwd,self.root)
            self.assertEqual(child['CARGO_TARGET_DIR'],str(self.root/'target'))
            self.assertEqual(child['CARGO_BUILD_BUILD_DIR'],str(self.root/'build'))
            self.assertEqual(child['CLIPPY_ARGS'],''.join(x+'__CLIPPY_HACKERY__' for x in flags))
        _,child,_=self.n.command(BATCH)
        self.assertEqual(child['CARGO_ADMITTED_LOCK_PYTHON'],self.n.plan['lock_union_fixture']['python']['path'])

    def test_batch_plan_requires_all_finite_measurement_rows(self):
        self.available();request=self.n.root/'request.json'
        parent={k:v for k,v in self.n.env.items() if k not in ('CARGO','RUSTC','RUSTDOC','CARGO_ADMITTED_RESOURCES')}
        with patch.object(self.a,'clippy_toolchain',return_value=self.n.tool):
            for missing in (None,*FLAGS):
                rows=[r for r in FLAGS if r!=missing]+[BATCH];request.write_text(json.dumps({'schema_version':1,'rows':rows}))
                if missing is None:self.assertIn('lock_union_fixture',self.a.nested_plan(request,parent,self.n.worktree,self.n.paths))
                else:
                    with self.subTest(missing=missing),self.assertRaises(self.a.Denied):self.a.nested_plan(request,parent,self.n.worktree,self.n.paths)

    def test_each_measurement_uses_its_bound_command_and_positive_oracle(self):
        self.available();self.assertIn('measurement',inspect.signature(self.f.fixture).parameters)
        for mode,row in MODE_ROWS.items():
            invoke=Mock(return_value=subprocess.CompletedProcess([],0,self.measurement_output(mode),''))
            self.assertEqual(self.f.fixture(self.n.env,invoke,measurement=mode),0)
            self.assertEqual(invoke.call_args.kwargs['cwd'],self.root)
            self.assertEqual(invoke.call_args.args[0],self.n.command(row)[0])

    def test_measurement_unknown_flags_and_absent_membership_refuse_before_launch(self):
        self.available();self.assertIn('measurement',inspect.signature(self.f.fixture).parameters)
        invoke=Mock()
        with self.assertRaises(self.a.Denied):self.f.fixture(self.n.env,invoke,measurement='--cap-lints=allow')
        invoke.assert_not_called()
        for mode,row in MODE_ROWS.items():
            self.n.plan['request']['rows'].remove(row);self.n.save()
            with self.subTest(mode=mode),self.assertRaises(self.a.Denied):self.f.fixture(self.n.env,invoke,measurement=mode)
            invoke.assert_not_called();self.n.plan['request']['rows'].append(row);self.n.save()

    def test_missing_positive_duplicate_wrong_owner_and_held_guard_refuse(self):
        self.available();self.assertIn('measurement',inspect.signature(self.f.fixture).parameters)
        for mode in MODE_ROWS:
            events=self.findings(mode)
            held=next(x.strip() for x in (self.root/'src/lib.rs').read_text().splitlines() if 'let held_' in x)
            for bad in (events[:-1],events+[events[0]],events+[self.finding(held,events[0]['message']['code']['code'])]):
                with self.subTest(mode=mode,bad=bad),self.assertRaises(self.a.Denied):self.f.diagnostic_success(self.measurement_output(mode,bad),0,self.root,measurement=mode)
        wrong=self.findings('rustc');wrong[0]['message']['code']['code']='clippy::let_underscore_lock'
        with self.assertRaises(self.a.Denied):self.f.diagnostic_success(self.measurement_output('rustc',wrong),0,self.root,measurement='rustc')

    def test_sweep_liveness_needs_owned_and_borrowed_warnings_and_checks_drop_statements(self):
        self.available();self.assertIn('measurement',inspect.signature(self.f.fixture).parameters)
        events=self.findings('sweep')
        for subset in (events[:2],events[2:4],events[4:]):
            with self.assertRaises(self.a.Denied):self.f.diagnostic_success(self.measurement_output('sweep',subset),0,self.root,measurement='sweep')
        for statement in ('drop(dropped_via_std_drop.lock());','drop(dropped_via_pl_drop.lock());'):
            with self.assertRaises(self.a.Denied):self.f.diagnostic_success(self.measurement_output('sweep',events+[self.finding(statement,'clippy::new_drop_lint')]),0,self.root,measurement='sweep')
        advice=self.finding(STD[0],'clippy::blanket_clippy_restriction_lints');advice['message']['spans']=[]
        self.f.diagnostic_success(self.measurement_output('sweep',events+[advice]),0,self.root,measurement='sweep')

    def test_batch_summary_rejects_missing_extra_ignored_duplicate_and_false_counts(self):
        self.available()
        good='running 5 tests\n'+''.join('test '+n+' ... ok\n' for n in NAMES)+'\ntest result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 4787 filtered out; finished in 2.5s\n'
        choices=(good,good.replace(NAMES[0],NAMES[1]),good.replace(NAMES[0],'another_test'),good.replace('5 passed','4 passed'),good.replace('0 ignored','1 ignored'),good.replace('running 5 tests','running 6 tests'),good+'test extra ... ok\n')
        for output in choices:
            invoke=Mock(return_value=subprocess.CompletedProcess([],0,output,''))
            with patch.object(self.f.shared,'capture_owning_artifact') as capture:
                if output==good:
                    self.assertEqual(self.f.shared.owning_test(self.n.env,invoke,tuple(FLAGS),BATCH,NAMES,'lock-remaining-owning'),0)
                    capture.assert_called_once_with(good,self.n.env,BATCH,NAMES,'lock-remaining-owning')
                else:
                    with self.subTest(output=output),self.assertRaises(self.a.Denied):self.f.shared.owning_test(self.n.env,invoke,tuple(FLAGS),BATCH,NAMES,'lock-remaining-owning')
                    capture.assert_not_called()

    def test_batch_receipt_preserves_actual_harness_and_exact_five_names(self):
        self.available()
        path=self.n.paths['build']/'debug/deps/xtask-batch-proof'
        path.parent.mkdir(parents=True);path.write_bytes(b'native fixture executable');path.chmod(0o755)
        artifact={'reason':'compiler-artifact','manifest_path':str(self.n.worktree/'xtask/Cargo.toml'),
                  'target':{'name':'xtask','kind':['bin'],'src_path':str(self.n.worktree/'xtask/src/main.rs')},
                  'profile':{'test':True},'executable':str(path),'filenames':[str(path)],'fresh':True}
        output=json.dumps(artifact)+'\n'+json.dumps({'reason':'build-finished','success':True})+'\n'
        self.f.shared.capture_owning_artifact(output,self.n.env,BATCH,NAMES,'lock-remaining-owning')
        copies=list(self.n.paths['temp'].glob('lock-remaining-owning-*.json'));self.assertEqual(len(copies),1)
        evidence=json.loads(copies[0].read_text())
        self.assertEqual(evidence['cargo_artifact'],artifact)
        self.assertEqual(evidence['named_tests'],list(NAMES));self.assertNotIn('named_test',evidence)
        self.assertEqual(evidence['artifact'],self.a.file_subject(path))
        self.assertEqual(evidence['marker_identity'],self.n.receipt['marker_identity'])
        self.assertTrue(evidence['copied_under_original_live_owner'])
