"""Explicit proc-parent discovery; kernel waits alone authorize ownership."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch


def sibling(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


clippy = sibling('test_cargo_admitted_clippy_tree')
builtin = sibling('test_cargo_admitted_builtin_tree')


@unittest.skipUnless(sys.platform == 'linux', 'Linux kernel child ownership only')
class ProcParentDiscoveryTests(unittest.TestCase):
    worker = clippy.ClippyTreeTests.worker

    def test_explicit_profile_cancels_unknown_detached_adopted_child(self):
        row = self.worker('''
tree=owner.ClippyTree(child_discovery='proc-parent-pidfd')
code="""import os,signal,time
if os.fork()==0:
 os.setsid();signal.signal(signal.SIGTERM,signal.SIG_IGN)
 print(os.getpid(),flush=True)
 for fd in (0,1,2):os.close(fd)
 time.sleep(5);os._exit(0)
os._exit(0)
"""
p=subprocess.Popen([sys.executable,'-c',code],stdout=subprocess.PIPE);tree.track(p.pid,p)
pid=None
try:
 pid=int(p.stdout.readline());assert p.wait()==0
 assert pid not in tree.handles and pid in tree.child_ids()
 final=tree.finish(stop=True,timeout=2,grace=.01)
 assert final['tree_settled'] and final['proof']=='kernel ECHILD (__WALL)',final
 assert {'pid':pid,'exit_code':-9} in final['reaped_descendants'],final
finally:
 if pid is not None and not locals().get('final',{}).get('tree_settled'):
  tree.track(pid);tree.finish(stop=True,timeout=2,grace=.01)
 p.stdout.close()
print(json.dumps(final))
''')
        self.assertEqual(row['child_discovery'], 'proc-parent-pidfd')
        self.assertTrue(row['tree_settled'])

    def test_discovery_repeats_after_cancellation_adopts_another_generation(self):
        row = self.worker('''
tree=owner.ClippyTree(child_discovery='proc-parent-pidfd')
code="""import os,signal,time
def terminate(signum,frame):
 if os.fork()==0:
  signal.signal(signal.SIGTERM,signal.SIG_IGN);time.sleep(4);os._exit(0)
 os._exit(0)
if os.fork()==0:
 os.setsid();signal.signal(signal.SIGTERM,terminate)
 print(os.getpid(),flush=True)
 for fd in (0,1,2):os.close(fd)
 time.sleep(4);os._exit(0)
os._exit(0)
"""
p=subprocess.Popen([sys.executable,'-c',code],stdout=subprocess.PIPE);tree.track(p.pid,p)
pid=int(p.stdout.readline());assert p.wait()==0
try:final=tree.finish(stop=True,timeout=3,grace=.3)
finally:
 if not locals().get('final',{}).get('tree_settled'):tree.finish(stop=True,timeout=5,grace=.01)
 p.stdout.close()
assert final['tree_settled'],final
assert {'pid':pid,'exit_code':0} in final['reaped_descendants'],final
assert len(final['reaped_descendants'])==2 and any(r['exit_code']==-9 for r in final['reaped_descendants']),final
print(json.dumps(final))
''')
        self.assertEqual(len(row['reaped_descendants']), 2)

    def test_adoption_from_a_multithreaded_descendant_is_discoverable(self):
        row = self.worker('''
tree=owner.ClippyTree(child_discovery='proc-parent-pidfd')
code="""import os,threading,time
def fork_child():
 if os.fork()==0:
  os.setsid();print(os.getpid(),flush=True)
  for fd in (0,1,2):os.close(fd)
  time.sleep(4);os._exit(0)
