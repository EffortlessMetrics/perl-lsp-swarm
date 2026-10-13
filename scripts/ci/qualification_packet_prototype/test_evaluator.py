import ast
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from evaluator import HEAD, evaluate, digest
from fixtures import fixture

HERE = Path(__file__).resolve().parent


class Controls(unittest.TestCase):
    def reject(self, document):
        result = evaluate(document)
        self.assertEqual(result['status'], 'NOT_PROVEN', result)
        self.assertEqual(result['production_qualification'], 'NOT_PROVEN')
        return result

    def test_complete_matching_cells(self):
        result = evaluate(fixture())
        self.assertEqual(result['status'], 'SYNTHETIC_PASS', result)
        self.assertEqual(len(result['savings']), 5)
        self.assertAlmostEqual(result['savings']['cold']['sampled_peak_fraction'], .2)
        self.assertEqual(result['production_qualification'], 'NOT_PROVEN')

    def test_missing_duplicate_and_foreign_cells(self):
        for mutation in ('missing', 'duplicate', 'foreign', 'warm', 'edit'):
            with self.subTest(mutation=mutation):
                d = fixture()
                if mutation == 'missing': d['packets'].pop(0)
                if mutation == 'duplicate': d['packets'].append(copy.deepcopy(d['packets'][0]))
                if mutation == 'foreign': d['packets'][0]['cell'] = 'timeout-retry'
                if mutation == 'warm': d['packets'] = [p for p in d['packets'] if p['cell'] != 'warm']
                if mutation == 'edit': d['packets'] = [p for p in d['packets'] if p['cell'] != 'producer-edit']
                self.reject(d)

    def test_each_missing_or_mismatched_binding(self):
        for field in fixture()['packets'][0]['bindings']:
            for missing in (False, True):
                with self.subTest(field=field, missing=missing):
                    d = fixture(); b = d['packets'][0]['bindings']
                    if missing: del b[field]
                    else: b[field] = {'foreign': True}
                    self.reject(d)

    def test_no_attestation_is_not_proven(self):
        d = fixture(); del d['packets'][0]['candidate_source_attempt_attestation']
        self.reject(d)

    def test_same_count_substitution_and_multiplicity(self):
        for kind in ('case', 'target', 'features', 'profile', 'ignored', 'hash'):
            with self.subTest(kind=kind):
                d = fixture(); p = d['packets'][1]; r = p['report']; key = next(iter(r['case_outcomes']))
                if kind == 'case': r['case_outcomes'][key]['cases'][0] = 'substituted'
                if kind == 'target': p['unit_projection'][key.replace('synthetic-target', 'different')] = p['unit_projection'].pop(key)
                if kind == 'features': p['unit_projection'][key]['features'] = ['extra']
                if kind == 'profile': p['unit_projection'][key]['profile'] = {'test': True, 'opt_level': 3}
                if kind == 'ignored': r['case_outcomes'][key]['ignored'] = ['same', 'same']
                if kind == 'hash': r['cases_sha256'] = '0' * 64
                self.reject(d)

    def test_failure_refusal_and_absence_distinct(self):
        for status, verdict in ((75, 'REFUSED'), (101, 'FAILED'), (124, 'FAILED'), (130, 'FAILED')):
            d = fixture(); p = d['packets'][0]
            p['report']['storage']['command_status'] = status
            p['report']['terminal_owner_binding']['product']['exit_code'] = status
            r = self.reject(d)
            self.assertEqual(r['cells']['cold']['ordinary']['execution'], verdict)
        d = fixture(); d['packets'].pop(0)
        self.assertEqual(self.reject(d)['cells']['cold']['ordinary']['execution'], 'NOT_PROVEN')

    def test_settlement_and_inner_receipt(self):
        for kind in ('earlier-build', 'inner-missing', 'unsettled', 'foreign-owner', 'cancelled'):
            d = fixture(); p = d['packets'][0]; b = p['report']['terminal_owner_binding']
            if kind == 'earlier-build': b['operation'] = 'build'
            if kind == 'inner-missing': p['inner_receipts_complete'] = False
            if kind == 'unsettled': b['settlement']['tree_settled'] = False
            if kind == 'foreign-owner': b['descriptor']['lease'] = '/foreign'
            if kind == 'cancelled': b['settlement']['cancelled'] = True
            self.reject(d)

    def test_measurement_and_accounting(self):
        for field, value in (('observer_alive_through_completion', False), ('whole_envelope_through_packaging', False), ('accounting_complete', False), ('basis_verified', False), ('sample_count', 0)):
            d = fixture(); d['packets'][0]['measurement'][field] = value; self.reject(d)
        for field, value in (('measurement_valid', False), ('observed_reserve_preserved', False), ('max_sample_gap_seconds', 10), ('vanished_entries', 1)):
            d = fixture(); d['packets'][0]['report'][field] = value; self.reject(d)
        d = fixture(); d['packets'][0]['report']['storage']['worker_failure'] = 'dead'; self.reject(d)
        d = fixture(); d['packets'][0]['report']['minimum_free_bytes'] = {}; self.reject(d)

    def test_performance_clock_and_mandatory_work(self):
        for field, value in (('mandatory-work', 1400), ('route', 1501), ('includes_bootstrap_and_setup', False), ('binding', {})):
            d = fixture(); d['packets'][0]['clocks'].update({'route': 1400, field: value}); self.reject(d)
        d = fixture(); del d['packets'][0]['clocks']['mandatory-work']; self.reject(d)

    def test_numeric_types(self):
        for bad in (False, True, None, '0', float('nan'), float('inf'), -1):
            for kind in ('status', 'time', 'bytes', 'reserve', 'count'):
                with self.subTest(kind=kind, bad=bad):
                    d = fixture(); p = d['packets'][0]
                    if kind == 'status': p['report']['storage']['command_status'] = bad
                    if kind == 'time': p['clocks']['mandatory-work'] = bad
                    if kind == 'bytes': p['report']['storage']['sampled_peak_allocated_bytes'] = bad
                    if kind == 'reserve': p['report']['minimum_free_bytes']['synthetic-fs'] = bad
                    if kind == 'count': p['report']['case_count'] = bad
                    self.reject(d)

    def test_invalid_comparison_denominators(self):
        for kind in ('zero', 'partial', 'different-basis', 'unaccounted'):
            d = fixture(); p = d['packets'][0]
            if kind == 'zero': p['report']['storage']['sampled_peak_allocated_bytes'] = 0
            if kind == 'partial': p['report']['case_inventory_complete'] = False
            if kind == 'different-basis': p['comparison_basis']['conditioning'] = 'timeout-retry'
            if kind == 'unaccounted': p['measurement']['accounting_complete'] = False
            result = self.reject(d)
            self.assertNotIn('cold', result['savings'])
            self.assertEqual(result['cells']['cold']['cohort']['storage'], 'PASS')

    def test_diagnostics_can_exit_zero_while_check_refuses(self):
        d = fixture(); d['packets'].pop()
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'partial.json'; path.write_text(json.dumps(d))
            for flags, expected in (([], 0), (['--check'], 1)):
                run = subprocess.run([sys.executable, '-B', str(HERE / 'evaluator.py'), str(path), *flags], capture_output=True, text=True)
                self.assertEqual(run.returncode, expected, run.stderr)
                self.assertEqual(json.loads(run.stdout)['status'], 'NOT_PROVEN')

    def test_exact_producer_contract_without_import_or_execution(self):
        contract = json.loads((HERE / 'producer_contract.json').read_text())
        self.assertEqual(contract['head'], HEAD)
        run = subprocess.run(['git', 'show', contract['head'] + ':' + contract['path']],
                             cwd=HERE.parents[2], capture_output=True)
        if run.returncode != 0:
            self.skipTest('Pinned producer object unavailable; fetch the exact ref as documented')
        raw = run.stdout
        import hashlib
        self.assertEqual(hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest(), 'c91dec382450e74bc431b952007a32e85ebde9e1')
        tree = ast.parse(raw)
        assigns = [n for n in ast.walk(tree) if isinstance(n, ast.Assign)]
        authority = [n for n in assigns if any(isinstance(t, ast.Subscript) and isinstance(t.slice, ast.Constant) and t.slice.value == 'qualification_status' for t in n.targets)]
        self.assertEqual(len(authority), 1)
        self.assertEqual(ast.literal_eval(authority[0].value), 'NOT_PROVEN')

    def test_review_counterexamples(self):
        for kind in ('argv', 'bool-attempt', 'gap-bool', 'gap-inf', 'contradictory-outcome', 'component', 'retained', 'manifest-shape', 'unit-shape', 'unit-key', 'owner-receipt', 'comparison-bool', 'measurement-attempt', 'clock-attempt'):
            with self.subTest(kind=kind):
                d = fixture(); p = d['packets'][0]
                if kind == 'argv':
                    d['manifest']['cells'][0]['routes']['cohort']['argv']['effective'].append('--filter=one')
                    for field in ('bindings', 'candidate_source_attempt_attestation'):
                        d['packets'][1][field]['argv']['effective'].append('--filter=one')
                if kind == 'bool-attempt':
                    p['bindings']['attempt']['attempt'] = True
                    p['candidate_source_attempt_attestation']['attempt']['attempt'] = True
                if kind == 'gap-bool': p['measurement']['maximum_gap_seconds'] = True
                if kind == 'gap-inf': p['measurement']['maximum_gap_seconds'] = float('inf')
                if kind == 'contradictory-outcome':
                    row = next(iter(p['report']['case_outcomes'].values()))
                    row.update(failed=['same'], footer_status='FAILED', footer_consistent=False)
                if kind == 'component': p['component_qualified'] = False
                if kind == 'retained': p['report']['storage']['retained_allocated_bytes'] = 100000000
                if kind == 'manifest-shape': d['manifest'] = None
                if kind == 'unit-shape': p['unit_projection'][next(iter(p['unit_projection']))] = None
                if kind == 'unit-key':
                    key = next(iter(p['unit_projection']))
                    p['unit_projection']['[]'] = p['unit_projection'].pop(key)
                    p['report']['case_outcomes']['[]'] = p['report']['case_outcomes'].pop(key)
                if kind == 'owner-receipt': p['report']['terminal_owner_binding']['settlement']['lease'] = '/foreign'
                if kind == 'comparison-bool': p['comparison_basis']['completed_work'] = 1
                if kind == 'measurement-attempt': p['measurement']['binding']['attempt']['attempt'] = 2
                if kind == 'clock-attempt': p['clocks']['subject_binding']['attempt']['attempt'] = 2
                r = self.reject(d)
                self.assertNotIn('cold', r['savings'])

    def test_outer_closure_survives_missing_inner(self):
        d = fixture(); p = d['packets'][1]
        p['inner_receipts_complete'] = False
        r = self.reject(d)
        self.assertEqual(r['cells']['cold']['cohort']['closure'], 'PASS')
        self.assertNotEqual(r['cells']['cold']['cohort']['execution'], 'PASS')

    def test_manifest_side_source_and_resource_omissions(self):
        for field, key in (('source', 'manifest_sha256'), ('resources', 'temp')):
            d = fixture()
            for cell in d['manifest']['cells']:
                for route in cell['routes'].values(): del route[field][key]
            for p in d['packets']:
                # Shared aliases within one fixture packet model a consistently
                # altered envelope; recursively omit in every matching object.
                def omit(value):
                    if isinstance(value, dict):
                        for k, child in list(value.items()):
                            if k == field and isinstance(child, dict): child.pop(key, None)
                            omit(child)
                    elif isinstance(value, list):
                        for child in value: omit(child)
                omit(p)
            self.reject(d)

    def test_all_consistent_required_subfield_omissions(self):
        template = fixture()['manifest']['cells'][0]['routes']['ordinary']
        for field, binding in template.items():
            for key in binding:
                with self.subTest(field=field, key=key):
                    d = fixture()
                    for cell in d['manifest']['cells']:
                        for b in cell['routes'].values(): b[field].pop(key, None)
                    for p in d['packets']:
                        for b in (p['bindings'], p['candidate_source_attempt_attestation'], p['measurement']['binding'], p['clocks']['subject_binding']):
                            b[field].pop(key, None)
                    self.reject(d)

    def test_consistent_typed_binding_corruption(self):
        for field, key in (('configuration', 'profile'), ('configuration', 'features'), ('toolchain', 'pin'), ('owner', 'lease'), ('owner', 'lease_marker'), ('attempt', 'attempt')):
            for value in (True, None, float('nan'), float('inf')):
                with self.subTest(field=field, key=key, value=value):
                    d = fixture()
                    for cell in d['manifest']['cells']:
                        for b in cell['routes'].values(): b[field][key] = value
                    for p in d['packets']:
                        for b in (p['bindings'], p['candidate_source_attempt_attestation'], p['measurement']['binding'], p['clocks']['subject_binding']):
                            b[field][key] = value
                    self.reject(d)

    def test_empty_and_duplicate_requested_cells(self):
        d = fixture(); d['manifest']['cells'] = []; d['packets'] = []; self.reject(d)
        d = fixture(); d['manifest']['cells'].append(copy.deepcopy(d['manifest']['cells'][0])); self.reject(d)

    def test_summary_numeric_and_clock_consistency(self):
        for field in ('sample_count', 'elapsed_seconds', 'command_elapsed_seconds'):
            for value in (True, None, '100', float('nan'), float('inf')):
                with self.subTest(field=field, value=value):
                    d = fixture(); d['packets'][0]['report']['storage'][field] = value
                    self.assertNotIn('cold', self.reject(d)['savings'])
        d = fixture(); d['packets'][0]['report']['storage']['command_elapsed_seconds'] = 1400
        d['packets'][0]['report']['storage']['elapsed_seconds'] = 1401
        self.reject(d)
        d = fixture(); d['packets'][0]['clocks'].update({'route': 1400, 'mandatory-work': 1400})
        d['packets'][0]['report']['storage'].update({'command_elapsed_seconds': 1400, 'elapsed_seconds': 1401})
        self.reject(d)

    def test_same_count_ignored_multiplicity_changes(self):
        d = fixture()
        for p in d['packets']:
            r = p['report']
            for row in r['case_outcomes'].values(): row['ignored'] = ['same', 'same']
            if p['route'] == 'ordinary':
                for row in r['case_outcomes'].values():
                    row['outcomes'] = [['ignored', 'ok'], ['same', 'ignored'], ['same', 'ignored']]
                    row['footer_counts'] = {'passed': 1, 'failed': 0, 'ignored': 2}
            r['ignored_count'] = 12
            r['cases_sha256'] = digest({k: {f: row[f] for f in ('cases', 'ignored')} for k, row in r['case_outcomes'].items()})
            for b in (p['bindings'], p['candidate_source_attempt_attestation'], p['measurement']['binding'], p['clocks']['subject_binding']):
                b['inventory']['cases_sha256'] = r['cases_sha256']
            for cell in d['manifest']['cells']:
                if cell['id'] == p['cell']: cell['routes'][p['route']]['inventory']['cases_sha256'] = r['cases_sha256']
        self.assertEqual(evaluate(d)['status'], 'SYNTHETIC_PASS')
        row = next(iter(d['packets'][1]['report']['case_outcomes'].values()))
        row['ignored'] = ['same', 'ignored']
        self.assertEqual(d['packets'][1]['report']['ignored_count'], 12)
        self.reject(d)

    def test_ordinary_outcome_and_footer_contradictions(self):
        for kind in ('failed-outcome', 'footer-failure', 'footer-bool', 'footer-null', 'outcome-missing', 'outcome-substituted', 'outcome-duplicate', 'ignored-outcome', 'footer-missing'):
            with self.subTest(kind=kind):
                d = fixture(); row = next(iter(d['packets'][0]['report']['case_outcomes'].values()))
                if kind == 'failed-outcome': row['outcomes'][1] = ['same', 'FAILED']
                if kind == 'footer-failure': row['footer_counts']['failed'] = 1
                if kind == 'footer-bool': row['footer_counts']['failed'] = False
                if kind == 'footer-null': row['footer_counts'] = None
                if kind == 'outcome-missing': del row['outcomes']
                if kind == 'outcome-substituted': row['outcomes'][1][0] = 'substituted'
                if kind == 'outcome-duplicate': row['outcomes'].append(['same', 'ok'])
                if kind == 'ignored-outcome': row['outcomes'][0][1] = 'ok'
                if kind == 'footer-missing': del row['footer_counts']
                result = self.reject(d)
                self.assertNotIn('cold', result['savings'])

    def test_storage_summary_and_redundant_inventory_consistency(self):
        for kind in ('sample-kind', 'null-kind', 'missing-kind', 'bound-claim', 'package-count', 'package-count-bool', 'package-count-missing'):
            with self.subTest(kind=kind):
                d = fixture(); r = d['packets'][0]['report']
                if kind == 'sample-kind': r['storage']['kind'] = 'sample'
                if kind == 'null-kind': r['storage']['kind'] = None
                if kind == 'missing-kind': del r['storage']['kind']
                if kind == 'bound-claim': r['storage']['peak_is_bound'] = True
                if kind == 'package-count': r['unit_count_by_package']['perl-dap'] = 2
                if kind == 'package-count-bool': r['unit_count_by_package']['perl-dap'] = True
                if kind == 'package-count-missing': del r['unit_count_by_package']['perl-dap']
                self.assertNotIn('cold', self.reject(d)['savings'])

    def test_optional_retained_attribution(self):
        for absent in (None, 'missing'):
            d = fixture()
            if absent is None: d['packets'][0]['report']['retained_by_root'] = None
            else: del d['packets'][0]['report']['retained_by_root']
            self.assertEqual(evaluate(d)['status'], 'SYNTHETIC_PASS')
        for bad in (float('nan'), float('inf'), True, None, '7000', -1, 7001):
            with self.subTest(bad=bad):
                d = fixture(); d['packets'][0]['report']['retained_by_root'][0]['allocated_bytes'] = bad
                self.assertNotIn('cold', self.reject(d)['savings'])

    def test_consistently_rehashed_nonfinite_and_noncanonical_units(self):
        import hashlib
        for kind in ('profile-nan', 'profile-inf', 'key-spacing', 'absolute-source', 'parent-source', 'dot-source'):
            with self.subTest(kind=kind):
                d = fixture()
                for p in d['packets']:
                    units = p['unit_projection']; r = p['report']; key = next(iter(units))
                    if kind == 'profile-nan': units[key]['profile']['opt_level'] = float('nan')
                    if kind == 'profile-inf': units[key]['profile']['opt_level'] = float('inf')
                    if kind in ('key-spacing', 'absolute-source', 'parent-source', 'dot-source'):
                        identity = json.loads(key)
                        if kind == 'absolute-source': identity[3] = '/outside/source.rs'
                        if kind == 'parent-source': identity[3] = '../outside/source.rs'
                        if kind == 'dot-source': identity[3] = '.'
                        replacement = json.dumps(identity) if kind == 'key-spacing' else json.dumps(identity, separators=(',', ':'))
                        units[replacement] = units.pop(key)
                        r['case_outcomes'][replacement] = r['case_outcomes'].pop(key)
                    # Deliberately use permissive JSON only in the hostile input
                    # constructor, proving consistent rehashing cannot hide NaN.
                    r['units_sha256'] = hashlib.sha256(json.dumps(units, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
                    r['cases_sha256'] = digest({k: {f: row[f] for f in ('cases', 'ignored')} for k, row in r['case_outcomes'].items()})
                    for b in (p['bindings'], p['candidate_source_attempt_attestation'], p['measurement']['binding'], p['clocks']['subject_binding']):
                        b['inventory'].update(units_sha256=r['units_sha256'], cases_sha256=r['cases_sha256'])
                    for cell in d['manifest']['cells']:
                        if cell['id'] == p['cell']:
                            cell['routes'][p['route']]['inventory'].update(units_sha256=r['units_sha256'], cases_sha256=r['cases_sha256'])
                result = self.reject(d)
                self.assertEqual(result['savings'], {})

    def test_nonfinite_json_rejected_even_in_advisory_fields(self):
        d = fixture(); d['packets'][0]['report']['phase_diagnostics'] = [{'elapsed': float('nan')}]
        self.reject(d)
        with self.assertRaises(ValueError): digest({'opt_level': float('nan')})

    def test_execution_requires_nonempty_case_map_for_both_routes(self):
        for route, index in (('ordinary', 0), ('cohort', 1)):
            for bad in ({}, [], ['unexpected'], 'unexpected', None, False, 0):
                with self.subTest(route=route, bad=bad):
                    d = fixture(); d['packets'][index]['report']['case_outcomes'] = bad
                    result = self.reject(d)
                    self.assertEqual(result['cells']['cold'][route]['execution'], 'REFUSED')
                    self.assertNotIn('cold', result['savings'])
                    self.assertEqual(result['cells']['cold'][route]['storage'], 'PASS')

    def test_execution_requires_case_map_presence_for_both_routes(self):
        for route, index in (('ordinary', 0), ('cohort', 1)):
            with self.subTest(route=route):
                d = fixture(); del d['packets'][index]['report']['case_outcomes']
                result = self.reject(d)
                self.assertEqual(result['cells']['cold'][route]['execution'], 'NOT_PROVEN')
                self.assertNotIn('cold', result['savings'])

    def test_cross_plane_case_row_negative_matrix(self):
        rows = ((None, 'REFUSED'), ({}, 'NOT_PROVEN'),
                ({'cases': None, 'ignored': []}, 'REFUSED'),
                ({'cases': [], 'ignored': None}, 'REFUSED'),
                ({'cases': [False], 'ignored': []}, 'REFUSED'),
                ({'cases': [], 'ignored': [0]}, 'REFUSED'))
        for route, index in (('ordinary', 0), ('cohort', 1)):
            for row, verdict in rows:
                with self.subTest(route=route, row=row):
                    d = fixture(); p = d['packets'][index]
                    key = next(iter(p['report']['case_outcomes']))
                    p['report']['case_outcomes'][key] = row
                    result = self.reject(d); v = result['cells']['cold'][route]
                    self.assertEqual(v['execution'], verdict)
                    self.assertEqual(v['equivalence'], verdict)
                    self.assertEqual(v['storage'], 'PASS')
                    self.assertNotIn('cold', result['savings'])

    def test_cross_plane_execution_failure_preserves_inventory_verdict(self):
        for route, index in (('ordinary', 0), ('cohort', 1)):
            for failure, execution in (('inner-receipt', 'REFUSED'), ('unsettled', 'REFUSED'), ('product', 'FAILED')):
                for inventory, equivalence in (('valid', 'NOT_PROVEN'), ('missing', 'NOT_PROVEN'), ('contradictory', 'REFUSED')):
                    with self.subTest(route=route, failure=failure, inventory=inventory):
                        d = fixture(); p = d['packets'][index]; r = p['report']
                        if failure == 'inner-receipt': p['inner_receipts_complete'] = False
                        if failure == 'unsettled': r['terminal_owner_binding']['settlement']['tree_settled'] = False
                        if failure == 'product':
                            r['storage']['command_status'] = 101
                            r['terminal_owner_binding']['product']['exit_code'] = 101
                        if inventory == 'missing': del r['units_sha256']
                        if inventory == 'contradictory':
                            next(iter(r['case_outcomes'].values()))['ignored'] = ['foreign']
                        result = self.reject(d); v = result['cells']['cold'][route]
                        self.assertEqual(v['execution'], execution)
                        self.assertEqual(v['equivalence'], equivalence)
                        self.assertEqual(v['storage'], 'PASS')
                        self.assertEqual(result['cells']['cold']['cohort' if route == 'ordinary' else 'ordinary']['equivalence'], 'NOT_PROVEN')
                        self.assertNotIn('cold', result['savings'])

    def test_legitimate_empty_harness_case_lists(self):
        d = fixture()
        for p in d['packets'][:2]:
            r = p['report']; row = next(iter(r['case_outcomes'].values()))
            row.update(cases=[], ignored=[])
            if p['route'] == 'ordinary':
                row.update(outcomes=[], failed=[], footer_counts={'passed': 0, 'failed': 0, 'ignored': 0})
            r.update(case_count=15, ignored_count=5)
            r['cases_sha256'] = digest({k: {f: v[f] for f in ('cases', 'ignored')} for k, v in r['case_outcomes'].items()})
            p['bindings']['inventory']['cases_sha256'] = r['cases_sha256']
            d['manifest']['cells'][0]['routes'][p['route']]['inventory']['cases_sha256'] = r['cases_sha256']
        result = evaluate(d)
        self.assertEqual(result['status'], 'SYNTHETIC_PASS', result)
        for v in result['cells']['cold'].values(): self.assertEqual(v['execution'], 'PASS')

    def test_negative_savings_is_valid_comparative_observation(self):
        d = fixture(); d['packets'][1]['report']['storage']['sampled_peak_allocated_bytes'] = 12000
        result = evaluate(d)
        self.assertEqual(result['status'], 'SYNTHETIC_PASS', result)
        self.assertAlmostEqual(result['savings']['cold']['sampled_peak_fraction'], -0.2)
        self.assertFalse(result['savings']['cold']['continuous_bound'])
        self.assertEqual(result['production_qualification'], 'NOT_PROVEN')


if __name__ == '__main__':
    unittest.main(verbosity=2)
