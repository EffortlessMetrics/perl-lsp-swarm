"""Compiler-free byte-delivery controls; invented packets never qualify."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import struct
import unittest
import zipfile
from unittest.mock import patch
import zlib

from evaluator import HEAD
from fixtures import fixture
from retained_packet import inspect_archive, strict_json


class RetainedPacketTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / 'packet.zip'
        packet = fixture()['packets'][0]
        self.report = copy.deepcopy(packet['report'])
        self.report['unit_projection'] = packet['unit_projection']
        self.argv = ['test', '-p', 'xtask', '--locked']
        self.report['terminal_owner_binding']['descriptor']['scope'] = {
            'cargo_argv_sha256': hashlib.sha256(json.dumps(self.argv, separators=(',', ':')).encode()).hexdigest(),
            'operation': 'test',
        }
        self.run = {'id': 10, 'run_attempt': 2, 'head_sha': HEAD}
        self.artifact = {'id': 20, 'expired': False, 'workflow_run': {'id': 10, 'head_sha': HEAD}}
        self.write_archive({'summary.json': self.report})

    def write_archive(self, reports):
        with zipfile.ZipFile(self.path, 'w') as archive:
            for name, report in reports.items():
                archive.writestr(name, json.dumps(report))
        raw = self.path.read_bytes()
        self.artifact.update(digest='sha256:' + hashlib.sha256(raw).hexdigest(), size_in_bytes=len(raw))

    def inspect(self, **kwargs):
        args = dict(expected_run=10, expected_attempt=2, expected_artifact=20,
                    expected_head=HEAD, report_members=['summary.json'], expected_argv=self.argv)
        args.update(kwargs)
        return inspect_archive(self.path, self.artifact, self.run, **args)

    def test_matching_delivered_packet_is_observation_only(self):
        result = self.inspect()
        self.assertEqual(result['delivery'], 'CONSISTENT')
        self.assertEqual(result['reports'][0]['binding'], 'CONSISTENT')
        self.assertEqual(result['reports'][0]['observations']['planes']['execution'], 'PASS')
        self.assertEqual(result['production_qualification'], 'NOT_PROVEN')
        self.assertEqual(result['time'], 'NOT_PROVEN')
        self.assertIn('artifact_to_execution_attempt_binding', result['missing_authority'])

    def test_delivery_identity_substitutions_refuse(self):
        for field, value in [('expected_run', 11), ('expected_attempt', 1),
                             ('expected_artifact', 21), ('expected_head', 'a' * 40)]:
            with self.subTest(field=field):
                self.assertEqual(self.inspect(**{field: value})['delivery'], 'REFUSED')

    def test_changed_archive_bytes_refuse(self):
        self.path.write_bytes(self.path.read_bytes() + b'substituted')
        self.assertEqual(self.inspect()['delivery'], 'REFUSED')

    def test_missing_bytes_never_empty_success(self):
        self.path = Path(self.temp.name) / 'not-delivered.zip'
        self.assertEqual(self.inspect()['delivery'], 'NOT_PROVEN')

    def test_expired_or_digestless_listing_not_delivery(self):
        self.artifact['expired'] = True
        self.assertEqual(self.inspect()['delivery'], 'REFUSED')
        self.artifact['expired'] = False
        del self.artifact['digest']
        self.assertEqual(self.inspect()['delivery'], 'NOT_PROVEN')

    def test_rehashed_wrong_report_head_refuses(self):
        self.report['head'] = 'a' * 40
        self.write_archive({'summary.json': self.report})
        self.assertEqual(self.inspect()['reports'][0]['binding'], 'REFUSED')

    def test_changed_requested_argv_refuses(self):
        result = self.inspect(expected_argv=self.argv + ['--all-features'])
        self.assertEqual(result['reports'][0]['binding'], 'REFUSED')

    def test_missing_unit_projection_does_not_invent_empty_graph(self):
        del self.report['unit_projection']
        self.write_archive({'summary.json': self.report})
        item = self.inspect()['reports'][0]
        self.assertEqual(item['observations']['planes']['inventory'], 'NOT_PROVEN')
        self.assertIn('producer_canonical_unit_projection', item['missing_authority'])

    def test_missing_product_cannot_turn_zero_status_into_success(self):
        self.report['terminal_owner_binding']['product'] = None
        self.write_archive({'summary.json': self.report})
        planes = self.inspect()['reports'][0]['observations']['planes']
        self.assertEqual(planes['closure'], 'PASS')
        self.assertEqual(planes['execution'], 'NOT_PROVEN')

    def test_timeout_closure_survives_missing_normal_product(self):
        self.report['storage']['command_status'] = 124
        self.report['terminal_owner_binding']['product'] = None
        self.report['terminal_owner_binding']['settlement']['cancelled'] = True
        self.report['outer_settlement']['cancelled'] = True
        self.write_archive({'summary.json': self.report})
        planes = self.inspect()['reports'][0]['observations']['planes']
        self.assertEqual(planes['closure'], 'PASS')
        self.assertEqual(planes['execution'], 'FAILED')

    def test_actual_failure_survives_delivery_and_green_aggregate(self):
        self.report['storage']['command_status'] = 124
        self.report['terminal_owner_binding']['product']['exit_code'] = 124
        self.write_archive({'summary.json': self.report})
        result = self.inspect()
        self.assertEqual(result['delivery'], 'CONSISTENT')
        self.assertEqual(result['reports'][0]['observations']['planes']['execution'], 'FAILED')
        self.assertEqual(result['reports'][0]['observations']['planes']['storage'], 'PASS')

    def test_two_route_attempt_clocks_are_not_new_policy(self):
        first, retry = copy.deepcopy(self.report), copy.deepcopy(self.report)
        first['storage']['command_elapsed_seconds'] = 1500
        retry['storage']['command_elapsed_seconds'] = 1000
        for report in (first, retry):
            report['mandatory_work'] = 1
            report['synthetic'] = False
            report['candidate_source_attempt_attestation'] = {'verified': True}
        self.write_archive({'summary.json': first, 'retry/summary.json': retry})
        result = self.inspect(report_members=['summary.json', 'retry/summary.json'])
        self.assertEqual([r['clock_observations']['command_elapsed_seconds'] for r in result['reports']], [1500, 1000])
        self.assertTrue(all(r['clock_observations']['gate_cumulative_elapsed_seconds'] is None for r in result['reports']))
        self.assertEqual(result['time'], 'NOT_PROVEN')
        self.assertEqual(result['production_qualification'], 'NOT_PROVEN')

    def test_duplicate_archive_member_refuses(self):
        with zipfile.ZipFile(self.path, 'a') as archive:
            import warnings
            with warnings.catch_warnings():
                warnings.simplefilter('ignore', UserWarning)
                archive.writestr('summary.json', json.dumps(self.report))
        raw = self.path.read_bytes()
        self.artifact.update(digest='sha256:' + hashlib.sha256(raw).hexdigest(), size_in_bytes=len(raw))
        self.assertEqual(self.inspect()['reports'][0]['binding'], 'REFUSED')

    def test_missing_selected_report_refuses(self):
        self.assertEqual(self.inspect(report_members=['missing.json'])['reports'][0]['binding'], 'REFUSED')

    def test_malformed_storage_is_a_diagnostic(self):
        for value in (None, [], 42):
            with self.subTest(value=value):
                self.report['storage'] = value
                self.write_archive({'summary.json': self.report})
                self.assertEqual(self.inspect()['reports'][0]['binding'], 'REFUSED')

    def test_path_replacement_cannot_swap_parsed_subject(self):
        original_reader = zipfile.ZipFile
        def replace_before_parse(subject):
            self.path.write_bytes(b'not the delivered artifact')
            return original_reader(subject)
        with patch('retained_packet.zipfile.ZipFile', side_effect=replace_before_parse):
            result = self.inspect()
        self.assertEqual(result['delivery'], 'CONSISTENT')
        self.assertEqual(result['reports'][0]['binding'], 'CONSISTENT')

    def test_corrupt_deflate_bytes_are_a_diagnostic(self):
        with zipfile.ZipFile(self.path, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr('summary.json', json.dumps(self.report))
        raw = bytearray(self.path.read_bytes())
        name_length, extra_length = struct.unpack_from('<HH', raw, 26)
        offset = 30 + name_length + extra_length
        raw[offset] = (raw[offset] & 0xf8) | 7  # Reserved DEFLATE BTYPE=3.
        self.path.write_bytes(raw)
        self.artifact.update(digest='sha256:' + hashlib.sha256(raw).hexdigest(), size_in_bytes=len(raw))
        report = self.inspect()['reports'][0]
        self.assertEqual(report['binding'], 'REFUSED')
        self.assertIn('invalid block type', report['reason'])

    def test_decompression_failure_is_a_diagnostic(self):
        for error in (zlib.error('corrupt compressed member'), EOFError('truncated member')):
            with self.subTest(error=type(error).__name__), patch('retained_packet.member_bytes', side_effect=error):
                result = self.inspect()
                self.assertEqual(result['reports'][0]['binding'], 'REFUSED')

    def test_absent_captured_metadata_is_not_a_package_guess(self):
        self.report['package_inventory_verified'] = False
        self.write_archive({'summary.json': self.report})
        planes = self.inspect()['reports'][0]['observations']['planes']
        self.assertEqual(planes['inventory'], 'NOT_PROVEN')

    def test_strict_json_does_not_hide_contradictions(self):
        for raw in ('{"head":1,"head":2}', '{"x":NaN}', '{"x":Infinity}', '{"x":1e999}', '{"x":-1e999}'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                strict_json(raw)


if __name__ == '__main__':
    unittest.main()
