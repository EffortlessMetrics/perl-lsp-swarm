"""Invented controls only. This is not the real #17655 campaign manifest."""
import copy
import json
from evaluator import HEAD, PACKAGES, digest


def fixture():
    units = {json.dumps([p, 'synthetic-target', ['lib'], 'synthetic/' + p + '.rs'], separators=(',', ':')):
             {'features': ['default'], 'profile': {'test': True}} for p in PACKAGES}
    cases = {key: {'cases': ['ignored', 'same', 'same'], 'ignored': ['ignored']} for key in units}
    policy = {'watchdog_seconds': 1500, 'performance_seconds': 1320, 'watchdog_clock': 'route', 'performance_clock': 'mandatory-work'}
    doc = {'manifest': {'synthetic': True, 'producer_head': HEAD, 'cells': []}, 'packets': []}
    for kind in ('cold', 'warm', 'producer-edit', 'production-edit', 'edit-revert'):
        cell = {'id': kind, 'kind': kind, 'routes': {}}
        source = {'head': HEAD if kind in ('cold', 'warm', 'cancellation') else digest(kind)[:40], 'manifest_sha256': digest(kind)}
        for mode in ('ordinary', 'cohort'):
            resources = {name: '/synthetic/' + kind + '/' + mode + '/' + name for name in ('target', 'build', 'cargo_home', 'temp')}
            owner = {'lease': '/synthetic/' + kind + '/' + mode + '/lease', 'lease_identity': [1, 2], 'lease_marker': '/synthetic/' + kind + '/' + mode + '/marker', 'marker_identity': [1, 3], 'resources': resources}
            binding = {'source': source, 'attempt': {'run': 'synthetic', 'attempt': 1, 'artifact': kind + '/' + mode},
                       'toolchain': {'pin': 'synthetic-pinned'}, 'configuration': {'profile': 'test', 'features': ['default']},
                       'argv': {'effective': ['synthetic-workload', *PACKAGES], 'route': mode}, 'resources': resources, 'owner': owner,
                       'policy': policy, 'inventory': {'units_sha256': digest(units), 'cases_sha256': digest({k: {f: r[f] for f in ('cases', 'ignored')} for k, r in cases.items()})}}
            cell['routes'][mode] = copy.deepcopy(binding)
            settlement = {'tree_settled': True, 'proof': 'kernel ECHILD (__WALL)', 'errors': [], 'pending_bound_pids': [], 'lease_released': True, 'cancelled': False}
            settlement.update({k: owner[k] for k in ('lease', 'lease_identity', 'lease_marker', 'marker_identity')})
            report = {'head': source['head'], 'mode': mode, 'expected_packages': PACKAGES, 'candidate_head_matches_caller': True,
                      'qualification_status': 'NOT_PROVEN', 'qualification_missing_authority': ['candidate_source_attempt_attestation'],
                      'terminal_owner_binding': {'verified': True, 'operation': 'test' if mode == 'ordinary' else 'run',
                          'descriptor': owner, 'settlement': settlement, 'product': {'lease': owner['lease'], 'exit_code': 0},
                          'current_lease_and_marker_absent': True, 'source_attempt_binding': 'NOT_PROVEN'},
                      'outer_settlement': settlement, 'package_inventory_verified': True,
                      'unit_inventory_complete': True, 'observed_case_inventory_complete': True, 'case_inventory_complete': True,
                      'case_execution_successful': True, 'case_receipts_successful_and_settled': True, 'receipt_errors': [],
                      'measurement_valid': True, 'observed_reserve_preserved': True,
                      'storage': {'kind': 'summary', 'peak_is_bound': False, 'sample_count': 3, 'command_elapsed_seconds': 100, 'elapsed_seconds': 101, 'command_status': 0, 'measurement_complete': True, 'worker_failure': None,
                          'observed_reserve_preserved': True, 'sampled_peak_allocated_bytes': 10000 if mode == 'ordinary' else 8000, 'retained_allocated_bytes': 7000},
                      'retained_by_root': [{'root': resources['target'], 'allocated_bytes': 7000}], 'unit_count_by_package': {name: 1 for name in PACKAGES},
                      'minimum_free_bytes': {'synthetic-fs': 9000}, 'max_sample_gap_seconds': 1, 'vanished_entries': 0,
                      'unit_count': 6, 'observed_case_targets': 6, 'case_count': 18, 'ignored_count': 6,
                      'units_sha256': digest(units), 'cases_sha256': digest({k: {f: r[f] for f in ('cases', 'ignored')} for k, r in cases.items()}), 'case_outcomes': cases, 'phase_diagnostics': []}
            if mode == 'ordinary':
                report['case_outcomes'] = {k: dict(row, failed=[], outcomes=[['ignored', 'ignored'], ['same', 'ok'], ['same', 'ok']],
                    footer_status='ok', footer_counts={'passed': 2, 'failed': 0, 'ignored': 1}, footer_consistent=True) for k, row in cases.items()}
            report['storage']['cancelled_signals'] = []
            doc['packets'].append(copy.deepcopy({'synthetic': True, 'producer_head': HEAD, 'cell': kind, 'route': mode,
                'bindings': binding, 'candidate_source_attempt_attestation': binding, 'report': report,
                'unit_projection': units, 'component_qualified': True, 'inner_receipts_complete': True,
                'measurement': {'binding': binding, 'resources': resources, 'observer_alive_through_completion': True,
                    'whole_envelope_through_packaging': True, 'accounting_complete': True, 'limitations_recorded': True,
                    'sample_count': 3, 'maximum_gap_seconds': 2, 'basis_verified': True, 'supported_reserve_bytes': {'synthetic-fs': 8000}},
                'clocks': {'subject_binding': binding, 'binding': policy, 'includes_bootstrap_and_setup': True, 'route': 100, 'mandatory-work': 90},
                'comparison_basis': {'metric': 'sampled whole-envelope allocated peak', 'conditioning': kind, 'completed_work': True}}))
        doc['manifest']['cells'].append(cell)
    return doc


if __name__ == '__main__':
    print(json.dumps(fixture(), sort_keys=True, indent=2))
