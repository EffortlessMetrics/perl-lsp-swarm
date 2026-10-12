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
    def test_invalid_observation_cannot_certify_success(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            good = {'allocated_bytes': 4096, 'filesystems': {'1': {'free_bytes': 10000}}, 'scan_seconds': 0}
            with patch.object(subject, 'observe', side_effect=[good, subject.MeasurementError('partial scan'), good]):
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

    def test_missing_roots_and_filesystem_observations(self):
        with tempfile.TemporaryDirectory() as directory:
            row = subject.observe([Path(directory) / 'missing'])
            self.assertEqual(row['allocated_bytes'], 0)
            self.assertGreater(next(iter(row['filesystems'].values()))['free_bytes'], 0)

    def test_failed_and_partial_output_is_unknown(self):
        with tempfile.TemporaryDirectory() as directory:
            for code, stdout in ((1, b'12\tpartial\n'), (0, b'12\twrong\n')):
                with patch.object(subject.subprocess, 'Popen') as constructor:
                    constructor.return_value.returncode = code
                    constructor.return_value.communicate.return_value = (stdout, b'error')
                    with self.assertRaises(subject.MeasurementError):
                        subject.allocation([Path(directory)])

    def test_interrupted_scan_is_invalid_and_sampler_reaped(self):
        with tempfile.TemporaryDirectory() as directory:
            stop = threading.Event(); stop.set()
            with patch.object(subject.subprocess, 'Popen') as constructor:
                process = constructor.return_value
                process.communicate.side_effect = [subprocess.TimeoutExpired('du', .1), (b'', b'')]
                with self.assertRaisesRegex(subject.MeasurementError, 'interrupted'):
                    subject.allocation([Path(directory)], stop)
                process.terminate.assert_called_once()
                self.assertEqual(process.communicate.call_count, 2)

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