t=threading.Thread(target=fork_child);t.start();t.join();os._exit(0)
"""
p=subprocess.Popen([sys.executable,'-c',code],stdout=subprocess.PIPE);tree.track(p.pid,p)
pid=int(p.stdout.readline());assert p.wait()==0
try:
 assert pid not in tree.handles and pid in tree.child_ids()
 final=tree.finish(stop=True,timeout=2,grace=.01)
finally:
 if not locals().get('final',{}).get('tree_settled'):
  tree.track(pid);tree.finish(stop=True,timeout=2,grace=.01)
 p.stdout.close()
assert final['tree_settled'] and any(r['pid']==pid for r in final['reaped_descendants']),final
print(json.dumps(final))
''')
        self.assertTrue(row['tree_settled'])

    def test_non_sigchld_clone_is_discovered_and_waited_with_wall(self):
        row = self.worker('''
if owner.platform.machine()!='x86_64':
 print(json.dumps({'not_applicable':True}));sys.exit(0)
tree=owner.ClippyTree(child_discovery='proc-parent-pidfd')
read_fd,write_fd=os.pipe()
pid=tree.libc.syscall(56,0,0,0,0,0)
if pid<0:raise OSError(ctypes.get_errno(),'clone fixture failed')
if pid==0:
 os.close(read_fd);os.write(write_fd,b'ready');os.close(write_fd)
 time.sleep(4);os._exit(0)
os.close(write_fd);assert os.read(read_fd,5)==b'ready';os.close(read_fd)
try:
 assert pid not in tree.handles and pid in tree.child_ids()
 final=tree.finish(stop=True,timeout=2,grace=.01)
finally:
 if not locals().get('final',{}).get('tree_settled'):
  tree.track(pid);tree.finish(stop=True,timeout=2,grace=.01)
assert final['tree_settled'] and any(r['pid']==pid for r in final['reaped_descendants']),final
print(json.dumps(final))
''')
        if row.get('not_applicable'):
            self.skipTest('raw clone fixture is x86_64 only')
        self.assertTrue(row['tree_settled'])

    def test_nonchild_nomination_is_rejected_without_signalling_it(self):
        outside = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(5)'])
        try:
            row = self.worker('''
tree=owner.ClippyTree(child_discovery='proc-parent-pidfd')
p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(3)']);tree.track(p.pid,p)
with patch.object(owner,'proc_parent_child_ids',return_value=[int(sys.argv[2])]):
 first=tree.finish(stop=True,timeout=.1,grace=.01)
assert not first['tree_settled'] and first['errors'],first
final=tree.finish(stop=True,timeout=2,grace=.01)
assert final['proof']=='kernel ECHILD (__WALL)' and not final['tree_settled'],final
print(json.dumps(first))
''', outside.pid)
            self.assertFalse(row['tree_settled'])
            self.assertIsNone(outside.poll())
        finally:
            outside.terminate()
            outside.wait(timeout=2)

    def test_completed_leader_numeric_key_does_not_hide_a_new_owned_child(self):
        row = self.worker('''
tree=owner.ClippyTree(child_discovery='proc-parent-pidfd')
old=subprocess.Popen([sys.executable,'-c','raise SystemExit(37)']);tree.track(old.pid,old)
assert old.wait()==37
old_handle=tree.handles.pop(old.pid)
new=subprocess.Popen([sys.executable,'-c','import time;time.sleep(3)'])
# Simulate numeric reuse without manipulating kernel PID allocation. Both
# descriptors/processes are real; the completed leader's stale key is the bug.
tree.handles[new.pid]=old_handle
try:
 with patch.object(os,'close',wraps=os.close) as close:
  tree.track(new.pid)
  assert tree.handles[new.pid][1] is None
  close.assert_called_once_with(old_handle[0])
 fd=tree.handles[new.pid][0]
 assert os.waitid(os.P_PIDFD,fd,os.WEXITED|os.WNOHANG|os.WNOWAIT|tree.WALL) is None
finally:
 new.kill();new.wait()
 final=tree.finish(timeout=2)
assert final['tree_settled'],final
print(json.dumps({'fresh_child_bound':True,'settlement':final}))
''')
        self.assertTrue(row['fresh_child_bound'])

    def test_visibility_and_record_failures_refuse_discovery(self):
        row = self.worker('''
read=owner._proc_read
for failure in ('hidepid','namespace','overlay','malformed-stat','wrong-pid','unreadable-stat'):
 def changed(path,limit):
  data=read(path,limit)
  if failure=='hidepid' and path=='/proc/self/mountinfo':
   return '\\n'.join(line.replace(' - proc proc rw',' - proc proc rw,hidepid=2') for line in data.splitlines())
  if failure=='overlay' and path=='/proc/self/mountinfo':
   return data+f'99999 83 0:30 / /proc/{os.getpid()}/stat ro - proc proc rw\\n'
  if failure=='namespace' and path=='/proc/self/status':
   return '\\n'.join('NSpid:\\t1 2' if line.startswith('NSpid:') else line for line in data.splitlines())
  if path.endswith('/stat') and path.split('/')[-2].isdecimal():
   if failure=='malformed-stat':return 'not a proc stat record'
   if failure=='wrong-pid':return '999999 '+data.split(' ',1)[1]
   if failure=='unreadable-stat':raise PermissionError(errno.EACCES,'fixture unreadable stat')
  return data
 with patch.object(owner,'_proc_read',side_effect=changed):
  try:owner.child_discovery_capability('proc-parent-pidfd')
  except owner.Denied:pass
  else:raise AssertionError(failure+' visibility failure accepted')
print(json.dumps({'refused':['hidepid','namespace','overlay','malformed-stat','wrong-pid','unreadable-stat']}))
''')
        self.assertEqual(len(row['refused']), 6)

    def test_discovery_turn_deadline_refuses_instead_of_returning_an_empty_table(self):
        row = self.worker('''
with patch.object(owner.time,'monotonic',side_effect=[0,2]):
 try:owner.proc_parent_child_ids()
 except owner.Denied as error:assert 'bounded turn' in str(error)
 else:raise AssertionError('unbounded discovery accepted')
print(json.dumps({'deadline_refused':True}))
''')
        self.assertTrue(row['deadline_refused'])

    def test_metadata_and_final_stat_overruns_refuse(self):
        row = self.worker('''
from types import SimpleNamespace
from contextlib import contextmanager
@contextmanager
def one_entry(path):
 yield iter([SimpleNamespace(name=str(os.getpid()))])
original=owner._proc_read
for phase in ('metadata','final-stat'):
 clock=[0]
 def delayed(path,limit):
  data=original(path,limit)
  if path==('/proc/self/status' if phase=='metadata' else '/proc/'+str(os.getpid())+'/stat'):clock[0]=2
  return data
 with patch.object(owner.time,'monotonic',side_effect=lambda:clock[0]),patch.object(owner,'_proc_read',side_effect=delayed),patch.object(owner.os,'scandir',side_effect=one_entry):
  try:owner.proc_parent_child_ids()
  except owner.Denied as error:assert 'bounded turn' in str(error)
  else:raise AssertionError(phase+' overrun accepted')
print(json.dumps({'overruns_refused':2}))
''')
        self.assertEqual(row['overruns_refused'], 2)

    def test_profile_selection_is_bound_into_the_native_budget_scope(self):
        row = self.worker('''
paths={name:Path('/tmp')/name for name in ('target','build','cargo_home','temp')}
base=owner.budget_scope(['test','--locked'],{},Path.cwd(),paths,2)
selected=owner.budget_scope(['test','--locked'],{'CARGO_ADMITTED_CHILD_DISCOVERY':'proc-parent-pidfd'},Path.cwd(),paths,2)
assert base['build_environment_sha256']!=selected['build_environment_sha256']
print(json.dumps({'scope_binds_profile':True}))
''')
        self.assertTrue(row['scope_binds_profile'])

    def test_unknown_profile_is_not_an_automatic_fallback(self):
        row = self.worker('''
with patch.dict(os.environ,{'CARGO_ADMITTED_CHILD_DISCOVERY':'unqualified'}):
 try:owner.ClippyTree()
 except owner.Denied:pass
 else:raise AssertionError('unqualified discovery profile accepted')
print(json.dumps({'refused':True}))
''')
        self.assertTrue(row['refused'])

    def test_default_profile_never_calls_namespace_enumeration(self):
        row = self.worker('''
with patch.dict(os.environ,{'CARGO_ADMITTED_CHILD_DISCOVERY':'proc-children'}),patch.object(owner,'proc_parent_child_ids',side_effect=AssertionError('automatic namespace scan')):
 tree=owner.ClippyTree()
 try:
  assert tree.child_discovery=='proc-children'
  tree.child_ids()
 finally:final=tree.finish(timeout=1)
assert final['tree_settled'],final
print(json.dumps({'default_preserved':True}))
''')
        self.assertTrue(row['default_preserved'])

    def test_proc_bytes_are_bounded_without_requiring_utf8_command_names(self):
        row = self.worker('''
import tempfile
with tempfile.TemporaryDirectory() as name:
 path=Path(name)/'record';path.write_bytes(b'process) (\\xff')
 assert owner._proc_read(path,64).encode('utf-8','surrogateescape')==path.read_bytes()
 try:owner._proc_read(path,2)
 except owner.Denied:pass
 else:raise AssertionError('oversized proc observation accepted')
print(json.dumps({'byte_bound':True,'non_utf8_preserved':True}))
''')
        self.assertTrue(row['byte_bound'])

    def test_native_admission_cancellation_preserves_caller_group_and_releases_matching_lease(self):
        child = builtin.CHILD.replace('original=owner.ClippyTree.finish',
                                     "env['CARGO_ADMITTED_CHILD_DISCOVERY']='proc-parent-pidfd'\noriginal=owner.ClippyTree.finish")
        with patch.object(builtin, 'CHILD', child):
            row = builtin.BuiltinNativeTreeTests().fixture(mode='cancel', terminal=True)
        self.assertEqual(row['result'], 130, row['stderr'])
        self.assertTrue(row['caller_group_survived'])
        self.assertEqual(row['settlement']['child_discovery'], 'proc-parent-pidfd')
        self.assertTrue(row['settlement']['tree_settled'])
        self.assertEqual(row['settlement']['proof'], 'kernel ECHILD (__WALL)')
        self.assertTrue(row['settlement']['lease_released'])
        self.assertFalse(row['lease_retained'])
        self.assertFalse(row['marker_retained'])
        for key in ('lease_identity', 'lease_marker', 'marker_identity'):
            self.assertEqual(row['settlement'][key], row['resources'][key])


if __name__ == '__main__':
    unittest.main()
