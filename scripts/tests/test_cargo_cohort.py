"""Portable filesystem/API controls and separate finite native tree evidence.

PortableTree is an explicit interface double, not host cancellation qualification.
Real native tests never replace process discovery. Unsupported hosts refuse launch.
"""
import hashlib
import json
import os
from pathlib import Path
import signal
import time
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import cargo_cohort as cohort


TARGET = {'package_id':'fixture', 'target': {'name':'fixture','kind':['test'],'test':True}, 'features':[], 'profile':{'test':True}}


class PortableTree:
    cancelled = False
    binding = False
    settled = True
    def track(self, pid, process):
        pass
    def finish(self, **kwargs):
        return {'tree_settled': self.settled, 'cancelled': self.cancelled,
                'proof': 'PORTABLE INTERFACE DOUBLE ONLY'}


@unittest.skipUnless(sys.platform == 'linux', 'Linux filesystem/interface controls')
class PortableControls(unittest.TestCase):
    def test_real_unreaped_zombie_cannot_supply_live_owner_identity(self):
        child = subprocess.Popen([sys.executable, '-c', 'raise SystemExit(0)'])
        try:
            deadline = time.monotonic() + 3
            while True:
                fields = Path(f'/proc/{child.pid}/stat').read_text().rsplit(')', 1)[1].split()
                if fields[0] == 'Z':
                    break
                self.assertLess(time.monotonic(), deadline)
                time.sleep(.01)
            fd = os.pidfd_open(child.pid)
            try:
                signal.pidfd_send_signal(fd, 0)  # This alone still succeeds.
                with self.assertRaisesRegex(cohort.Denied, 'outer owner has exited'):
                    cohort.Cohort._process_identity(child.pid)
            finally:
                os.close(fd)
        finally:
            child.wait()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.old = Path.cwd(); os.chdir(self.root); self.addCleanup(os.chdir, self.old)
        self.paths = {key: self.root / key for key in ('target', 'build', 'cargo_home', 'temp')}
        for path in self.paths.values(): path.mkdir()
        self.slot = self.root / 'slot'; self.slot.mkdir()
        self.lease = self.slot / 'cargo-active'; self.lease.mkdir()
        self.marker = self.lease / ('owner-'+'a'*32); self.marker.mkdir()
        self.source = self.root / 'source'; self.source.write_text('original')
        self.profile = self.root / 'profile'; self.profile.write_text('original-profile')
        self.manifests = []
        for name, subject in [('sources.json', self.source), ('profile.json', self.profile)]:
            manifest = self.root / name
            manifest.write_text(json.dumps({'files': [{'path': str(subject), 'sha256': hashlib.sha256(subject.read_bytes()).hexdigest()}]}))
            self.manifests.append(manifest)
        self.descriptor = {'worktree': str(self.root), 'resources': {k: str(v) for k,v in self.paths.items()},
                           'lease': str(self.lease), 'lease_identity': cohort.directory_identity(self.lease),
                           'lease_marker': str(self.marker), 'marker_identity': cohort.directory_identity(self.marker),
                           'pid': os.getppid()}
        self.exe = self.paths['target'] / 'executable'

    def owner(self, **kwargs):
        with patch.object(cohort, 'resource_plan', return_value=(self.root,self.slot,self.paths)):
            instance = cohort.Cohort(self.descriptor, *(hashlib.sha256(p.read_bytes()).hexdigest() for p in self.manifests),
                                source_subject=self.manifests[0], profile_subject=self.manifests[1], expected_targets=kwargs.pop('expected_targets', [TARGET]), evidence_path=self.root/('evidence-'+str(len(list(self.root.glob('evidence-*'))))), **kwargs)
        self.addCleanup(lambda: os.close(instance.ancestor_fd) if instance.ancestor_fd is not None else None)
        return instance

    def executable(self, path=None):
        path = path or self.exe; path.write_text('#!' + sys.executable + '\nimport sys\nsys.exit(0)\n'); path.chmod(0o700)
        return path

    def producer(self, status=0, consumer_status=0, mutation='', fresh=False):
        program = "import json,sys\nfrom pathlib import Path\np=Path(" + repr(str(self.exe)) + ")\np.write_text(" + repr('#!' + sys.executable + '\nimport sys\nsys.exit(' + str(consumer_status) + ')\n') + ")\np.chmod(0o700)\n" + mutation + "\nprint(json.dumps(dict({'package_id': 'fixture', 'target': {'name': 'fixture', 'kind': ['test'], 'test': True}, 'features': [], 'profile': {'test': True}},reason='compiler-artifact',executable=str(p),fresh=" + repr(fresh) + ")))\nsys.exit(" + str(status) + ")"
        return [sys.executable, '-c', program]

    def run_portable(self, instance, **kwargs):
        env = dict(os.environ, CARGO_TARGET_DIR=str(self.paths['target']), CARGO_BUILD_BUILD_DIR=str(self.paths['build']),
                   CARGO_HOME=str(self.paths['cargo_home']), TMPDIR=str(self.paths['temp']))
        with patch.object(cohort, 'children_capability', return_value='PORTABLE DOUBLE'), patch.object(cohort, 'ClippyTree', PortableTree):
            return instance.run(kwargs.pop('producer', self.producer()), kwargs.pop('consumers', {str(self.exe): []}), env, **kwargs)

    def test_success_consumes_exact_executable_then_reclaims_once(self):
        instance = self.owner()
        expected_bindings = [hashlib.sha256(path.read_bytes()).hexdigest() for path in self.manifests]
        observed = {}
        reclaim = instance._reclaim
        def observe_then_reclaim(path):
            metadata = path.lstat()
            parent = path.parent.stat()
            observed['file_identity'] = [metadata.st_dev, metadata.st_ino, metadata.st_size,
                                         metadata.st_mtime_ns, metadata.st_ctime_ns,
                                         metadata.st_mode, metadata.st_nlink]
            observed['parent_identity'] = [parent.st_dev, parent.st_ino]
            observed['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
            return reclaim(path)
        with patch.object(instance, '_reclaim', side_effect=observe_then_reclaim):
            row = self.run_portable(instance)
        self.assertEqual(row['consumed'], [str(self.exe)]); self.assertEqual(row['reclaimed'], [str(self.exe)])
        receipts = [json.loads(line) for line in Path(row['authorization_evidence']).read_text().splitlines()]
        self.assertEqual(receipts[0]['event'], 'authorization')
        self.assertEqual(receipts[0]['records'][0]['path'], str(self.exe))
        record = receipts[0]['records'][0]
        for field, value in observed.items():
            self.assertEqual(record[field], value)
        expected_executable = ('#!' + sys.executable + '\nimport sys\nsys.exit(0)\n').encode()
        self.assertEqual(record['sha256'], hashlib.sha256(expected_executable).hexdigest())
        self.assertEqual(record['binding'], expected_bindings)
        self.assertEqual(receipts[0]['source_profile_binding'], expected_bindings)
        self.assertEqual(receipts[0]['consumed'], [str(self.exe)])
        self.assertEqual(receipts[0]['product_status'], 0)
        self.assertTrue(receipts[0]['settlement']['tree_settled'])
        self.assertEqual(receipts[1], {'event':'reclaimed','path':str(self.exe)})
        self.executable()
        with self.assertRaises(cohort.Denied): instance._reclaim(self.exe)
        self.assertTrue(self.exe.exists())
        with self.assertRaises(cohort.Denied): self.run_portable(instance)

    def test_manifest_read_failure_or_interrupt_closes_descriptor(self):
        instance = self.owner()
        before = len(list(Path('/proc/self/fd').iterdir()))
        for error, outcome in ((OSError('read control'), cohort.Denied),
                               (KeyboardInterrupt('interrupt control'), KeyboardInterrupt)):
            with patch.object(cohort.os, 'read', side_effect=error):
                with self.assertRaises(outcome):
                    instance._check_owner()
            self.assertEqual(len(list(Path('/proc/self/fd').iterdir())), before)

    def test_failed_producer_consumer_unsettled_or_interrupted_preserve(self):
        for producer_status, consumer_status, settled, cancelled in [(17,0,True,False), (0,19,True,False), (0,0,False,False)]:
            with self.subTest(values=(producer_status,consumer_status,settled,cancelled)):
                if self.exe.exists(): self.exe.unlink()
                instance = self.owner()
                with patch.object(PortableTree, 'settled', settled), patch.object(PortableTree, 'cancelled', cancelled):
                    row = self.run_portable(instance, producer=self.producer(producer_status,consumer_status))
                self.assertEqual(row['reclaimed'], []); self.assertTrue(self.exe.exists())
                self.assertEqual(row['status'], 75 if not settled else (producer_status or consumer_status))
                self.assertTrue(Path(row['evidence']).exists())

    def test_foreign_symlink_hardlink_preexisting_and_pinned_controls(self):
        instance = self.owner(); self.executable()
        cases = [self.root / 'foreign', self.paths['target'] / 'alias']
        self.executable(cases[0]); cases[1].symlink_to(self.exe)
        for path in cases:
            with self.assertRaises((cohort.Denied,OSError)): instance._open(path)
        hardlink = self.paths['target'] / 'hardlink'; os.link(self.exe, hardlink)
        with self.assertRaises(cohort.Denied): instance._open(self.exe)
        hardlink.unlink()
        preexisting = self.owner()
        with self.assertRaises(cohort.Denied): preexisting._open(self.exe)
        self.exe.unlink(); pinned = self.owner(pinned=[self.exe]); self.executable()
        with self.assertRaises(cohort.Denied): pinned._open(self.exe)
        with self.assertRaises(cohort.Denied): instance._reclaim(self.exe)


    def test_pinned_dotdot_alias_preserves_canonical_executable(self):
        subdir = self.paths['target'] / 'subdir'; subdir.mkdir()
        alias = subdir / '..' / self.exe.name
        instance = self.owner(pinned=[alias])
        with self.assertRaises(cohort.Denied): self.run_portable(instance)
        self.assertTrue(self.exe.exists(), 'canonical executable reached through pin alias must remain')
        self.assertTrue(instance.evidence_path.exists())

    def test_linked_pin_refuses_before_any_launch(self):
        self.executable()
        alias = self.paths['target'] / 'pin-alias'; alias.symlink_to(self.exe)
        with patch.object(cohort.subprocess,'Popen') as launch:
            with self.assertRaises(cohort.Denied): self.owner(pinned=[alias])
            launch.assert_not_called()
        self.assertTrue(self.exe.exists()); self.assertTrue(alias.is_symlink())

    def test_source_profile_and_lease_replacement_preserve(self):
        for subject in (self.source,self.profile):
            instance = self.owner()
            mutation = 'Path(' + repr(str(subject)) + ").write_text('changed')"
            with self.assertRaises(cohort.Denied): self.run_portable(instance, producer=self.producer(mutation=mutation))
            self.assertTrue(self.exe.exists()); self.exe.unlink(); subject.write_text('original' if subject == self.source else 'original-profile')
        instance = self.owner(); self.marker.rename(self.lease/'old-marker'); self.marker.mkdir()
        with self.assertRaises(cohort.Denied): instance._check_owner()

    def test_exact_coverage_and_replacement_controls(self):
        instance = self.owner()
        with self.assertRaises(cohort.Denied): self.run_portable(instance, consumers={})
        self.assertTrue(self.exe.exists()); self.exe.unlink()
        instance = self.owner(); self.executable()
        instance._record(dict(TARGET, reason='compiler-artifact',executable=str(self.exe),fresh=False))
        self.exe.rename(self.paths['target']/'saved'); self.executable()
        with self.assertRaises(cohort.Denied):
            parent, parent_identity, identity, digest = instance._open(self.exe)
            os.close(parent)
            if (parent_identity, identity, digest, instance.binding) != instance.records[self.exe]: raise cohort.Denied('replacement')
        self.assertTrue(self.exe.exists())

    def test_missing_capability_refuses_before_launch(self):
        instance = self.owner()
        with patch.object(cohort, 'children_capability', side_effect=cohort.Denied('absent')), patch.object(cohort.subprocess, 'Popen') as launch:
            with self.assertRaises(cohort.Denied): instance.run([], {}, {})
            launch.assert_not_called()
            self.assertIsNone(instance.ancestor_fd)


    def test_interrupted_cohort_retains_evidence_and_artifacts(self):
        instance = self.owner()
        original = PortableTree.finish
        def cancelled_finish(tree, **kwargs):
            result = original(tree, **kwargs); result['cancelled'] = True; return result
        with patch.object(PortableTree, 'finish', cancelled_finish):
            row = self.run_portable(instance)
        self.assertEqual(row['status'],130); self.assertEqual(row['reclaimed'],[])
        self.assertTrue(self.exe.exists()); self.assertTrue(Path(row['evidence']).exists())




    def test_receipt_fsync_failure_preserves_consumed_executable(self):
        instance = self.owner(); original = os.fsync; count = 0
        def fail_receipt(fd):
            nonlocal count
            count += 1
            if count == 2: raise OSError('injected receipt persistence failure')
            original(fd)
        with patch.object(cohort.os,'fsync',side_effect=fail_receipt):
            with self.assertRaises(OSError): self.run_portable(instance)
        self.assertTrue(self.exe.exists(), 'failed authorization persistence must precede all deletion')
        self.assertTrue(instance.evidence_path.exists()); self.assertTrue(instance.receipt_path.exists())
        self.assertFalse(instance._cleanup_authorized)

    def test_receipt_write_failure_preserves_consumed_executable(self):
        instance = self.owner()
        with patch.object(instance,'_persist',side_effect=OSError('injected receipt write failure')):
            with self.assertRaises(OSError): self.run_portable(instance)
        self.assertTrue(self.exe.exists()); self.assertFalse(instance._cleanup_authorized)

    def test_fresh_artifact_is_rejected_and_retained(self):
        instance = self.owner()
        with self.assertRaises(cohort.Denied):
            self.run_portable(instance, producer=self.producer(fresh=True))
        self.assertTrue(self.exe.exists(), 'fresh compiler output must remain available')
        self.assertTrue(instance.evidence_path.exists(), 'compiler diagnostics must remain available')
        self.assertIn(b'"fresh": true', instance.evidence_path.read_bytes())

    def test_omitted_expected_target_is_rejected_and_retained(self):
        second = dict(TARGET, package_id='second', target=dict(TARGET['target'], name='second'))
        instance = self.owner(expected_targets=[TARGET, second])
        # Producer and consumer agree on the first executable but omit the
        # second requested harness. Agreement alone is insufficient coverage.
        with self.assertRaises(cohort.Denied):
            self.run_portable(instance, producer=self.producer(), consumers={str(self.exe): []})
        self.assertTrue(self.exe.exists(), 'incomplete coverage must preserve artifacts')
        self.assertTrue(instance.evidence_path.exists(), 'incomplete coverage must preserve diagnostics')



    def test_unsettled_malformed_producer_retains_error_evidence_and_executable(self):
        # Producer really writes an executable before malformed compiler output;
        # only settlement is an explicit interface double, not native proof.
        instance = self.owner()
        producer = self.producer(mutation="print('not-cargo-json');sys.exit(0)")
        with patch.object(PortableTree, 'settled', False):
            row = self.run_portable(instance, producer=producer)
        self.assertEqual(row['status'], 75)
        self.assertEqual(row['product_status'], 0)
        self.assertFalse(row['settlement']['tree_settled'])
        self.assertEqual(row['settlement']['error']['type'], 'Denied')
        self.assertIn('not Cargo JSON', row['settlement']['error']['message'])
        self.assertEqual(row['reclaimed'], []); self.assertEqual(row['consumed'], [])
        self.assertTrue(self.exe.exists())
        self.assertIn(b'not-cargo-json', instance.evidence_path.read_bytes())
        self.assertFalse(instance.receipt_path.exists())
        json.dumps(row)  # transferable failure context is serializable

    def test_settled_malformed_producer_remains_ordinary_refusal(self):
        instance = self.owner()
        producer = self.producer(mutation="print('not-cargo-json');sys.exit(0)")
        with self.assertRaisesRegex(cohort.Denied, 'not Cargo JSON'):
            self.run_portable(instance, producer=producer)
        self.assertTrue(instance.receipt['tree_settled'])
        self.assertEqual(instance.receipt['error']['type'], 'Denied')
        self.assertTrue(self.exe.exists())
        self.assertIn(b'not-cargo-json', instance.evidence_path.read_bytes())

    def test_actual_interruption_respects_settlement_status_and_preserves(self):
        # Explicit launch-interface fault, not native cancellation qualification.
        for settled in (False, True):
            with self.subTest(settled=settled):
                if self.exe.exists(): self.exe.unlink()
                instance = self.owner()
                launch = instance._launch
                def interrupted(tree, command, env, **kwargs):
                    launch(tree, command, env, **kwargs)
                    tree.cancelled = True
                    raise KeyboardInterrupt('injected interruption after producer')
                with patch.object(instance, '_launch', side_effect=interrupted), patch.object(PortableTree, 'settled', settled):
                    row = self.run_portable(instance)
                self.assertEqual(row['status'], 130 if settled else 75)
                self.assertEqual(row['reclaimed'], [])
                self.assertTrue(self.exe.exists()); self.assertTrue(instance.evidence_path.exists())
                self.assertIn('injected interruption',row['settlement']['interruption'])

    def test_target_executable_mapping_is_one_to_one(self):
        instance = self.owner(); self.executable()
        instance._record(dict(TARGET,reason='compiler-artifact',executable=str(self.exe),fresh=False))
        alias = self.paths['target'] / 'second'; self.executable(alias)
        with self.assertRaises(cohort.Denied):
            instance._record(dict(TARGET,reason='compiler-artifact',executable=str(alias),fresh=False))
        second = dict(TARGET,package_id='second')
        instance.expected_targets.add(json.dumps(second,sort_keys=True))
        with self.assertRaises(cohort.Denied):
            instance._record(dict(second,reason='compiler-artifact',executable=str(self.exe),fresh=False))
        self.assertTrue(alias.exists()); self.assertTrue(self.exe.exists())

    def test_non_test_product_is_never_reclaimable(self):
        instance = self.owner(); self.executable()
        public = dict(TARGET, target={'name':'public','kind':['bin'],'test':False})
        instance._record(dict(public, reason='compiler-artifact', executable=str(self.exe),fresh=False))
        self.assertEqual(instance.records,{})
        self.assertTrue(self.exe.exists())

    def test_environment_mismatch_closes_observation_without_launch(self):
        instance = self.owner()
        with patch.object(cohort, 'children_capability', return_value='PORTABLE DOUBLE'), patch.object(cohort, 'ClippyTree', PortableTree), patch.object(cohort.subprocess,'Popen') as launch:
            with self.assertRaises(cohort.Denied): instance.run([],{}, {})
            launch.assert_not_called()
            self.assertIsNone(instance.ancestor_fd)

    def test_foreign_owner_and_pid_reuse_refused(self):
        self.descriptor['pid'] = os.getpid()
        with self.assertRaises(cohort.Denied): self.owner()
        self.descriptor['pid'] = os.getppid(); instance = self.owner()
        with patch.object(instance, '_process_identity', return_value=(999,999)):
            with self.assertRaises(cohort.Denied): instance._check_owner()


@unittest.skipUnless(sys.platform == 'linux', 'native Linux tree')
class NativeControls(unittest.TestCase):
    def test_live_detached_descendant_delays_kernel_closure(self):
        program = r'''import os,subprocess,sys,time,json
from cargo_admitted import ClippyTree
tree=ClippyTree()
p=subprocess.Popen([sys.executable,'-c',"import os,time;\nif os.fork()==0:\n os.setsid();time.sleep(.25);os._exit(0)\nos._exit(0)"])
tree.track(p.pid,p);p.wait();start=time.monotonic();receipt=tree.finish(timeout=2)
assert receipt['tree_settled'],receipt
assert time.monotonic()-start >= .15,receipt
print(json.dumps(receipt))
'''
        env=dict(os.environ, PYTHONPATH=str(Path(cohort.__file__).parent))
        result=subprocess.run([sys.executable,'-c',program], env=env, capture_output=True,text=True,timeout=4)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('kernel ECHILD',result.stdout)


    def test_actual_host_capability_refusal_or_native_success(self):
        # Filesystem resource admission is a fixture; supervision/discovery are real.
        program = r"""import json,os,sys
from unittest.mock import patch
from test_cargo_cohort import PortableControls
import cargo_cohort as cohort
fixture=PortableControls(); fixture.setUp()
try:
 instance=fixture.owner()
 env=dict(os.environ,CARGO_TARGET_DIR=str(fixture.paths['target']),CARGO_BUILD_BUILD_DIR=str(fixture.paths['build']),CARGO_HOME=str(fixture.paths['cargo_home']),TMPDIR=str(fixture.paths['temp']))
 try:
  path=cohort.children_capability()
 except cohort.Denied:
  try:instance.run(fixture.producer(),{str(fixture.exe):[]},env)
  except cohort.Denied:pass
  else:raise AssertionError('unsupported native launch admitted')
  assert not fixture.exe.exists()
  assert instance.ancestor_fd is None
  print('NATIVE CAPABILITY REFUSAL')
 else:
  row=instance.run(fixture.producer(),{str(fixture.exe):[]},env)
  assert row['status']==0,row
  assert row['settlement']['tree_settled'],row
  assert row['reclaimed']==[str(fixture.exe)],row
  print('NATIVE SUCCESS ECHILD')
finally:fixture.doCleanups()
"""
        paths = [str(Path(__file__).parent),str(Path(cohort.__file__).parent)]
        result=subprocess.run([sys.executable,'-c',program],env=dict(os.environ,PYTHONPATH=os.pathsep.join(paths)),capture_output=True,text=True,timeout=4)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertTrue('NATIVE CAPABILITY REFUSAL' in result.stdout or 'NATIVE SUCCESS ECHILD' in result.stdout,result.stdout)


    def test_supported_host_actual_cohort_sigterm_cancellation(self):
        try:
            cohort.children_capability()
        except cohort.Denied as error:
            self.skipTest('native cancellation requires live-thread proc children: ' + str(error))
        # Admission uses disposable filesystem fixtures; native tree discovery,
        # SIGTERM handling, pidfd signals, subreaper adoption and waits are real.
        program = r"""import ctypes,json,os,sys
from cargo_admitted import ClippyTree
from test_cargo_cohort import PortableControls
fixture=PortableControls();fixture.setUp()
try:
 instance=fixture.owner()
 env=dict(os.environ,CARGO_TARGET_DIR=str(fixture.paths['target']),CARGO_BUILD_BUILD_DIR=str(fixture.paths['build']),CARGO_HOME=str(fixture.paths['cargo_home']),TMPDIR=str(fixture.paths['temp']))
 mutation="import os,signal,time\nready=Path("+repr(str(fixture.root/'helper-ready'))+")\nif os.fork()==0:\n os.setsid();ready.write_text('ready');time.sleep(5);os._exit(0)\nwhile not ready.exists():time.sleep(.001)\nos.kill(os.getppid(),signal.SIGTERM)\ntime.sleep(5)"
 row=instance.run(fixture.producer(mutation=mutation),{str(fixture.exe):[]},env,settlement_timeout=1)
 assert row['status']==130,row
 assert row['settlement']['tree_settled'],row
 assert row['settlement']['cancelled'],row
 assert row['reclaimed']==[],row
 assert fixture.exe.exists()
 assert instance.evidence_path.exists()
 print(json.dumps(row))
finally:
 # The unknown helper's finite fallback exceeds settlement deadline. Even a
 # broken cancellation mutant must drain all fixture descendants before exit.
 while True:
  try:os.waitpid(-1,ClippyTree.WALL)
  except ChildProcessError:break
 ctypes.CDLL(None,use_errno=True).prctl(36,0,0,0,0)
 fixture.doCleanups()
"""
        paths=[str(Path(__file__).parent),str(Path(cohort.__file__).parent)]
        result=subprocess.run([sys.executable,'-c',program],env=dict(os.environ,PYTHONPATH=os.pathsep.join(paths)),capture_output=True,text=True,timeout=8)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('kernel ECHILD',result.stdout)

    def test_pidfd_wait_failure_never_authorizes_signal(self):
        # Portable fault injection of the existing owner seam, not native proof.
        from cargo_admitted import ClippyTree
        tree=object.__new__(ClippyTree); tree.handles={}
        with patch('os.pidfd_open',return_value=123), patch('os.waitid',side_effect=ChildProcessError), patch('os.close') as close:
            with self.assertRaises(ChildProcessError): tree.track(456)
            close.assert_called_once_with(123)
            self.assertEqual(tree.handles,{})


class WorkflowControls(unittest.TestCase):
    def test_existing_job_reaches_cohort_suite_for_both_source_paths(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / '.github/workflows/ci-gate-self-tests.yml').read_text()
        triggers = workflow.split('\n  pull_request:', 1)[1].split('\njobs:', 1)[0]
        for path in ('scripts/cargo_cohort.py', 'scripts/tests/test_cargo_cohort.py'):
            self.assertIn("      - '" + path + "'", triggers.splitlines())
        job = workflow.split('\n  cargo-admitted-storage-self-test:', 1)[1].split('\n  cargo-toolchain-guard-self-test:', 1)[0]
        run = "python3 -m unittest discover -s scripts/tests -p 'test_cargo_cohort.py' -v"
        self.assertIn('        run: ' + run, job.splitlines())
        self.assertNotIn('continue-on-error:', job)
        self.assertNotIn('if:', job)
        self.assertNotIn('|| true', job)
        if sys.platform != 'linux':
            return  # Existing owning job is Ubuntu; no native Windows claim.
        with tempfile.TemporaryDirectory(prefix='cohort-workflow-') as name:
            directory = Path(name)
            executor = directory / 'python3'
            executor.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$ARGUMENT_LOG"\nexit "$EXECUTOR_STATUS"\n')
            executor.chmod(0o700)
            log = directory / 'argv'
            for status in (0, 23):
                env = dict(os.environ, PATH=str(directory) + os.pathsep + os.defpath,
                           ARGUMENT_LOG=str(log), EXECUTOR_STATUS=str(status))
                result = subprocess.run(['sh', '-c', run], env=env, capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, status)
                self.assertEqual(log.read_text().splitlines(),
                                 ['-m', 'unittest', 'discover', '-s', 'scripts/tests',
                                  '-p', 'test_cargo_cohort.py', '-v'])


if __name__ == '__main__': unittest.main()
