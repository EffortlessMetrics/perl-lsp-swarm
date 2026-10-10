"""Builtin Linux production route discriminator; finite fake Cargo, no Rust build.

A separate outer owner settles only these test fixtures, including the known-bad
leader-only implementation. It never authorizes production lease release.
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

OWNER = Path(__file__).resolve().parents[1] / 'cargo_admitted.py'
PRELUDE = '''import importlib.util,json,os,subprocess,sys,time
from pathlib import Path
from unittest.mock import patch
spec=importlib.util.spec_from_file_location('owner',sys.argv[1])
owner=importlib.util.module_from_spec(spec);spec.loader.exec_module(owner)
'''
CHILD = PRELUDE + '''
root=Path(sys.argv[2]);mode=sys.argv[3];status=int(sys.argv[4])
paths={key:root/key for key in ('target','build','cargo_home','temp')}
env={'PATH':str(root/'bin')+os.pathsep+os.defpath,'MIN_FREE_GB':'0.001','MAX_USED_PCT':'100',
     'FIXTURE_ROOT':str(root),'FIXTURE_STATUS':str(status),'FIXTURE_MODE':mode}
original=owner.ClippyTree.finish
if mode=='unproven':
 owner.ClippyTree.finish=lambda self,**kw:original(self,timeout=.01,**kw)
with patch.dict(os.environ,env,clear=True),patch.object(owner,'resource_plan',return_value=(root,root/'slot',paths)):
 result=owner.main(['test','-p','consumer','--locked','--offline'])
print(json.dumps({'result':result}),flush=True)
'''
FAKE = '''import json,os,signal,sys,time
from pathlib import Path
root=Path(os.environ['FIXTURE_ROOT']);status=int(os.environ['FIXTURE_STATUS'])
if os.fork()==0:
 os.setsid()
 for fd in (0,1,2):os.close(fd)
 (root/'helper-live').write_text('live')
 time.sleep(.3)
 (root/'helper-end').write_text('lease-present' if (root/'slot/cargo-active').exists() else 'lease-absent')
 os._exit(0)
while not (root/'helper-live').exists():time.sleep(.001)
if os.environ['FIXTURE_MODE']=='cancel':
 os.kill(os.getppid(),signal.SIGTERM)
 time.sleep(1)
os._exit(status)
'''


@unittest.skipUnless(sys.platform == 'linux', 'Linux ownership only')
class BuiltinNativeTreeTests(unittest.TestCase):
    def fixture(self, mode='normal', status=0):
        with tempfile.TemporaryDirectory(prefix='builtin-tree-control-') as name:
            root=Path(name);(root/'bin').mkdir()
            cargo=root/'bin/cargo'
            cargo.write_text('#!'+sys.executable+'\n'+FAKE);cargo.chmod(0o700)
            outer=PRELUDE+'''
tree=owner.ClippyTree();root=Path(sys.argv[2]);mode=sys.argv[3]
p=subprocess.Popen([sys.executable,'-c',sys.argv[5],sys.argv[1],str(root),mode,sys.argv[4]],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
tree.track(p.pid,p)
stdout,stderr=p.communicate(timeout=5)
assert p.returncode==0,(stdout,stderr)
result=json.loads(stdout.splitlines()[-1])
result['early_release']=(root/'helper-live').exists() and not (root/'helper-end').exists() and not (root/'slot/cargo-active').exists()
result['lease_retained']=(root/'slot/cargo-active').exists()
result['stderr']=stderr
settlement=tree.finish(timeout=2)
assert settlement['tree_settled'],settlement
result['helper_end']=(root/'helper-end').read_text()
print(json.dumps(result))
'''
            p=subprocess.run([sys.executable,'-c',outer,str(OWNER),str(root),mode,str(status),CHILD],capture_output=True,text=True,timeout=8)
            self.assertEqual(p.returncode,0,p.stderr+p.stdout)
            return json.loads(p.stdout.splitlines()[-1])

    def test_surviving_detached_helper_keeps_lease_for_success_and_cargo_failure(self):
        for status in (0,101):
            with self.subTest(status=status):
                row=self.fixture(status=status)
                self.assertFalse(row['early_release'],'leader exit cannot release while detached helper lives')
                self.assertEqual(row['helper_end'],'lease-present')
                self.assertEqual(row['result'],status)
                self.assertFalse(row['lease_retained'])
                self.assertIn('kernel ECHILD (__WALL)',row['stderr'])

    def test_unproven_live_descendant_retains_lease_and_overrides_success(self):
        row=self.fixture(mode='unproven')
        self.assertEqual(row['result'],75)
        self.assertTrue(row['lease_retained'])
        self.assertFalse(row['early_release'])
        self.assertEqual(row['helper_end'],'lease-present')

    def test_actual_sigterm_cancellation_settles_owned_detached_helper(self):
        row=self.fixture(mode='cancel')
        self.assertEqual(row['result'],130)
        self.assertFalse(row['lease_retained'])
        self.assertFalse(row['early_release'])
        self.assertEqual(row['helper_end'],'lease-present')
        self.assertIn('"cancelled": true',row['stderr'])


@unittest.skipUnless(sys.platform == 'linux', 'Linux route controls')
class BuiltinAdmissionControls(unittest.TestCase):
    def setUp(self):
        from unittest.mock import Mock
        spec=importlib.util.spec_from_file_location('safe',OWNER)
        self.safe=importlib.util.module_from_spec(spec);spec.loader.exec_module(self.safe)
        self.temp=tempfile.TemporaryDirectory(prefix='builtin-admission-');self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        self.paths={key:self.root/key for key in ('target','build','cargo_home','temp')}
        self.guard=Mock();self.guard.finish.return_value={'tree_settled':True,'cancelled':False}

    def invoke(self,status=0,settled=True,cancelled=False,host='linux',failure=None):
        import contextlib,io
        from unittest.mock import Mock,patch
        output=io.StringIO();self.guard.finish.return_value={'tree_settled':settled,'cancelled':cancelled}
        launch=Mock(return_value=status,side_effect=failure)
        with patch.dict(os.environ,{'MIN_FREE_GB':'0.001','MAX_USED_PCT':'100'},clear=True), \
             patch.object(self.safe.sys,'platform',host),contextlib.redirect_stderr(output), \
             patch.object(self.safe,'resource_plan',return_value=(self.root,self.root/'slot',self.paths)), \
             patch.object(self.safe,'ClippyTree',return_value=self.guard) as tree, \
             patch.object(self.safe,'call_clippy',launch),patch.object(self.safe.subprocess,'call',return_value=status,side_effect=failure if host != 'linux' else None) as legacy:
            result=self.safe.main(['test','-p','consumer','--locked','--offline'])
        return result,output.getvalue(),tree,launch,legacy

    def test_product_status_cannot_replace_closure(self):
        for status in (0,101,17,-9):
            with self.subTest(status=status):
                self.root=Path(self.temp.name)/str(status);self.root.mkdir()
                result,output,_,launch,legacy=self.invoke(status=status)
                self.assertEqual(result,status)
                self.assertFalse((self.root/'slot/cargo-active').exists())
                self.assertIn('cargo-admitted Cargo settlement:',output)
                self.assertIn('"lease_identity"',output);self.assertIn('"lease_marker"',output)
                self.assertEqual(launch.call_args.kwargs,{'operation':'Cargo'})
                self.assertEqual(launch.call_args.args[0][0],'cargo')
                legacy.assert_not_called()

    def test_unknown_closure_retains_and_overrides_product_success(self):
        result,output,_,_,_=self.invoke(settled=False)
        self.assertEqual(result,75);self.assertTrue((self.root/'slot/cargo-active').exists())
        self.assertIn('"lease_released": false',output)

    def test_capability_refusal_does_not_fallback_or_spawn(self):
        from unittest.mock import patch
        with patch.object(self.safe,'ClippyTree',side_effect=self.safe.Denied('missing pidfd')):
            # Invoke creates its own mock, so use a constructor failure directly.
            import contextlib,io
            with patch.dict(os.environ,{'MIN_FREE_GB':'0.001','MAX_USED_PCT':'100'},clear=True), \
                 patch.object(self.safe,'resource_plan',return_value=(self.root,self.root/'slot',self.paths)), \
                 patch.object(self.safe,'call_clippy') as launch,patch.object(self.safe.subprocess,'call') as legacy, \
                 contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(self.safe.main(['check']),75)
                launch.assert_not_called();legacy.assert_not_called()
        self.assertFalse((self.root/'slot/cargo-active').exists())

    def test_cancelled_and_release_failure_status_precedence(self):
        from unittest.mock import patch
        self.assertEqual(self.invoke(cancelled=True)[0],130)
        self.assertFalse((self.root/'slot/cargo-active').exists())
        self.assertEqual(self.invoke(failure=KeyboardInterrupt,settled=False)[0],75)
        self.assertTrue((self.root/'slot/cargo-active').exists())
        self.root=Path(self.temp.name)/'release';self.root.mkdir()
        with patch.object(self.safe,'release_lease'):
            self.assertEqual(self.invoke(cancelled=True)[0],75)
        self.assertTrue((self.root/'slot/cargo-active').exists())

    def test_other_platform_uses_legacy_route_without_claiming_kernel_closure(self):
        result,output,tree,launch,legacy=self.invoke(host='darwin')
        self.assertEqual(result,0);tree.assert_not_called();launch.assert_not_called();legacy.assert_called_once()
        self.assertNotIn('kernel ECHILD',output);self.assertNotIn('Cargo settlement:',output)

    def test_legacy_abnormal_exit_retains_lease_and_interruption_propagates(self):
        from unittest.mock import patch
        result,_,tree,launch,legacy=self.invoke(status=-9,host='darwin')
        self.assertEqual(result,-9);self.assertTrue((self.root/'slot/cargo-active').exists())
        tree.assert_not_called();launch.assert_not_called()
        self.root=Path(self.temp.name)/'legacy-interrupt';self.root.mkdir()
        with self.assertRaises(KeyboardInterrupt):
            self.invoke(host='darwin',failure=KeyboardInterrupt)


if __name__ == "__main__":
    unittest.main()
