"""Cross-process Linux admission proof, independent of mocked storage fixtures.

Finite fake Cargo writes <=48 KiB. The opt-in, dependency-free real Cargo run
declares 128 MiB growth and 256 MiB reserve for this fixture only; these are
conservative declarations, not general workload sizing or an OS quota.
"""
import ctypes
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

OWNER = Path(os.environ.get("CARGO_ADMITTED_PARALLEL_OWNER",
                           Path(__file__).resolve().parents[1] / "cargo_admitted.py"))
PAYLOAD = '''import os,signal,time
from pathlib import Path
r=Path(os.environ['FIXTURE_ROOT'])
for key in ('CARGO_TARGET_DIR','CARGO_BUILD_BUILD_DIR','TMPDIR'):
 (Path(os.environ[key])/'bounded-payload').write_bytes(b'x'*16384)
with (r/'launches').open('a') as f:f.write(str(os.getpid())+'\\n')
if os.fork()==0:
 os.setsid();signal.signal(signal.SIGTERM,signal.SIG_IGN)
 for fd in (0,1,2):os.close(fd)
 (r/'helper').write_text(str(os.getpid()))
 deadline=time.monotonic()+15
 while not (r/'helper-exit').exists() and time.monotonic()<deadline:time.sleep(.01)
 (r/'helper-lease').write_text(str(Path(os.environ['FIXTURE_LEASE']).is_dir()))
 os._exit(0)
deadline=time.monotonic()+15
while not (r/'leader-exit').exists() and time.monotonic()<deadline:time.sleep(.01)
os._exit(0)
'''


