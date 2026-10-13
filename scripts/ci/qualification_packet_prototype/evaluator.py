"""Synthetic qualification consumer prototype. No execution or lifecycle authority."""
import argparse
import collections
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import re

HEAD = '540201d5dad7b7622628c61e463e4a366154b0b4'
PACKAGES = ['perl-dap', 'perl-lsp-rs', 'perl-parser-core', 'perl-semantic-analyzer', 'perl-workspace', 'xtask']
PLANES = ('component', 'execution', 'equivalence', 'storage', 'reserve', 'time', 'closure', 'savings')
BINDINGS = ('source', 'attempt', 'toolchain', 'configuration', 'argv', 'resources', 'owner', 'policy', 'inventory')


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()).hexdigest()


def number(value, integer=False):
    return type(value) in ((int,) if integer else (int, float)) and math.isfinite(value) and value >= 0


class EvidenceError(ValueError):
    pass


def require(condition, field):
    if not condition:
        raise EvidenceError(field)


def identical(left, right):
    return json.dumps(left, sort_keys=True, allow_nan=False) == json.dumps(right, sort_keys=True, allow_nan=False)


def closed(receipt):
    return (isinstance(receipt, dict) and receipt.get('tree_settled') is True
            and receipt.get('proof') == 'kernel ECHILD (__WALL)'
            and receipt.get('errors') == [] and receipt.get('pending_bound_pids') == []
            and receipt.get('lease_released') is True)


def ordinary_outcome_consistent(row):
    """Reconcile the pinned reporter's exported projections, never reparse libtest."""
    outcomes = row['outcomes']
    require(isinstance(outcomes, list), 'ordinary outcomes list')
    for outcome in outcomes:
        require(isinstance(outcome, (list, tuple)) and len(outcome) == 2
                and isinstance(outcome[0], str) and bool(outcome[0])
                and outcome[1] in ('ok', 'FAILED', 'ignored'), 'ordinary outcome shape/status')
    for field, status in (('cases', None), ('ignored', 'ignored'), ('failed', 'FAILED')):
        projection = row[field]
        require(isinstance(projection, list) and all(isinstance(x, str) for x in projection), 'ordinary ' + field + ' list')
        observed = [name for name, outcome in outcomes if status is None or outcome == status]
        require(collections.Counter(projection) == collections.Counter(observed), 'ordinary ' + field + ' outcome multiplicity')
    counts = {'passed': sum(status == 'ok' for _, status in outcomes),
              'failed': sum(status == 'FAILED' for _, status in outcomes),
              'ignored': sum(status == 'ignored' for _, status in outcomes)}
    footer = row['footer_counts']
    require(isinstance(footer, dict) and set(footer) == set(counts)
            and all(number(value, True) for value in footer.values()), 'ordinary typed footer counts')
    require(identical(footer, counts) and row['footer_consistent'] is True, 'ordinary footer/outcome counts')
    require(row['failed'] == [] and counts['failed'] == 0 and row['footer_status'] == 'ok', 'ordinary failed outcomes')


