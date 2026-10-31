"""Instrument controls only: synthetic observations are never oracle receipts."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('signature_conformance', Path(__file__).with_name('signature_conformance.py'))
oracle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(oracle)

class SignatureConformanceTests(unittest.TestCase):
    def setUp(self):
        self.matrix, self.digest = oracle.load_matrix()

    def fixture(self, name):
        return next(row for row in self.matrix['cases'] if row['id'] == name)

    def completed(self, stdout='', stderr='', code=0):
        return {'status': 'completed', 'exit': code, 'stdout': stdout, 'stderr': stderr}

    def test_native_terminal_report_cannot_be_ordinary_rejection(self):
        rows = [{'id': case['id'], 'source': case['source'], 'expected': case['native'], 'observed': {'accepted': case['native']['accepted'], 'stop_cause': None}, 'status': 'OBSERVED', 'failures': []} for case in self.matrix['cases']]
        report = {'schema': 'signature-native-report/v1', 'matrix': self.matrix, 'rows': rows, 'status': 'PASS', 'mismatch_count': 0}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'native.json'
            path.write_text(json.dumps(report))
            self.assertEqual(0, oracle.check_native_report(path, 0))
            rows[0]['observed']['accepted'] = not rows[0]['expected']['accepted']
            path.write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, 'unreported native disposition mismatch'):
                oracle.check_native_report(path, 0)
            rows[0]['observed']['accepted'] = rows[0]['expected']['accepted']
            observation = rows[0].pop('observed')
            path.write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, 'missing native observation'):
                oracle.check_native_report(path, 0)
            rows[0]['observed'] = observation
            rows[0].update(status='NOT_PROVEN', failures=['terminal'])
            rows[0]['observed'].update(accepted=None, stop_cause='Cancelled')
            report.update(status='NOT_PROVEN', mismatch_count=1)
            path.write_text(json.dumps(report))
            self.assertEqual(2, oracle.check_native_report(path, 101))
            rows[0].update(status='OBSERVED', failures=[])
            report.update(status='PASS', mismatch_count=0)
            path.write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, 'invalid native disposition'):
                oracle.check_native_report(path, 0)

    def test_text_subject_identity_normalizes_only_line_endings(self):
        matrix_lf = oracle.MATRIX.read_bytes().replace(b'\r\n', b'\n')
        runner_lf = Path(oracle.__file__).read_bytes().replace(b'\r\n', b'\n')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'cases.json'
            for data in (matrix_lf, matrix_lf.replace(b'\n', b'\r\n')):
                path.write_bytes(data)
                self.assertEqual(self.digest, oracle.load_matrix(path)[1])
            path.write_bytes(matrix_lf + b' ')
            self.assertNotEqual(self.digest, oracle.load_matrix(path)[1])
        self.assertEqual(oracle.text_digest(runner_lf), oracle.text_digest(runner_lf.replace(b'\n', b'\r\n')))
        self.assertNotEqual(oracle.text_digest(runner_lf), oracle.text_digest(runner_lf + b'# changed'))
        self.assertNotEqual(oracle.digest(b'binary\r\n'), oracle.digest(b'binary\n'))
        receipt = {'schema': oracle.SCHEMA, 'matrix_sha256': oracle.text_digest(matrix_lf), 'runner_sha256': oracle.text_digest(runner_lf), 'version': '5.36.0', 'identity': self.completed('5.36.0'), 'executable': '/test-only/perl', 'executable_sha256': '0'*64, 'rows': []}
        with self.assertRaisesRegex(ValueError, 'missing or duplicate'):
            oracle.validate_receipts([receipt])
        for key, changed in [('matrix_sha256', matrix_lf + b' '), ('runner_sha256', runner_lf + b'# changed')]:
            bad = copy.deepcopy(receipt); bad[key] = oracle.text_digest(changed)
            with self.assertRaisesRegex(ValueError, 'stale'):
                oracle.validate_receipts([bad])
        receipt['schema'] = 'signature-oracle-receipt/v1'
        with self.assertRaisesRegex(ValueError, 'stale'):
            oracle.validate_receipts([receipt])

    def test_compile_and_runtime_discriminate(self):
        row = self.fixture('default_assign')
        self.assertEqual([], oracle.assess(row, '5.36.0', self.completed(), self.completed('1|undef|0|2')))
        self.assertTrue(oracle.assess(row, '5.36.0', self.completed(code=1), None))
        self.assertTrue(oracle.assess(row, '5.36.0', self.completed(), self.completed('1|1|0|2')))
        bad = self.completed(); bad['status'] = 'timeout'
        self.assertTrue(oracle.assess(row, '5.36.0', bad, None))

    def test_version_boundary_and_warning_are_independent(self):
        row = self.fixture('default_defined')
        self.assertEqual([], oracle.assess(row, '5.36.0', self.completed(code=1), None))
        self.assertTrue(oracle.assess(row, '5.38.0', self.completed(code=1), None))
        old = self.fixture('anonymous_default')
        self.assertTrue(oracle.assess(old, '5.32.1', self.completed(), None))
        self.assertEqual([], oracle.assess(old, '5.32.1', self.completed(stderr='The signatures feature is experimental'), None))
        self.assertTrue(oracle.assess(old, '5.36.0', self.completed(stderr='The signatures feature is experimental'), None))

    def test_matrix_rejects_wrong_geometry_and_duplicates(self):
        for mutation in ('geometry', 'duplicate', 'schema'):
            matrix = copy.deepcopy(self.matrix)
            if mutation == 'geometry': matrix['cases'][0]['native']['header_span'] = [0, 1]
            if mutation == 'duplicate': matrix['cases'].append(matrix['cases'][0])
            if mutation == 'schema': matrix['schema'] = 'old'
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'cases.json'; path.write_text(json.dumps(matrix), encoding='utf-8')
                with self.assertRaises(ValueError): oracle.load_matrix(path)

    def test_receipts_reject_missing_stale_and_version_mismatch(self):
        with self.assertRaisesRegex(ValueError, 'missing versions'): oracle.validate_receipts([])
        receipt = {'schema': oracle.SCHEMA, 'matrix_sha256': self.digest, 'runner_sha256': oracle.text_digest(Path(oracle.__file__).read_bytes()), 'version': '5.36.0', 'identity': self.completed('5.36.0'), 'executable': '/test-only/perl', 'executable_sha256': '0'*64, 'rows': []}
        with self.assertRaisesRegex(ValueError, 'missing or duplicate'): oracle.validate_receipts([receipt])
        bad = copy.deepcopy(receipt); bad['matrix_sha256'] = 'old'
        with self.assertRaisesRegex(ValueError, 'stale'): oracle.validate_receipts([bad])
        bad = copy.deepcopy(receipt); bad['identity']['stdout'] = '5.32.1'
        with self.assertRaisesRegex(ValueError, 'version identity'): oracle.validate_receipts([bad])

    def test_named_and_args_warning_patterns(self):
        row = self.fixture('named_pair')
        self.assertTrue(oracle.assess(row, '5.44.0', self.completed(), self.completed('3:2|4:1')))
        self.assertEqual([], oracle.assess(row, '5.44.0', self.completed(stderr='Named parameters in signatures are experimental'), self.completed('3:2|4:1')))
        args = self.fixture('args_array')
        self.assertTrue(oracle.assess(args, '5.44.0', self.completed(), None))
        self.assertEqual([], oracle.assess(args, '5.44.0', self.completed(stderr='Use of @_ in scalar with signatured subroutine is experimental'), None))

    def test_missing_exit_and_identity_are_not_rejection_evidence(self):
        row = self.fixture('named_array')
        for code in (None, True, '1', -1, 0xC0000005):
            self.assertTrue(oracle.assess(row, '5.44.0', self.completed(code=code), None))
        receipt = {'schema': oracle.SCHEMA, 'matrix_sha256': self.digest, 'runner_sha256': oracle.text_digest(Path(oracle.__file__).read_bytes()), 'version': '5.44.0', 'identity': self.completed('5.44.0'), 'rows': []}
        with self.assertRaisesRegex(ValueError, 'executable identity'):
            oracle.validate_receipts([receipt], required=['5.44'])

    def test_timeout_and_missing_executable_are_instrument_failures(self):
        import sys
        self.assertEqual('timeout', oracle.execute([sys.executable, '-c', 'import time; time.sleep(3)'], timeout=0.05)['status'])
        self.assertEqual('spawn_error', oracle.execute(['no-such-signature-conformance-executable'])['status'])

    def test_interrupted_fixture_is_reaped_before_evidence_closes(self):
        real_popen = subprocess.Popen
        real_wait = real_popen.wait
        for during_timeout in (False, True):
            for interruption in (KeyboardInterrupt(), SystemExit(7)):
                with self.subTest(during_timeout=during_timeout, error=type(interruption).__name__):
                    children, streams = [], []
                    calls = 0

                    def spawn(*args, **kwargs):
                        child = real_popen(*args, **kwargs)
                        children.append(child)
                        streams.extend((kwargs['stdout'], kwargs['stderr']))

                        def interrupted_wait(timeout=None):
                            nonlocal calls
                            calls += 1
                            # Every production wait, including cleanup, must precede
                            # closing the temporary evidence files.
                            self.assertTrue(all(not stream.closed for stream in streams))
                            if calls == (2 if during_timeout else 1):
                                raise interruption
                            return real_wait(child, timeout=timeout)

                        child.wait = interrupted_wait
                        return child

                    try:
                        with mock.patch.object(oracle.subprocess, 'Popen', side_effect=spawn):
                            with self.assertRaises(type(interruption)) as raised:
                                oracle.execute([sys.executable, '-c', 'import time; time.sleep(60)'], timeout=0.01)
                        self.assertIs(interruption, raised.exception)
                        self.assertEqual(1, len(children))
                        # Do not poll here: polling could reap a kill-only repair
                        # and hide the missing production wait.
                        self.assertIsNotNone(children[0].returncode)
                        if os.name == 'posix':
                            with self.assertRaises(ChildProcessError):
                                os.waitpid(children[0].pid, os.WNOHANG)
                        self.assertTrue(all(stream.closed for stream in streams))
                    finally:
                        # Keep the regression safe against the leaking old runner.
                        for child in children:
                            if child.returncode is None:
                                child.kill()
                            real_wait(child, timeout=5)

    def test_real_process_outcomes_and_bounded_evidence(self):
        result = oracle.execute([sys.executable, '-c', 'import sys; print("out"); print("err", file=sys.stderr); sys.exit(7)'])
        # execute() returns the child's raw decoded bytes with no newline
        # translation, and a Windows child's print() emits CRLF.
        normalized = {
            'status': result['status'],
            'exit': result['exit'],
            'stdout': result['stdout'].replace('\r\n', '\n'),
            'stderr': result['stderr'].replace('\r\n', '\n'),
        }
        self.assertEqual({'status': 'completed', 'exit': 7, 'stdout': 'out\n', 'stderr': 'err\n'}, normalized)
        for stream in ('stdout', 'stderr'):
            with self.subTest(stream=stream):
                result = oracle.execute([sys.executable, '-c', f'import sys; sys.{stream}.write("x" * {oracle.LIMIT + 1})'])
                self.assertEqual('output_limit', result['status'])
                self.assertEqual(0, result['exit'])
                self.assertEqual('x' * oracle.LIMIT, result[stream])

if __name__ == '__main__': unittest.main()