@unittest.skipUnless(sys.platform == 'linux', 'Linux pidfd/subreaper proof')
class ParallelAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.libc = ctypes.CDLL(None, use_errno=True)
        prior = ctypes.c_int()
        self.assertEqual(self.libc.prctl(37, ctypes.byref(prior), 0, 0, 0), 0)
        self.prior_subreaper = prior.value
        if prior.value:
            self.skipTest('independent outer cleanup requires an unclaimed subreaper')
        try:
            os.waitid(os.P_ALL, 0, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        except ChildProcessError:
            pass
        else:
            self.skipTest('independent cleanup cannot adopt existing unrelated children')
        self.assertEqual(self.libc.prctl(36, 1, 0, 0, 0), 0)
        self.addCleanup(self.libc.prctl, 36, self.prior_subreaper, 0, 0, 0)
        self.temp = tempfile.TemporaryDirectory(prefix='admission-parallel-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / 'repo'
        self.repo.mkdir()
        subprocess.run(['git', 'init', '-q', str(self.repo)], check=True)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        cargo = self.bin / 'cargo'
        cargo.write_text('#!' + sys.executable + '\n' + PAYLOAD)
        cargo.chmod(0o700)
        self.env = {'PATH': str(self.bin) + os.pathsep + os.defpath,
                    'DEVPLANE': str(self.root / 'devplane'),
                    'CARGO_HOME': str(self.root / 'cargo-home'),
                    'TMPDIR': str(self.root / 'tmp'),
                    'FIXTURE_ROOT': str(self.root), 'RUSTUP_AUTO_INSTALL': '0'}
        self.processes = []
        self.streams = []
        self.peak_file_bytes = self.peak_allocated_bytes = 0

    def tearDown(self):
        # This outer owner is independent of production settlement. Even a
        # broken candidate is contained: reap our leaders, then only children
        # waitable by this test process; never signal inferred process groups.
        try:
            for p in self.processes:
                if p.poll() is None:
                    p.kill()
                p.wait(timeout=3)
            deadline = time.monotonic() + 3
            while True:
                try:
                    pid, _ = os.waitpid(-1, os.WNOHANG)
                except ChildProcessError:
                    break
                if pid:
                    continue
                self.assertLess(time.monotonic(), deadline, 'fixture cleanup timed out')
                children = []
                for entry in Path('/proc').iterdir():
                    if entry.name.isdecimal():
                        try:
                            fields = (entry / 'stat').read_text().rsplit(')', 1)[1].split()
                            if int(fields[1]) == os.getpid():
                                children.append(entry.name)
                        except (FileNotFoundError, ProcessLookupError):
                            pass
                for child in children:
                    try:
                        fd = os.pidfd_open(int(child))
                        try:
                            os.waitid(os.P_PIDFD, fd, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                            signal.pidfd_send_signal(fd, signal.SIGKILL)
                        finally:
                            os.close(fd)
                    except (ProcessLookupError, ChildProcessError):
                        pass
                time.sleep(.01)
        finally:
            for stream in self.streams:
                stream.close()

    def wait_for(self, predicate, seconds=6):
        deadline = time.monotonic() + seconds
        while not predicate():
            self.sample_storage()
            self.assertLess(time.monotonic(), deadline, 'fixture did not reach observation')
            time.sleep(.01)

    def sample_storage(self):
        sizes = []
        for path in self.root.rglob('*'):
            try:
                if path.is_file():
                    sizes.append(path.stat())
            except FileNotFoundError:
                pass  # Compiler temporary files may disappear between samples.
        logical = sum(s.st_size for s in sizes)
        allocated = sum(s.st_blocks * 512 for s in sizes)
        self.peak_file_bytes = max(self.peak_file_bytes, logical)
        self.peak_allocated_bytes = max(self.peak_allocated_bytes, allocated)
        return logical, allocated

    def prepare(self, discovery='proc-children', real=False, repo=None):
        self.cwd = self.repo if repo is None else repo
        self.env['CARGO_ADMITTED_CHILD_DISCOVERY'] = discovery
        args = ['run', '--offline', '--quiet'] if real else ['test', '--offline']
        preflight = subprocess.run([sys.executable, str(OWNER), '--preflight', *args],
                                   env=self.env, cwd=self.cwd, capture_output=True, text=True, timeout=5)
        # Preflight may refuse this host's real free space under the preserved
        # 40-GiB default. Its scope still binds the explicit fixture budget.
        rows = [json.loads(line.split(': ', 1)[1]) for line in preflight.stderr.splitlines()
                if line.startswith('cargo-admitted preflight: ')]
        self.assertEqual(len(rows), 1, preflight.stderr)
        scope = rows[0]['scope']
        self.assertIsNotNone(scope, preflight.stderr)
        target = Path(scope['resources']['target'])
        self.target = target
        self.lease = target.parents[2] / 'cargo-active'
        self.env['FIXTURE_LEASE'] = str(self.lease)
        self.budget = self.root / 'budget.json'
        self.budget.write_text(json.dumps({'schema_version': 1, 'scope': scope,
            'reserve_bytes': 256 * 1024**2, 'expected_growth_bytes': (128 if real else 8) * 1024**2,
            'basis': 'finite dependency-free parallel admission fixture; declared bound, not universal sizing'}))
        self.args = ['--budget-file', str(self.budget), *args]

    def start(self):
        log = self.root / ('owner-' + str(len(self.processes)) + '.log')
        stream = log.open('w')
        self.streams.append(stream)
        p = subprocess.Popen([sys.executable, str(OWNER), *self.args], env=self.env,
                             cwd=self.cwd, stdout=stream, stderr=stream)
        self.processes.append(p)
        p.fixture_log = log
        return p

    def rows(self, p, kind):
        prefix = 'cargo-admitted Cargo ' + kind + ': '
        return [json.loads(line[len(prefix):]) for line in p.fixture_log.read_text().splitlines()
                if line.startswith(prefix)]

    def assert_settled(self, p, status):
        self.assertEqual(p.wait(timeout=8), status, p.fixture_log.read_text())
        row, = self.rows(p, 'settlement')
        self.assertTrue(row['tree_settled'], row)
        self.assertTrue(row['lease_released'], row)
        self.assertEqual(row['proof'], 'kernel ECHILD (__WALL)')
        self.assertFalse(self.lease.exists())
        self.assertFalse(Path('/proc/' + (self.root / 'helper').read_text()).exists())
        resources = next(json.loads(line.split(': ', 1)[1]) for line in p.fixture_log.read_text().splitlines()
                         if line.startswith('cargo-admitted resources: '))
        for key in ('lease_identity', 'lease_marker', 'marker_identity'):
            self.assertEqual(row[key], resources[key])
        self.assertFalse(Path(resources['lease_marker']).exists())
        return row

    def test_parallel_contenders_refuse_through_detached_settlement_and_reuse(self):
        self.prepare()
        contenders = [self.start() for _ in range(6)]
        self.wait_for(lambda: sum(p.poll() is not None for p in contenders) == 5)
        winner, = [p for p in contenders if p.poll() is None]
        self.wait_for(lambda: (self.root / 'helper').exists())
        self.assertEqual(len((self.root / 'launches').read_text().splitlines()), 1)
        marker, = self.lease.iterdir()
        original = (self.lease.stat().st_ino, marker.stat().st_ino)
        for p in contenders:
            if p is not winner:
                self.assertEqual(p.returncode, 75, p.fixture_log.read_text())
                self.assertIn('slot is active', p.fixture_log.read_text())
        (self.root / 'leader-exit').touch()
        self.wait_for(lambda: bool(self.rows(winner, 'product')))
        retry = self.start()
        self.assertEqual(retry.wait(timeout=5), 75, retry.fixture_log.read_text())
        self.assertEqual((self.lease.stat().st_ino, marker.stat().st_ino), original)
        (self.root / 'helper-exit').touch()
        self.assert_settled(winner, 0)
        self.assertEqual((self.root / 'helper-lease').read_text(), 'True')
        again = self.start()
        self.assert_settled(again, 0)
        self.assertEqual(len((self.root / 'launches').read_text().splitlines()), 2)

    def test_linked_worktree_contender_shares_lease_but_has_private_outputs(self):
        self.prepare()
        original_target, original_lease = self.target, self.lease
        owner = self.start()
        self.wait_for(lambda: (self.root / 'helper').exists())
        subprocess.run(['git', '-C', str(self.repo), '-c', 'user.name=Fixture',
                        '-c', 'user.email=fixture@example.invalid', 'commit',
                        '--allow-empty', '-qm', 'fixture'], check=True)
        sibling = self.root / 'sibling'
        subprocess.run(['git', '-C', str(self.repo), 'worktree', 'add', '-qb',
                        'sibling', str(sibling)], check=True)
        self.prepare(repo=sibling)
        self.assertEqual(self.lease, original_lease)
        self.assertNotEqual(self.target, original_target)
        contender = self.start()
        self.assertEqual(contender.wait(timeout=5), 75, contender.fixture_log.read_text())
        self.assertIn('slot is active', contender.fixture_log.read_text())
        self.assertFalse(self.target.exists(), 'refused sibling must not allocate outputs')
        (self.root / 'leader-exit').touch()
        (self.root / 'helper-exit').touch()
        self.assert_settled(owner, 0)

    def cancellation(self, discovery='proc-children', real=False):
        self.prepare(discovery, real=real)
        sentinel = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(20)'])
        self.processes.append(sentinel)
        owner = self.start()
        self.wait_for(lambda: (self.root / 'helper').exists(), seconds=20 if real else 6)
        retry = self.start()
        self.assertEqual(retry.wait(timeout=5), 75, retry.fixture_log.read_text())
        self.assertIn('slot is active', retry.fixture_log.read_text())
        start = time.monotonic()
        owner.send_signal(signal.SIGINT)
        time.sleep(.1)
        owner.send_signal(signal.SIGTERM)
        self.wait_for(lambda: owner.poll() is not None, seconds=8)
        final = self.assert_settled(owner, 130)
        self.assertTrue(final['cancelled'])
        self.assertTrue(final['termination_requested'])
        self.assertIsNone(sentinel.poll(), 'unrelated caller child must survive')
        logical, allocated = self.sample_storage()
        measurement = {'discovery': discovery, 'real_cargo': real,
                       'elapsed_cancel_seconds': round(time.monotonic() - start, 3),
                       'retained_file_bytes': logical, 'retained_allocated_bytes': allocated,
                       'sampled_peak_file_bytes': self.peak_file_bytes,
                       'sampled_peak_allocated_bytes': self.peak_allocated_bytes}
        print('parallel admission measurement: ' + json.dumps(measurement), flush=True)
        self.assertLess(self.peak_allocated_bytes, (128 if real else 8) * 1024**2)

    def test_repeated_cancellation_settles_term_ignoring_detached_child(self):
        if not Path(f'/proc/self/task/{os.getpid()}/children').exists():
            self.skipTest('default discovery needs proc children; alternate profile tested separately')
        self.cancellation()

    def test_alternate_discovery_cancellation_preserves_unrelated_child(self):
        self.cancellation('proc-parent-pidfd')

    @unittest.skipUnless(os.environ.get('CARGO_ADMITTED_SMALL_REAL_TOOLCHAIN'),
                         'opt-in installed toolchain for dependency-free offline build')
    def test_real_cargo_small_parallel_refusal_and_cancel(self):
        toolchain = Path(os.environ['CARGO_ADMITTED_SMALL_REAL_TOOLCHAIN']).resolve()
        self.assertTrue((toolchain / 'cargo').is_file())
        self.env['PATH'] = str(toolchain) + os.pathsep + os.defpath
        self.env['FIXTURE_PYTHON'] = sys.executable
        payload = self.root / 'payload.py'
        payload.write_text(PAYLOAD)
        self.env['FIXTURE_PAYLOAD'] = str(payload)
        (self.repo / 'Cargo.toml').write_text('[package]\nname="small-admission"\nversion="0.0.0"\nedition="2021"\n[[bin]]\nname="small-admission"\npath="main.rs"\n')
        (self.repo / 'main.rs').write_text('fn main() { let status = std::process::Command::new(std::env::var("FIXTURE_PYTHON").unwrap()).arg(std::env::var("FIXTURE_PAYLOAD").unwrap()).status().unwrap(); std::process::exit(status.code().unwrap_or(1)); }\n')
        self.cancellation('proc-parent-pidfd', real=True)


if __name__ == '__main__':
    unittest.main()
