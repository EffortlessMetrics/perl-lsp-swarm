"""Native ownership microfixtures: no Cargo, whole-host scan or environment read."""
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import unittest

OWNER = Path(__file__).resolve().parents[1] / 'cargo_admitted.py'
PRELUDE = '''import ctypes, errno, importlib.util, json, os, signal, subprocess, sys, time
from pathlib import Path
from unittest.mock import patch
spec=importlib.util.spec_from_file_location('owner', sys.argv[1])
owner=importlib.util.module_from_spec(spec);spec.loader.exec_module(owner)
'''


@unittest.skipUnless(sys.platform == 'linux', 'Linux process-local ownership only')
class ClippyTreeTests(unittest.TestCase):
    def worker(self, code, *args):
        result = subprocess.run([sys.executable, '-c', PRELUDE + code, str(OWNER), *map(str,args)],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        return json.loads(result.stdout.splitlines()[-1])

    def test_detached_successive_generations_wait_for_kernel_closure(self):
        row = self.worker('''
tree=owner.ClippyTree()
code="""import os,time
if os.fork()==0:
 os.setsid()
 if os.fork()==0:
  if os.fork()==0:time.sleep(.25);os._exit(0)
  time.sleep(.15);os._exit(0)
 time.sleep(.05);os._exit(0)
os._exit(0)
"""
p=subprocess.Popen([sys.executable,'-c',code],start_new_session=True)
tree.track(p.pid,p)
assert p.wait()==0
first=tree.finish(timeout=.02)
assert not first['tree_settled']
flag=ctypes.c_int();tree.libc.prctl(37,ctypes.byref(flag),0,0,0);assert flag.value==1
final=tree.finish(timeout=2)
assert final['tree_settled'] and len(final['reaped_descendants'])>=3
print(json.dumps(final))
''')
        self.assertTrue(row['tree_settled'])
        self.assertGreaterEqual(len(row['reaped_descendants']), 3)

    def test_owned_probe_descendants_settle_in_same_scope(self):
        row = self.worker('''
tree=owner.ClippyTree()
code="""import os,time
if os.fork()==0:
 os.setsid()
 for fd in (0,1,2):os.close(fd)
 time.sleep(.15);os._exit(0)
print('probe',flush=True)
"""
result=tree.probe([sys.executable,'-c',code],os.environ.copy())
assert result.returncode==0 and result.stdout.strip()==b'probe'
final=tree.finish(timeout=2)
assert final['tree_settled'] and len(final['reaped_descendants'])==1
print(json.dumps(final))
''')
        self.assertTrue(row['tree_settled'])

    def test_cancellation_kills_bound_leader_ignoring_term(self):
        row = self.worker('''
tree=owner.ClippyTree()
p=subprocess.Popen([sys.executable,'-c',"import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);print('ready',flush=True);time.sleep(10)"],stdout=subprocess.PIPE)
tree.track(p.pid,p)
assert p.stdout.readline()==b'ready\\n'
tree.cancelled=True
final=tree.finish(stop=True,timeout=2,grace=.1)
assert final['tree_settled'] and final['cancelled'] and p.returncode==-signal.SIGKILL
p.stdout.close()
print(json.dumps(final))
''')
        self.assertTrue(row['cancelled'])

    def test_signal_during_probe_is_scoped_and_reaped(self):
        row = self.worker('''
tree=owner.ClippyTree()
code="import os,signal,time;time.sleep(.05);os.kill(os.getppid(),signal.SIGTERM);time.sleep(10)"
try:tree.probe([sys.executable,'-c',code],os.environ.copy())
except KeyboardInterrupt:pass
else:raise AssertionError('cancellation missing')
final=tree.finish(stop=True,timeout=2,grace=.1)
assert final['tree_settled'] and final['cancelled']
print(json.dumps(final))
''')
        self.assertTrue(row['tree_settled'])

    def test_existing_child_refuses_without_reaping_or_signalling(self):
        row = self.worker('''
p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(.2)'])
try:owner.ClippyTree()
except owner.Denied:pass
else:raise AssertionError('foreign child adopted')
assert p.poll() is None
assert p.wait()==0
print(json.dumps({'unrelated_child_untouched':True}))
''')
        self.assertTrue(row['unrelated_child_untouched'])

    def test_missing_pidfd_and_ignored_sigchld_refuse(self):
        row = self.worker('''
with patch.object(os,'pidfd_open',side_effect=OSError(errno.ENOSYS,'missing')):
 try:owner.ClippyTree()
 except OSError:pass
 else:raise AssertionError('missing pidfd accepted')
signal.signal(signal.SIGCHLD,signal.SIG_IGN)
try:owner.ClippyTree()
except owner.Denied:pass
else:raise AssertionError('auto-reaping handler accepted')
signal.signal(signal.SIGCHLD,signal.SIG_DFL)
print(json.dumps({'unsupported_refused':True}))
''')
        self.assertTrue(row['unsupported_refused'])

    def test_wait_errors_never_become_completion(self):
        row = self.worker('''
tree=owner.ClippyTree()
with patch.object(os,'waitpid',side_effect=OSError(errno.EIO,'ambiguous wait')):
 final=tree.finish(timeout=.1)
assert not final['tree_settled'] and final['errors']
# Restore only after a separate positive kernel closure, not the failed check.
second=tree.finish(timeout=1)
assert not second['tree_settled']  # original instrument error remains recorded
flag=ctypes.c_int();tree.libc.prctl(37,ctypes.byref(flag),0,0,0);assert flag.value==0
print(json.dumps(final))
''')
        self.assertFalse(row['tree_settled'])

    def test_competing_thread_and_claimed_subreaper_refuse(self):
        row = self.worker('''
import threading
thread=threading.Thread(target=lambda:time.sleep(.2));thread.start()
try:owner.ClippyTree()
except owner.Denied:pass
else:raise AssertionError('competing thread accepted')
thread.join()
libc=ctypes.CDLL(None);assert libc.prctl(36,1,0,0,0)==0
try:owner.ClippyTree()
except owner.Denied:pass
else:raise AssertionError('prior scope adopted')
assert libc.prctl(36,0,0,0,0)==0
print(json.dumps({'foreign_scope_refused':True}))
''')
        self.assertTrue(row['foreign_scope_refused'])

    def test_lost_subreaper_never_becomes_completion(self):
        row = self.worker('''
tree=owner.ClippyTree();assert tree.libc.prctl(36,0,0,0,0)==0
final=tree.finish(timeout=.1)
assert not final['tree_settled'] and final['errors']
print(json.dumps(final))
''')
        self.assertFalse(row['tree_settled'])

    def test_probe_startup_cancellation_binds_before_interrupting(self):
        row = self.worker('''
tree=owner.ClippyTree()
real_spawn=subprocess.Popen
def spawn(*args,**kwargs):
 process=real_spawn(*args,**kwargs)
 tree.interrupted(signal.SIGTERM,None)
 return process
with patch.object(subprocess,'Popen',side_effect=spawn):
 try:tree.probe([sys.executable,'-c','import time;time.sleep(10)'],os.environ.copy())
 except KeyboardInterrupt:pass
 else:raise AssertionError('pending cancellation lost')
assert len(tree.handles)==1
final=tree.finish(stop=True,timeout=2,grace=.1)
assert final['tree_settled'] and final['cancelled']
print(json.dumps(final))
''')
        self.assertTrue(row['tree_settled'])

    def test_unrelated_pidfd_candidate_never_signalled(self):
        outside = subprocess.Popen([sys.executable,'-c','import time;time.sleep(5)'])
        try:
            row = self.worker('''
tree=owner.ClippyTree()
p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(.4)'])
tree.track(p.pid,p)
tree.child_ids=lambda:[int(sys.argv[2])]
final=tree.finish(stop=True,timeout=.1)
assert not final['tree_settled'] and final['errors']
tree.child_ids=lambda:[]
tree.finish(stop=True,timeout=2,grace=.01)
print(json.dumps(final))
''', outside.pid)
            self.assertFalse(row['tree_settled'])
            self.assertIsNone(outside.poll())
        finally:
            outside.terminate()
            outside.wait(timeout=2)

    def test_non_sigchld_clone_is_included_by_wall(self):
        row = self.worker('''
if owner.platform.machine()!='x86_64':
 print(json.dumps({'not_applicable':True}));sys.exit(0)
tree=owner.ClippyTree()
pid=tree.libc.syscall(56,0,0,0,0,0)  # fork-like clone, no SIGCHLD exit signal
if pid<0:raise OSError(ctypes.get_errno(),'clone fixture failed')
if pid==0:time.sleep(.1);os._exit(0)
final=tree.finish(timeout=2)
assert final['tree_settled'] and any(r['pid']==pid for r in final['reaped_descendants'])
print(json.dumps(final))
''')
        if row.get('not_applicable'):
            self.skipTest('raw clone fixture is x86_64 only')
        self.assertTrue(row['tree_settled'])


if __name__ == '__main__':
    unittest.main()
