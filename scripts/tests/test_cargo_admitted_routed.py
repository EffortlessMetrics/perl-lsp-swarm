"""Canonical prepared projection controls; injected compiler output is not native proof."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import unittest
from unittest.mock import Mock, patch
import test_cargo_admitted_jsonrpc as rpc_support
import test_cargo_admitted_lock_batch as lock_support
import test_cargo_admitted_fixture as field_support
import test_cargo_admitted_occupancy as occupancy_support


class RoutedTests(unittest.TestCase):
    def setUp(self):
        self.rpc = rpc_support.JsonRpcTests()
        with contextlib.redirect_stdout(io.StringIO()):self.rpc.setUp()
        self.addCleanup(self.rpc.doCleanups)
        self.n, self.a = self.rpc.n, self.rpc.a
        self.actual = Path(__file__).resolve().parents[2]
        self.field = field_support.FixtureTests();self.field.n = self.n
        self.field.root = self.n.bind_disallowed_fixture()
        self.lock = lock_support.LockBatchTests();self.lock.n = self.n
        self.lock.root = self.n.bind_lock_fixture()
        self.occ = occupancy_support.OccupancyTests();self.occ.n = self.n
        for rel in ('scripts/ci/parser_occupancy_prepare.py','scripts/ci/routed_nested_prepare.py',
                    'crates/perl-parser/tests/collapsible_if_occupancy.rs'):
            dest = self.n.worktree / rel;dest.parent.mkdir(parents=True,exist_ok=True)
            shutil.copyfile(self.actual / rel,dest)
        spec = importlib.util.spec_from_file_location('routed_test_adapter', self.actual/'scripts/ci/routed_nested_prepare.py')
        self.f = importlib.util.module_from_spec(spec);spec.loader.exec_module(self.f)
        for module in (self.f,self.f.fields,self.f.locks,self.f.rpc,self.f.occupancy,self.f.rpc.shared,self.f.occupancy.shared):
            p = patch.object(module,'owner',self.a);p.start();self.addCleanup(p.stop)
        p = patch.dict(sys.modules, {'routed_nested_prepare':self.f});p.start();self.addCleanup(p.stop)
        self.n.plan['parser_occupancy'] = self.a.parser_occupancy_binding(self.n.worktree)
        self.n.plan['jsonrpc_fixture'] = self.a.jsonrpc_fixture(self.n.worktree,self.n.paths)
        self.n.plan['disallowed_fields_fixture'] = self.a.disallowed_fixture(self.n.worktree,self.n.paths)
        self.map_path = self.n.worktree/'.spec/17479-nested-admission/canonical-source-map.json'
        self.mapping = json.loads((self.actual/'.spec/17479-nested-admission/canonical-source-map.json').read_text())
        # Only this synthetic source fixture has all unknowns resolved. Production retains six blockers.
        self.mapping['unknown_dynamic'] = []
        self.mapping['compiler_looking_source_subjects'] = {
            str(p.relative_to(self.n.worktree)):self.a.file_subject(p)['sha256']
            for p in self.n.worktree.rglob('*.rs') if re.search(self.a.ROUTED_SOURCE_PATTERN,p.read_text())}
        self.bind_mapping()
    def bind_mapping(self):
        self.map_path.write_text(json.dumps(self.mapping))
        self.n.plan['routed_preparation'] = self.a.routed_preparation_binding(self.n.worktree)
        self.n.save()
    def compiler(self,command,**kwargs):
        cwd = Path(kwargs['cwd'])
        if cwd == self.field.root:return subprocess.CompletedProcess(command,101,self.field.output(),'original field stderr')
        if cwd == self.lock.root:
            args = command[command.index('--target-dir')+2:]
            row = next(row for row in self.a.LOCK_FIXTURE_ROWS if args == list(self.a.NESTED_COMMANDS[row])[1:])
            mode = next(mode for mode,row_value in self.f.locks.MEASUREMENTS.items() if row_value == row)
            return subprocess.CompletedProcess(command,0,self.lock.measurement_output(mode),'original lock stderr')
        if cwd == self.rpc.root/'neutral':
            if 'generate-lockfile' in command:
                (cwd/'Cargo.lock').write_text(rpc_support.LOCK)
            return subprocess.CompletedProcess(command,0,'','original neutral stderr')
        if cwd == self.rpc.root/'rejected':return subprocess.CompletedProcess(command,101,'',rpc_support.ERROR)
        if cwd == self.n.worktree:return subprocess.CompletedProcess(command,0,self.occ.stream(),'original occupancy stderr')
        self.fail('unexpected compiler cwd '+str(cwd))
    def prepare(self):
        invoke = Mock(side_effect=self.compiler)
        with contextlib.redirect_stdout(io.StringIO()),contextlib.redirect_stderr(io.StringIO()):
            self.env = self.f.prepare(self.n.env,invoke)
        self.assertEqual(invoke.call_count,10)
        return invoke
    def test_preparation_once_and_runtime_replays_original_diagnostics_without_compilation(self):
        self.prepare()
        command, child, cwd = self.a.nested_command('routed-runtime',self.env)
        self.assertEqual(cwd,self.n.worktree)
        self.assertEqual(command[command.index('--target-dir')+2:],list(self.a.NESTED_COMMANDS['routed-runtime'])[1:])
        self.assertEqual(child[self.a.ROUTED_MEASUREMENTS],self.env[self.a.ROUTED_MEASUREMENTS])
        invoke = Mock(side_effect=AssertionError('runtime compiler fallback'))
        with contextlib.redirect_stdout(io.StringIO()),contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(self.f.fields.fixture(child,invoke),101)
            for mode in self.f.locks.MEASUREMENTS:self.assertEqual(self.f.locks.fixture(child,invoke,mode),0)
            for mode in self.f.rpc.MODES:self.assertEqual(self.f.rpc.phase(mode,child,invoke),101 if mode=='rejected' else 0)
            self.assertEqual(self.f.occupancy.read(child),0)
        invoke.assert_not_called()
        for row in self.a.ROUTED_FIXTURE_ROWS:
            output = self.f.replay(row,child)
            original,_ = self.a.routed_phase_record(self.n.plan,self.n.receipt,row,child)
            self.assertEqual((output.returncode,output.stdout,output.stderr),(original['exit_code'],original['stdout'],original['stderr']))
    def test_unknown_missing_new_and_changed_source_refuse_before_any_compiler(self):
        invoke = Mock(side_effect=AssertionError('unclassified native launch'))
        for key in ('unknown_dynamic','missing_active_rows'):
            self.mapping[key] = ['unclassified command'];self.bind_mapping()
            with self.subTest(key=key),self.assertRaises(self.a.Denied):self.f.prepare(self.n.env,invoke)
            self.mapping[key] = [];self.bind_mapping()
        source = self.n.worktree/'xtask/tests/new_dynamic.rs';source.write_text('Command::new("cargo")')
        with self.assertRaises(self.a.Denied):self.f.prepare(self.n.env,invoke)
        invoke.assert_not_called()
    def test_each_missing_frozen_row_refuses_runtime_and_replay_without_fallback(self):
        self.prepare();subjects = json.loads(self.env[self.a.ROUTED_MEASUREMENTS])
        invoke = Mock(side_effect=AssertionError('missing-row native fallback'))
        for row in self.a.ROUTED_FIXTURE_ROWS:
            partial = {key:value for key,value in subjects.items() if key != row}
            env = {**self.env,self.a.ROUTED_MEASUREMENTS:json.dumps(partial)}
            with self.subTest(row=row),self.assertRaises(self.a.Denied):self.a.nested_command('routed-runtime',env)
            with self.subTest(replay=row),self.assertRaises(self.a.Denied):self.f.fields.prepared_output(row,env)
        invoke.assert_not_called()
    def test_changed_raw_status_owner_source_and_command_refuse_even_when_refrozen(self):
        self.prepare();row = self.a.DISALLOWED_FIXTURE_ROW
        subjects = json.loads(self.env[self.a.ROUTED_MEASUREMENTS]);path = Path(subjects[row]['path'])
        good = json.loads(path.read_text())
        changes = {'tested_source':'other','owner_process':{},'lease_identity':[0,0],
                   'marker_identity':[0,0],'snapshot':{},'argv':['cargo','check'],'cwd':'/other',
                   'exit_code':0,'schema_version':True,'recorded_under_original_live_owner':False,
                   'stdout':[],'diagnostic_validation':{'passed':False,'error':None}}
        for key,value in changes.items():
            path.write_text(json.dumps({**good,key:value}));subjects[row] = self.a.file_subject(path)
            env = {**self.env,self.a.ROUTED_MEASUREMENTS:json.dumps(subjects)}
            with self.subTest(key=key),self.assertRaises(self.a.Denied):self.f.replay(row,env)
        path.write_text(json.dumps(good));subjects[row] = self.a.file_subject(path)
        env = {**self.env,self.a.ROUTED_MEASUREMENTS:json.dumps(subjects)}
        path.write_text(json.dumps({**good,'stderr':'replaced'}))
        with self.assertRaises(self.a.Denied):self.f.replay(row,env)
    def test_production_unknown_groups_are_explicit_and_block_runtime(self):
        mapping = json.loads((self.actual/'.spec/17479-nested-admission/canonical-source-map.json').read_text())
        self.assertEqual(len(mapping['unknown_dynamic']),6)
        self.assertEqual(mapping['missing_active_rows'],[])
        self.mapping['unknown_dynamic'] = mapping['unknown_dynamic'];self.bind_mapping()
        invoke = Mock(side_effect=AssertionError('unqualified broad runtime'))
        with self.assertRaisesRegex(self.a.Denied,'dynamic compiler obligation'):self.f.prepare(self.n.env,invoke)
        invoke.assert_not_called()