def evaluate(document):
    """Evaluate only an explicitly synthetic scope; missing facts never default green."""
    result = {'scope': 'synthetic-only', 'production_qualification': 'NOT_PROVEN',
              'status': 'NOT_PROVEN', 'cells': {}, 'reasons': [], 'savings': {}}
    try:
        require(isinstance(document, dict), 'packet object')
        # JSON itself cannot contain NaN or Infinity, including a hashed profile
        # or an advisory field. No nonfinite value becomes valid by rehashing it.
        json.dumps(document, allow_nan=False)
        manifest = document['manifest']
        require(manifest.get('synthetic') is True and manifest.get('producer_head') == HEAD, 'synthetic manifest/producer pin')
        requested = manifest['cells']
        require(isinstance(requested, list) and bool(requested), 'nonempty requested cells')
        expected = {}
        for cell in requested:
            require(isinstance(cell, dict) and isinstance(cell.get('id'), str) and bool(cell['id']), 'cell identity')
            require(cell['id'] not in expected, 'duplicate requested cell')
            require(cell.get('kind') in ('cold', 'warm', 'producer-edit', 'production-edit', 'edit-revert', 'cancellation'), 'cell kind')
            require(set(cell['routes']) == {'ordinary', 'cohort'}, 'paired routes')
            for route in cell['routes'].values():
                require(set(route) == set(BINDINGS), 'requested binding fields')
                require(all(isinstance(route[k], dict) and bool(route[k]) for k in BINDINGS), 'nonempty requested bindings')
            # Route argv/resources/owners may differ; intended subject cannot.
            for field in ('source', 'toolchain', 'configuration', 'inventory', 'policy'):
                require(identical(cell['routes']['ordinary'][field], cell['routes']['cohort'][field]), 'paired ' + field)
            require(identical(cell['routes']['ordinary']['argv']['effective'], cell['routes']['cohort']['argv']['effective']), 'paired intended workload argv')
            for route in cell['routes'].values():
                require(set(route['source']) == {'head', 'manifest_sha256'}, 'complete source subject')
                require(isinstance(route['source']['head'], str) and re.fullmatch(r'[0-9a-f]{40}', route['source']['head']) is not None, 'source head type')
                require(isinstance(route['source']['manifest_sha256'], str) and re.fullmatch(r'[0-9a-f]{64}', route['source']['manifest_sha256']) is not None, 'source manifest digest type')
                require(set(route['resources']) == {'target', 'build', 'cargo_home', 'temp'}, 'complete resource envelope')
                require(all(isinstance(x, str) and Path(x).is_absolute() for x in route['resources'].values()), 'absolute resource identity')
                require(set(route['toolchain']) == {'pin'} and isinstance(route['toolchain']['pin'], str) and bool(route['toolchain']['pin']), 'complete typed toolchain pin')
                config = route['configuration']
                require(set(config) == {'profile', 'features'} and config['profile'] == 'test', 'complete test profile configuration')
                require(isinstance(config['features'], list) and all(isinstance(x, str) and bool(x) for x in config['features']), 'typed configuration features')
                require(config['features'] == sorted(set(config['features'])), 'canonical configuration features')
                require(set(route['attempt']) == {'run', 'attempt', 'artifact'}, 'complete attempt binding')
                require(type(route['attempt']['attempt']) is int and route['attempt']['attempt'] > 0, 'typed attempt')
                require(all(isinstance(route['attempt'][k], str) and bool(route['attempt'][k]) for k in ('run', 'artifact')), 'attempt identity')
                require(isinstance(route['argv']['effective'], list) and bool(route['argv']['effective']) and all(isinstance(x, str) for x in route['argv']['effective']), 'effective argv')
                require(set(route['argv']) == {'effective', 'route'} and route['argv']['route'] in ('ordinary', 'cohort'), 'complete typed route argv')
                require(set(route['owner']) == {'lease', 'lease_identity', 'lease_marker', 'marker_identity', 'resources'}, 'complete original owner binding')
                require(all(isinstance(route['owner'][k], str) and Path(route['owner'][k]).is_absolute() for k in ('lease', 'lease_marker')), 'original owner paths')
            for name, route in cell['routes'].items():
                require(route['argv']['route'] == name, 'route argv identity')
            expected[cell['id']] = cell
        packets = document['packets']
        require(isinstance(packets, list), 'packets list')
        grouped = collections.defaultdict(list)
        for packet in packets:
            require(isinstance(packet, dict) and packet.get('cell') in expected and packet.get('route') in ('ordinary', 'cohort'), 'unexpected packet')
            grouped[(packet['cell'], packet['route'])].append(packet)
        for cell_id, cell in expected.items():
            verdicts = {}
            result['cells'][cell_id] = verdicts
            for route in ('ordinary', 'cohort'):
                v = {plane: 'NOT_PROVEN' for plane in PLANES}
                verdicts[route] = v
                rows = grouped[(cell_id, route)]
                if len(rows) != 1:
                    result['reasons'].append(f'{cell_id}/{route}: missing or duplicate packet')
                    continue
                p = rows[0]
                def plane(name, check):
                    try:
                        check()
                        v[name] = 'PASS'
                    except (EvidenceError, KeyError, TypeError, ValueError, AttributeError, OverflowError) as error:
                        v[name] = 'NOT_PROVEN' if isinstance(error, KeyError) else 'REFUSED'
                        result['reasons'].append(f'{cell_id}/{route}/{name}: {error}')
                try:
                    require(p.get('synthetic') is True, 'synthetic packet')
                    require(p['producer_head'] == HEAD, 'producer contract pin')
                    require(identical(p['bindings'], cell['routes'][route]), 'source/attempt/resource/policy/inventory binding')
                    require(identical(p['candidate_source_attempt_attestation'], p['bindings']), 'candidate_source_attempt_attestation')
                    r = p['report']
                    require(r['head'] == p['bindings']['source']['head'] and r['mode'] == route, 'report source/mode')
                    require(r['candidate_head_matches_caller'] is True, 'caller head')
                except (EvidenceError, KeyError, TypeError, ValueError, AttributeError, OverflowError) as error:
                    result['reasons'].append(f'{cell_id}/{route}: {error}')
                    continue
                plane('component', lambda: require(p['component_qualified'] is True, 'component evidence'))
                def closure():
                    b = r['terminal_owner_binding']
                    require(b['verified'] is True and b['operation'] == ('test' if route == 'ordinary' else 'run'), 'terminal operation')
                    require(identical(b['descriptor'], p['bindings']['owner']), 'terminal owner')
                    require(identical(b['descriptor']['resources'], p['bindings']['resources']), 'owner resources')
                    for key in ('lease', 'lease_identity', 'lease_marker', 'marker_identity'):
                        require(identical(b['descriptor'][key], b['settlement'][key]), 'original owner ' + key)
                    for key in ('lease_identity', 'marker_identity'):
                        identity = b['descriptor'][key]
                        require(isinstance(identity, list) and len(identity) == 2 and all(number(x, True) for x in identity), 'typed owner inode')
                    require(closed(b['settlement']) and identical(r['outer_settlement'], b['settlement']), 'terminal closure')
                    require(b['current_lease_and_marker_absent'] is True, 'lease absence')
                    require(b['product']['lease'] == b['descriptor']['lease'], 'product owner')
                    require(type(b['product']['exit_code']) is int and b['product']['exit_code'] == r['storage']['command_status'], 'product status')
                plane('closure', closure)
                def execution():
                    status = r['storage']['command_status']
                    require(type(status) is int, 'typed command status')
                    if status != 0:
                        v['execution'] = 'REFUSED' if status == 75 else 'FAILED'
                        raise EvidenceError('nonzero command status ' + str(status))
                    require(v['closure'] == 'PASS', 'closure unproven')
                    require(p['inner_receipts_complete'] is True, 'missing inner receipt')
                    require(r['terminal_owner_binding']['settlement']['cancelled'] is False, 'cancelled owner')
                    require(r['expected_packages'] == PACKAGES and r['package_inventory_verified'] is True, 'package inventory')
                    for field in ('unit_inventory_complete', 'observed_case_inventory_complete', 'case_inventory_complete', 'case_execution_successful', 'case_receipts_successful_and_settled'):
                        require(r[field] is True, field)
                    require(r['receipt_errors'] == [], 'receipt errors')
                    outcomes = r['case_outcomes']
                    require(isinstance(outcomes, dict) and bool(outcomes), 'nonempty case outcomes map')
                    for row in outcomes.values():
                        require(isinstance(row, dict), 'case outcome row object')
                        require(isinstance(row['cases'], list) and isinstance(row['ignored'], list), 'case outcome row lists')
                        require(all(isinstance(name, str) for name in row['cases'] + row['ignored']), 'case outcome row identities')
                        if route == 'ordinary':
                            ordinary_outcome_consistent(row)
                plane('execution', execution)
                status = r.get('storage', {}).get('command_status')
                if type(status) is int and status != 0:
                    v['execution'] = 'REFUSED' if status == 75 else 'FAILED'
                def inventory():
                    units = p['unit_projection']
                    cases = r['case_outcomes']
                    require(isinstance(units, dict) and bool(units) and set(cases) == set(units), 'unit/case population')
                    identities = []
                    for key in units:
                        identity = json.loads(key)
                        require(isinstance(identity, list) and len(identity) == 4, 'canonical unit identity shape')
                        require(all(isinstance(identity[i], str) and bool(identity[i]) for i in (0, 1, 3)) and isinstance(identity[2], list) and bool(identity[2]) and all(isinstance(x, str) for x in identity[2]), 'canonical unit identity types')
                        require(key == json.dumps(identity, separators=(',', ':')), 'canonical serialized unit key')
                        source_path = PurePosixPath(identity[3])
                        require(not source_path.is_absolute() and source_path.as_posix() == identity[3]
                                and '..' not in source_path.parts and bool(source_path.parts), 'relative canonical unit source path')
                        identities.append(identity[0])
                    require(set(identities) == set(PACKAGES), 'unit packages')
                    require(identical(r['unit_count_by_package'], dict(collections.Counter(identities))), 'unit count by package contradicts projection')
                    for key, unit in units.items():
                        require(isinstance(unit.get('features'), list) and isinstance(unit.get('profile'), dict), 'unit features/profile')
                        require(unit['features'] == p['bindings']['configuration']['features'] and unit['profile'].get('test') is True, 'synthetic unit configuration applicability')
                        row = cases[key]
                        require(isinstance(row['cases'], list) and isinstance(row['ignored'], list), 'case lists')
                        require(all(isinstance(x, str) for x in row['cases'] + row['ignored']), 'case identities')
                        require(collections.Counter(row['ignored']) <= collections.Counter(row['cases']), 'ignored multiplicity')
                    projection = {key: {field: row[field] for field in ('cases', 'ignored')} for key, row in cases.items()}
                    require(r['units_sha256'] == digest(units) == p['bindings']['inventory']['units_sha256'], 'unit hash')
                    require(r['cases_sha256'] == digest(projection) == p['bindings']['inventory']['cases_sha256'], 'case hash')
                    for field, count in (('unit_count', len(units)), ('observed_case_targets', len(cases)), ('case_count', sum(len(x['cases']) for x in cases.values())), ('ignored_count', sum(len(x['ignored']) for x in cases.values()))):
                        require(type(r[field]) is int and r[field] == count, field)
                plane('equivalence', inventory)
                def storage():
                    s = r['storage']; m = p['measurement']
                    require(s['kind'] == 'summary' and s['peak_is_bound'] is False, 'terminal sampled storage summary')
                    require(r['measurement_valid'] is True and s['measurement_complete'] is True, 'measurement validity')
                    require(s['worker_failure'] is None and m['observer_alive_through_completion'] is True, 'observer failure')
                    require(identical(m['binding'], p['bindings']), 'measurement source/attempt binding')
                    require(m['resources'] == p['bindings']['resources'] and m['whole_envelope_through_packaging'] is True, 'measurement envelope')
                    require(m['accounting_complete'] is True and m['limitations_recorded'] is True, 'accounting/limitations')
                    require(number(s['sampled_peak_allocated_bytes'], True) and number(s['retained_allocated_bytes'], True) and s['retained_allocated_bytes'] <= s['sampled_peak_allocated_bytes'], 'typed allocated bytes/retained peak')
                    # This attribution is diagnostic, not a savings denominator or
                    # reserve authority. Absence is permitted; present facts must
                    # not contradict the same retained observation.
                    retained = r.get('retained_by_root')
                    if retained is not None:
                        require(isinstance(retained, list), 'retained attribution list')
                        for row in retained:
                            require(isinstance(row, dict) and isinstance(row.get('root'), str)
                                    and Path(row['root']).is_absolute() and number(row.get('allocated_bytes'), True), 'typed retained attribution')
                        require(sum(row['allocated_bytes'] for row in retained) == s['retained_allocated_bytes'], 'retained attribution total')
                    require(number(r['max_sample_gap_seconds']) and number(m['maximum_gap_seconds']) and r['max_sample_gap_seconds'] <= m['maximum_gap_seconds'], 'sample gap')
                    require(type(m['sample_count']) is int and m['sample_count'] >= 2, 'sample count')
                    require(type(s['sample_count']) is int and s['sample_count'] == m['sample_count'], 'summary sample count')
                    require(number(s['command_elapsed_seconds']) and number(s['elapsed_seconds']) and s['elapsed_seconds'] >= s['command_elapsed_seconds'], 'summary elapsed clocks')
                    require(number(r['vanished_entries'], True) and r['vanished_entries'] == 0, 'vanished entries')
                plane('storage', storage)
                def reserve():
                    m = p['measurement']
                    require(v['storage'] == 'PASS', 'valid measurement required')
                    require(r['observed_reserve_preserved'] is True and r['storage']['observed_reserve_preserved'] is True, 'reserve loss')
                    require(m['basis_verified'] is True and bool(m['supported_reserve_bytes']), 'supported reserve basis')
                    require(set(r['minimum_free_bytes']) == set(m['supported_reserve_bytes']), 'filesystem population')
                    for fs, reserve_bytes in m['supported_reserve_bytes'].items():
                        require(number(reserve_bytes, True) and number(r['minimum_free_bytes'][fs], True) and r['minimum_free_bytes'][fs] >= reserve_bytes, 'filesystem reserve ' + fs)
                plane('reserve', reserve)
                def timing():
                    policy = p['bindings']['policy']; clocks = p['clocks']
                    require(identical(policy, {'watchdog_seconds': 1500, 'performance_seconds': 1320, 'watchdog_clock': 'route', 'performance_clock': 'mandatory-work'}), 'policy clock contract')
                    require(identical(clocks['subject_binding'], p['bindings']), 'clock source/attempt binding')
                    require(clocks['binding'] == policy and clocks['includes_bootstrap_and_setup'] is True, 'clock/accounting binding')
                    for clock, limit in (('route', 1500), ('mandatory-work', 1320)):
                        require(number(clocks[clock]) and clocks[clock] <= limit, clock + ' budget')
                    require(clocks['mandatory-work'] <= clocks['route'], 'clock ordering')
                    require(number(r['storage']['command_elapsed_seconds']) and clocks['route'] == r['storage']['command_elapsed_seconds'], 'producer route clock consistency')
                plane('time', timing)
            ordinary, cohort = verdicts['ordinary'], verdicts['cohort']
            if all(v['execution'] == 'PASS' and v['equivalence'] == 'PASS' for v in (ordinary, cohort)):
                # The manifest hash bindings are identical within a cell, so both
                # validated projections match. Counts alone never establish this.
                if all(v['component'] == 'PASS' and v['closure'] == 'PASS' and v['storage'] == 'PASS' and v['reserve'] == 'PASS' and v['time'] == 'PASS' for v in (ordinary, cohort)):
                    a = grouped[(cell_id, 'ordinary')][0]; b = grouped[(cell_id, 'cohort')][0]
                    denominator = a['report']['storage']['sampled_peak_allocated_bytes']
                    if denominator > 0 and identical(a['comparison_basis'], b['comparison_basis']) and identical(a['comparison_basis'], {'metric': 'sampled whole-envelope allocated peak', 'conditioning': cell['kind'], 'completed_work': True}):
                        result['savings'][cell_id] = {'sampled_peak_fraction': (denominator - b['report']['storage']['sampled_peak_allocated_bytes']) / denominator, 'continuous_bound': False}
                        ordinary['savings'] = cohort['savings'] = 'PASS'
            if cell_id not in result['savings']:
                result['reasons'].append(f'{cell_id}/savings: complete equivalent measured reference, positive denominator and matching basis required')
            if not all(v['execution'] == 'PASS' for v in (ordinary, cohort)):
                for v in (ordinary, cohort):
                    if v['equivalence'] == 'PASS':
                        v['equivalence'] = 'NOT_PROVEN'
        if all(v[plane] == 'PASS' for cell in result['cells'].values() for v in cell.values() for plane in PLANES):
            result['status'] = 'SYNTHETIC_PASS'
    except (EvidenceError, KeyError, TypeError, ValueError, AttributeError, OverflowError) as error:
        result['reasons'].append('packet: ' + str(error))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('packet', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    try:
        result = evaluate(json.loads(args.packet.read_text()))
    except (OSError, ValueError) as error:
        result = {'status': 'NOT_PROVEN', 'production_qualification': 'NOT_PROVEN', 'reasons': [str(error)]}
    print(json.dumps(result, sort_keys=True, allow_nan=False))
    return 1 if args.check and result['status'] != 'SYNTHETIC_PASS' else 0


if __name__ == '__main__':
    raise SystemExit(main())
