import importlib.util
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
