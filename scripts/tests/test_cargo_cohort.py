from types import SimpleNamespace
import stat
"""Portable filesystem/API controls and separate finite native tree evidence.

PortableTree is an explicit interface double, not host cancellation qualification.
Real native tests never replace process discovery. Unsupported hosts refuse launch.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import signal
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import cargo_cohort as cohort


TARGET = {'package_id':'fixture', 'target': {'name':'fixture','kind':['test'],'test':True}, 'features':[], 'profile':{'test':True}}


class PortableTree:
    def __init__(self, *, child_discovery='proc-children'):
        self.child_discovery = child_discovery
        self.leaders = []
    cancelled = False
    binding = False
    settled = True
    def track(self, pid, process):
        self.leaders.append(process)
    def reap_adopted(self):
        pass  # Explicit service/interface double, not native reaping proof.
    def finish(self, **kwargs):
        for process in self.leaders:
            if kwargs.get('stop') and process.poll() is None: process.terminate()
            process.wait(timeout=5)
        return {'tree_settled': self.settled, 'cancelled': self.cancelled,
                'proof': 'PORTABLE INTERFACE DOUBLE ONLY'}


@unittest.skipUnless(sys.platform == 'linux', 'Linux wait-owner service contract')
class ActiveWaitControls(unittest.TestCase):
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

    def test_service_uses_masked_wait_and_preserves_exact_leader_status(self):
        original = signal.pthread_sigmask(signal.SIG_BLOCK, set())
        calls = []
        def service():
            blocked = signal.pthread_sigmask(signal.SIG_BLOCK, set())
            self.assertTrue({signal.SIGINT, signal.SIGTERM} <= blocked)
            calls.append('service')
        tree = Mock(); tree.reap_adopted.side_effect = service
        process = Mock(); process.wait.side_effect = [subprocess.TimeoutExpired('fixture', .01), 37]
        self.assertEqual(cohort.Cohort._wait_active(tree, process), 37)
        self.assertEqual(calls, ['service', 'service'])
        self.assertEqual(signal.pthread_sigmask(signal.SIG_BLOCK, set()), original)
        self.assertTrue(all(call.kwargs == {'timeout': .01} for call in process.wait.call_args_list))

    def test_service_failure_propagates_and_restores_mask(self):
        original = signal.pthread_sigmask(signal.SIG_BLOCK, set())
        tree = Mock(); tree.reap_adopted.side_effect = cohort.Denied('active wait failure')
        process = Mock()
        with self.assertRaisesRegex(cohort.Denied, 'active wait failure'):
            cohort.Cohort._wait_active(tree, process)
        process.wait.assert_not_called()
        self.assertEqual(signal.pthread_sigmask(signal.SIG_BLOCK, set()), original)


class SourcePathControls(unittest.TestCase):
    def test_fresh_all_ancestor_link_checks_and_replacement(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            parent = root / 'parent'; parent.mkdir()
            subject = parent / 'file'; subject.write_text('source')
            self.assertTrue(cohort.source_path_is_canonical(subject))
            parent.rename(root / 'old-parent')
            parent.symlink_to(root / 'old-parent', target_is_directory=True)
            self.assertFalse(cohort.source_path_is_canonical(subject))
            parent.unlink(); (root / 'old-parent').rename(parent)
            subject.rename(parent / 'original')
            subject.symlink_to(parent / 'original')
            self.assertFalse(cohort.source_path_is_canonical(subject))

    def test_noncanonical_and_missing_subjects_refuse(self):
        for path in [Path('relative'), Path('/a/../b'), Path('//host/source'),
                     Path('/mnt/c/source'), Path('/source/{template}')]:
            self.assertFalse(cohort.source_path_is_canonical(path), str(path))
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(FileNotFoundError):
                cohort.source_path_is_canonical(Path(directory) / 'missing')

    def test_every_current_component_is_checked_on_every_call(self):
        path = Path('/source/parent/file')
        metadata = SimpleNamespace(st_mode=stat.S_IFREG, st_file_attributes=0)
        with patch.object(Path, 'lstat', return_value=metadata) as observe:
            self.assertTrue(cohort.source_path_is_canonical(path))
            self.assertTrue(cohort.source_path_is_canonical(path))
            self.assertEqual(observe.call_count, 2 * len((path, *path.parents)))


@unittest.skipUnless(sys.platform == 'linux', 'Linux filesystem/interface controls')
class PortableControls(unittest.TestCase):
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
                           'pid': os.getppid(), 'admission': {'policy':'percent','reserve_bytes':1024,'max_used_pct':100}}
        self.exe = self.paths['target'] / 'executable'

    def test_discovery_profile_is_explicit_bound_and_shared_by_tree(self):
        instance = self.owner()
        with patch.dict(os.environ, {'CARGO_ADMITTED_CHILD_DISCOVERY': 'proc-parent-pidfd'}):
            with self.assertRaisesRegex(cohort.Denied, 'profile binding changed'):
                instance._check_owner()
            with self.assertRaisesRegex(cohort.Denied, 'differs from admitted environment'):
                self.owner()
            self.descriptor['child_discovery'] = 'proc-parent-pidfd'
            alternate = self.owner()
            with patch.object(cohort, 'child_discovery_capability', return_value={'mode': 'proc-parent-pidfd'}) as probe:
                self.assertEqual(cohort.children_capability('proc-parent-pidfd'), {'mode': 'proc-parent-pidfd'})
                probe.assert_called_once_with('proc-parent-pidfd')
            env = dict(os.environ, CARGO_TARGET_DIR=str(self.paths['target']),
                       CARGO_BUILD_BUILD_DIR=str(self.paths['build']), CARGO_HOME=str(self.paths['cargo_home']),
                       TMPDIR=str(self.paths['temp']))
            with patch.object(cohort, 'children_capability', return_value='PORTABLE DOUBLE') as capability, \
                    patch.object(cohort, 'ClippyTree', wraps=PortableTree) as tree:
                alternate.run(self.producer(), {str(self.exe): []}, env)
                capability.assert_called_once_with('proc-parent-pidfd')
                tree.assert_called_once_with(child_discovery='proc-parent-pidfd')
            alternate.descriptor['child_discovery'] = 'proc-children'
            with self.assertRaisesRegex(cohort.Denied, 'profile binding changed'):
                alternate._check_owner()

    def test_launch_profile_mismatch_refuses_before_process(self):
        instance = self.owner()
        with patch.object(cohort.subprocess, 'Popen') as launch:
            with self.assertRaisesRegex(cohort.Denied, 'launch discovery profile differs'):
                instance._launch(PortableTree(), ['must-not-launch'], {'CARGO_ADMITTED_CHILD_DISCOVERY': 'proc-parent-pidfd'})
            launch.assert_not_called()

    def test_wait_owner_profile_mismatch_refuses_before_process(self):
        instance = self.owner()
        with patch.object(cohort.subprocess, 'Popen') as launch:
            with self.assertRaisesRegex(cohort.Denied, 'wait owner discovery profile differs'):
                instance._launch(PortableTree(child_discovery='proc-parent-pidfd'), ['must-not-launch'], {})
            launch.assert_not_called()

    def test_missing_active_wait_service_refuses_before_process_launch(self):
        instance = self.owner()
        tree = PortableTree(); tree.reap_adopted = None
        try:
            with patch.object(cohort.subprocess, 'Popen') as launch:
                with self.assertRaisesRegex(cohort.Denied, 'active adopted-child reaping service'):
                    instance._launch(tree, ['must-not-launch'], {})
                launch.assert_not_called()
        finally:
            instance.close()

    def owner(self, **kwargs):
        with patch.object(cohort, 'resource_plan', return_value=(self.root,self.slot,self.paths)):
            instance = cohort.Cohort(self.descriptor, *(hashlib.sha256(p.read_bytes()).hexdigest() for p in self.manifests),
                                source_subject=self.manifests[0], profile_subject=self.manifests[1], expected_targets=kwargs.pop('expected_targets', [TARGET]), evidence_path=self.root/('evidence-'+str(len(list(self.root.glob('evidence-*'))))), **kwargs)
        self.addCleanup(instance.close)
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

    def test_invalid_eighth_final_record_blocks_authorization_and_every_unlink(self):
        targets = [dict(TARGET, target=dict(TARGET['target'], name='fixture'+str(i))) for i in range(8)]
        paths = [self.paths['target'] / ('image'+str(i)) for i in range(8)]
        aliases = [self.paths['build'] / ('alias'+str(i)) / 'image' for i in range(8)]
        instance = self.owner(expected_targets=targets)
        program = 'import json,os\nfrom pathlib import Path\n'
        for path, alias, target in zip(paths, aliases, targets):
            program += ('p=Path(' + repr(str(path)) + ');p.write_text(' + repr('#!'+sys.executable+'\nraise SystemExit(0)\n') + ');p.chmod(0o700)\n'
                        + 'a=Path(' + repr(str(alias)) + ');a.parent.mkdir();os.link(p,a)\n'
                        + 'print(' + repr(json.dumps(dict(target, reason='compiler-artifact', executable=str(path), fresh=False))) + ')\n')
        original_finish = PortableTree.finish
        def finish(tree, **kwargs):
            parent = aliases[-1].parent; held = parent.with_name('held-parent')
            parent.rename(held); parent.symlink_to(held, target_is_directory=True)
            return original_finish(tree, **kwargs)
        with patch.object(PortableTree, 'finish', finish), patch.object(cohort.os, 'unlink', wraps=os.unlink) as unlink:
            with self.assertRaises((cohort.Denied, OSError)):
                self.run_portable(instance, producer=[sys.executable, '-c', program], consumers={str(path): [] for path in paths})
            unlink.assert_not_called()
        self.assertFalse(instance._cleanup_authorized)
        self.assertFalse(instance.receipt_path.exists())
        self.assertTrue(all(path.is_file() for path in paths))
        self.assertTrue(all(alias.is_file() for alias in aliases))

    def test_retained_mode_consumes_preexisting_without_reclaiming(self):
        self.executable()
        before = cohort.facts(self.exe.stat())
        instance = self.owner(retain_preexisting=True)
        producer = [sys.executable, '-c', 'import json;print('+repr(json.dumps(dict(TARGET, reason='compiler-artifact', executable=str(self.exe), fresh=True)))+')']
        row = self.run_portable(instance, producer=producer)
        self.assertEqual(row['status'], 0)
        self.assertEqual(row['consumed'], [str(self.exe)])
        self.assertEqual(row['retained'], [str(self.exe)])
        self.assertEqual(row['reclaimed'], [])
        self.assertEqual(cohort.facts(self.exe.stat()), before)
        authorization = json.loads(instance.receipt_path.read_text().splitlines()[0])
        self.assertFalse(authorization['alias_groups'][0]['reclaimable'])

    def test_retained_mode_never_reclaims_rebuilt_preexisting_alias(self):
        self.executable()
        instance = self.owner(retain_preexisting=True)
        row = self.run_portable(instance)
        self.assertEqual(row['status'], 0)
        self.assertEqual(row['reclaimed'], [])
        self.assertTrue(self.exe.exists())

    def test_retained_group_cannot_be_reclaimed_by_direct_call(self):
        self.executable()
        instance = self.owner(retain_preexisting=True)
        instance._record(dict(TARGET, reason='compiler-artifact', executable=str(self.exe), fresh=True))
        instance.finished = True; instance.receipt = {'tree_settled': True}
        instance._cleanup_authorized = True; instance._receipt_stream = object()
        with self.assertRaisesRegex(cohort.Denied, 'no reclamation authority'):
            instance._reclaim(self.exe)
        self.assertTrue(self.exe.exists())

    def test_budget_file_launch_preserves_exact_admission(self):
        instance = self.owner()
        instance.descriptor['admission'] = {'policy': 'budget-file', 'reserve_bytes': 1024}
        with patch.object(cohort, 'check_capacity') as capacity:
            self.run_portable(instance)
        self.assertEqual(capacity.call_args.args[2], instance.descriptor['admission'])

    def test_bridge_claims_result_before_owner_construction(self):
        request = self.root / 'request.json'; request.write_text('{}')
        result = self.root / 'result.json'; result.write_text('prior evidence')
        with patch.object(cohort, 'Cohort') as constructor:
            with self.assertRaises(FileExistsError):
                cohort.main(['--request', str(request), '--result', str(result)])
            constructor.assert_not_called()
        self.assertEqual(result.read_text(), 'prior evidence')

    def test_duplicate_native_registrations_preserve_exact_multiplicity(self):
        for variant in ['complete', 'framed', 'inline', 'drop', 'extra', 'ignored_drop']:
            with self.subTest(variant=variant):
                instance = self.owner()
                regular = 1 if variant == 'drop' else 3 if variant == 'extra' else 2
                ignored = 1 if variant == 'ignored_drop' else 2
                run_lines = ['test repeated ... ok'] * regular + ['test ignored_repeat ... ignored'] * ignored
                if variant == 'ignored_drop':
                    run_lines.append('test ignored_repeat ... ok')
                if variant != 'complete':
                    payload = json.dumps({'jsonrpc':'2.0', 'method':'fixture',
                                          'params':{'text':'test repeated ... ok\n雪'}},
                                         ensure_ascii=False).encode('utf-8')
                    frame = b'Content-Length: ' + str(len(payload)).encode() + b'\r\n\r\n' + payload
                    if variant == 'inline':
                        run_lines = [line.replace(' ... ', ' ... ' + frame.decode('utf-8'), 1)
                                     for line in run_lines]
                    else:
                        run_lines = [frame.decode('utf-8') + line for line in run_lines]
                body = ('#!' + sys.executable + '\nimport sys\n'
                        "if '--list' in sys.argv:\n"
                        " print('ignored_repeat: test\\nignored_repeat: test' if '--ignored' in sys.argv else "
                        "'repeated: test\\nrepeated: test\\nignored_repeat: test\\nignored_repeat: test')\n"
                        'else:\n print(' + repr('\n'.join(run_lines)) + ')\n')
                program = ('import json\nfrom pathlib import Path\np=Path(' + repr(str(self.exe)) + ')\n'
                           'p.write_text(' + repr(body) + ');p.chmod(0o700)\n'
                           'print(json.dumps(' + repr(dict(TARGET, reason='compiler-artifact', executable=str(self.exe), fresh=False)) + '))')
                producer = [sys.executable, '-c', program]
                consumers = {str(self.exe): {'args': [], 'cwd': str(self.root), 'verify_harness': True}}
                if variant not in ('complete', 'framed', 'inline'):
                    with self.assertRaisesRegex(cohort.Denied, 'omitted/duplicated cases or changed ignored population') as failure:
                        self.run_portable(instance, producer=producer, consumers=consumers)
                    detail = json.loads(str(failure.exception).split('population: ', 1)[1])
                    key = {'drop':'missing', 'extra':'excess', 'ignored_drop':'ignored_missing'}[variant]
                    name = 'ignored_repeat' if variant == 'ignored_drop' else 'repeated'
                    self.assertEqual(detail[key], {'occurrences':1,'first_names':{name:1}})
                    self.assertEqual(detail['executable'],str(self.exe))
                    self.assertEqual(len(detail['logs']),3)
                    self.assertTrue(all(Path(log).is_file() for log in detail['logs']))
                    self.assertTrue(self.exe.exists())
                    self.exe.unlink()  # fixture-owned control reset, after settled double
                else:
                    row = self.run_portable(instance, producer=producer, consumers=consumers)
                    self.assertEqual(row['status'], 0)
                    coverage = row['coverage'][0]
                    self.assertEqual(coverage['cases'], ['ignored_repeat'] * 2 + ['repeated'] * 2)
                    self.assertEqual(coverage['ignored'], ['ignored_repeat'] * 2)
                    self.assertEqual((coverage['case_count'], coverage['ignored_count']), (4, 2))
                    self.assertFalse(self.exe.exists())

    def test_rpc_frames_use_exact_byte_boundaries_and_hide_body_labels(self):
        payload = json.dumps({'jsonrpc':'2.0', 'id':1, 'result':'test forged ... ok\n雪'},
                             ensure_ascii=False).encode('utf-8')
        frame = b'Content-Length: ' + str(len(payload)).encode() + b'\r\nContent-Type: application/json\r\n\r\n' + payload
        raw = frame + frame + b'test actual ... ok\n'
        self.assertEqual(cohort.libtest_text_without_rpc_frames(raw), '\n\ntest actual ... ok\n')
        # Arbitrary diagnostic prefixes never manufacture a completion line.
        self.assertEqual(cohort.libtest_text_without_rpc_frames(b'prefix test actual ... ok\n'),
                         'prefix test actual ... ok\n')

    def test_rpc_inline_frames_preserve_only_anchored_pending_completion(self):
        body = json.dumps({'jsonrpc': '2.0', 'id': 1,
                           'result': 'test forged ... ok\n雪'}, ensure_ascii=False).encode('utf-8')
        frame = b'Content-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body
        for suffix in (b'ok', b'ignored', b'ignored, reason'):
            with self.subTest(suffix=suffix):
                raw = b'test actual ... ' + frame + frame + suffix + b'\n'
                self.assertEqual(cohort.libtest_text_without_rpc_frames(raw),
                                 'test actual ... ' + suffix.decode() + '\n')
        # A diagnostic prefix must not become an anchored completion.
        raw = b'prefix test actual ... ' + frame + b'ok\n'
        self.assertEqual(cohort.libtest_text_without_rpc_frames(raw), raw.decode())
        # Boundary separation must not concatenate two outside fragments.
        raw = b'test actual ... \n' + frame + b'ok\n'
        self.assertEqual(cohort.libtest_text_without_rpc_frames(raw),
                         'test actual ... \n\nok\n')
        # Eligible inline malformed frames still refuse, never rescue a line.
        malformed = b'Content-Length: 99\r\n\r\n{}'
        with self.assertRaises(cohort.Denied):
            cohort.libtest_text_without_rpc_frames(b'test actual ... ' + malformed + b'ok\n')

    def test_rpc_malformed_frames_fail_closed(self):
        good = b'{"jsonrpc":"2.0","id":1,"result":null}'
        bodies = [b'{', b'{"jsonrpc":"1.0","id":1,"result":null}',
                  b'{"jsonrpc":"2.0","jsonrpc":"2.0","id":1,"result":null}',
                  b'{"jsonrpc":"2.0","id":true,"result":null}',
                  b'{"jsonrpc":"2.0","id":1,"error":"bad"}',
                  b'{"jsonrpc":"2.0","method":"x","params":1}',
                  b'{"jsonrpc":"2.0","id":1,"result":NaN}',
                  good + b'test forged ... ok\n']
        frames = [b'Content-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body for body in bodies]
        frames.extend([b'Content-Length: 99\r\n\r\n' + good,
                       b'Content-Length: 1\r\nContent-Length: 1\r\n\r\n{}',
                       b'Content-Length: -1\r\n\r\n{}', b'Content-Length: 1\n\n{}'])
        for frame in frames:
            with self.subTest(frame=frame), self.assertRaises(cohort.Denied):
                cohort.libtest_text_without_rpc_frames(frame)

    def test_authenticated_declaration_reuse_never_reuses_source_observations(self):
        with patch.object(cohort, '_prepare_source_declaration', wraps=cohort._prepare_source_declaration) as prepare, \
             patch.object(cohort, '_source_read_and_check', wraps=cohort._source_read_and_check) as read:
            instance = self.owner()
            prepared = prepare.call_count
            reads = read.call_count
            self.assertEqual(prepared, 2)
            instance._check_owner()
            self.assertEqual(prepare.call_count, prepared)
            self.assertEqual(read.call_count, 2 * reads)
            self.source.write_text('changed after declaration reuse')
            with self.assertRaisesRegex(cohort.Denied, 'source/profile bytes'):
                instance._check_owner()
            self.source.write_text('original')
            instance._check_owner()
            manifest = self.manifests[0]
            original = manifest.read_bytes()
            manifest.write_bytes(original + b' ')
            with self.assertRaisesRegex(cohort.Denied, 'manifest invalid or changed'):
                instance._check_owner()
            manifest.write_bytes(original)
            self.source.rename(self.root / 'original-source')
            self.source.symlink_to(self.root / 'original-source')
            with self.assertRaises(cohort.Denied):
                instance._check_owner()
            self.source.unlink()
            (self.root / 'original-source').rename(self.source)
            instance._check_owner()
            self.assertEqual(prepare.call_count, prepared)

    def test_source_epoch_rechecks_popped_ancestor_names(self):
        root = self.root / 'epoch'; (root / 'a').mkdir(parents=True); (root / 'z').mkdir()
        first = root / 'a' / 'first'; last = root / 'z' / 'last'
        first.write_bytes(b'first'); last.write_bytes(b'last')
        entries = [{'path':str(p), 'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in (first,last)]
        original = cohort._source_read_and_check
        def replace_visited(path, *args):
            original(path, *args)
            if path == last:
                (root / 'a').rename(root / 'old-a'); (root / 'a').mkdir()
        with patch.object(cohort, '_source_read_and_check', side_effect=replace_visited):
            with self.assertRaisesRegex(cohort.Denied, 'ancestor current'):
                cohort.validate_source_entries(root, entries)

    def test_source_epoch_closes_unstacked_descriptor_on_fstat_failure(self):
        subject = self.root / 'source-fd'; subject.write_bytes(b'source')
        entry = {'path':str(subject), 'sha256':hashlib.sha256(subject.read_bytes()).hexdigest()}
        original = os.fstat; count = 0
        def fail_second(descriptor):
            nonlocal count
            count += 1
            if count == 2:
                raise OSError('injected ancestor fstat failure')
            return original(descriptor)
        with patch.object(cohort.os, 'open', wraps=os.open) as opened, \
             patch.object(cohort.os, 'close', wraps=os.close) as closed, \
             patch.object(cohort.os, 'fstat', side_effect=fail_second):
            with self.assertRaisesRegex(OSError, 'injected'):
                cohort.validate_source_entries(self.root, [entry])
            self.assertEqual(opened.call_count, 2)
            self.assertEqual(closed.call_count, 2)

    def test_source_epoch_fallback_preserves_hash_checks_and_fresh_calls(self):
        subject = self.root / 'deep-source'; subject.write_bytes(b'original')
        entry = {'path':str(subject), 'sha256':hashlib.sha256(subject.read_bytes()).hexdigest()}
        with patch.object(cohort, '_SOURCE_DIRECTORY_STACK_LIMIT', 1), \
             patch.object(cohort, 'source_path_is_canonical', wraps=cohort.source_path_is_canonical) as canonical:
            cohort.validate_source_entries(self.root, [entry])
            canonical.assert_called_once_with(subject)
            subject.write_bytes(b'changed')
            with self.assertRaisesRegex(cohort.Denied, 'bytes or current identity'):
                cohort.validate_source_entries(self.root, [entry])
            self.assertEqual(canonical.call_count, 2)

    def test_success_consumes_exact_executable_then_reclaims_once(self):
        instance = self.owner()
        expected_bindings = [hashlib.sha256(path.read_bytes()).hexdigest() for path in self.manifests]
        observed = {}
        allocation = {}
        reclaim = instance._reclaim
        def observe_then_reclaim(path):
            metadata = path.lstat()
            parent = path.parent.stat()
            observed['file_identity'] = [metadata.st_dev, metadata.st_ino, metadata.st_size,
                                         metadata.st_mtime_ns, metadata.st_ctime_ns,
                                         metadata.st_mode, metadata.st_nlink]
            observed['parent_identity'] = [parent.st_dev, parent.st_ino]
            observed['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
            allocation.update(device=metadata.st_dev, inode=metadata.st_ino,
                              allocated_bytes=metadata.st_blocks * 512,
                              link_count=0, descriptor_held=True,
                              open_unlinked_bytes=metadata.st_blocks * 512)
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
        self.assertEqual(receipts[1], {'event':'reclaimed','path':str(self.exe), 'held_inode':allocation})
        self.assertNotEqual(allocation['allocated_bytes'], len(expected_executable))
        self.assertIsNone(instance.groups[self.exe]['fd'])  # normal close still runs before return
        self.executable()
        with self.assertRaises(cohort.Denied): instance._reclaim(self.exe)
        self.assertTrue(self.exe.exists())
        with self.assertRaises(cohort.Denied): self.run_portable(instance)

    def test_cargo_bin_environment_uses_observed_enabled_and_disabled_profile_paths(self):
        profile = self.paths['target'] / 'debug'; deps = profile / 'deps'
        deps.mkdir(parents=True)
        harness = deps / 'runtime-1234'
        enabled = profile / 'enabled_probe'
        disabled = profile / 'unbuilt_probe'
        package_id = 'fixture-package'
        target = {'package_id':package_id,'target':{'name':'runtime','kind':['test'],'src_path':str(self.root/'tests/runtime.rs'),'test':True},'features':[],'profile':{'test':True}}
        enabled.write_text('#!'+sys.executable+'\nraise SystemExit(0)\n'); enabled.chmod(0o700)
        consumer = '#!'+sys.executable+'\nimport os\nfrom pathlib import Path\n'+\
                   'assert os.environ["CARGO_BIN_EXE_enabled_probe"]=='+repr(str(enabled))+'\n'+\
                   'assert os.environ["CARGO_BIN_EXE_unbuilt_probe"]=='+repr(str(disabled))+'\n'+\
                   'assert Path(os.environ["CARGO_BIN_EXE_enabled_probe"]).is_file()\n'+\
                   'assert not Path(os.environ["CARGO_BIN_EXE_unbuilt_probe"]).exists()\n'
        producer = 'from pathlib import Path\nimport json,sys\n'+\
                   'h=Path('+repr(str(harness))+')\n'+\
                   'h.write_text('+repr(consumer)+')\nh.chmod(0o700)\n'+\
                   'print(json.dumps('+repr(dict(target,reason='compiler-artifact',executable=str(harness),fresh=False))+'))\n'+\
                   'print(json.dumps('+repr({'package_id':package_id,'target':{'name':'enabled_probe','kind':['bin'],'test':False},'features':[],'profile':{'test':False},'reason':'compiler-artifact','executable':str(enabled),'fresh':True})+'))\n'
        command = [sys.executable,'-c',producer]
        instance = self.owner(expected_targets=[target])
        consumers = {str(harness):{'args':[],'cwd':str(self.root),'env':{},'verify_harness':False,
                    'cargo_bin_exe':[{'name':'enabled_probe','enabled':True},{'name':'unbuilt_probe','enabled':False}]}}
        row = self.run_portable(instance,producer=command,consumers=consumers)
        self.assertEqual(row['status'],0)
        self.assertEqual(row['reclaimed'],[str(harness)])
        self.assertTrue(enabled.exists(),'companion Cargo binary must be retained')
        self.assertFalse(disabled.exists(),'disabled Cargo target remains an unbuilt path')

    def test_same_bin_test_harness_does_not_replace_runtime_artifact(self):
        profile=self.paths['target']/'debug';profile.mkdir(parents=True)
        public=profile/'enabled_probe';public.write_bytes(b'public binary')
        harness=self.paths['build']/'enabled-probe-test';harness.write_bytes(b'test harness')
        instance=self.owner()
        try:
            base={'reason':'compiler-artifact','package_id':'fixture-package',
                  'target':{'name':'enabled_probe','kind':['bin'],'test':True},'features':[],'fresh':False}
            runtime=dict(base,profile={'test':False},executable=str(public))
            test=dict(base,profile={'test':True},executable=str(harness))
            instance._record(test)
            exact={key:test[key] for key in ('package_id','target','features','profile')}
            exact_owner=self.owner(expected_targets=[exact],retain_preexisting=True)
            try:
                harness.chmod(0o700)
                exact_owner._record(dict(test,fresh=True))
                self.assertEqual(exact_owner.captured_targets,exact_owner.expected_targets)
                self.assertEqual(exact_owner.binary_artifacts,{})
                self.assertFalse(exact_owner.groups[harness]['reclaimable'])
            finally:exact_owner.close()
            with self.assertRaises(cohort.Denied):
                instance._binary_runtime_environment(harness,'fixture-package',[{'name':'enabled_probe','enabled':True}])
            instance._record(runtime)
            self.assertEqual(instance._binary_runtime_environment(harness,'fixture-package',[{'name':'enabled_probe','enabled':True}]),
                             {'CARGO_BIN_EXE_enabled_probe':str(public)})
            self.assertTrue(harness.exists());self.assertTrue(public.exists())
            for value in (None,1,'false'):
                with self.assertRaises(cohort.Denied):
                    instance._record(dict(base,profile={'test':value},executable=str(public)))
            foreign=self.paths['build']/'foreign-bin';foreign.write_bytes(b'foreign')
            instance._record(dict(runtime,executable=str(foreign)))
            with self.assertRaises(cohort.Denied):
                instance._binary_runtime_environment(harness,'fixture-package',[{'name':'enabled_probe','enabled':True}])
        finally:instance.close()

    def test_cargo_bin_environment_disagreement_retains_harness(self):
        profile=self.paths['target']/'debug';deps=profile/'deps';deps.mkdir(parents=True)
        harness=deps/'runtime-1234';package_id='fixture-package'
        target={'package_id':package_id,'target':{'name':'runtime','kind':['test'],'src_path':str(self.root/'tests/runtime.rs'),'test':True},'features':[],'profile':{'test':True}}
        producer='from pathlib import Path\nimport json\n'+\
                'h=Path('+repr(str(harness))+')\nh.write_text('+repr('#!'+sys.executable+'\nraise SystemExit(0)\n')+')\nh.chmod(0o700)\n'+\
                'print(json.dumps('+repr(dict(target,reason='compiler-artifact',executable=str(harness),fresh=False))+'))\n'
        instance=self.owner(expected_targets=[target])
        consumers={str(harness):{'args':[],'cwd':str(self.root),'env':{},'verify_harness':False,
                    'cargo_bin_exe':[{'name':'enabled_probe','enabled':True}]}}
        with self.assertRaises(cohort.Denied):self.run_portable(instance,producer=[sys.executable,'-c',producer],consumers=consumers)
        self.assertTrue(harness.exists(),'missing enabled-bin artifact must refuse cleanup')


    def phase_journal(self, instance):
        request_path = self.root/('request-'+str(len(list(self.root.glob('request-*'))))+'.json')
        request = {'evidence_path': str(instance.evidence_path), 'source_digest': instance.binding[0],
                   'profile_digest': instance.binding[1], 'expected_targets': [TARGET]}
        raw = json.dumps(request).encode(); request_path.write_bytes(raw)
        journal = cohort.PhaseJournal(request_path, raw, request, self.descriptor)
        self.addCleanup(journal.close); instance.journal = journal
        return journal, request_path

    def test_phase_journal_constructor_enter_precedes_expensive_owner_validation(self):
        instance = self.owner(); request_path = self.root/'original-request.json'
        request = {'evidence_path': str(instance.evidence_path), 'source_digest': instance.binding[0],
                   'profile_digest': instance.binding[1], 'source_subject': str(self.manifests[0]),
                   'profile_subject': str(self.manifests[1]), 'expected_targets': [TARGET], 'operation': 'observe'}
        request_path.write_text(json.dumps(request)); result_path = self.root/'original-result.json'
        def blocked(*args, **kwargs):
            rows = [json.loads(line) for line in kwargs['journal'].path.read_text().splitlines()]
            self.assertEqual(rows[-1]['phase'], 'construction')
            self.assertEqual(rows[-1]['event'], 'started')
            raise cohort.Denied('constructor refused')
        with patch.dict(os.environ, CARGO_ADMITTED_COHORT_DESCRIPTOR=json.dumps(self.descriptor)), patch.object(cohort, 'Cohort', side_effect=blocked):
            with self.assertRaisesRegex(cohort.Denied, 'constructor refused'):
                cohort.main(['--request',str(request_path),'--result',str(result_path)])
        self.assertEqual(result_path.read_bytes(), b'')

    def test_phase_journal_records_fast_child_and_real_failure_status_without_authority(self):
        instance = self.owner(); journal, _ = self.phase_journal(instance)
        row = self.run_portable(instance, producer=self.producer(status=101))
        self.assertEqual(row['status'], 101); self.assertEqual(row['reclaimed'], [])
        rows = [json.loads(line) for line in journal.path.read_text().splitlines()]
        children = [r for r in rows if r['event']=='child']
        self.assertEqual(len(children), 1)
        self.assertIs(type(children[0]['kernel_starttime']), int)
        self.assertEqual(next(r['exit_status'] for r in rows if r['event']=='completed' and r['phase']=='producer'), 101)
        self.assertTrue(self.exe.exists()); self.assertFalse(instance._cleanup_authorized)

    def test_phase_journal_write_or_fsync_failure_prevents_launch_and_preserves_settlement(self):
        for failing in ('write','fsync'):
            with self.subTest(failing=failing):
                instance = self.owner(); journal, _ = self.phase_journal(instance)
                with patch.object(cohort.os, failing, side_effect=OSError('journal persistence failed')), patch.object(cohort.subprocess, 'Popen') as launch:
                    with self.assertRaisesRegex(OSError, 'persistence'):
                        self.run_portable(instance)
                    launch.assert_not_called()
                self.assertFalse(instance._cleanup_authorized)

    def test_phase_journal_failure_after_child_binding_finishes_tree_and_retains(self):
        instance = self.owner(); journal, _ = self.phase_journal(instance)
        finish = Mock(wraps=PortableTree.finish)
        original_finish = PortableTree.finish
        settled = []
        def finished(tree, **kwargs):
            settled.append(kwargs['stop']);return original_finish(tree, **kwargs)
        with patch.object(journal, 'child', side_effect=OSError('child journal failed')), patch.object(PortableTree, 'finish', finished):
            with self.assertRaisesRegex(OSError, 'child journal failed'):
                self.run_portable(instance)
        self.assertEqual(settled, [True]); self.assertFalse(instance._cleanup_authorized)
        self.assertFalse(instance.receipt_path.exists())

    def test_phase_journal_persistence_error_does_not_mask_settled_or_unsettled_interrupt(self):
        for settled in (True, False):
            with self.subTest(settled=settled):
                instance = self.owner(); journal, _ = self.phase_journal(instance)
                original = journal.end
                def end(token, *args, **kwargs):
                    if token['phase']=='producer': raise OSError('interrupt journal failed')
                    return original(token, *args, **kwargs)
                with patch.object(journal,'end',side_effect=end), patch.object(instance,'_launch_impl',side_effect=KeyboardInterrupt('original interrupt')), patch.object(PortableTree,'settled',settled), patch.object(PortableTree,'cancelled',True):
                    row = self.run_portable(instance)
                self.assertEqual(row['status'],130 if settled else 75)
                self.assertEqual(row['settlement']['interruption'],'original interrupt')
                self.assertEqual(row['settlement']['journal_error']['type'],'OSError')
                self.assertFalse(instance._cleanup_authorized);self.assertEqual(row['reclaimed'],[])

    def test_phase_journal_interrupted_live_child_preserves_enter_identity_and_artifact(self):
        instance = self.owner(); journal, _ = self.phase_journal(instance)
        original = journal.child
        def interrupted(process):
            original(process)
            deadline = time.monotonic()+3
            while not self.exe.exists():
                self.assertLess(time.monotonic(),deadline);time.sleep(.01)
            raise KeyboardInterrupt('explicit interrupted-child interface control')
        program = [sys.executable,'-c','from pathlib import Path;import time;p=Path('+repr(str(self.exe))+');p.write_text("retained");time.sleep(5)']
        with patch.object(journal,'child',side_effect=interrupted):
            row = self.run_portable(instance,producer=program)
        self.assertEqual(row['status'],130);self.assertEqual(row['reclaimed'],[])
        self.assertTrue(self.exe.exists());self.assertFalse(instance._cleanup_authorized)
        records = [json.loads(l) for l in journal.path.read_text().splitlines()]
        self.assertTrue(any(r['event']=='child' for r in records))
        self.assertTrue(any(r['event']=='completed' and r['outcome']=='raised' for r in records))

    def test_phase_child_identity_accepts_observed_zombie_without_reaping(self):
        instance = self.owner(); journal, _ = self.phase_journal(instance)
        child = subprocess.Popen([sys.executable,'-c','raise SystemExit(0)'])
        try:
            deadline = time.monotonic()+3
            while Path('/proc/{}/stat'.format(child.pid)).read_text().rsplit(')',1)[1].split()[0] != 'Z':
                self.assertLess(time.monotonic(),deadline);time.sleep(.01)
            with journal.phase('consumer'):
                journal.child(child)
            row = next(json.loads(line) for line in journal.path.read_text().splitlines() if json.loads(line)['event']=='child')
            self.assertEqual(row['state'],'Z');self.assertIs(type(row['kernel_starttime']),int)
            self.assertEqual(child.wait(timeout=3),0)
        finally:
            if child.poll() is None: child.terminate();child.wait(timeout=3)

    def observed_capture(self, instance, path):
        parent, _, identity, digest = instance._open(path, allow_links=True)
        os.close(parent)
        group = instance._capture_group(path, identity, digest)
        instance.groups[path] = group
        return group

    def test_single_link_capture_avoids_scan_but_reopens_hashes_and_binds_exact_primary(self):
        instance = self.owner(); self.executable()
        parent, parent_id, identity, digest = instance._open(self.exe, allow_links=True)
        os.close(parent)
        real_read = os.read; image_reads = []
        def read(fd, size):
            chunk = real_read(fd, size)
            if chunk and cohort.facts(os.fstat(fd))[:2] == identity[:2]:
                image_reads.append(fd)
            return chunk
        with patch.object(Path, 'rglob', side_effect=AssertionError('single-link capture scanned')), \
                patch.object(instance, '_check_owner', wraps=instance._check_owner) as owner, \
                patch.object(cohort.os, 'read', side_effect=read):
            group = instance._capture_group(self.exe, identity, digest)
        instance.groups[self.exe] = group
        self.assertEqual(owner.call_count, 1)
        self.assertEqual(len(image_reads), 1)
        self.assertEqual(group['identity'], identity)
        self.assertEqual(group['initial_identity'], identity)
        self.assertEqual(group['sha256'], digest)
        self.assertEqual(cohort.facts(os.fstat(group['fd'])), identity)
        self.assertEqual(identity[-1], 1)
        self.assertEqual(group['remaining'], group['aliases'])
        self.assertTrue(group['reclaimable'])
        self.assertEqual(len(group['aliases']), 1)
        alias = group['aliases'][0]
        self.assertEqual(alias['path'], self.exe)
        self.assertEqual(alias['parent_identity'], parent_id)
        self.assertEqual(cohort.directory_identity(self.exe.parent), parent_id)
        self.assertEqual((os.fstat(alias['parent']).st_dev, os.fstat(alias['parent']).st_ino), parent_id)

    def test_multi_link_capture_keeps_both_root_scans_and_every_alias_fresh_open(self):
        instance = self.owner(); self.executable()
        aliases = [self.paths['target']/'second', self.paths['build']/'third']
        for alias in aliases: os.link(self.exe, alias)
        scanned = []; original = Path.rglob
        def scan(root, pattern):
            scanned.append(root)
            return original(root, pattern)
        with patch.object(Path, 'rglob', scan), patch.object(instance, '_check_owner', wraps=instance._check_owner) as owner:
            group = self.observed_capture(instance, self.exe)
        self.assertEqual(scanned, list(instance.roots))
        self.assertEqual(owner.call_count, 4)  # Initial observation + three alias opens.
        self.assertEqual(group['identity'][-1], 3)
        self.assertEqual({entry['path'] for entry in group['aliases']}, {self.exe, *aliases})
        self.assertEqual(cohort.facts(os.fstat(group['fd'])), group['identity'])

    def test_capture_final_held_inode_check_refuses_link_added_after_fresh_open(self):
        instance = self.owner(); self.executable()
        parent, _, identity, digest = instance._open(self.exe, allow_links=True); os.close(parent)
        late = self.paths['build']/'late-after-open'; original = instance._open
        def opened(path, **kwargs):
            result = original(path, **kwargs)
            os.link(path, late)
            return result
        with patch.object(instance, '_open', side_effect=opened):
            with self.assertRaisesRegex(cohort.Denied, 'changed during capture'):
                instance._capture_group(self.exe, identity, digest)
        self.assertTrue(self.exe.is_file()); self.assertTrue(late.is_file())

    def test_capture_refuses_late_link_path_parent_pin_preexisting_or_source_changes(self):
        for change in ('add', 'add-remove', 'remove', 'alias-replace', 'primary', 'parent', 'pin', 'preexisting', 'source'):
            with self.subTest(change=change):
                directory = self.paths['target']/change; directory.mkdir()
                path = directory/'image'
                instance = self.owner(); self.executable(path)
                alias = self.paths['build']/change
                if change in ('remove', 'alias-replace'): os.link(path, alias)
                parent, _, identity, digest = instance._open(path, allow_links=True); os.close(parent)
                if change in ('add', 'add-remove'):
                    os.link(path, alias)
                    if change == 'add-remove': alias.unlink()
                elif change == 'remove': alias.unlink()
                elif change == 'alias-replace': alias.unlink(); alias.write_bytes(b'replaced')
                elif change == 'primary': path.rename(path.with_name('held-image')); self.executable(path)
                elif change == 'parent':
                    directory.rename(directory.with_name(change+'-held'))
                    directory.symlink_to(directory.with_name(change+'-held'), target_is_directory=True)
                elif change == 'pin': instance.pinned.add(path)
                elif change == 'preexisting': instance.preexisting.add(path)
                elif change == 'source': self.source.write_text('changed')
                with patch.object(cohort.os, 'unlink', wraps=os.unlink) as unlink:
                    with self.assertRaises((cohort.Denied, OSError)):
                        instance._capture_group(path, identity, digest)
                    unlink.assert_not_called()
                self.assertTrue(path.is_file())
                if change == 'source': self.source.write_text('original')

    def test_new_private_cargo_alias_group_consumed_once_and_all_names_reclaimed(self):
        alias = self.paths['build'] / 'cargo-alias'
        library = self.paths['target'] / 'shared.rlib'; library.write_bytes(b'library')
        metadata = self.paths['build'] / 'shared.rmeta'; metadata.write_bytes(b'metadata')
        instance = self.owner()
        mutation = 'import os\nos.link(p,' + repr(str(alias)) + ')'
        observed = {}
        launch = instance._launch
        def independent_preconsume_observation(tree, command, env, **kwargs):
            if command[0] == str(self.exe):
                metadata = self.exe.stat()
                observed['file_identity'] = [metadata.st_dev, metadata.st_ino, metadata.st_size,
                                             metadata.st_mtime_ns, metadata.st_ctime_ns,
                                             metadata.st_mode, metadata.st_nlink]
                observed['sha256'] = hashlib.sha256(self.exe.read_bytes()).hexdigest()
                observed['allocated_bytes'] = metadata.st_blocks * 512
                observed['parents'] = {str(path): [path.parent.stat().st_dev, path.parent.stat().st_ino]
                                       for path in (self.exe, alias)}
            return launch(tree, command, env, **kwargs)
        with patch.object(instance, '_launch', side_effect=independent_preconsume_observation):
            row = self.run_portable(instance, producer=self.producer(mutation=mutation))
        self.assertEqual(row['consumed'], [str(self.exe)])
        self.assertEqual(set(row['reclaimed']), {str(self.exe), str(alias)})
        self.assertFalse(self.exe.exists()); self.assertFalse(alias.exists())
        self.assertEqual(library.read_bytes(),b'library'); self.assertEqual(metadata.read_bytes(),b'metadata')
        events = [json.loads(line) for line in instance.receipt_path.read_text().splitlines()]
        group = events[0]['alias_groups'][0]
        self.assertEqual(group['file_identity'],observed['file_identity'])
        self.assertEqual(group['sha256'],observed['sha256'])
        for entry in group['aliases']:
            self.assertEqual(entry['parent_identity'],observed['parents'][entry['path']])
        self.assertEqual(group['file_identity'][-1],2)
        self.assertEqual({entry['path'] for entry in events[0]['alias_groups'][0]['aliases']},{str(self.exe),str(alias)})
        self.assertEqual({entry['path'] for entry in events[1:]},{str(self.exe),str(alias)})
        self.assertEqual(len(events), 3)  # one authorization and the same two unlink receipts
        held = [entry['held_inode'] for entry in events[1:]]
        self.assertEqual([entry['link_count'] for entry in held], [1, 0])
        self.assertEqual([entry['open_unlinked_bytes'] for entry in held], [0, observed['allocated_bytes']])
        self.assertEqual({(entry['device'], entry['inode']) for entry in held},
                         {tuple(observed['file_identity'][:2])})
        self.assertTrue(all(entry['descriptor_held'] is True for entry in held))
        self.assertTrue(all(entry['allocated_bytes'] == observed['allocated_bytes'] for entry in held))
        self.assertIsNone(instance.groups[self.exe]['fd'])
        self.executable()
        with self.assertRaises(cohort.Denied): instance._reclaim(self.exe)
        self.assertTrue(self.exe.exists())

    def test_unknown_preexisting_or_pinned_alias_preserves_entire_group(self):
        for case in ('foreign','preexisting','pinned'):
            with self.subTest(case=case):
                if self.exe.exists(): self.exe.unlink()
                alias = (self.root if case=='foreign' else self.paths['build']) / ('alias-'+case)
                if case=='preexisting': alias.write_text('old')
                instance = self.owner(pinned=[alias] if case=='pinned' else [])
                mutation = 'import os\n' + ('os.unlink('+repr(str(alias))+')\n' if case=='preexisting' else '') + 'os.link(p,'+repr(str(alias))+')'
                with self.assertRaises(cohort.Denied): self.run_portable(instance, producer=self.producer(mutation=mutation))
                self.assertTrue(self.exe.exists()); self.assertTrue(alias.exists())
                self.assertEqual(self.exe.stat().st_ino,alias.stat().st_ino)
                self.exe.unlink(); alias.unlink()

    def test_alias_replacement_or_new_link_during_consumption_preserves(self):
        for case in ('replace','link'):
            with self.subTest(case=case):
                if self.exe.exists(): self.exe.unlink()
                alias = self.paths['build'] / ('consume-'+case)
                late = self.paths['target'] / ('late-'+case)
                action = ('os.unlink('+repr(str(alias))+');Path('+repr(str(alias))+').write_text("replacement")' if case=='replace' else 'os.link('+repr(str(self.exe))+','+repr(str(late))+')')
                body = '#!'+sys.executable+'\nimport os\nfrom pathlib import Path\n'+action+'\n'
                mutation = 'import os\np.write_text('+repr(body)+')\np.chmod(0o700)\nos.link(p,'+repr(str(alias))+')'
                instance = self.owner()
                with self.assertRaises(cohort.Denied): self.run_portable(instance,producer=self.producer(mutation=mutation))
                self.assertTrue(self.exe.exists()); self.assertTrue(alias.exists())
                self.exe.unlink(); alias.unlink()
                if late.exists(): late.unlink()

    def test_interrupted_or_unsettled_alias_group_retains_all_names(self):
        for settled in (False,True):
            with self.subTest(settled=settled):
                alias=self.paths['build']/('interrupted-'+str(settled))
                instance=self.owner(); launch=instance._launch
                def interrupt(tree,command,env,**kwargs):
                    launch(tree,command,env,**kwargs);tree.cancelled=True
                    raise KeyboardInterrupt('alias interrupted')
                with patch.object(instance,'_launch',side_effect=interrupt),patch.object(PortableTree,'settled',settled):
                    row=self.run_portable(instance,producer=self.producer(mutation='import os\nos.link(p,'+repr(str(alias))+')'))
                self.assertEqual(row['status'],130 if settled else 75)
                self.assertTrue(self.exe.exists());self.assertTrue(alias.exists())
                self.exe.unlink();alias.unlink()

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




def record_sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

class RecordInterfaceFixture(cohort.Cohort):
    def __init__(self, root, count=1):
        self.worktree = root; self.roots = (root/'target', root/'build')
        for p in self.roots: p.mkdir()
        self.source = root/'source'; self.source.write_bytes(b'bound source')
        self.pin = root/'public'; self.pin.write_bytes(b'bound public product')
        self.lease = root/'lease'; self.lease.mkdir()
        self.owner_expected = (record_sha(self.source), record_sha(self.pin), cohort.directory_identity(self.lease))
        self.root_expected = {p:cohort.directory_identity(p) for p in self.roots}
        self.pinned = {self.pin}; self.preexisting = set(); self.retain_preexisting = False
        self.binding = (record_sha(self.source), 'profile declaration')
        self.records = {}; self.groups = {}; self.owner_calls = 0
        self.paths = []; self.aliases = []
        for i in range(count):
            a = self.roots[0]/('p'+str(i)); a.mkdir()
            b = self.roots[1]/('a'+str(i)); b.mkdir()
            p = a/'image'; p.write_bytes(b'exact executable bytes\n'*500); p.chmod(0o700)
            alias = b/'alias'; os.link(p, alias)
            self.paths.append(p); self.aliases.append(alias)
        for p in self.paths:
            parent, parent_id, identity, digest = self._open(p, allow_links=True)
            os.close(parent)
            self.records[p] = (parent_id, identity, digest, self.binding)
            self.groups[p] = self._capture_group(p, identity, digest)
        self.owner_calls = 0
    def _check_owner(self):
        # Explicit interface double. Real filesystem source/pin/lease facts;
        # no PID, subreaper, kernel settlement or native owner qualification.
        self.owner_calls += 1
        if (record_sha(self.source), record_sha(self.pin), cohort.directory_identity(self.lease)) != self.owner_expected:
            raise cohort.Denied('interface source/pin/lease changed')
        if any(cohort.directory_identity(p) != key for p,key in self.root_expected.items()):
            raise cohort.Denied('interface root changed')
    def close_fixture(self):
        for group in self.groups.values():
            os.close(group['fd'])
            for alias in group['aliases']: os.close(alias['parent'])


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux filesystem/API controls; no native lifecycle proof')
class RecordValidationControls(unittest.TestCase):
    def fixture(self, count=1):
        temporary = tempfile.TemporaryDirectory(); self.addCleanup(temporary.cleanup)
        instance = RecordInterfaceFixture(Path(temporary.name), count)
        self.addCleanup(instance.close_fixture)
        return instance

    def exact_record(self, instance, path):
        if instance._validate_record(path) != instance.records[path]:
            raise cohort.Denied('fresh primary record differs')

    def corrupt_alias_parent(self, instance, index=0):
        parent = instance.aliases[index].parent
        old = parent.with_name(parent.name + '-held')
        parent.rename(old); parent.symlink_to(old, target_is_directory=True)

    def test_two_sites_keep_all_32_image_reads_and_reduce_owner_checks_32_to_16(self):
        instance = self.fixture(8); real_read = os.read
        image_ids = {group['identity'][:2] for group in instance.groups.values()}
        reads = []
        def read(fd, size):
            chunk = real_read(fd, size)
            if chunk and (os.fstat(fd).st_dev, os.fstat(fd).st_ino) in image_ids:
                reads.append(fd)
            return chunk
        observed = []
        for baseline in (True, False):
            instance.owner_calls = 0; reads.clear()
            with patch.object(cohort.os, 'read', side_effect=read):
                for site in ('preconsumption', 'preauthorization'):
                    for path in instance.paths:
                        if baseline:
                            instance._validate_group(instance.groups[path])
                            parent, parent_id, identity, digest = instance._open(path, allow_links=True)
                            try:
                                self.assertEqual((parent_id, identity, digest, instance.binding), instance.records[path])
                            finally:
                                os.close(parent)
                        else:
                            self.exact_record(instance, path)
            observed.append((instance.owner_calls, len(reads)))
        self.assertEqual(observed, [(32, 32), (16, 32)])

    def test_fresh_open_refuses_after_observation_source_pin_lease_primary_parent_mutations(self):
        for kind in ('source', 'pin', 'lease', 'primary', 'parent'):
            with self.subTest(kind=kind):
                instance = self.fixture(); path = instance.paths[0]
                original = instance._validate_group_observation
                def observation(group):
                    original(group)
                    if kind in ('source', 'pin'):
                        getattr(instance, kind).write_bytes(b'changed')
                    elif kind == 'lease':
                        instance.lease.rename(instance.worktree / 'held-lease'); instance.lease.mkdir()
                    elif kind == 'primary':
                        path.rename(path.with_name('held-image')); path.write_bytes(b'changed'); path.chmod(0o700)
                    else:
                        path.parent.rename(path.parent.with_name('held-parent'))
                        path.parent.mkdir(); path.write_bytes(b'changed'); path.chmod(0o700)
                with patch.object(instance, '_validate_group_observation', side_effect=observation):
                    with self.assertRaises((cohort.Denied, OSError)):
                        self.exact_record(instance, path)
                self.assertTrue(path.exists())

    def test_observation_refuses_alias_bytes_or_path_mutation_during_read(self):
        for kind in ('bytes', 'alias'):
            with self.subTest(kind=kind):
                instance = self.fixture(); path = instance.paths[0]
                fd = instance.groups[path]['fd']; real_read = os.read; changed = []
                def read(handle, size):
                    chunk = real_read(handle, size)
                    if handle == fd and chunk and not changed:
                        changed.append(True)
                        if kind == 'bytes':
                            path.write_bytes(b'changed bytes')
                        else:
                            alias = instance.aliases[0]
                            alias.rename(alias.with_name('held-alias')); alias.write_bytes(b'replacement')
                    return chunk
                with patch.object(cohort.os, 'read', side_effect=read):
                    with self.assertRaises((cohort.Denied, OSError)):
                        self.exact_record(instance, path)
                self.assertTrue(changed); self.assertTrue(path.is_file())

    def test_other_alias_parent_requires_observation_even_when_primary_open_valid(self):
        instance = self.fixture(); self.corrupt_alias_parent(instance)
        parent, *_ = instance._open(instance.paths[0], allow_links=True)
        os.close(parent)
        with self.assertRaises((cohort.Denied, OSError)):
            self.exact_record(instance, instance.paths[0])


if __name__ == '__main__': unittest.main()
