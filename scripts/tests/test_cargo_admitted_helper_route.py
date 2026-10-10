"""Exact helper route controls; no native Cargo or 277-harness execution claim."""
import contextlib
import io
import json
from pathlib import Path
import shutil
from unittest.mock import Mock, patch
import importlib.util
import test_cargo_admitted_routed as support

class HelperRouteTests(support.RoutedTests):
    def setUp(self):
        super().setUp()
        for rel in self.a.HELPER_TRANSITIVE_INPUTS:
            destination = self.n.worktree/rel;destination.parent.mkdir(parents=True,exist_ok=True)
            if not destination.exists():shutil.copyfile(self.actual/rel,destination)
        declared = json.loads((self.actual/'.spec/17479-nested-admission/helper-source-map.json').read_text())['declared_targets']
        self.mapping['declared_targets'] = declared
        for targets in declared.values():
            for target in targets:
                destination = self.n.worktree/target['src_path'];destination.parent.mkdir(parents=True,exist_ok=True)
                if not destination.exists():destination.write_text('// synthetic target declaration for projection control\n')
        self.n.plan['request']['rows'] = ['perllsp-build',self.a.HELPER_RUNTIME,*self.a.ROUTED_FIXTURE_ROWS]
        self.map_path = self.n.worktree/'.spec/17479-nested-admission/helper-source-map.json'
        self.mapping.update(packages=list(self.a.HELPER_PACKAGES),fixture_rows=list(self.a.ROUTED_FIXTURE_ROWS),
                            runtime_row=self.a.HELPER_RUNTIME,excluded_dynamic_group_count=6)
        # Whole selected source census adds even a non-matching constructor route.
        self.mapping['reviewed_source_subjects'] = self.a.helper_reviewed_subjects(self.n.worktree)
        self.mapping['compiler_looking_source_subjects'] = {key:value for key,value in self.mapping['compiler_looking_source_subjects'].items() if not key.startswith('crates/perl-parser/')}
        self.bind_mapping()
    def bind_mapping(self):
        self.map_path.write_text(json.dumps(self.mapping))
        row = self.a.HELPER_RUNTIME if self.mapping.get('runtime_row') == self.a.HELPER_RUNTIME else 'routed-runtime'
        self.n.plan['routed_preparation'] = self.a.routed_preparation_binding(self.n.worktree,row)
        self.n.save()
    def prepare(self):
        invoke = Mock(side_effect=self.compiler)
        with contextlib.redirect_stdout(io.StringIO()),contextlib.redirect_stderr(io.StringIO()):self.env = self.f.prepare(self.n.env,invoke)
        self.assertEqual(invoke.call_count,9)
        return invoke
    def test_preparation_once_and_runtime_replays_original_diagnostics_without_compilation(self):
        self.prepare()
        command,child,cwd = self.a.nested_command(self.a.HELPER_RUNTIME,self.env)
        self.assertEqual(command[command.index('--target-dir')+2:],['--locked','--tests','-p','perl-ci-hygiene','-p','xtask'])
        self.assertNotIn('CARGO_ADMITTED_OCCUPANCY_PYTHON',child)
        self.assertNotIn('CARGO_ADMITTED_OCCUPANCY_MEASUREMENT',child)
        invoke = Mock(side_effect=AssertionError('compiler during helper replay'))
        with contextlib.redirect_stdout(io.StringIO()),contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(self.f.fields.fixture(child,invoke),101)
            for mode in self.f.locks.MEASUREMENTS:self.assertEqual(self.f.locks.fixture(child,invoke,mode),0)
            for mode in self.f.rpc.MODES:self.assertEqual(self.f.rpc.phase(mode,child,invoke),101 if mode=='rejected' else 0)
        invoke.assert_not_called()
    def test_each_missing_frozen_row_refuses_runtime_and_replay_without_fallback(self):
        self.prepare();subjects=json.loads(self.env[self.a.ROUTED_MEASUREMENTS])
        for row in self.a.ROUTED_FIXTURE_ROWS:
            env={**self.env,self.a.ROUTED_MEASUREMENTS:json.dumps({key:value for key,value in subjects.items() if key!=row})}
            with self.subTest(row=row),self.assertRaises(self.a.Denied):self.a.nested_command(self.a.HELPER_RUNTIME,env)
            with self.subTest(replay=row),self.assertRaises(self.a.Denied):self.f.replay(row,env)
    def test_new_unmatched_source_and_changed_transitive_script_refuse_before_compiler(self):
        invoke=Mock(side_effect=AssertionError('unreviewed native launch'))
        new=self.n.worktree/'xtask/tests/unknown.rs';new.write_text('spawn(variable_command);')
        with self.assertRaisesRegex(self.a.Denied,'finite helper source'):self.f.prepare(self.n.env,invoke)
        new.unlink()
        script=self.n.worktree/'scripts/zed_exact_source_prepare.py';script.write_text(script.read_text()+'\n# changed transitive producer\n')
        with self.assertRaisesRegex(self.a.Denied,'finite helper source'):self.f.prepare(self.n.env,invoke)
        invoke.assert_not_called()
    def test_production_unknown_groups_are_explicit_and_block_runtime(self):
        # Narrow helper exclusions must not clear the separate broad guard.
        super().test_production_unknown_groups_are_explicit_and_block_runtime()

    def test_swapped_adapter_modes_refuse_before_product_or_measurements(self):
        spec = importlib.util.spec_from_file_location('helper_product_adapter',self.actual/'scripts/ci/perllsp_workspace_prepare.py')
        product=importlib.util.module_from_spec(spec);spec.loader.exec_module(product)
        invoke=Mock(side_effect=AssertionError('wrong-mode native launch'))
        with patch.object(product,'owner',self.a):
            with self.assertRaisesRegex(self.a.Denied,'requested runtime differs'):product.run('--runtime',self.n.env,invoke)
            self.n.plan['routed_preparation'] = self.a.routed_preparation_binding(self.n.worktree)
            self.n.save()
            with self.assertRaisesRegex(self.a.Denied,'requested runtime differs'):product.run('--helper-runtime',self.n.env,invoke)
        invoke.assert_not_called()
    def test_missing_composition_target_refuses_before_native_preparation(self):
        target = self.mapping['declared_targets']['perl-ci-hygiene'][0]['src_path']
        path=self.n.worktree/target;path.unlink()
        self.mapping['reviewed_source_subjects'] = self.a.helper_reviewed_subjects(self.n.worktree)
        self.bind_mapping()
        invoke=Mock(side_effect=AssertionError('missing composition native launch'))
        with self.assertRaisesRegex(self.a.Denied,'composition target/source'):self.f.prepare(self.n.env,invoke)
        invoke.assert_not_called()

    def test_real_plan_binds_only_exact_helper_runtime_and_nine_prerequisites(self):
        request=self.n.root/'helper-request.json'
        request.write_text(json.dumps(self.n.plan['request']))
        with patch.object(self.a,'clippy_toolchain',return_value=self.n.tool):
            parent={key:value for key,value in self.n.env.items() if key not in ('CARGO','RUSTC','RUSTDOC','CARGO_ADMITTED_RESOURCES')}
            plan=self.a.nested_plan(request,parent,self.n.worktree,self.n.paths)
        self.assertEqual(plan['routed_preparation']['runtime_row'],self.a.HELPER_RUNTIME)
        self.assertNotIn('parser_occupancy',plan)
        self.assertEqual(plan['request']['rows'],['perllsp-build',self.a.HELPER_RUNTIME,*self.a.ROUTED_FIXTURE_ROWS])
