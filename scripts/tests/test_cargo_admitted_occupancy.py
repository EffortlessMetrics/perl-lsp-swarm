"""Exact diagnostic preparation, frozen replay and original Rust-oracle controls."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import unittest
from unittest.mock import Mock, patch
import test_cargo_admitted_nested as support

ROW='parser-collapsible-if-measure';TEST='parser-collapsible-if-test'
NAMES=('cfg_attr_allow_fixture_is_detected','cfg_attr_without_collapsible_if_does_not_occupy',
       'clippy_all_targets_has_no_collapsible_if_hits','comment_and_string_literals_do_not_occupy',
       'crate_level_allow_fixture_is_detected','expect_attr_fixture_is_detected','item_allow_fixture_is_detected',
       'lib_rs_crate_allow_list_does_not_name_collapsible_if','nested_cfg_attr_allow_fixture_is_detected',
       'occupancy_requires_allow_attr_not_scanner_literals','perl_parser_sources_do_not_allow_collapsible_if',
       'successful_clippy_without_hits_is_clean','unsuccessful_clippy_with_hits_still_reports_occupancy',
       'unsuccessful_clippy_without_hits_is_instrument_failure')
CLIPPY=('clippy','-p','perl-parser','--all-targets','--features','incremental','--locked','--no-deps',
        '--message-format=json','--','--cap-lints=allow','--force-warn','clippy::collapsible_if')
HIT={'reason':'compiler-message','message':{'code':{'code':'clippy::collapsible_if'},
     'spans':[{'file_name':'crates/perl-parser/src/lib.rs','line_start':10}]}}

class OccupancyTests(unittest.TestCase):
    def setUp(self):
        self.n=support.NestedTests();self.n.setUp();self.addCleanup(self.n.doCleanups);self.a=support.a
        self.n.receipt['pid']=os.getpid();self.actual=Path(__file__).resolve().parents[2]
        for rel in ('scripts/ci/parser_occupancy_prepare.py','scripts/ci/disallowed_fields_prepare.py',
                    'crates/perl-parser/tests/collapsible_if_occupancy.rs'):
            dst=self.n.worktree/rel;dst.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(self.actual/rel,dst)
        path=Path(os.environ.get('OCCUPANCY_ADAPTER_TEST_FILE',str(self.actual/'scripts/ci/parser_occupancy_prepare.py')))
        spec=importlib.util.spec_from_file_location('occupancy_adapter',path);self.f=importlib.util.module_from_spec(spec);spec.loader.exec_module(self.f)
        for module in (self.f,self.f.shared):
            p=patch.object(module,'owner',self.a);p.start();self.addCleanup(p.stop)
        self.n.plan['parser_occupancy']=self.a.parser_occupancy_binding(self.n.worktree);self.n.save()
        self.path=self.n.paths['temp']/('parser-occupancy-measurement-'+str(os.getpid())+'.json')
    def stream(self,code=0,hit=False):
        events=[{'reason':'compiler-artifact','manifest_path':str(self.n.worktree/'crates/perl-parser/Cargo.toml'),
                 'features':['default','incremental','workspace','lsp-compat'],
                 'target':{'kind':['lib'],'src_path':str(self.n.worktree/'crates/perl-parser/src/lib.rs')}}]
        if hit:events.append(HIT)
        events.append({'reason':'build-finished','success':code==0})
        return '\n'.join(json.dumps(e,separators=(',',':')) for e in events)+'\n'
    def measure(self,code=0,hit=False,stderr='native diagnostics'):
        invoke=Mock(return_value=subprocess.CompletedProcess([],code,self.stream(code,hit),stderr))
        with contextlib.redirect_stdout(io.StringIO()):self.assertEqual(self.f.measure(self.n.env,invoke),code)
        return invoke
    def frozen(self):
        return {**self.n.env,'CARGO_ADMITTED_OCCUPANCY_MEASUREMENT':json.dumps(self.a.file_subject(self.path))}
    def test_literal_commands_all_fourteen_names_native_private_roots(self):
        self.assertEqual(self.a.NESTED_COMMANDS[ROW],CLIPPY)
        self.assertEqual(self.a.NESTED_COMMANDS[TEST],('test','-p','perl-parser','--test','collapsible_if_occupancy','--locked','--message-format=json','--','--exact',*NAMES,'--test-threads=1','--color','never'))
        command,env,cwd=self.n.command(ROW);self.assertEqual(cwd,self.n.worktree)
        self.assertEqual(command[0],self.n.tool['subjects']['cargo-clippy']['path'])
        self.assertEqual(env['CARGO_TARGET_DIR'],str(self.n.paths['target']));self.assertEqual(env['CARGO_BUILD_BUILD_DIR'],str(self.n.paths['build']))
        self.assertEqual(env['CLIPPY_ARGS'],'--cap-lints=allow__CLIPPY_HACKERY__--force-warn__CLIPPY_HACKERY__clippy::collapsible_if__CLIPPY_HACKERY__')
        self.measure();_,child,_=self.n.command(TEST)
        self.assertEqual(json.loads(child['CARGO_ADMITTED_OCCUPANCY_MEASUREMENT']),self.a.file_subject(self.path))
        self.assertEqual(child['CARGO_ADMITTED_OCCUPANCY_PYTHON'],self.n.plan['parser_occupancy']['python']['path'])
    def test_missing_measurement_cannot_launch_owning_or_capture(self):
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,'',''))
        with patch.object(self.f.shared,'capture_owning_artifact') as capture,self.assertRaises(self.a.Denied):
            self.f.shared.owning_test(self.n.env,invoke,ROW,TEST,NAMES,'parser-occupancy-owning')
        invoke.assert_not_called();capture.assert_not_called()
    def test_current_native_measurement_once_before_test_and_failed_prep_blocks_runtime(self):
        invoke=Mock(return_value=subprocess.CompletedProcess([],101,self.stream(101,True),'compiler failed'))
        with contextlib.redirect_stdout(io.StringIO()),patch.object(self.f.shared,'owning_test') as owning:
            self.assertEqual(self.f.test(self.n.env,invoke),101)
        self.assertEqual(invoke.call_count,1);owning.assert_not_called()
        with self.assertRaises(self.a.Denied):self.f.measure(self.n.env,invoke)
        self.assertEqual(invoke.call_count,1)
    def test_replay_preserves_zero_hit_failure_with_and_without_hits(self):
        for code,hit in ((0,False),(0,True),(101,True),(101,False)):
            with self.subTest(code=code,hit=hit):
                self.measure(code,hit,'original stderr');output=io.StringIO();errors=io.StringIO()
                with contextlib.redirect_stdout(output),contextlib.redirect_stderr(errors):
                    self.assertEqual(self.f.read(self.frozen()),code)
                self.assertEqual(output.getvalue(),self.stream(code,hit));self.assertEqual(errors.getvalue(),'original stderr')
                self.path.unlink()
    def test_changed_stale_wrong_owner_scope_and_command_refuse_before_replay(self):
        self.measure();good=json.loads(self.path.read_text())
        for key,value in (('tested_source','stale'),('snapshot',{}),('owner_process',{}),('marker_identity',[0,0]),
                          ('argv',['cargo','clippy']),('cwd','/other'),('exit_code',75),
                          ('instrument_validation',{'passed':False,'error':'missing'})):
            self.path.write_text(json.dumps({**good,key:value}));env=self.frozen()
            with self.subTest(key=key),self.assertRaises(self.a.Denied):self.f.read(env)
        self.path.write_text(json.dumps(good));env=self.frozen();self.path.write_text(json.dumps({**good,'stderr':'changed'}))
        with self.assertRaises(self.a.Denied):self.f.read(env)
    def test_false_success_invalid_terminal_and_absent_incremental_parser_artifact_refuse(self):
        good=self.stream();artifact=json.loads(good.splitlines()[0]);terminal={'reason':'build-finished','success':True}
        variants=['',json.dumps(terminal),good+json.dumps(terminal),'not JSON',
                  json.dumps({'reason':'build-finished','success':0}),self.stream(101)]
        for change in ({'manifest_path':'/other/Cargo.toml'},{'features':['default']},{'target':{'kind':['lib'],'src_path':'/other.rs'}}):
            variants.append(json.dumps({**artifact,**change})+'\n'+json.dumps(terminal)+'\n')
        for text in variants:
            invoke=Mock(return_value=subprocess.CompletedProcess([],0,text,''))
            with self.subTest(text=text[:90]),self.assertRaises((self.a.Denied,ValueError)):
                self.f.measure(self.n.env,invoke)
            self.assertFalse(json.loads(self.path.read_text())['instrument_validation']['passed']);self.path.unlink()
    def test_unknown_status_source_drift_member_interpreter_and_missing_selectors_refuse(self):
        for code in (-9,75,1):
            invoke=Mock(return_value=subprocess.CompletedProcess([],code,self.stream(101),'bad tool'))
            with self.subTest(code=code),self.assertRaises(self.a.Denied):self.f.measure(self.n.env,invoke)
            self.path.unlink()
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,self.stream(),''))
        self.n.plan['request']['rows'].remove(ROW);self.n.save()
        with self.assertRaises(self.a.Denied):self.f.measure(self.n.env,invoke)
        invoke.assert_not_called();self.n.plan['request']['rows'].append(ROW);self.n.save()
        good=self.n.plan['parser_occupancy']['python']['sha256'];self.n.plan['parser_occupancy']['python']['sha256']='wrong';self.n.save()
        with self.assertRaises(self.a.Denied):self.f.measure(self.n.env,invoke)
        invoke.assert_not_called();self.n.plan['parser_occupancy']['python']['sha256']=good;self.n.save()
        p=self.n.worktree/'crates/perl-parser/tests/collapsible_if_occupancy.rs';p.write_text(p.read_text()+'\n// drift\n')
        with self.assertRaises(self.a.Denied):self.f.measure(self.n.env,invoke)
        invoke.assert_not_called()
    def test_large_raw_trace_retained_and_over_bound_reader_refuses(self):
        self.measure(stderr='trace\n'*20000)
        with self.assertRaises(self.a.Denied):self.a.bounded_json(self.path)
        record,subject=self.a.parser_occupancy_measurement(self.n.plan,self.n.receipt)
        self.assertEqual(record['stderr'],'trace\n'*20000)
        subject['file_identity'][2]=2*self.a.PARSER_OCCUPANCY_STREAM_LIMIT*6+self.a.BUDGET_FILE_LIMIT+1
        with patch.object(self.a,'file_subject',return_value=subject),self.assertRaises(self.a.Denied):
            self.a.parser_occupancy_measurement(self.n.plan,self.n.receipt)
    def test_exact_owning_summary_and_frozen_receipt_after_run(self):
        self.measure();env=self.frozen()
        good='running 14 tests\n'+''.join('test '+n+' ... ok\n' for n in NAMES)+'test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n'
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,good,''))
        with patch.object(self.f.shared,'capture_owning_artifact') as capture:
            self.assertEqual(self.f.shared.owning_test(env,invoke,ROW,TEST,NAMES,'parser-occupancy-owning'),0)
            capture.assert_called_once()
        bad=Mock(return_value=subprocess.CompletedProcess([],0,good.replace('running 14','running 0'),''))
        with patch.object(self.f.shared,'capture_owning_artifact') as capture,self.assertRaises(self.a.Denied):
            self.f.shared.owning_test(env,bad,ROW,TEST,NAMES,'parser-occupancy-owning')
        capture.assert_not_called()
        def drift(*args,**kwargs):
            d=json.loads(self.path.read_text());d['stderr']='changed after owning';self.path.write_text(json.dumps(d));return subprocess.CompletedProcess([],0,good,'')
        with patch.object(self.f.shared,'capture_owning_artifact') as capture,self.assertRaises(self.a.Denied):
            self.f.shared.owning_test(env,Mock(side_effect=drift),ROW,TEST,NAMES,'parser-occupancy-owning')
        capture.assert_not_called()
    def test_actual_parser_integration_kind_manifest_source_and_readonly_capture(self):
        self.measure();env=self.frozen();path=self.n.paths['build']/'debug/deps/collapsible_if_occupancy-proof';path.parent.mkdir(parents=True);path.write_bytes(b'actual parser harness');path.chmod(0o700)
        artifact={'reason':'compiler-artifact','manifest_path':str(self.n.worktree/'crates/perl-parser/Cargo.toml'),
                  'target':{'name':'collapsible_if_occupancy','kind':['test'],'src_path':str(self.n.worktree/'crates/perl-parser/tests/collapsible_if_occupancy.rs')},
                  'profile':{'test':True},'executable':str(path),'filenames':[str(path)]}
        for change in ({'manifest_path':str(self.n.worktree/'xtask/Cargo.toml')},{'target':{**artifact['target'],'kind':['bin']}},{'target':{**artifact['target'],'src_path':'/wrong.rs'}}):
            text=json.dumps({**artifact,**change})+'\n'+json.dumps({'reason':'build-finished','success':True})
            with self.assertRaises(self.a.Denied):self.f.shared.capture_owning_artifact(text,env,TEST,NAMES,'parser-occupancy-owning')
        text=json.dumps(artifact)+'\n'+json.dumps({'reason':'build-finished','success':True})
        with contextlib.redirect_stdout(io.StringIO()):self.f.shared.capture_owning_artifact(text,env,TEST,NAMES,'parser-occupancy-owning')
        copy=self.n.paths['temp']/('parser-occupancy-owning-'+str(os.getpid()));self.assertEqual(copy.stat().st_mode&0o777,0o444);self.assertEqual(copy.read_bytes(),path.read_bytes())
    def test_real_std_transport_retains_occupancy_failures_and_refuses_partial_or_75(self):
        source=Path(os.environ.get('OCCUPANCY_RUST_TEST_FILE',str(self.actual/'crates/perl-parser/tests/collapsible_if_occupancy.rs'))).read_text()
        def function(name):
            start=source.index('fn '+name+'(');end=source.find('\nfn ',start+1);body=source[start:end if end>=0 else len(source)]
            return body.rsplit('\n#[test]',1)[0]
        program='use std::{path::Path,process::Command};\nconst LINT:&str="clippy::collapsible_if";\n'
        program+='\n'.join(function(name) for name in ('json_quoted_after','clippy_hits_from_stdout','clippy_hits_from_parts','clippy_collapsible_if_hits'))
        program+='''
fn main(){let result=clippy_collapsible_if_hits();match std::env::args().nth(1).as_deref(){
Some("hit")=>assert!(result.is_ok_and(|v|!v.is_empty()),"failure hits must occupy"),
Some("clean")=>assert!(result.is_ok_and(|v|v.is_empty()),"success silence is clean"),
Some("partial")=>assert!(result.is_err_and(|e|e.contains("incomplete")),"partial selectors refuse"),
Some("refused")=>assert!(result.is_err_and(|e|e.contains("instrument refused")),"75 is instrument refusal"),
_=>assert!(result.is_err_and(|e|e.contains("unsuccessfully")),"failure silence is instrument error")}}
'''
        path=self.n.root/'transport.rs';path.write_text(program);binary=self.n.root/'transport'
        script=self.n.root/'python';script.write_text('#!'+os.sys.executable+'\nimport os,sys\nprint(os.environ.get("FIXTURE_STDOUT",""),end="")\nprint("native stderr",file=sys.stderr)\nsys.exit(int(os.environ["FIXTURE_STATUS"]))\n');script.chmod(0o700)
        env=os.environ.copy();env.update(CARGO_MANIFEST_DIR=str(self.n.worktree/'crates/perl-parser'),CARGO=str(script))
        rustc='/workspace/.cloud-tools/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc'
        r=subprocess.run([rustc,'--edition','2024',str(path),'-o',str(binary)],env=env,text=True,capture_output=True);self.assertEqual(r.returncode,0,r.stderr)
        env.update(CARGO_ADMITTED_RESOURCES='present',CARGO_ADMITTED_OCCUPANCY_PYTHON=str(script),CARGO_ADMITTED_OCCUPANCY_MEASUREMENT='present')
        for mode,code,hit in [('refused',75,True),('hit',101,True),('failure',101,False),('clean',0,False)]:
            env.update(FIXTURE_STATUS=str(code),FIXTURE_STDOUT=json.dumps(HIT,separators=(',',':')) if hit else '')
            r=subprocess.run([str(binary),mode],env=env,text=True,capture_output=True);self.assertEqual(r.returncode,0,r.stderr)
        env.pop('CARGO_ADMITTED_OCCUPANCY_MEASUREMENT');r=subprocess.run([str(binary),'partial'],env=env,text=True,capture_output=True);self.assertEqual(r.returncode,0,r.stderr)
