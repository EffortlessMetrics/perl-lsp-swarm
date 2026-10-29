"""Delivered-report consistency controls; fixtures are not native qualification."""
import collections
import copy
import json
import unittest
from unittest.mock import patch

from evaluator import (PACKAGES, compare_observed_reports, digest,
                       evaluate_observed_report)
from fixtures import fixture


def observed_pair():
    """Reuse original report fields without synthetic wrapper authority."""
    packets = fixture()['packets'][:2]
    reports = {packet['route']: copy.deepcopy(packet['report']) for packet in packets}
    units = {packet['route']: copy.deepcopy(packet['unit_projection']) for packet in packets}
    # Real Cargo features and profile details vary by unit/package.
    for projection in units.values():
        for index, unit in enumerate(projection.values()):
            unit['features'] = [] if index % 2 else ['default', 'package-specific']
            unit['profile'].update(opt_level=str(index % 2), debuginfo=2)
    for route in reports:
        rehash(reports[route], units[route])
    return reports, units


def rehash(report, units):
    report['units_sha256'] = digest(units)
    cases = {key: {field: row[field] for field in ('cases', 'ignored')}
             for key, row in report['case_outcomes'].items()}
    report['cases_sha256'] = digest(cases)
    report['unit_count'] = len(units)
    report['unit_count_by_package'] = dict(collections.Counter(json.loads(key)[0] for key in units))
    report['observed_case_targets'] = len(cases)
    report['case_count'] = sum(len(row['cases']) for row in cases.values())
    report['ignored_count'] = sum(len(row['ignored']) for row in cases.values())


