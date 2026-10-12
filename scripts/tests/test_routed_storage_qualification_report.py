import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('report', Path(__file__).resolve().parents[1] / 'ci/report_routed_storage_qualification.py')
subject = importlib.util.module_from_spec(spec); spec.loader.exec_module(subject)


class QualificationInventory(unittest.TestCase):
    def test_two_complete_producer_formats_join_and_missing_package_does_not(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            (evidence / 'head.txt').write_text('candidate')
            storage = [{'kind': 'retained', 'valid': True, 'elapsed_seconds': 0,
                        'filesystems': {}, 'allocated_bytes': 0}, {'kind': 'summary', 'command_status': 0}]
            (evidence / 'storage.jsonl').write_text('\n'.join(json.dumps(row) for row in storage))
            rows = []
            for package in subject.PACKAGES:
                row = self.row()
                row['target']['src_path'] = '/work/' + ('xtask' if package == 'xtask' else 'crates/' + package) + '/tests/shared.rs'
                row['executable'] = '/private/' + package
                rows.append(row)
            compiler = '\n'.join(json.dumps(row) for row in [*rows, {'reason': 'build-finished', 'success': True}])
            (evidence / 'route.stdout').write_text(compiler + '\n' + ('running 1 test\ntest case ... ok\ntest result: ok.\n' * 6))
            (evidence / 'route.stderr').write_text(''.join('Running tests/shared.rs (' + row['executable'] + ')\n' for row in rows)
                + 'cargo-admitted Cargo settlement: {"tree_settled":true}\n')
            ordinary = subject.report(evidence, 'ordinary', Path('/work'))
            self.assertTrue(ordinary['case_inventory_complete'])
            cohort = evidence / 'cohort'; cohort.mkdir()
            for index, producer in enumerate((['cargo', 'check', '--tests'], ['cargo', 'test', '--no-run'])):
                stem = f'{index:04}'
                (cohort / (stem + '-request.json')).write_text(json.dumps({'producer': producer, 'evidence_path': '/host/' + stem + '-compiler.jsonl'}))
                (cohort / (stem + '-compiler.jsonl')).write_text(compiler)
                receipt = {'status': 0, 'settlement': {'tree_settled': True}, 'coverage': []}
                if index:
                    receipt['coverage'] = [{'executable': row['executable'], 'cases': ['case'], 'ignored': []} for row in rows]
                (cohort / (stem + '-result.json')).write_text(json.dumps(receipt))
            native = subject.report(evidence, 'cohort', Path('/work'))
            self.assertTrue(native['case_inventory_complete'])
            self.assertEqual(native['units_sha256'], ordinary['units_sha256'])
            self.assertEqual(native['cases_sha256'], ordinary['cases_sha256'])
            result_path = cohort / '0001-result.json'
            for status, settled in [(101, True), (0, False)]:
                invalid = json.loads(result_path.read_text())
                invalid.update(status=status, settlement={'tree_settled': settled})
                result_path.write_text(json.dumps(invalid))
                failed = subject.report(evidence, 'cohort', Path('/work'))
                self.assertTrue(failed['observed_case_inventory_complete'])
                self.assertFalse(failed['case_inventory_complete'])
                self.assertFalse(failed['case_receipts_successful_and_settled'])
                self.assertEqual(failed['case_count'], 6)
            (cohort / '0000-compiler.jsonl').write_text('\n'.join(json.dumps(row) for row in [*rows[:-1], {'reason': 'build-finished', 'success': True}]))
            self.assertFalse(subject.report(evidence, 'cohort', Path('/work'))['unit_inventory_complete'])

    def row(self, root='/work', features=None):
        return {'reason': 'compiler-artifact', 'target': {'src_path': root + '/crates/perl-dap/tests/shared.rs',
            'name': 'shared', 'kind': ['test']}, 'features': features or [],
            'profile': {'test': True, 'opt_level': '1'}, 'executable': '/private/shared'}

    def test_cross_host_roots_normalize_but_features_and_omissions_do_not(self):
        a = subject.units([self.row('/one')], Path('/one'))
        b = subject.units([self.row('/two')], Path('/two'))
        self.assertEqual(subject.digest(a), subject.digest(b))
        self.assertNotEqual(subject.digest(a), subject.digest(subject.units([self.row('/one', ['extra'])], Path('/one'))))
        self.assertNotEqual(subject.digest(a), subject.digest({}))

    def test_conflicting_unit_context_refuses(self):
        with self.assertRaisesRegex(ValueError, 'conflicting'):
            subject.units([self.row(), self.row(features=['extra'])], Path('/work'))

    def test_extra_package_is_not_silently_dropped(self):
        row = self.row()
        row['target']['src_path'] = '/work/crates/unselected/tests/shared.rs'
        with self.assertRaisesRegex(ValueError, 'unexpected package'):
            subject.units([row], Path('/work'))

    def test_actual_stdout_maps_cases_and_ignored_labels_to_compiler_target(self):
        stdout = 'running 2 tests\ntest panic - should panic ... ok\ntest skipped ... ignored, reason\ntest result: ok. 1 passed; 0 failed; 1 ignored\n'
        result = subject.ordinary_cases(stdout, '     Running tests/shared.rs (/private/shared)\n', [self.row()], Path('/work'))
        self.assertEqual(next(iter(result.values())), {'cases': ['panic', 'skipped'], 'ignored': ['skipped']})
        changed = {key: {**value, 'ignored': []} for key, value in result.items()}
        self.assertNotEqual(subject.digest(result), subject.digest(changed))

    def test_omissions_duplicates_and_ambiguous_nesting_refuse(self):
        for body in ('running 2 tests\ntest one ... ok\ntest result: ok.\n',
                     'running 2 tests\ntest one ... ok\ntest one ... ok\ntest result: ok.\n',
                     'running 1 test\nrunning 1 test\n'):
            with self.assertRaises(ValueError):
                subject.ordinary_cases(body, 'Running tests/shared.rs (/private/shared)\n', [self.row()], Path('/work'))


if __name__ == '__main__':
    unittest.main()
