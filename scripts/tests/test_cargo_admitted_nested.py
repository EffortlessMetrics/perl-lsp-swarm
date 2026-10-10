"""Cheap exact-contract and owned-handoff controls; no Cargo product build."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location('owner_nested', Path(os.environ.get('NESTED_TEST_OWNER', str(Path(__file__).resolve().parents[1] / 'cargo_admitted.py'))))
a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a)

ORIGINAL_CONFIGURATION = a.nested_configuration
ORIGINAL_TOOL_REVALIDATE = a.revalidate_clippy_toolchain

EXPECTED = {
    'parser-check': ['check','--package','perl-parser','--message-format','json','--locked','--offline'],
    'parser-build': ['build','--package','perl-parser','--locked','--offline'],
    'parser-clippy': ['clippy','--package','perl-parser','--locked','--offline','--','-D','warnings'],
    'parser-lib': ['test','--package','perl-parser','--lib','--locked','--offline'],
    'dap-lsp-build': ['build','-p','perl-lsp-rs','--message-format=short','--locked'],
    'dap-core-build': ['build','-p','perl-lsp-rs-core','--message-format=short','--locked'],
    'dap-bin-build': ['build','-p','perl-dap','--bin','perl-dap','--locked'],
    'dap-clippy': ['clippy','-p','perl-dap','--lib','--locked','--','-D','warnings','-A','clippy::wildcard_imports'],
}

class NestedTests(unittest.TestCase):
    def setUp(self):
        t=tempfile.TemporaryDirectory(prefix='nested-owner-proof-');self.addCleanup(t.cleanup)
        self.root=Path(t.name);self.worktree=self.root/'repo';self.worktree.mkdir()
        self.slot=self.root/'slot';self.slot.mkdir()
        self.paths={k:self.slot/'worktrees'/'candidate'/k for k in ['target','build','cargo_home','temp']}
        for p in self.paths.values():p.mkdir(parents=True)
        for n in ['perl-parser','perl-incremental-parsing']:(self.worktree/'crates'/n).mkdir(parents=True)
        self.lock=self.slot/'cargo-active';self.lock.mkdir();self.marker=self.lock/'owner-test';self.marker.mkdir()
        self.tool={'root':str(self.root/'tools'),'subjects':{}}
        for n in ['cargo','rustc','rustdoc','cargo-clippy','clippy-driver']:
            p=self.root/n;p.write_text(n);self.tool['subjects'][n]=a.file_subject(p)
        self.plan={'request':{'schema_version':1,'rows':list(a.NESTED_COMMANDS)},'source':{'head':'fixture'},
                                                    'toolchain':self.tool,'configuration':[], 'network_environment':{}, 'build_environment':{}, 'compiler_environment':{'CARGO_PROFILE_DEV_DEBUG':'line-tables-only'},
                                                    'renderer':a.file_subject(Path(a.__file__))}
        self.receipt={'worktree':str(self.worktree),'resources':{k:str(v) for k,v in self.paths.items()},
                'lease':str(self.lock),'lease_marker':str(self.marker),'lease_identity':list(a.directory_identity(self.lock)),
                'marker_identity':list(a.directory_identity(self.marker)),'owner_process':a.process_fact(os.getpid()),'scope':{'jobs':1}}
        self.env={'CARGO_TARGET_DIR':str(self.paths['target']),'CARGO_BUILD_BUILD_DIR':str(self.paths['build']),
                                                'CARGO_HOME':str(self.paths['cargo_home']),'CARGO_BUILD_JOBS':'1','CARGO_INCREMENTAL':'0','RUSTUP_AUTO_INSTALL':'0',
                                                'CARGO_PROFILE_DEV_DEBUG':'line-tables-only',**{k:str(self.paths['temp']) for k in ['TEMP','TMP','TMPDIR']}}
        for k,n in [('CARGO','cargo'),('RUSTC','rustc'),('RUSTDOC','rustdoc')]:self.env[k]=self.tool['subjects'][n]['path']
        self.snapshot=self.slot/'snapshot.json';self.save()
        for name,kw in [('resource_plan',{'return_value':(self.worktree,self.slot,self.paths)}),
                                                                        ('nested_source',{'return_value':{'head':'fixture'}}),('nested_configuration',{'return_value':[]}),
                                                                        ('revalidate_clippy_toolchain',{'return_value':None})]:
            p=patch.object(a,name,**kw);p.start();self.addCleanup(p.stop)
    def save(self):
        self.receipt.pop('nested_snapshot',None)
        self.snapshot.write_text(json.dumps({'descriptor':self.receipt,'plan':self.plan}))
        self.receipt['nested_snapshot']=a.file_subject(self.snapshot)
        self.env['CARGO_ADMITTED_RESOURCES']=json.dumps(self.receipt)
    def command(self,row):return a.nested_command(row,self.env)
    def loader_fixture(self):
        toolroot=Path(self.tool['root']);self.tool['host']='x86_64-unknown-linux-gnu'
        dirs=[self.paths['target']/'debug',self.paths['build']/'debug/deps',toolroot/'lib/rustlib'/self.tool['host']/'lib']
        for d in dirs:d.mkdir(parents=True,exist_ok=True)
        self.plan['cargo_runtime']={'profile':'debug','loader_paths':list(map(str,dirs)),'executable':str(self.paths['target']/'debug/xtask')}
        self.save();self.env['LD_LIBRARY_PATH']=':'.join(map(str,dirs))
        return dirs
    def test_cargo_runtime_loader_normalized_after_validation(self):
        self.loader_fixture()
        with patch.object(a,'nested_runtime_ancestry',create=True) as origin:
            try:
                _,env,_=self.command('parser-build')
            except a.Denied as error:
                self.fail('legitimate Cargo runtime loader refused: '+str(error))
        origin.assert_called_once();self.assertNotIn('LD_LIBRARY_PATH',env)
        self.assertIn('LD_LIBRARY_PATH',self.env)
    def test_loader_exact_order_and_no_broad_subtree(self):
        dirs=self.loader_fixture();good=self.env['LD_LIBRARY_PATH']
        for bad in ['',good+':',':'+good,good+':/tmp',':'.join(map(str,reversed(dirs))),good.replace('/debug:', '/release:'),good.replace('/debug/deps:', '/debug/arbitrary:'),good+':'+str(dirs[0]),good.replace('/debug:', '/debug/../debug:')]:
            with self.subTest(value=bad),patch.object(a,'nested_runtime_ancestry'),self.assertRaises(a.Denied):
                self.env['LD_LIBRARY_PATH']=bad;self.command('parser-build')
        self.env['LD_LIBRARY_PATH']=good
        with patch.object(a,'nested_runtime_ancestry',side_effect=a.Denied('wrong origin')),self.assertRaises(a.Denied):self.command('parser-build')
        self.plan.pop('cargo_runtime');self.save()
        with self.assertRaises(a.Denied):self.command('parser-build')
    def test_loader_other_injections_and_stale_subject_still_refuse(self):
        self.loader_fixture()
        with patch.object(a,'nested_runtime_ancestry'):
            for key in ['LD_PRELOAD','LD_AUDIT','DYLD_LIBRARY_PATH']:
                self.env[key]='/tmp/evil'
                with self.assertRaises(a.Denied):self.command('parser-build')
                self.env.pop(key)
            with patch.object(a,'nested_source',return_value={'head':'changed'}),self.assertRaises(a.Denied):self.command('parser-build')
            with patch.object(a,'revalidate_clippy_toolchain',side_effect=a.Denied('changed')),self.assertRaises(a.Denied):self.command('parser-build')
    def test_loader_initial_ambient_rejection_preserved(self):
        self.loader_fixture()
        with self.assertRaises(a.Denied):a.clippy_environment({'LD_LIBRARY_PATH':self.env['LD_LIBRARY_PATH']})
        request=self.root/'plan-request';request.write_text(json.dumps(self.plan['request']))
        with self.assertRaises(a.Denied):a.nested_plan(str(request),{'LD_LIBRARY_PATH':self.env['LD_LIBRARY_PATH']},self.worktree,self.paths)
    def test_runtime_contract_only_exact_debug_xtask_bootstrap(self):
        self.loader_fixture()
        for prefix in [['build','-p','xtask','--locked'],['run','-p','xtask','--bin','xtask','--locked','--release'],['run','-p','xtask','--bin','xtask','--locked','--profile','agent'],['run','-p','other','--bin','xtask','--locked'],['run','-p','xtask','--bin','xtask','--locked','--target','x86_64-unknown-linux-gnu']]:
            self.assertIsNone(a.nested_runtime_contract(prefix+['--','gates'],self.paths,self.tool))

    def test_loader_validated_before_git_and_git_env_is_normalized(self):
        self.loader_fixture()
        with patch.object(a,'nested_runtime_ancestry'),patch.object(a,'resource_plan',return_value=(self.worktree,self.slot,self.paths)) as resource,patch.object(a,'nested_source',return_value={'head':'fixture'}) as source:
            self.command('parser-build')
        self.assertNotIn('LD_LIBRARY_PATH',source.call_args.args[1])
        self.assertNotIn('LD_LIBRARY_PATH',resource.call_args.args[0])
        self.env['LD_LIBRARY_PATH']+='/evil'
        with patch.object(a,'nested_source') as source,patch.object(a,'resource_plan') as resource,self.assertRaises(a.Denied):self.command('parser-build')
        source.assert_not_called();resource.assert_not_called()
    def test_loader_linked_directory_refuses(self):
        dirs=self.loader_fixture();dirs[1].rmdir();dirs[1].symlink_to(dirs[0],target_is_directory=True)
        with patch.object(a,'nested_runtime_ancestry'),self.assertRaises(a.Denied):self.command('parser-build')
    def test_runtime_ancestry_requires_order_identity_and_owner_boundary(self):
        self.loader_fixture();runtime=self.plan['cargo_runtime'];owner={'pid':100,'parent':1,'start':'1'}
        xtask=Path(runtime['executable']);xtask.write_text('xtask')
        facts={10:{'pid':10,'parent':20,'start':'10'},20:{'pid':20,'parent':100,'start':'20'},30:{'pid':30,'parent':100,'start':'30'},100:owner}
        links={10:'/usr/bin/python3',20:str(xtask),30:self.tool['subjects']['cargo']['path']}
        real_stat=Path.stat
        def proc_stat(p,*args,**kwargs):
            if str(p).startswith('/proc/') and p.name=='exe':return real_stat(Path(links[int(p.parent.name)]))
            return real_stat(p,*args,**kwargs)
        with patch.object(a.os,'getpid',return_value=10),patch.object(a,'process_fact',side_effect=lambda pid:facts[pid]),patch.object(a.os,'readlink',side_effect=lambda p:links[int(p.parent.name)]),patch.object(Path,'stat',proc_stat):
            a.nested_runtime_ancestry(owner,runtime)
            for pid in [20]:
                old=links[pid];links[pid]='/usr/bin/python3'
                with self.assertRaises(a.Denied):a.nested_runtime_ancestry(owner,runtime)
                links[pid]=old
            with self.assertRaises(a.Denied):a.nested_runtime_ancestry(facts[20],runtime)
            facts[20]['parent']=30
            with self.assertRaises(a.Denied):a.nested_runtime_ancestry(owner,runtime)
            facts[20]['parent']=100
            def changed_stat(p,*args,**kw):
                return type('S',(),{'st_dev':-1,'st_ino':-1})() if str(p)=='/proc/20/exe' else proc_stat(p,*args,**kw)
            with patch.object(Path,'stat',changed_stat),self.assertRaises(a.Denied):a.nested_runtime_ancestry(owner,runtime)

    def test_exact_fixed_contract_and_lints(self):
        for row,args in EXPECTED.items():
            with self.subTest(row=row):
                cmd,env,cwd=self.command(row)
                # Drop renderer-owned configs/output location, preserving consumer argv.
                pos=1 if args[0]=='clippy' else cmd.index(args[0]);suffix=cmd[pos+1:]
                while suffix and suffix[0]=='--config':suffix=suffix[2:]
                self.assertEqual(suffix[:2],['--target-dir',str(self.paths['target'])])
                self.assertEqual(suffix[2:],args[1:]);self.assertEqual(cwd,self.worktree)
                self.assertNotIn('--profile',cmd);self.assertNotIn('--all-targets',cmd);self.assertNotIn('--target',cmd)
                self.assertEqual(env['CARGO_PROFILE_DEV_DEBUG'],'line-tables-only')
                self.assertEqual(cmd[0],self.tool['subjects']['cargo-clippy' if args[0]=='clippy' else 'cargo']['path'])
                if args[0]=='clippy':self.assertEqual(env['CLIPPY_ARGS'],''.join(x+'__CLIPPY_HACKERY__' for x in args[args.index('--')+1:]))
    def test_all_finite_rows_render_private_controls(self):
        for row in a.NESTED_COMMANDS:
            with self.subTest(row=row):
                cmd,env,cwd=self.command(row)
                for key in ['CARGO_TARGET_DIR','CARGO_BUILD_BUILD_DIR','CARGO_HOME','TEMP','TMP','TMPDIR','CARGO_BUILD_JOBS','CARGO_INCREMENTAL']:
                    self.assertEqual(env[key],self.env[key])
                self.assertIn('build.build-dir='+json.dumps(str(self.paths['build'])),cmd)
                self.assertFalse(any(x in cmd for x in ['--release','--fix','--workspace']))
    def test_routed_denominator_and_compile_runtime_distinct(self):
        self.assertEqual(sum([139,8,16,290,108,181,1,16,236]),995)
        for row in ['routed-compile','routed-runtime']:
            cmd,_,_=self.command(row);packages=[cmd[i+1] for i,v in enumerate(cmd) if v=='-p']
            self.assertEqual(packages,['perl-dap','perl-incremental-parsing','perl-lsp-perltidy','perl-lsp-rs','perl-lsp-rs-core','perl-parser','perl-parser-bench','perllsp','xtask'])
            self.assertEqual('--no-run' in cmd,row=='routed-compile')
    def test_docs_only_flags_and_manifest_cwd(self):
        cmd,env,cwd=self.command('parser-doc')
        self.assertEqual(cwd,self.worktree/'crates/perl-parser');self.assertEqual(env['RUSTFLAGS'],'-W missing_docs')
        self.assertEqual(env['CARGO_ENCODED_RUSTFLAGS'],'-W\x1fmissing_docs')
        self.assertIn('env.CARGO_ENCODED_RUSTFLAGS.value='+json.dumps('-W\x1fmissing_docs'),cmd)
        self.env['RUSTFLAGS']='-W missing_docs'
        with self.assertRaises(a.Denied):self.command('parser-build')
    def test_graph_queries_no_unsupported_target_dir(self):
        for row in ['parser-tree','incremental-metadata']:
            cmd,_,_=self.command(row);self.assertNotIn('--target-dir',cmd)
        self.assertEqual(self.command('incremental-metadata')[2],self.worktree/'crates/perl-incremental-parsing')
    def test_no_unenumerated_raw_fallback(self):
        for row in ['server-run','formatter-alias','legacy-parser-run','clippy_full','clean','unknown']:
            with self.assertRaises(a.Denied):self.command(row)
        self.plan['request']['rows'].remove('dap-clippy');self.save()
        with self.assertRaises(a.Denied):self.command('dap-clippy')
    def test_original_lease_and_marker_identity(self):
        for name in ['lease_identity','marker_identity']:
            old=self.receipt[name];self.receipt[name]=[old[0],old[1]+1];self.save()
            with self.assertRaises(a.Denied):self.command('parser-build')
            self.receipt[name]=old
    def test_stale_mutated_descriptor_snapshot_source_tool_config(self):
        original=self.snapshot.read_text();self.snapshot.write_text(original+' ')
        with self.assertRaises(a.Denied):self.command('parser-build')
        self.save()
        for name,kw in [('nested_source',{'return_value':{'head':'other'}}),('nested_configuration',{'return_value':[{'changed':True}]}),
                                                                        ('revalidate_clippy_toolchain',{'side_effect':a.Denied('changed driver')})]:
            with patch.object(a,name,**kw),self.assertRaises(a.Denied):self.command('parser-build')
        self.receipt['worktree']=str(self.root);self.env['CARGO_ADMITTED_RESOURCES']=json.dumps(self.receipt)
        with self.assertRaises(a.Denied):self.command('parser-build')
    def test_environment_controls_and_injection(self):
        changes={'CARGO_TARGET_DIR':str(self.root),'CARGO_BUILD_BUILD_DIR':str(self.root),'CARGO_BUILD_JOBS':'4',
                'CARGO_INCREMENTAL':'1','TMPDIR':str(self.root),'RUSTUP_AUTO_INSTALL':'1','CARGO':'foreign',
                'RUSTC_WRAPPER':'foreign','LD_PRELOAD':'foreign','CLIPPY_ARGS':'-A warnings','RUSTFLAGS':'-A warnings',
                'CARGO_PROFILE_DEV_DEBUG':'0','CARGO_BUILD_RUSTC':'foreign','RUSTC_BOOTSTRAP':'1','RUSTUP_TOOLCHAIN':'foreign','CARGO_NET_OFFLINE':'true','CARGO_HTTP_PROXY':'private','HTTPS_PROXY':'private','PATH':'foreign','CC':'foreign','CXXFLAGS':'foreign'}
        for key,val in changes.items():
            with self.subTest(key=key),self.assertRaises(a.Denied):a.nested_command('parser-build',{**self.env,key:val})
    def test_owner_ancestry_not_just_pid_presence(self):
        self.receipt['owner_process']={**self.receipt['owner_process'],'start':'wrong'};self.save()
        with self.assertRaises(a.Denied):self.command('parser-build')
        self.receipt['owner_process']=a.process_fact(1);self.save()
        with self.assertRaises(a.Denied):self.command('parser-build')
    def test_bounded_request_rejects_unknown_fields_duplicates_variants(self):
        request=self.root/'request.json'
        cases=[{'schema_version':True,'rows':['parser-build']},{'schema_version':1,'rows':['unknown']},
            {'schema_version':1,'rows':[]},{'schema_version':1,'rows':['parser-build']*2},
            {'schema_version':1,'rows':['parser-build'],'argv':['--config','x']},
            {'schema_version':1,'rows':[{'id':'parser-build'}]}]
        for d in cases:
            request.write_text(json.dumps(d))
            with self.assertRaises(a.Denied):a.nested_plan(request,{},self.worktree,self.paths)
        request.write_text('{"schema_version":1,"schema_version":1,"rows":[]}')
        with self.assertRaises(a.Denied):a.bounded_json(request)
        request.write_text(' '*65537)
        with self.assertRaises(a.Denied):a.bounded_json(request)
    def test_missing_descriptor_and_nonlinux_refuse(self):
        with self.assertRaises(a.Denied):a.nested_command('parser-build',{})
        with patch.object(a.sys,'platform','win32'),self.assertRaises(a.Denied):a.nested_plan('/missing',{},self.worktree,self.paths)

    def test_main_binds_plan_before_launch_and_release_uses_existing_owner(self):
        self.marker.rmdir();self.lock.rmdir()
        parent={k:v for k,v in self.env.items() if k not in ['CARGO','RUSTC','RUSTDOC','CARGO_ADMITTED_RESOURCES']}
        tree=Mock();tree.finish.return_value={'tree_settled':True,'cancelled':False}
        seen=[]
        def launch(command,env,lock,owner,operation):
            self.assertIs(owner,tree);self.assertEqual(operation,'Cargo')
            self.assertEqual(command[0],self.tool['subjects']['cargo']['path'])
            receipt=json.loads(env['CARGO_ADMITTED_RESOURCES'])
            self.assertIn('nested_plan_sha256',receipt['scope'])
            self.assertTrue(Path(receipt['lease_marker']).is_dir())
            child,_,_=a.nested_command('dap-clippy',env)
            self.assertIn('clippy::wildcard_imports',child);seen.append(receipt)
            return 101
        with patch.object(a.Path,'cwd',return_value=self.worktree),patch.dict(os.environ,parent,clear=True),patch.object(a,'nested_plan',return_value=self.plan), \
                            patch.object(a,'check_capacity',return_value={'policy':'fixture'}),patch.object(a,'ClippyTree',return_value=tree), \
                            patch.object(a,'clippy_version_check',return_value={}),patch.object(a,'call_clippy',side_effect=launch):
            self.assertEqual(a.main(['--nested-plan','fixture','build','-p','xtask','--locked']),101)
        self.assertEqual(len(seen),1);tree.finish.assert_called_once();self.assertFalse(self.lock.exists())
        self.assertTrue(Path(seen[0]['nested_snapshot']['path']).is_file())  # retained owned evidence
    def test_main_refuses_bad_plan_before_lease_or_tree(self):
        self.marker.rmdir();self.lock.rmdir()
        with patch.object(a,'nested_plan',side_effect=a.Denied('unknown nested row')),patch.object(a,'ClippyTree') as tree:
            self.assertEqual(a.main(['--nested-plan','bad','build','-p','xtask','--locked']),75)
        tree.assert_not_called();self.assertFalse(self.lock.exists())
    def test_main_budget_scope_includes_plan_before_capacity(self):
        self.marker.rmdir();self.lock.rmdir()
        parent={k:v for k,v in self.env.items() if k not in ['CARGO','RUSTC','RUSTDOC','CARGO_ADMITTED_RESOURCES']}
        scopes=[]
        def budget(filename,scope,env):scopes.append(scope);return {'fixture':True}
        with patch.object(a.Path,'cwd',return_value=self.worktree),patch.dict(os.environ,parent,clear=True),patch.object(a,'nested_plan',return_value=self.plan), \
                            patch.object(a,'read_budget_file',side_effect=budget),patch.object(a,'check_capacity',return_value={'policy':'fixture'}):
            self.assertEqual(a.main(['--nested-plan','fixture','--preflight','--budget-file','fixture','build','-p','xtask','--locked']),0)
        self.assertIn('nested_plan_sha256',scopes[0]);self.assertFalse(self.lock.exists())
        self.plan['request']['rows']=['parser-build']
        with patch.object(a.Path,'cwd',return_value=self.worktree),patch.dict(os.environ,parent,clear=True),patch.object(a,'nested_plan',return_value=self.plan), \
                            patch.object(a,'read_budget_file',side_effect=budget),patch.object(a,'check_capacity',return_value={'policy':'fixture'}):
            self.assertEqual(a.main(['--nested-plan','fixture','--preflight','--budget-file','fixture','build','-p','xtask','--locked']),0)
        self.assertNotEqual(scopes[0]['nested_plan_sha256'],scopes[1]['nested_plan_sha256'])

    def test_recursive_main_refuses_before_path_allocation(self):
        with patch.dict(os.environ,self.env,clear=True),patch.object(a,'resource_plan') as allocate:
            self.assertEqual(a.main(['build','-p','xtask','--locked']),75)
        allocate.assert_not_called()
    def test_native_descendant_and_nonancestor(self):
        # Real kernel ancestry, with no age/PID-presence proxy for ownership.
        pid=os.fork()
        if pid==0:
            try:a.nested_descendant(a.process_fact(os.getppid()))
            except BaseException:os._exit(2)
            os._exit(0)
        _,status=os.waitpid(pid,0);self.assertEqual(os.waitstatus_to_exitcode(status),0)
        with self.assertRaises(a.Denied):a.nested_descendant({'pid':2147483647,'parent':1,'start':'0'})

    def test_effective_flag_add_change_delete_and_target_selectors(self):
        for key in ['RUSTDOCFLAGS','CARGO_ENCODED_RUSTDOCFLAGS','CARGO_BUILD_RUSTDOCFLAGS',
                                                        'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS','CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTDOCFLAGS']:
            with self.subTest(key=key),self.assertRaises(a.Denied):a.nested_command('parser-build',{**self.env,key:'-Cdebuginfo=1'})
            self.plan['compiler_environment'][key]='-Cdebuginfo=1';self.env[key]='-Cdebuginfo=1';self.save()
            self.command('parser-build')
            with self.assertRaises(a.Denied):a.nested_command('parser-build',{**self.env,key:'-Cdebuginfo=2'})
            dropped=dict(self.env);dropped.pop(key)
            with self.assertRaises(a.Denied):a.nested_command('parser-build',dropped)
            self.plan['compiler_environment'].pop(key);self.env.pop(key);self.save()
        for key in ['CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER','CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER']:
            with self.assertRaises(a.Denied):a.nested_command('parser-build',{**self.env,key:'foreign'})
    def test_lint_suppression_is_not_a_declared_qualifying_input(self):
        for key,value in [('RUSTFLAGS','--cap-lints allow'),('CARGO_ENCODED_RUSTFLAGS','--cap-lints\x1fallow'),
                ('CARGO_BUILD_RUSTFLAGS','-A warnings'),('RUSTDOCFLAGS','-A missing_docs'),
                ('CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS','--cap-lints allow')]:
            self.plan['compiler_environment'][key]=value;self.env[key]=value;self.save()
            with self.subTest(key=key),self.assertRaises(a.Denied):self.command('parser-clippy')
            self.plan['compiler_environment'].pop(key);self.env.pop(key)
    def test_effective_config_suppression_selectors_and_flags(self):
        config=self.worktree/'.cargo/config.toml';config.parent.mkdir()
        # Use actual config discovery/validation, not the happy-path fixture mock.
        native=ORIGINAL_CONFIGURATION
        for text in ['[build]\nrustflags=["--cap-lints", "allow"]',
                                                            '[target.x86_64-unknown-linux-gnu]\nlinker="foreign"',
                                                            '[env]\nRUSTDOCFLAGS="-A missing_docs"',
                                                            '[env]\nCARGO_BUILD_RUSTC="foreign"']:
            config.write_text(text)
            with self.assertRaises(a.Denied):native(self.worktree,self.paths)
        config.write_text('[build]\nrustflags=["-C", "debuginfo=1"]')
        self.assertEqual(len(native(self.worktree,self.paths)),1)

    def test_child_cli_execs_only_bound_direct_leaf_without_reacquisition(self):
        class ExecObserved(Exception):pass
        rendered=self.command('dap-clippy')
        with patch.object(a,'nested_command',return_value=rendered),patch.object(a.os,'chdir') as cwd, \
                            patch.object(a.os,'execve',side_effect=ExecObserved) as execute,patch.object(a,'resource_plan') as allocate:
            with self.assertRaises(ExecObserved):a.main(['--nested-row','dap-clippy'])
        cwd.assert_called_once_with(rendered[2]);execute.assert_called_once_with(rendered[0][0],rendered[0],rendered[1])
        allocate.assert_not_called()
        for argv in [['--nested-row'],['--nested-row','dap-clippy','--fix']]:
            with patch.object(a,'nested_command') as render:
                self.assertEqual(a.main(argv),75);render.assert_not_called()

    def test_undeclared_doc_flag_refuses(self):
        with self.assertRaises(a.Denied):
            a.nested_command('parser-build',{**self.env,'RUSTDOCFLAGS':'-Cdebuginfo=1'})

    def test_plan_reader_binds_exact_valid_request_and_refuses_suppression(self):
        request=self.root/'request.json';request.write_text(json.dumps({'schema_version':1,'rows':['parser-build','dap-clippy']}))
        parent={k:v for k,v in self.env.items() if k not in ['CARGO','RUSTC','RUSTDOC','CARGO_ADMITTED_RESOURCES']}
        with patch.object(a,'clippy_toolchain',return_value=self.tool):
            plan=a.nested_plan(request,parent,self.worktree,self.paths)
            self.assertEqual(plan['request']['rows'],['parser-build','dap-clippy'])
            self.assertEqual(plan['compiler_environment'],{'CARGO_PROFILE_DEV_DEBUG':'line-tables-only'})
            for key,val in [('RUSTFLAGS','--cap-lints allow'),('RUSTDOCFLAGS','-A missing_docs')]:
                with self.assertRaises(a.Denied):a.nested_plan(request,{**parent,key:val},self.worktree,self.paths)
    def test_real_tool_subject_mutation_refuses(self):
        native=ORIGINAL_TOOL_REVALIDATE
        pin=self.root/'pin.toml';pin.write_text('pin')
        self.tool['pin_subject']=a.file_subject(pin);self.save()
        native(self.tool)
        Path(self.tool['subjects']['clippy-driver']['path']).write_text('changed driver')
        with self.assertRaises(a.Denied):native(self.tool)
    def test_snapshot_json_subjects_are_roundtrip_stable(self):
        self.assertEqual(a.file_subject(self.snapshot),json.loads(json.dumps(a.file_subject(self.snapshot))))

    def test_proxy_add_change_delete_and_git_network_selectors(self):
        for key in ['HTTP_PROXY','HTTPS_PROXY','http_proxy','https_proxy','ALL_PROXY','NO_PROXY','SSL_CERT_FILE']:
            with self.subTest(key=key),self.assertRaises(a.Denied):
                a.nested_command('parser-tree',{**self.env,key:'private-proxy'})
            self.plan['network_environment'][key]='private-proxy';self.env[key]='private-proxy';self.save()
            cmd,_,_=self.command('parser-tree');self.assertFalse(any('private-proxy' in arg for arg in cmd))
            with self.assertRaises(a.Denied):a.nested_command('parser-tree',{**self.env,key:'changed-proxy'})
            dropped=dict(self.env);dropped.pop(key)
            with self.assertRaises(a.Denied):a.nested_command('parser-tree',dropped)
            self.plan['network_environment'].pop(key);self.env.pop(key);self.save()
        for key in ['GIT_SSH','GIT_SSH_COMMAND','GIT_PROXY_COMMAND']:
            with self.assertRaises(a.Denied):a.nested_command('parser-tree',{**self.env,key:'foreign'})

if __name__=='__main__':unittest.main()
