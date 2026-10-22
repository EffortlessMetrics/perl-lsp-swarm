import importlib.util
import contextlib
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'measure_cargo_storage.py'
spec = importlib.util.spec_from_file_location('measurement', SCRIPT)
subject = importlib.util.module_from_spec(spec)
spec.loader.exec_module(subject)


class StorageMeasurement(unittest.TestCase):
    def test_unexpected_periodic_failures_signal_real_child_even_if_it_exits_zero(self):
        for error_type in (RuntimeError, MemoryError):
            with self.subTest(error=error_type.__name__), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); ready = root / 'ready'; signalled = root / 'signalled'
                natural = root / 'natural'; evidence = root / 'receipt'
                program = ("from pathlib import Path\nimport signal,time\n"
                           "def stop(*_):\n Path(" + repr(str(signalled)) + ").touch()\n raise SystemExit(0)\n"
                           "signal.signal(signal.SIGINT,stop)\nPath(" + repr(str(ready)) + ").touch()\n"
                           "time.sleep(.4)\nPath(" + repr(str(natural)) + ").touch()\n")
                real_observe = subject.observe
                def observation(roots, event=None):
                    if event is not None:
                        deadline = time.monotonic() + 2
                        while not ready.exists() and time.monotonic() < deadline:
                            time.sleep(.005)
                        if not ready.exists():
                            raise AssertionError('real child did not become ready')
                        raise error_type('injected periodic observer failure')
                    return real_observe(roots, event)
                diagnostics = io.StringIO()
                with patch.object(subject, 'observe', side_effect=observation), contextlib.redirect_stderr(diagnostics):
                    status = subject.main(['--root', str(root / 'target'), '--evidence', str(evidence),
                                           '--interval', '.01', '--', sys.executable, '-c', program])
                summary = json.loads(evidence.read_text().splitlines()[-1])
                self.assertEqual(status, 75)
                self.assertFalse(summary['measurement_complete'])
                self.assertEqual(summary['command_status'], 0)
                self.assertEqual(summary['cancelled_signals'], [])
                self.assertTrue(signalled.exists())
                self.assertFalse(natural.exists())
                self.assertIn('injected periodic observer failure', diagnostics.getvalue())
                self.assertEqual(summary['worker_failure']['error_type'], error_type.__name__)
                self.assertEqual(summary['worker_failure']['phase'], 'observation')

    def test_worker_failure_after_natural_child_completion_is_still_incomplete(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); finished = root / 'finished'; evidence = root / 'receipt'
            program = 'from pathlib import Path; import time; time.sleep(.1); Path(' + repr(str(finished)) + ').touch()'
            real_observe = subject.observe; real_popen = subject.subprocess.Popen; children = []
            def launch(*args, **kwargs):
                child = real_popen(*args, **kwargs); children.append(child); return child
            def observation(roots, event=None):
                if event is not None:
                    deadline = time.monotonic() + 2
                    while (not finished.exists() or children[0].poll() is None) and time.monotonic() < deadline:
                        time.sleep(.005)
                    if not finished.exists() or children[0].returncode != 0:
                        raise AssertionError('real child did not finish naturally')
                    raise RuntimeError('failure concurrent with successful child completion')
                return real_observe(roots, event)
            with patch.object(subject, 'observe', side_effect=observation), \
                 patch.object(subject.subprocess, 'Popen', side_effect=launch), contextlib.redirect_stderr(io.StringIO()):
                status = subject.main(['--root', str(root / 'target'), '--evidence', str(evidence),
                                       '--interval', '.01', '--', sys.executable, '-c', program])
            summary = json.loads(evidence.read_text().splitlines()[-1])
            self.assertEqual((status, summary['command_status']), (75, 0))
            self.assertFalse(summary['measurement_complete'])
            self.assertTrue(finished.exists())

    def test_short_healthy_command_needs_no_periodic_sample(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); evidence = root / 'receipt'
            status = subject.main(['--root', str(root / 'target'), '--evidence', str(evidence),
                                   '--interval', '10', '--', sys.executable, '-c', 'pass'])
            samples = [json.loads(line) for line in evidence.read_text().splitlines()]
            self.assertEqual(status, 0)
            self.assertTrue(samples[-1]['measurement_complete'])
            self.assertEqual([r['kind'] for r in samples], ['header', 'initial', 'retained', 'summary'])

    def test_periodic_persistence_failure_stops_real_child_and_remains_incomplete(self):
        for error_type in (OSError, RuntimeError, MemoryError):
            with self.subTest(error=error_type.__name__), tempfile.TemporaryDirectory() as directory:
                root = Path(directory); ready = root / 'ready'; signalled = root / 'signalled'
                evidence = root / 'receipt'
                program = ("from pathlib import Path\nimport signal,time\n"
                           "def stop(*_):\n Path(" + repr(str(signalled)) + ").touch()\n raise SystemExit(0)\n"
                           "signal.signal(signal.SIGINT,stop)\nPath(" + repr(str(ready)) + ").touch()\ntime.sleep(.4)\n")
                real_fsync = subject.os.fsync; calls = 0
                def fsync(fd):
                    nonlocal calls
                    calls += 1
                    if calls == 3:  # Header + initial durable writes precede periodic write.
                        deadline = time.monotonic() + 2
                        while not ready.exists() and time.monotonic() < deadline:
                            time.sleep(.005)
                        if not ready.exists():
                            raise AssertionError('real child did not become ready')
                        raise error_type('injected periodic persistence failure')
                    return real_fsync(fd)
                with patch.object(subject.os, 'fsync', side_effect=fsync), contextlib.redirect_stderr(io.StringIO()):
                    status = subject.main(['--root', str(root / 'target'), '--evidence', str(evidence),
                                           '--interval', '.01', '--', sys.executable, '-c', program])
                summary = json.loads(evidence.read_text().splitlines()[-1])
                self.assertEqual((status, summary['command_status']), (75, 0))
                self.assertFalse(summary['measurement_complete'])
                self.assertTrue(signalled.exists())
                self.assertEqual(summary['worker_failure']['phase'], 'persistence')
                self.assertEqual(summary['worker_failure']['error_type'], error_type.__name__)

    def test_healthy_periodic_measurement_stays_complete(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); evidence = root / 'receipt'
            status = subject.main(['--root', str(root / 'target'), '--evidence', str(evidence),
                                   '--interval', '.01', '--', sys.executable, '-c', 'import time; time.sleep(.08)'])
            persisted = [json.loads(line) for line in evidence.read_text().splitlines()]
            self.assertEqual(status, 0)
            self.assertTrue(any(row['kind'] == 'periodic' for row in persisted))
            self.assertTrue(persisted[-1]['measurement_complete'])
            self.assertIsNone(persisted[-1]['worker_failure'])

    def test_periodic_reserve_loss_still_stops_real_child(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); ready = root / 'ready'; signalled = root / 'signalled'; evidence = root / 'receipt'
            program = ("from pathlib import Path\nimport signal,time\n"
                       "def stop(*_):\n Path(" + repr(str(signalled)) + ").touch()\n raise SystemExit(0)\n"
                       "signal.signal(signal.SIGINT,stop)\nPath(" + repr(str(ready)) + ").touch()\ntime.sleep(.4)\n")
            def observe(_roots, event=None):
                if event is not None:
                    deadline = time.monotonic() + 2
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(.005)
                    if not ready.exists():
                        raise AssertionError('real child did not become ready')
                return {'allocated_bytes': 0, 'filesystems': {'1': {'free_bytes': 9 if event is not None else 100}}}
            with patch.object(subject, 'observe', side_effect=observe):
                status = subject.main(['--root', str(root / 'target'), '--evidence', str(evidence),
                                       '--min-free-bytes', '10', '--interval', '.01', '--', sys.executable, '-c', program])
            summary = json.loads(evidence.read_text().splitlines()[-1])
            self.assertEqual((status, summary['command_status']), (75, 0))
            self.assertTrue(signalled.exists())
            self.assertFalse(summary['observed_reserve_preserved'])
            self.assertTrue(summary['measurement_complete'])

    def test_invalid_periodic_sample_signals_the_running_command(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ready, signalled = root / 'ready', root / 'signalled'
            program = ('from pathlib import Path; import signal,time,sys; '
                       'signal.signal(signal.SIGINT, lambda *_: (Path(' + repr(str(signalled)) + ').touch(), sys.exit(130))); '
                       'Path(' + repr(str(ready)) + ').touch(); time.sleep(.5)')
            good = {'allocated_bytes': 0, 'filesystems': {'1': {'free_bytes': 10000}}, 'scan_seconds': 0}
            calls = 0
            def observation(*_args):
                nonlocal calls
                calls += 1
                if calls == 2:
                    deadline = time.monotonic() + 3
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(.01)
                    self.assertTrue(ready.exists())
                    raise subject.MeasurementError('unreadable allocation')
                return good
            with patch.object(subject, 'observe', side_effect=observation):
                self.assertEqual(subject.main(['--root', str(root / 'target'), '--evidence', str(root / 'receipt'),
                                               '--interval', '.01', '--', sys.executable, '-c', program]), 75)
            self.assertTrue(signalled.exists())
            summary = json.loads((root / 'receipt').read_text().splitlines()[-1])
            self.assertEqual(summary['command_status'], 130)
            self.assertFalse(summary['measurement_complete'])

    def test_invalid_observation_cannot_certify_success(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            good = {'allocated_bytes': 4096, 'filesystems': {'1': {'free_bytes': 10000}}, 'scan_seconds': 0}
            calls = iter([good, subject.MeasurementError('partial scan')])
            def observations(*_args):
                row = next(calls, good)
                if isinstance(row, Exception):
                    raise row
                return row
            with patch.object(subject, 'observe', side_effect=observations):
                result = subject.main(['--root', str(root / 'target'), '--evidence', str(root / 'receipt'),
                                       '--interval', '.01', '--', sys.executable, '-c', 'import time; time.sleep(.1)'])
            self.assertEqual(result, 75)
            rows = [json.loads(line) for line in (root / 'receipt').read_text().splitlines()]
            self.assertFalse(rows[-1]['measurement_complete'])
            self.assertEqual(rows[2]['error'], 'partial scan')

    def test_reserve_refusal_starts_no_command(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(subject, 'observe', return_value={'allocated_bytes': 0, 'filesystems': {'1': {'free_bytes': 9}}}), \
                 patch.object(subject.subprocess, 'Popen') as child:
                self.assertEqual(subject.main(['--root', str(root / 'target'), '--evidence', str(root / 'receipt'),
                                               '--min-free-bytes', '10', '--', 'unused']), 75)
                child.assert_not_called()

    def test_cli_interruption_forwards_to_child_and_preserves_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            marker = root / 'ready'
            program = 'from pathlib import Path; import signal,time; signal.signal(signal.SIGINT, lambda *_: exit(130)); Path(' + repr(str(marker)) + ').touch(); time.sleep(10)'
            child = subprocess.Popen([sys.executable, str(SCRIPT), '--root', str(root / 'target'),
                '--evidence', str(root / 'receipt'), '--interval', '.01', '--', sys.executable, '-c', program])
            try:
                deadline = time.monotonic() + 3
                while not marker.exists() and time.monotonic() < deadline:
                    time.sleep(.01)
                self.assertTrue(marker.exists())
                child.send_signal(signal.SIGINT)
                self.assertEqual(child.wait(timeout=3), 130)
                summary = json.loads((root / 'receipt').read_text().splitlines()[-1])
                self.assertEqual(summary['command_status'], 130)
                self.assertEqual(summary['cancelled_signals'], [signal.SIGINT])
            finally:
                if child.poll() is None:
                    child.kill(); child.wait()

    def test_multiple_roots_deduplicate_hardlinks_and_overlap(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            a, b = root / 'a', root / 'b'
            a.mkdir(); b.mkdir()
            file = a / 'with\nnewline'
            file.write_bytes(b'x' * 65536)
            os.link(file, b / 'alias')
            expected = sum(p.stat().st_blocks * 512 for p in (a, b, file))
            self.assertEqual(subject.allocation([a, b]), expected)
            self.assertEqual(subject.allocation([a, a, b]), expected)
            diagnostics = {}
            subject.allocation([a, b], diagnostics=diagnostics)
            self.assertEqual([row['allocated_bytes'] for row in diagnostics['allocated_by_root']],
                             [(a.stat().st_blocks + file.stat().st_blocks) * 512,
                              b.stat().st_blocks * 512])
            # Independent existing storage-doctor accounting oracle.
            raw = subprocess.check_output(['du', '-s', '-B1', '-c', '--', str(a), str(b)],
                                          env={**os.environ, 'LC_ALL': 'C'})
            self.assertEqual(subject.allocation([a, b]), int(raw.splitlines()[-1].split(b'\t')[0]))

    def test_external_interrupt_does_not_resend_for_interrupted_scan(self):
        # One real user signal interrupts a mocked in-flight scan. The child
        # acknowledges it, then remains alive during orderly shutdown so a
        # duplicate signal is observable rather than hidden by fast exit.
        with tempfile.TemporaryDirectory() as name:
            root = Path(name); ready = root / 'ready'; signals = root / 'signals'
            entered = threading.Event(); calls = 0
            program = ("from pathlib import Path\nimport signal,time\n"
                       "def stop(*_):\n with Path(" + repr(str(signals)) + ").open('a') as stream:stream.write('INT\\n')\n"
                       " time.sleep(.15)\n raise SystemExit(130)\n"
                       "signal.signal(signal.SIGINT,stop)\nPath(" + repr(str(ready)) + ").touch()\ntime.sleep(1)\n")
            good = {'allocated_bytes': 0, 'filesystems': {'1': {'free_bytes': 10000}}, 'scan_seconds': 0}
            def observe(_roots, event=None):
                nonlocal calls
                calls += 1
                if calls == 2:
                    deadline = time.monotonic() + 2
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(.005)
                    self.assertTrue(ready.exists()); entered.set()
                    self.assertTrue(event.wait(2))
                    while not signals.exists() and time.monotonic() < deadline:
                        time.sleep(.005)
                    self.assertTrue(signals.exists())
                    raise subject.MeasurementError('scan interrupted by external cancellation')
                return good
            def cancel_once():
                if entered.wait(2):
                    os.kill(os.getpid(), signal.SIGINT)
            actor = threading.Thread(target=cancel_once); actor.start()
            try:
                with patch.object(subject, 'observe', side_effect=observe):
                    status = subject.main(['--root', str(root / 'target'), '--evidence', str(root / 'receipt'),
                                           '--interval', '.01', '--', sys.executable, '-c', program])
            finally:
                actor.join(timeout=2)
            self.assertFalse(actor.is_alive())
            summary = json.loads((root / 'receipt').read_text().splitlines()[-1])
            self.assertEqual(signals.read_text().splitlines(), ['INT'])
            self.assertEqual((status, summary['command_status']), (130, 130))
            self.assertEqual(summary['cancelled_signals'], [signal.SIGINT])
            self.assertFalse(summary['measurement_complete'])

    def test_missing_roots_and_filesystem_observations(self):
        with tempfile.TemporaryDirectory() as directory:
            row = subject.observe([Path(directory) / 'missing'])
            self.assertEqual(row['allocated_bytes'], 0)
            self.assertGreater(next(iter(row['filesystems'].values()))['free_bytes'], 0)

    def test_symlink_does_not_adopt_external_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); measured = root / 'measured'; measured.mkdir()
            external = root / 'external'; external.mkdir()
            (external / 'payload').write_bytes(b'x' * 65536)
            link = measured / 'link'; link.symlink_to(external, target_is_directory=True)
            self.assertEqual(subject.allocation([measured]),
                             (measured.stat().st_blocks + link.lstat().st_blocks) * 512)

    def test_unreadable_or_partial_tree_is_unknown(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(subject.os, 'scandir', side_effect=PermissionError('unreadable tree')):
                with self.assertRaises(PermissionError):
                    subject.allocation([Path(directory)])

    def test_interrupted_scan_is_invalid(self):
        stop = threading.Event(); stop.set()
        with self.assertRaisesRegex(subject.MeasurementError, 'interrupted'):
            subject.allocation([Path('/unused')], stop)

    def test_vanishing_entries_are_reported_sampling_limit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            diagnostics = {}
            with patch.object(subject.os, 'scandir', side_effect=FileNotFoundError('concurrent removal')):
                count = subject.allocation([root], diagnostics=diagnostics)
            self.assertEqual(count, root.stat().st_blocks * 512)
            self.assertEqual(diagnostics['vanished_entries'], 1)

    def test_command_status_retention_and_exclusive_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); observed = root / 'target'; observed.mkdir()
            evidence = root / 'evidence.jsonl'
            command = [sys.executable, str(SCRIPT), '--root', str(observed), '--evidence', str(evidence),
                       '--interval', '.01', '--', sys.executable, '-c', 'raise SystemExit(7)']
            self.assertEqual(subprocess.run(command).returncode, 7)
            rows = [json.loads(line) for line in evidence.read_text().splitlines()]
            self.assertEqual(rows[-1]['command_status'], 7)
            self.assertTrue(rows[-1]['measurement_complete'])
            self.assertFalse(rows[-1]['peak_is_bound'])
            original = evidence.read_bytes()
            self.assertNotEqual(subprocess.run(command, stderr=subprocess.DEVNULL).returncode, 0)
            self.assertEqual(evidence.read_bytes(), original)


if __name__ == '__main__':
    unittest.main()
