"""Independent generated-lock transition and expected compiler failure controls."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import unittest
from unittest.mock import Mock, patch
import test_cargo_admitted_nested as support

ROWS=('xtask-jsonrpc-lock','xtask-jsonrpc-neutral','xtask-jsonrpc-rejected')
BATCH='xtask-jsonrpc-test'
NAMES=('jsonrpc_model_is_dependency_closed_and_rejects_indirect_perl_taxonomy',
       'probe_lock_rejects_path_git_and_unreviewed_registry_sources')
CHECK=('check','--quiet','--locked','--offline','--manifest-path','@manifest@')
LOCK='''version = 4
[[package]]
name = "lsp-jsonrpc-boundary-probe"
version = "0.0.0"
dependencies = ["serde", "serde_json"]
[[package]]
name = "serde"
version = "1.0.229"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba"
[[package]]
name = "serde_json"
version = "1.0.151"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14"
'''
ERROR='error[E0432]: unresolved import `perl_parser_core`\n --> src/lib.rs:7:13\n'

class JsonRpcTests(unittest.TestCase):
    def setUp(self):
        self.n=support.NestedTests();self.n.setUp();self.addCleanup(self.n.doCleanups);self.a=support.a
        self.n.receipt['pid']=os.getpid();self.n.save()
        actual=Path(__file__).resolve().parents[2]
        for rel in ('scripts/ci/jsonrpc_prepare.py','scripts/ci/disallowed_fields_prepare.py',
                    'crates/perl-lsp-rs-core/src/protocol/jsonrpc.rs','xtask/tests/lsp_jsonrpc_dependency_probe.rs',
                    '.spec/17479-nested-admission/jsonrpc-fixture/Cargo.toml',
                    '.spec/17479-nested-admission/jsonrpc-fixture/neutral.rs.in',
                    '.spec/17479-nested-admission/jsonrpc-fixture/rejected.rs.in'):
            dst=self.n.worktree/rel;dst.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(actual/rel,dst)
        path=Path(os.environ.get('JSONRPC_ADAPTER_TEST_FILE',str(actual/'scripts/ci/jsonrpc_prepare.py')))
        spec=importlib.util.spec_from_file_location('jsonrpc_adapter',path);self.f=importlib.util.module_from_spec(spec);spec.loader.exec_module(self.f)
        for module in (self.f,self.f.shared):
            p=patch.object(module,'owner',self.a);p.start();self.addCleanup(p.stop)
        # Preparation refuses real active owners; construct inputs before attaching
        # this synthetic existing owner's live descriptor.
        self.n.marker.rmdir();self.n.lock.rmdir();self.f.prepare(self.n.env)
        self.n.lock.mkdir();self.n.marker.mkdir();self.n.receipt['lease_identity']=list(self.a.directory_identity(self.n.lock));self.n.receipt['marker_identity']=list(self.a.directory_identity(self.n.marker));self.n.save()
        self.root=self.n.paths['temp']/'jsonrpc-17479'
        self.n.plan['jsonrpc_fixture']=self.a.jsonrpc_fixture(self.n.worktree,self.n.paths);self.n.save()

    def lock(self):
        def generated(*args,**kwargs):
            (self.root/'neutral/Cargo.lock').write_text(LOCK)
            return subprocess.CompletedProcess(args[0],0,'','Locking reviewed packages\n')
        invoke=Mock(side_effect=generated);self.assertEqual(self.f.phase('lock',self.n.env,invoke),0)
        self.assertEqual(invoke.call_count,1)
        return invoke
    def neutral(self):
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,'',''));self.assertEqual(self.f.phase('neutral',self.n.env,invoke),0);return invoke
    def receipt(self,mode):
        return self.root/(('lock-generation-' if mode=='lock' else 'native-'+mode+'-')+str(os.getpid())+'.json')

    def test_exact_three_commands_private_roots_and_two_names(self):
        self.assertEqual(self.a.NESTED_COMMANDS[ROWS[0]],('generate-lockfile','--offline','--manifest-path','@manifest@'))
        for row in ROWS[1:]:self.assertEqual(self.a.NESTED_COMMANDS[row],CHECK)
        self.assertEqual(self.a.NESTED_COMMANDS[BATCH],('test','-p','xtask','--test','lsp_jsonrpc_dependency_probe','--locked','--message-format=json','--','--exact',*NAMES,'--test-threads=1','--color','never'))
        command,env,cwd=self.n.command(ROWS[0]);self.assertEqual(cwd,self.root/'neutral');self.assertNotIn('--target-dir',command)
        self.assertEqual(command[-3:],['--offline','--manifest-path',str(cwd/'Cargo.toml')])
        for row in ROWS:
            command,env,cwd=self.a.render_nested(row,self.n.env.copy(),self.n.worktree,self.n.paths,self.n.tool,self.n.plan['jsonrpc_fixture'])
            self.assertEqual(env['CARGO_TARGET_DIR'],str(self.root/'target'));self.assertEqual(env['CARGO_BUILD_BUILD_DIR'],str(self.root/'build'))
            self.assertEqual(cwd,self.root/('rejected' if row==ROWS[2] else 'neutral'))
            self.assertEqual(command[command.index('--manifest-path')+1],str(cwd/'Cargo.toml'))
        _,env,_=self.n.command(BATCH);self.assertEqual(env['CARGO_ADMITTED_JSONRPC_ROOT'],str(self.root));self.assertEqual(env['CARGO_ADMITTED_JSONRPC_PYTHON'],self.n.plan['jsonrpc_fixture']['python']['path'])

    def test_current_generation_and_neutral_are_prerequisites_before_launch(self):
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,'',''))
        for mode in ('neutral','rejected'):
            with self.assertRaises(self.a.Denied):self.f.phase(mode,self.n.env,invoke)
            invoke.assert_not_called()
        self.lock()
        with self.assertRaises(self.a.Denied):self.f.phase('rejected',self.n.env,invoke)
        invoke.assert_not_called();self.neutral()
        negative=Mock(return_value=subprocess.CompletedProcess([],101,'',ERROR))
        self.assertEqual(self.f.phase('rejected',self.n.env,negative),101)
        self.a.jsonrpc_measurements(self.n.plan,self.n.receipt)
        record=json.loads(self.receipt('rejected').read_text());self.assertEqual(record['stderr'],ERROR);self.assertTrue(record['diagnostic_validation']['passed'])
        with self.assertRaises(self.a.Denied):self.f.phase('rejected',self.n.env,invoke)
        invoke.assert_not_called()

    def test_lock_generation_is_actual_and_lock_identity_freezes_both_checks(self):
        invoke=self.lock();record=json.loads(self.receipt('lock').read_text())
        self.assertTrue(record['generated_by_native_command']);self.assertEqual(record['argv'],invoke.call_args.args[0]);self.assertEqual(record['exit_code'],0)
        self.assertEqual((self.root/'neutral/Cargo.lock').read_bytes(),(self.root/'rejected/Cargo.lock').read_bytes())
        for mode in ('neutral','rejected'):
            path=self.root/mode/'Cargo.lock';data=path.read_bytes();path.write_bytes(data+b'\n')
            invoke=Mock(return_value=subprocess.CompletedProcess([],0,'',''))
            with self.assertRaises(self.a.Denied):self.f.phase('neutral',self.n.env,invoke)
            invoke.assert_not_called();path.write_bytes(data)
            # Restore only synthetic test receipt identity for the next independent fault.
            rec=json.loads(self.receipt('lock').read_text());rec['locks'][mode]=self.a.file_subject(path);self.receipt('lock').write_text(json.dumps(rec))

    def test_missing_member_changed_source_and_false_generation_refuse(self):
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,'',''))
        self.n.plan['request']['rows'].remove(ROWS[1]);self.n.save()
        with self.assertRaises(self.a.Denied):self.n.command(BATCH)
        self.n.plan['request']['rows'].append(ROWS[1]);self.n.save()
        model=self.n.worktree/'crates/perl-lsp-rs-core/src/protocol/jsonrpc.rs';model.write_text(model.read_text()+'\n')
        with self.assertRaises(self.a.Denied):self.f.phase('lock',self.n.env,invoke)
        invoke.assert_not_called()

    def test_widened_sources_versions_checksums_and_duplicate_packages_refuse(self):
        self.f.validate_lock(LOCK.encode())
        wrong=(LOCK.replace('"serde", "serde_json"','"serde", "serde_json", "perl-parser-core"'),
               LOCK.replace('registry+https://github.com/rust-lang/crates.io-index','git+https://example.invalid/serde'),
               LOCK.replace('version = "1.0.229"','version = "1.0.228"'),
               LOCK.replace('4148590a','00000000'),LOCK+'\n[[package]]\n'+LOCK.split('[[package]]',2)[2])
        for data in wrong:
            with self.subTest(data=data),self.assertRaises((self.a.Denied,ValueError)):self.f.validate_lock(data.encode())

    def test_neutral_failure_and_wrong_negative_cause_are_not_semantic_success(self):
        self.lock();invoke=Mock(return_value=subprocess.CompletedProcess([],101,'','neutral error'))
        with self.assertRaises(self.a.Denied):self.f.phase('neutral',self.n.env,invoke)
        record=json.loads(self.receipt('neutral').read_text());self.assertFalse(record['diagnostic_validation']['passed'])
        self.receipt('neutral').unlink();self.neutral()
        for code,stderr in ((0,''),(101,'error[E0432]: unresolved import `other`\nperl_parser_core appears in unrelated context'),(101,'missing linker'),(75,ERROR),(-9,ERROR)):
            invoke=Mock(return_value=subprocess.CompletedProcess([],code,'',stderr))
            with self.subTest(code=code,stderr=stderr),self.assertRaises(self.a.Denied):self.f.phase('rejected',self.n.env,invoke)
            record=json.loads(self.receipt('rejected').read_text());self.assertEqual(record['stderr'],stderr);self.assertFalse(record['diagnostic_validation']['passed']);self.receipt('rejected').unlink()

    def test_generation_failure_or_absent_output_cannot_launch_checks(self):
        for code in (101,0):
            invoke=Mock(return_value=subprocess.CompletedProcess([],code,'','lock failure'))
            with self.subTest(code=code),self.assertRaises((self.a.Denied,OSError)):self.f.phase('lock',self.n.env,invoke)
            self.assertFalse(json.loads(self.receipt('lock').read_text())['generated_by_native_command']);self.receipt('lock').unlink()
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,'',''))
        with self.assertRaises(self.a.Denied):self.f.phase('neutral',self.n.env,invoke)
        invoke.assert_not_called()

    def test_generation_receipt_wrong_owner_snapshot_argv_or_success_refuse(self):
        self.lock();path=self.receipt('lock');good=json.loads(path.read_text())
        for key,value in (('exit_code',101),('generated_by_native_command',False),('tested_source','stale'),('marker_identity',[0,0]),('snapshot',{}),('argv',['cargo','check'])):
            bad={**good,key:value};path.write_text(json.dumps(bad));invoke=Mock(return_value=subprocess.CompletedProcess([],0,'',''))
            with self.subTest(key=key),self.assertRaises(self.a.Denied):self.f.phase('neutral',self.n.env,invoke)
            invoke.assert_not_called()
        path.write_text(json.dumps(good))

    def test_owning_summary_cannot_capture_without_actual_current_phases(self):
        good='running 2 tests\n'+''.join('test '+n+' ... ok\n' for n in NAMES)+'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n'
        invoke=Mock(return_value=subprocess.CompletedProcess([],0,good,''))
        with patch.object(self.f.shared,'capture_owning_artifact') as capture:
            with self.assertRaises(self.a.Denied):self.f.shared.owning_test(self.n.env,invoke,ROWS[0],BATCH,NAMES,'jsonrpc-owning')
            capture.assert_not_called()
        self.lock();self.neutral();self.f.phase('rejected',self.n.env,Mock(return_value=subprocess.CompletedProcess([],101,'',ERROR)))
        with patch.object(self.f.shared,'capture_owning_artifact') as capture:
            self.assertEqual(self.f.shared.owning_test(self.n.env,invoke,ROWS[0],BATCH,NAMES,'jsonrpc-owning'),0)
            capture.assert_called_once_with(good,self.n.env,BATCH,NAMES,'jsonrpc-owning')

    def test_actual_integration_artifact_kind_names_source_and_readonly_capture(self):
        path=self.n.paths['build']/'debug/deps/lsp_jsonrpc_dependency_probe-proof'
        path.parent.mkdir(parents=True);path.write_bytes(b'current integration executable');path.chmod(0o755)
        artifact={'reason':'compiler-artifact','manifest_path':str(self.n.worktree/'xtask/Cargo.toml'),
                  'target':{'name':'lsp_jsonrpc_dependency_probe','kind':['test'],'src_path':str(self.n.worktree/'xtask/tests/lsp_jsonrpc_dependency_probe.rs')},
                  'profile':{'test':True},'executable':str(path),'filenames':[str(path)],'fresh':True}
        output=json.dumps(artifact)+'\n'+json.dumps({'reason':'build-finished','success':True})+'\n'
        try:self.f.shared.capture_owning_artifact(output,self.n.env,BATCH,NAMES,'jsonrpc-owning')
        except self.a.Denied as error:self.fail('valid integration artifact refused: '+str(error))
        copies=list(self.n.paths['temp'].glob('jsonrpc-owning-*.json'));self.assertEqual(len(copies),1)
        record=json.loads(copies[0].read_text());self.assertEqual(record['cargo_artifact'],artifact);self.assertEqual(record['named_tests'],list(NAMES))
        self.assertEqual(record['copy']['sha256'],self.a.file_subject(path)['sha256'])
        for kind in (['bin'],['lib']):
            artifact['target']['kind']=kind;bad=json.dumps(artifact)+'\n'+json.dumps({'reason':'build-finished','success':True})+'\n'
            with self.assertRaises(self.a.Denied):self.f.shared.capture_owning_artifact(bad,self.n.env,BATCH,NAMES,'jsonrpc-owning')

    def test_real_std_transport_refusal_cannot_be_negative_compiler_success(self):
        # Compile the actual portable helper bodies, without the owning module's
        # dependency closure. A 75/error-name stand-in falsifies a swallowed
        # adapter refusal; this is not native owning-harness qualification.
        actual=Path(__file__).resolve().parents[2]
        source=Path(os.environ.get('JSONRPC_RUST_TEST_FILE',str(actual/'xtask/tests/lsp_jsonrpc_dependency_probe.rs'))).read_text()
        def function(name):
            start=source.index('fn '+name+'(');end=source.find('\nfn ',start+1)
            return source[start:end if end>=0 else len(source)]
        program='use std::{io,path::PathBuf,process::{Command,Output}};\n'
        program+='fn repo_root()->PathBuf {PathBuf::from('+json.dumps(str(self.n.worktree))+')}\n'
        program+='\n'.join(function(name) for name in ('admitted_roots','admitted_phase','output_text'))
        program+='\nfn main(){if std::env::args().nth(1).as_deref()==Some("partial"){assert!(admitted_roots().is_err());}else{assert!(admitted_phase("rejected").is_err(),"adapter refusal cannot satisfy negative compiler proof");}}\n'
        path=self.n.root/'transport.rs';path.write_text(program);binary=self.n.root/'transport'
        rustc=Path('/workspace/.cloud-tools/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc')
        result=subprocess.run([str(rustc),'--edition','2024',str(path),'-o',str(binary)],text=True,capture_output=True)
        self.assertEqual(result.returncode,0,result.stderr)
        script=self.n.worktree/'scripts/ci/jsonrpc_prepare.py';script.write_text('import sys\nprint("instrument refused mentioning perl_parser_core",file=sys.stderr)\nsys.exit(75)\n')
        env=os.environ.copy();env.update(CARGO_ADMITTED_RESOURCES='present',CARGO_ADMITTED_JSONRPC_PYTHON=os.path.realpath(os.sys.executable),CARGO_ADMITTED_JSONRPC_ROOT=str(self.root))
        result=subprocess.run([str(binary)],env=env,text=True,capture_output=True);self.assertEqual(result.returncode,0,result.stderr)
        env.pop('CARGO_ADMITTED_JSONRPC_ROOT')
        result=subprocess.run([str(binary),'partial'],env=env,text=True,capture_output=True);self.assertEqual(result.returncode,0,result.stderr)