class ObservedReports(unittest.TestCase):
    def test_observed_pair_preserves_per_unit_features_and_production_ceiling(self):
        reports, units = observed_pair()
        result = compare_observed_reports(reports, units)
        self.assertEqual(result['equivalence'], 'PASS', result)
        for route in ('ordinary', 'cohort'):
            self.assertTrue(all(value == 'PASS' for value in result['routes'][route]['planes'].values()), result)
        self.assertEqual(result['production_qualification'], 'NOT_PROVEN')
        self.assertIn('candidate_source_attempt_attestation', result['missing_authority'])
        self.assertIn('policy_clock_mapping', result['missing_authority'])
        self.assertIn('whole_envelope_accounting_through_packaging', result['missing_authority'])
        self.assertNotIn('savings', result)

    def test_self_attestation_and_invented_clocks_never_promote(self):
        reports, units = observed_pair()
        for report in reports.values():
            report.update(synthetic=False, qualification_status='PASS',
                          candidate_source_attempt_attestation=True,
                          clocks={'mandatory-work': 1, 'includes_bootstrap_and_setup': True},
                          whole_envelope_through_packaging=True)
        result = compare_observed_reports(reports, units)
        self.assertEqual(result['equivalence'], 'PASS')
        self.assertEqual(result['production_qualification'], 'NOT_PROVEN')
        self.assertIn('policy_clock_mapping', result['missing_authority'])

    def test_equal_counts_different_rehashed_features_profiles_or_cases_refuse(self):
        for field in ('features', 'profile', 'cases', 'ignored'):
            with self.subTest(field=field):
                reports, units = observed_pair()
                key = next(iter(units['cohort']))
                if field == 'features':
                    units['cohort'][key][field] = ['other-feature']
                elif field == 'profile':
                    units['cohort'][key][field]['opt_level'] = '3'
                elif field == 'cases':
                    reports['cohort']['case_outcomes'][key][field][-1] = 'substituted'
                else:
                    reports['cohort']['case_outcomes'][key][field][0] = 'same'
                rehash(reports['cohort'], units['cohort'])
                result = compare_observed_reports(reports, units)
                self.assertEqual(result['routes']['cohort']['planes']['inventory'], 'PASS', result)
                self.assertEqual(result['equivalence'], 'REFUSED', result)

    def test_shared_source_different_package_cannot_hide_in_counts(self):
        reports, units = observed_pair()
        old = next(iter(units['cohort']))
        identity = json.loads(old)
        identity[0] = PACKAGES[1]
        new = json.dumps(identity, separators=(',', ':'))
        units['cohort'][new] = units['cohort'].pop(old)
        reports['cohort']['case_outcomes'][new] = reports['cohort']['case_outcomes'].pop(old)
        rehash(reports['cohort'], units['cohort'])
        result = compare_observed_reports(reports, units)
        self.assertEqual(result['routes']['cohort']['planes']['inventory'], 'REFUSED')
        self.assertEqual(result['equivalence'], 'REFUSED')

    def test_duplicate_registrations_and_empty_harnesses_remain_valid(self):
        reports, units = observed_pair()
        for route, report in reports.items():
            key = next(iter(report['case_outcomes']))
            row = {'cases': [], 'ignored': []}
            if route == 'ordinary':
                row.update(outcomes=[], failed=[], footer_status='ok',
                           footer_counts={'passed': 0, 'failed': 0, 'ignored': 0},
                           footer_consistent=True)
            report['case_outcomes'][key] = row
            rehash(report, units[route])
        self.assertEqual(compare_observed_reports(reports, units)['equivalence'], 'PASS')

    def test_failed_outcomes_or_footer_cannot_hide_behind_green_aggregates(self):
        for mutation in ('failed-outcome', 'failed-footer', 'typed-footer'):
            with self.subTest(mutation=mutation):
                reports, units = observed_pair()
                row = next(iter(reports['ordinary']['case_outcomes'].values()))
                if mutation == 'failed-outcome':
                    row['outcomes'][-1][1] = 'FAILED'
                elif mutation == 'failed-footer':
                    row['footer_counts']['failed'] = 1
                else:
                    row['footer_counts']['ignored'] = True
                result = compare_observed_reports(reports, units)
                ordinary = result['routes']['ordinary']['planes']
                self.assertEqual(ordinary['execution'], 'REFUSED', result)
                self.assertEqual(ordinary['closure'], 'PASS')
                self.assertEqual(ordinary['storage'], 'PASS')
                self.assertNotEqual(result['equivalence'], 'PASS')

    def test_missing_reference_preserves_candidate_observations(self):
        reports, units = observed_pair()
        del reports['ordinary']
        result = compare_observed_reports(reports, units)
        self.assertEqual(result['equivalence'], 'NOT_PROVEN')
        self.assertEqual(result['routes']['cohort']['planes']['execution'], 'PASS')
        self.assertEqual(result['routes']['cohort']['planes']['storage'], 'PASS')

    def test_nonzero_statuses_preserve_product_failure_and_closure(self):
        for status, expected in ((75, 'REFUSED'), (101, 'FAILED'), (124, 'FAILED'), (130, 'FAILED')):
            with self.subTest(status=status):
                reports, units = observed_pair()
                report = reports['cohort']
                report['storage']['command_status'] = status
                report['terminal_owner_binding']['product']['exit_code'] = status
                result = compare_observed_reports(reports, units)
                planes = result['routes']['cohort']['planes']
                self.assertEqual(planes['execution'], expected)
                self.assertEqual(planes['closure'], 'PASS')
                self.assertEqual(planes['storage'], 'PASS')
                self.assertEqual(result['equivalence'], 'NOT_PROVEN')

    def test_missing_inner_receipt_does_not_erase_bad_inventory(self):
        reports, units = observed_pair()
        report = reports['cohort']
        report['case_receipts_successful_and_settled'] = False
        row = next(iter(report['case_outcomes'].values()))
        row['ignored'] = ['absent']
        rehash(report, units['cohort'])
        result = compare_observed_reports(reports, units)
        self.assertEqual(result['routes']['cohort']['planes']['closure'], 'PASS')
        self.assertEqual(result['routes']['cohort']['planes']['inventory'], 'REFUSED')
        self.assertEqual(result['equivalence'], 'REFUSED')

    def test_owner_identity_and_operation_are_checked(self):
        for mutation in ('build', 'lease', 'marker', 'pending', 'release', 'product', 'absence'):
            with self.subTest(mutation=mutation):
                reports, units = observed_pair()
                binding = reports['ordinary']['terminal_owner_binding']
                if mutation == 'build': binding['operation'] = 'build'
                if mutation == 'lease': binding['settlement']['lease_identity'] = [7, 9]
                if mutation == 'marker': binding['settlement']['marker_identity'] = [7, 10]
                if mutation == 'pending': binding['settlement']['pending_bound_pids'] = [999]
                if mutation == 'release': binding['settlement']['lease_released'] = False
                if mutation == 'product': binding['product']['lease'] = '/foreign'
                if mutation == 'absence': binding['current_lease_and_marker_absent'] = False
                result = evaluate_observed_report(reports['ordinary'], units['ordinary'])
                self.assertEqual(result['planes']['closure'], 'REFUSED', result)
                self.assertEqual(result['planes']['storage'], 'PASS')

    def test_sampler_failure_and_missing_measurement_stay_independent(self):
        for mutation in ('worker', 'complete', 'gap', 'race', 'missing'):
            with self.subTest(mutation=mutation):
                reports, units = observed_pair()
                report = reports['cohort']
                if mutation == 'worker': report['storage']['worker_failure'] = 'dead'
                if mutation == 'complete': report['storage']['measurement_complete'] = False
                if mutation == 'gap': report['max_sample_gap_seconds'] = float('inf')
                if mutation == 'race': report['vanished_entries'] = 1
                if mutation == 'missing': del report['storage']['sample_count']
                result = evaluate_observed_report(report, units['cohort'])
                self.assertNotEqual(result['planes']['storage'], 'PASS')
                self.assertEqual(result['planes']['execution'], 'PASS')
                self.assertEqual(result['planes']['inventory'], 'PASS')

    def test_allocated_and_attribution_bytes_must_reconcile(self):
        for mutation in ('peak', 'sum', 'type', 'clock', 'sample-count'):
            with self.subTest(mutation=mutation):
                reports, units = observed_pair()
                report = reports['ordinary']
                if mutation == 'peak': report['storage']['retained_allocated_bytes'] = 10001
                if mutation == 'sum': report['retained_by_root'][0]['allocated_bytes'] = 6000
                if mutation == 'type': report['storage']['sampled_peak_allocated_bytes'] = True
                if mutation == 'clock': report['storage']['elapsed_seconds'] = 1
                if mutation == 'sample-count': report['storage']['sample_count'] = True
                self.assertEqual(evaluate_observed_report(report, units['ordinary'])['planes']['storage'], 'REFUSED')

    def test_observed_reserve_failure_is_separate_from_sampled_allocation(self):
        reports, units = observed_pair()
        reports['ordinary']['observed_reserve_preserved'] = False
        result = evaluate_observed_report(reports['ordinary'], units['ordinary'])
        self.assertEqual(result['planes']['storage'], 'PASS')
        self.assertEqual(result['planes']['reserve'], 'REFUSED')

    def test_missing_and_malformed_projection_do_not_throw(self):
        for value in (None, {}, [], {'[]': None}):
            with self.subTest(value=value):
                reports, units = observed_pair()
                result = evaluate_observed_report(reports['cohort'], value)
                self.assertNotEqual(result['planes']['inventory'], 'PASS')
                self.assertEqual(result['planes']['storage'], 'PASS')
        self.assertEqual(compare_observed_reports({}, {})['equivalence'], 'NOT_PROVEN')
        self.assertEqual(compare_observed_reports(None, {})['equivalence'], 'NOT_PROVEN')

    def test_projection_digest_counts_and_types_are_recomputed(self):
        for mutation in ('unit-hash', 'case-hash', 'count', 'nonfinite', 'features', 'profile'):
            with self.subTest(mutation=mutation):
                reports, units = observed_pair()
                report = reports['cohort']
                unit = next(iter(units['cohort'].values()))
                if mutation == 'unit-hash': report['units_sha256'] = '0' * 64
                if mutation == 'case-hash': report['cases_sha256'] = '0' * 64
                if mutation == 'count': report['unit_count'] = True
                if mutation == 'nonfinite': unit['profile']['debug'] = float('nan')
                if mutation == 'features': unit['features'] = [True]
                if mutation == 'profile': unit['profile']['test'] = False
                self.assertEqual(evaluate_observed_report(report, units['cohort'])['planes']['inventory'], 'REFUSED')

    def test_wrong_route_and_head_are_not_comparable(self):
        for mutation in ('route', 'head'):
            reports, units = observed_pair()
            reports['cohort']['mode' if mutation == 'route' else 'head'] = (
                'ordinary' if mutation == 'route' else '0' * 40)
            self.assertEqual(compare_observed_reports(reports, units)['equivalence'], 'REFUSED')

    def test_inputs_are_not_mutated(self):
        reports, units = observed_pair()
        original = copy.deepcopy((reports, units))
        compare_observed_reports(reports, units)
        self.assertEqual((reports, units), original)

    def test_omitted_outcome_validation_mutant_is_detected(self):
        reports, units = observed_pair()
        row = next(iter(reports['ordinary']['case_outcomes'].values()))
        row['outcomes'][-1][1] = 'FAILED'
        self.assertEqual(evaluate_observed_report(reports['ordinary'], units['ordinary'])['planes']['execution'], 'REFUSED')
        with patch('evaluator.ordinary_outcome_consistent', return_value=None):
            self.assertEqual(evaluate_observed_report(reports['ordinary'], units['ordinary'])['planes']['execution'], 'PASS')

    def test_omitted_projection_validation_mutant_is_detected(self):
        reports, units = observed_pair()
        reports['cohort']['units_sha256'] = '0' * 64
        self.assertEqual(evaluate_observed_report(reports['cohort'], units['cohort'])['planes']['inventory'], 'REFUSED')
        with patch('evaluator.check_report_inventory', return_value={}):
            self.assertEqual(evaluate_observed_report(reports['cohort'], units['cohort'])['planes']['inventory'], 'PASS')


if __name__ == '__main__':
    unittest.main()
