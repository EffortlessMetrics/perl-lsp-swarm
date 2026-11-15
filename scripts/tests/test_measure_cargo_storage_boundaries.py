import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'measure_cargo_storage.py'
spec = importlib.util.spec_from_file_location('measurement', SCRIPT)
subject = importlib.util.module_from_spec(spec)
spec.loader.exec_module(subject)


class PendingChild:
    def __init__(self):
        self.returncode = None
        self.signals = []
        self.wait_calls = 0

    def poll(self):
        return self.returncode

    def send_signal(self, signum):
        self.signals.append(signum)
        self.returncode = 130

    def wait(self):
        self.wait_calls += 1
        if self.returncode is None:
            self.returncode = 0
        return self.returncode


class StorageMeasurementReviewBoundaries(unittest.TestCase):
    def test_cancellation_during_launch_is_forwarded_after_child_binding(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            evidence = root / 'receipt'
            child = PendingChild()
            installed = {}

            def install(signum, handler):
                previous = installed.get(signum, signal.SIG_DFL)
                installed[signum] = handler
                return previous

            def launch(_command):
                self.assertIn(signal.SIGINT, installed)
                installed[signal.SIGINT](signal.SIGINT, None)
                return child

            with patch.object(subject.signal, 'signal', side_effect=install), \
                 patch.object(subject.subprocess, 'Popen', side_effect=launch):
                status = subject.main([
                    '--root', str(root / 'target'), '--evidence', str(evidence),
                    '--interval', '10', '--', 'unused',
                ])

            summary = json.loads(evidence.read_text().splitlines()[-1])
            self.assertEqual((status, summary['command_status']), (130, 130))
            self.assertEqual(summary['cancelled_signals'], [signal.SIGINT])
            self.assertEqual(child.signals, [signal.SIGINT])
            self.assertEqual(child.wait_calls, 1)

    def test_evidence_inside_canonical_root_is_rejected_through_aliases(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            measured = root / 'measured'
            measured.mkdir()
            symlink = root / 'measured-link'
            symlink.symlink_to(measured, target_is_directory=True)
            aliases = (measured / '..' / 'measured', symlink)

            for index, alias in enumerate(aliases):
                with self.subTest(alias=alias):
                    evidence = measured / f'receipt-{index}'
                    diagnostics = io.StringIO()
                    with contextlib.redirect_stderr(diagnostics), \
                         self.assertRaises(SystemExit) as raised, \
                         patch.object(
                             subject.subprocess, 'Popen',
                             side_effect=AssertionError('aliased evidence launched a command'),
                         ) as child:
                        subject.main([
                            '--root', str(alias), '--evidence', str(evidence),
                            '--', 'unused',
                        ])
                    self.assertEqual(raised.exception.code, 2)
                    self.assertIn('evidence must be outside observed roots', diagnostics.getvalue())
                    self.assertFalse(evidence.exists())
                    child.assert_not_called()

    def test_external_evidence_remains_accepted_after_canonicalization(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            measured = root / 'measured'
            measured.mkdir()
            evidence = root / 'evidence' / 'receipt'
            evidence.parent.mkdir()
            status = subject.main([
                '--root', str(measured / '..' / 'measured'),
                '--evidence', str(evidence), '--interval', '10',
                '--', sys.executable, '-c', 'pass',
            ])
            self.assertEqual(status, 0)
            header = json.loads(evidence.read_text().splitlines()[0])
            self.assertEqual(header['roots'], [str(measured.resolve())])


if __name__ == '__main__':
    unittest.main()
