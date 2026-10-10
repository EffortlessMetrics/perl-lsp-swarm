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
if os.environ['FIXTURE_MODE']=='terminal':
 tty_error=None
 try:
  with open('/dev/tty','rb'):pass
 except OSError as error:tty_error=error.errno
 (root/'terminal').write_text(json.dumps({'tty_open':tty_error is None,'tty_error':tty_error,'session':os.getsid(0),'parent_session':os.getsid(os.getppid()),'group':os.getpgrp(),'parent_group':os.getpgid(os.getppid())}))
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
    def fixture(self, mode='normal', status=0, terminal=False):
        with tempfile.TemporaryDirectory(prefix='builtin-tree-control-') as name:
            root=Path(name);(root/'bin').mkdir()
            cargo=root/'bin/cargo'
            cargo.write_text('#!'+sys.executable+'\n'+FAKE);cargo.chmod(0o700)
            outer=PRELUDE+'''
import signal
tree=owner.ClippyTree();root=Path(sys.argv[2]);mode=sys.argv[3]
sentinel=None
if sys.argv[6]=='terminal':
 sentinel=subprocess.Popen([sys.executable,'-c','import time;time.sleep(4)'])
 tree.track(sentinel.pid,sentinel)
p=subprocess.Popen([sys.executable,'-c',sys.argv[5],sys.argv[1],str(root),mode,sys.argv[4]],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
tree.track(p.pid,p)
stdout,stderr=p.communicate(timeout=5)
assert p.returncode==0,(stdout,stderr)
result=json.loads(stdout.splitlines()[-1])
if mode!='cancel':
 result['early_release']=(root/'helper-live').exists() and not (root/'helper-end').exists() and not (root/'slot/cargo-active').exists()
result['lease_retained']=(root/'slot/cargo-active').exists()
result['stderr']=stderr
if mode=='terminal':result['terminal']=json.loads((root/'terminal').read_text())
if sentinel is not None:
 result['caller_group_survived']=sentinel.poll() is None
 signal.pidfd_send_signal(tree.handles[sentinel.pid][0],signal.SIGTERM)
settlement=tree.finish(timeout=2)
assert settlement['tree_settled'],settlement
if mode!='cancel':
 result['helper_end']=(root/'helper-end').read_text()
else:
 result['settlement']=next(json.loads(line.split(': ',1)[1]) for line in stderr.splitlines() if line.startswith('cargo-admitted Cargo settlement: '))
 result['resources']=next(json.loads(line.split(': ',1)[1]) for line in stderr.splitlines() if line.startswith('cargo-admitted resources: '))
 result['marker_retained']=os.path.lexists(result['settlement']['lease_marker'])
print(json.dumps(result))
'''
            # The sentinel shares the controlling group but is outside the inner
            # admitted owner's descendants; cancellation must leave it alive.
            terminal=terminal or mode=='terminal'
            command=[sys.executable,'-c',outer,str(OWNER),str(root),mode,str(status),CHILD,
                     'terminal' if terminal else 'ordinary']
            if terminal:
                import fcntl,pty,termios
                master,slave=pty.openpty()
                def controlling_terminal():
                    os.setsid()
                    fcntl.ioctl(0,termios.TIOCSCTTY,0)
                try:
                    p=subprocess.run(command,stdin=slave,stdout=subprocess.PIPE,stderr=subprocess.PIPE,
                                     text=True,timeout=8,preexec_fn=controlling_terminal)
                finally:
                    os.close(slave);os.close(master)
            else:
                p=subprocess.run(command,capture_output=True,text=True,timeout=8)
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
        # Cancellation may kill the helper before it writes its completion file.
        # Only kernel closure and release of the admitted lease establish success.
        settlement=row['settlement']
        self.assertTrue(settlement['cancelled'])
        self.assertTrue(settlement['tree_settled'])
        self.assertEqual(settlement['proof'],'kernel ECHILD (__WALL)')
        self.assertTrue(settlement['lease_released'])
        for key in ('lease_identity','lease_marker','marker_identity'):
            self.assertEqual(settlement[key],row['resources'][key])
        self.assertFalse(row['marker_retained'])

    def test_builtin_preserves_controlling_terminal_and_caller_session(self):
        row=self.fixture(mode='terminal')
        self.assertEqual(row['result'],0)
        self.assertFalse(row['lease_retained'])
        self.assertTrue(row['terminal']['tty_open'],row['terminal'])
        self.assertEqual(row['terminal']['session'],row['terminal']['parent_session'])
        self.assertEqual(row['terminal']['group'],row['terminal']['parent_group'])
        self.assertEqual(row['helper_end'],'lease-present')

    def test_terminal_cancellation_preserves_shared_caller_group(self):
        row=self.fixture(mode='cancel',terminal=True)
        self.assertEqual(row['result'],130)
        self.assertTrue(row['caller_group_survived'])
        self.assertTrue(row['settlement']['tree_settled'])
        self.assertTrue(row['settlement']['lease_released'])


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

    def test_builtin_spawn_cancellation_binds_before_unwind_without_group_signal(self):
        import contextlib,io,signal
        from unittest.mock import Mock,patch
        handlers={};output=io.StringIO();process=Mock(pid=98765,returncode=None)
        def install(signum,handler):
            old=handlers.get(signum,'old');handlers[signum]=handler;return old
        def spawn(*args,**kwargs):
            self.assertFalse(kwargs['start_new_session'])
            handlers[signal.SIGTERM](signal.SIGTERM,None)
            self.guard.track.assert_not_called()
            return process
        with patch.object(self.safe.subprocess,'Popen',side_effect=spawn), \
             patch.object(self.safe.signal,'signal',side_effect=install), \
             patch.object(self.safe.os,'killpg') as group_signal, \
             patch.object(self.safe.signal,'pidfd_send_signal') as descriptor_signal, \
             contextlib.redirect_stderr(output),self.assertRaises(KeyboardInterrupt):
            self.safe.call_clippy(['fixture'],{},self.root/'lease',self.guard,operation='Cargo')
        self.guard.track.assert_called_once_with(process.pid,process)
        group_signal.assert_not_called();descriptor_signal.assert_not_called()
        process.wait.assert_not_called()
        receipt=json.loads(output.getvalue().split(': ',1)[1])
        self.assertTrue(receipt['session_inherited'])
        self.assertEqual(receipt['process_group'],os.getpgrp())
        self.assertEqual(handlers[signal.SIGTERM],'old')

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
