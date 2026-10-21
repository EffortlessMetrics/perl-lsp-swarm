#!/usr/bin/env python3
"""Read persisted campaign evidence; no execution, admission or cleanup authority."""
import argparse
import collections
import hashlib
import json
import os
import socket
import sys
from pathlib import Path
import re

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cargo_cohort import Denied, libtest_text_without_rpc_frames

PACKAGES = ['perl-dap', 'perl-lsp-rs', 'perl-parser-core', 'perl-semantic-analyzer', 'perl-workspace', 'xtask']


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def messages(path, framed=False):
    rows = []
    text = libtest_text_without_rpc_frames(path.read_bytes()) if framed else path.read_text()
    for line in text.splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue  # Libtest's ordinary stdout shares this stream with Cargo JSON.
        if isinstance(row, dict):
            rows.append(row)
    return rows


def package_inventory(metadata):
    if not isinstance(metadata, dict) or not isinstance(metadata.get('packages'), list) or not isinstance(metadata.get('workspace_members'), list):
        raise ValueError('invalid captured Cargo package inventory')
    members = metadata['workspace_members']
    if any(not isinstance(item, str) or not item for item in members) or len(set(members)) != len(members):
        raise ValueError('invalid captured Cargo member identities')
    result = {}
    for package in metadata['packages']:
        if not isinstance(package, dict) or not isinstance(package.get('id'), str) or not package['id'] or not isinstance(package.get('name'), str) or not package['name']:
            raise ValueError('invalid captured Cargo package identity')
        if package['id'] in members:
            if package['id'] in result:
                raise ValueError('duplicate captured Cargo package identity')
            result[package['id']] = package['name']
    if set(result) != set(members) or not isinstance(metadata.get('workspace_root'), str) or not Path(metadata['workspace_root']).is_absolute():
        raise ValueError('incomplete captured Cargo package inventory')
    return result, Path(metadata['workspace_root'])


def units(rows, root, metadata=None):
    result = {}
    names, root = package_inventory(metadata) if metadata is not None else ({}, root)
    for row in rows:
        if row.get('reason') != 'compiler-artifact' or row.get('profile', {}).get('test') is not True:
            continue
        target = row['target']
        relative = Path(target['src_path']).relative_to(root).as_posix()
        package_id = row.get('package_id')
        if not isinstance(package_id, str) or not package_id:
            raise ValueError('compiler artifact omitted actual Cargo package_id')
        # Missing metadata preserves opaque observed identities, never a source-
        # path guess. Such observations cannot satisfy the selected-package set.
        package = names.get(package_id, package_id)
        if metadata is not None and package_id not in names:
            raise ValueError('compiler artifact absent from captured Cargo metadata')
        if metadata is not None and package not in PACKAGES:
            raise ValueError('unexpected package test unit: ' + package)
        key = json.dumps([package, target['name'], target['kind'], relative], separators=(',', ':'))
        value = {'features': sorted(row['features']), 'profile': row['profile']}
        if key in result and result[key] != value:
            raise ValueError('conflicting feature/profile units for ' + key)
        result[key] = value
    return result


def ordinary_cases(stdout, stderr, rows, root, metadata=None):
    stdout = libtest_text_without_rpc_frames(stdout.encode('utf-8') if isinstance(stdout, str) else stdout)
    by_executable = {}
    for row in rows:
        if row.get('executable') and row.get('profile', {}).get('test') is True:
            for key in units([row], root, metadata):
                by_executable[row['executable']] = key
    clean = re.sub(r'\x1b\[[0-9;]*m', '', stderr)
    paths = re.findall(r'^\s*Running .+ \((.+)\)$', clean, re.MULTILINE)
    keys = [by_executable[path] for path in paths if path in by_executable]
    blocks = []
    current = None
    count = None
    for line in stdout.splitlines():
        start = re.fullmatch(r'running (\d+) tests?', line)
        if start:
            if current is not None:
                raise ValueError('ambiguous nested harness stdout')
            current, count = [], int(start[1])
        if current is not None:
            case = re.fullmatch(r'test (.+?)(?: - should panic)? \.\.\. (ok|FAILED|ignored(?:, .*)?)', line)
            if case:
                current.append((case[1], 'ignored' if case[2].startswith('ignored') else case[2]))
            if line.startswith('test result:'):
                if len(current) != count:
                    raise ValueError('incomplete or duplicate ordinary case output')
                footer = re.match(r'^test result: (ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored(?:;|$)', line)
                footer_counts = ({'passed': int(footer[2]), 'failed': int(footer[3]), 'ignored': int(footer[4])} if footer else None)
                observed_counts = {'passed': sum(status == 'ok' for _, status in current),
                                   'failed': sum(status == 'FAILED' for _, status in current),
                                   'ignored': sum(status == 'ignored' for _, status in current)}
                blocks.append({'cases': sorted(name for name, _ in current),
                               'ignored': sorted(name for name, status in current if status == 'ignored'),
                               'failed': sorted(name for name, status in current if status == 'FAILED'),
                               'outcomes': sorted(current),
                               'footer_status': footer[1] if footer else 'NOT_PROVEN',
                               'footer_counts': footer_counts,
                               'footer_consistent': footer_counts == observed_counts})
                current = None
    if current is not None or len(blocks) != len(keys) or len(set(keys)) != len(keys):
        raise ValueError('ordinary harness boundaries incomplete or ambiguous')
    return dict(zip(keys, blocks))


def closed_receipt(receipt):
    return (isinstance(receipt, dict) and receipt.get('tree_settled') is True
            and receipt.get('proof') == 'kernel ECHILD (__WALL)'
            and receipt.get('errors') == [] and receipt.get('pending_bound_pids') == [])


def successful_receipt(receipt):
    return closed_receipt(receipt) and receipt.get('cancelled') is False


def validate_owner_identity(descriptor, receipt, product):
    if not isinstance(descriptor, dict) or not isinstance(receipt, dict):
        raise ValueError('owner descriptor or settlement missing')
    for key in ('lease', 'lease_identity', 'lease_marker', 'marker_identity'):
        if key not in descriptor or descriptor[key] != receipt.get(key):
            raise ValueError('original owner lease identity mismatch')
    for key in ('lease', 'lease_marker'):
        if not isinstance(descriptor[key], str) or not Path(descriptor[key]).is_absolute():
            raise ValueError('invalid original owner lease path')
    for key in ('lease_identity', 'marker_identity'):
        value = descriptor[key]
        if not isinstance(value, list) or len(value) != 2 or any(type(item) is not int or item < 0 for item in value):
            raise ValueError('invalid original owner inode identity')
    if product is not None and (not isinstance(product, dict) or product.get('lease') != descriptor['lease'] or type(product.get('exit_code')) is not int):
        raise ValueError('product belongs to another owner or has invalid status')


def argv_digest(argv):
    return hashlib.sha256(json.dumps(argv, ensure_ascii=True, separators=(',', ':')).encode()).hexdigest()


def terminal_owner_binding(stderr, mode, root, cohort_path, resources=None, allow_cancelled_build=False):
    """Reconcile the last started owner episode; never borrow an older receipt.

    Existing records bind operation/argv/resources and the original lease epoch.
    They do not attest candidate source or run/attempt identity.
    """
    package_args = [item for package in PACKAGES for item in ('-p', package)]
    expected = {'test': ['test', '--locked', '--tests', *package_args, '--message-format=json-render-diagnostics']}
    if mode == 'cohort':
        package, binary = ('routed-test-cohorts-bootstrap', 'routed-test-cohorts-bootstrap') if (root / 'crates/routed-test-cohorts-bootstrap/Cargo.toml').is_file() else ('xtask', 'routed-test-cohorts')
        expected = {'run': ['run', '--locked', '-p', package, '--bin', binary, '--', *package_args, '--evidence-dir', str(cohort_path)]}
    elif allow_cancelled_build:
        expected['build'] = ['build', '-p', 'perllsp', '--locked']
    episode = None
    try:
        for line in stderr.splitlines():
            if line.startswith('cargo-admitted resources: '):
                if episode is not None:
                    prior, prior_receipt, prior_product = episode['descriptor'], episode['settlement'], episode['product']
                    validate_owner_identity(prior, prior_receipt, prior_product)
                    if (mode != 'ordinary' or not isinstance(prior_receipt, dict)
                            or prior.get('scope', {}).get('operation') != 'build'
                            or prior['scope'].get('cargo_argv_sha256') != argv_digest(['build', '-p', 'perllsp', '--locked'])
                            or prior.get('worktree') != str(root)
                            or prior['scope'].get('worktree') != str(root) or prior['scope'].get('working_directory') != str(root)
                            or prior.get('resources') != prior['scope'].get('resources')
                            or (resources is not None and prior.get('resources') != resources)
                            or prior_receipt.get('tree_settled') is not True
                            or prior_receipt.get('proof') != 'kernel ECHILD (__WALL)'
                            or prior_receipt.get('errors') != [] or prior_receipt.get('pending_bound_pids') != []
                            or prior_receipt.get('lease_released') is not True or prior_receipt.get('cancelled') is not False
                            or any(key not in prior or prior[key] != prior_receipt.get(key) for key in ('lease', 'lease_identity', 'lease_marker', 'marker_identity'))
                            or not isinstance(prior_product, dict) or prior_product.get('exit_code') != 0
                            or prior_product.get('lease') != prior.get('lease')):
                        raise ValueError('prior owner episode missing, foreign or unsettled')
                descriptor = json.loads(line[len('cargo-admitted resources: '):])
                if not isinstance(descriptor, dict) or not isinstance(descriptor.get('scope'), dict):
                    raise ValueError('invalid original owner descriptor')
                if episode is not None and descriptor['scope'].get('operation') != 'test':
                    raise ValueError('unexpected operation after original build')
                episode = {'descriptor': descriptor, 'settlement': None, 'product': None}
            elif line.startswith('cargo-admitted Cargo product: '):
                if episode is None or episode['product'] is not None:
                    raise ValueError('product has no unique original owner descriptor')
                episode['product'] = json.loads(line[len('cargo-admitted Cargo product: '):])
            elif line.startswith('cargo-admitted Cargo settlement: '):
                if episode is None or episode['settlement'] is not None:
                    raise ValueError('settlement has no unique original owner episode')
                episode['settlement'] = json.loads(line[len('cargo-admitted Cargo settlement: '):])
        if episode is None:
            raise ValueError('terminal operation owner descriptor missing')
        descriptor, receipt = episode['descriptor'], episode['settlement']
        if not isinstance(descriptor, dict) or not isinstance(receipt, dict):
            raise ValueError('terminal operation settlement missing')
        scope = descriptor.get('scope', {})
        operation = scope.get('operation')
        if operation not in expected or scope.get('cargo_argv_sha256') != argv_digest(expected[operation]):
            raise ValueError('terminal operation/effective argv does not match original caller')
        if descriptor.get('worktree') != str(root) or scope.get('worktree') != str(root) or scope.get('working_directory') != str(root):
            raise ValueError('terminal operation worktree mismatch')
        paths = descriptor.get('resources')
        if not isinstance(paths, dict) or set(paths) != {'target', 'build', 'cargo_home', 'temp'} or any(not isinstance(path, str) or not Path(path).is_absolute() for path in paths.values()) or scope.get('resources') != paths:
            raise ValueError('invalid terminal operation resource binding')
        if resources is not None and paths != resources:
            raise ValueError('terminal operation resources mismatch')
        validate_owner_identity(descriptor, receipt, episode['product'])
        if (receipt.get('tree_settled') is not True or receipt.get('proof') != 'kernel ECHILD (__WALL)' or receipt.get('errors') != [] or receipt.get('pending_bound_pids') != [] or receipt.get('lease_released') is not True):
            raise ValueError('terminal owner closure/release unproven')
        product = episode['product']
        current_absence = not any(os.path.lexists(descriptor[key]) for key in ('lease', 'lease_marker'))
        local_host = scope.get('hostname') == socket.gethostname()
        return {'verified': True, 'operation': operation, 'descriptor': descriptor, 'settlement': receipt, 'product': product,
                'current_lease_and_marker_absent': current_absence, 'local_owner_host': local_host,
                'retry_eligible': receipt.get('cancelled') is True and current_absence and local_host,
                'source_attempt_binding': 'NOT_PROVEN'}
    except (ValueError, TypeError, KeyError, json.JSONDecodeError) as error:
        return {'verified': False, 'retry_eligible': False, 'reason': str(error), 'source_attempt_binding': 'NOT_PROVEN'}


def retry_eligible(evidence, mode, root, expected_head):
    """Only the original single timeout retry; no execution authority."""
    try:
        if not re.fullmatch(r'[0-9a-f]{40}', expected_head) or (evidence / 'head.txt').read_text().strip() != expected_head:
            return False
        if (evidence / 'status.txt').read_text().strip() != '124':
            return False
        observations = [json.loads(line) for line in (evidence / 'storage.jsonl').read_text().splitlines()]
        storage = observations[-1]
        if (not isinstance(storage, dict) or storage.get('kind') != 'summary'
                or any(not isinstance(row, dict) or row.get('valid') is False for row in observations)
                or storage.get('worker_failure') is not None):
            return False
        if storage.get('command_status') != 124 or storage.get('measurement_complete') is not True or storage.get('observed_reserve_preserved') is not True:
            return False
        logs = evidence / 'logs' if (evidence / 'logs').exists() else evidence
        resources = json.loads((evidence / 'resources.json').read_text())
        binding = terminal_owner_binding((logs / 'route.stderr').read_text(), mode, root, evidence / 'cohort', resources, allow_cancelled_build=True)
        return binding['verified'] and binding['retry_eligible']
    except (OSError, ValueError, TypeError, IndexError):
        return False


def captured_metadata(evidence, mode):
    path = evidence / 'cargo-metadata.json'
    if path.exists():
        return json.loads(path.read_text())
    if mode == 'cohort':
        candidates = []
        for request_path in sorted((evidence / 'cohort').glob('*-request.json')):
            request = json.loads(request_path.read_text())
            producer = request.get('producer', [])
            if request.get('operation') == 'observe' and producer[-5:] == ['metadata', '--locked', '--no-deps', '--format-version', '1']:
                candidates.append(request_path.parent / Path(request['evidence_path']).name)
        if len(candidates) == 1:
            return json.loads(candidates[0].read_text())
        if candidates:
            raise ValueError('ambiguous captured Cargo metadata')
    return None



def phase_diagnostics(request_path, outer_descriptor=None):
    """Validate observational progress, never completion/cleanup authority."""
    result = {'authority': 'observations only; NOT_PROVEN source/attempt/settlement', 'available': False}
    try:
        request_bytes = request_path.read_bytes()
        request = json.loads(request_bytes)
        path = Path(request['evidence_path']).with_name(Path(request['evidence_path']).name + '.phases.jsonl')
        # A downloaded archive may relocate the entire evidence directory.
        path = request_path.parent / path.name
        result['path'] = str(path)
        if path.stat().st_size > 8 * 1024 * 1024:
            raise ValueError('phase journal byte bound exceeded')
        raw = path.read_bytes()
        result.update(raw_sha256=hashlib.sha256(raw).hexdigest(), raw_bytes=len(raw))
        split = raw.splitlines(keepends=True)
        truncated = bool(split and not split[-1].endswith(b'\n'))
        if truncated: split.pop()
        if not split or len(split) > 65536:
            raise ValueError('phase journal empty or record bound exceeded')
        rows = [json.loads(line) for line in split]
        if any(not isinstance(row, dict) for row in rows):
            raise ValueError('phase journal records must be objects')
        header = rows[0]
        if header.get('event') != 'header' or header.get('advisory_only') is not True:
            raise ValueError('phase header missing')
        binding = header['binding']
        if not isinstance(binding, dict):
            raise ValueError('phase journal binding must be an object')
        if (binding['request_sha256'] != hashlib.sha256(request_bytes).hexdigest()
                or binding['evidence_path'] != request['evidence_path']
                or any(binding[key] != request[key] for key in ('source_digest','profile_digest','expected_targets'))):
            raise ValueError('phase request/source/target binding differs')
        declared_owner = binding['original_descriptor']
        if not isinstance(declared_owner, dict):
            raise ValueError('phase original owner must be an object')
        if outer_descriptor is not None:
            observed_owner = dict(declared_owner)
            # The original owner's log deliberately retains the runtime digest
            # without printing its values; its child descriptor carries both.
            if 'runtime_environment' in observed_owner and 'runtime_environment' not in outer_descriptor:
                runtime = observed_owner.pop('runtime_environment')
                if (not isinstance(runtime, dict)
                        or hashlib.sha256(json.dumps(runtime, sort_keys=True).encode()).hexdigest()
                        != outer_descriptor.get('runtime_environment_sha256')):
                    raise ValueError('phase runtime environment digest differs')
            if observed_owner != outer_descriptor:
                raise ValueError('phase original owner differs')
        expected = hashlib.sha256(json.dumps(binding, sort_keys=True).encode()).hexdigest()
        stack = []; totals = {}; last_time = -1; last_child = None
        for sequence, row in enumerate(rows):
            if (type(row.get('sequence')) is not int or row['sequence'] != sequence
                    or type(row.get('version')) is not int or row['version'] != 1
                    or row.get('binding_sha256') != expected
                    or type(row.get('monotonic_ns')) is not int or row['monotonic_ns'] < 0 or row['monotonic_ns'] < last_time):
                raise ValueError('phase sequence/time/binding invalid')
            last_time = row['monotonic_ns']
            event = row['event']
            if sequence == 0: continue
            if event == 'started':
                if type(row.get('phase_id')) is not int or row['phase_id'] != sequence or not isinstance(row.get('phase'), str) or not isinstance(row.get('context'), dict):
                    raise ValueError('phase start invalid')
                stack.append(row)
            elif event == 'completed':
                if not stack or type(row.get('phase_id')) is not int or row['phase_id'] != stack[-1]['phase_id'] or row.get('phase') != stack[-1]['phase']:
                    raise ValueError('phase completion order invalid')
                started = stack.pop()
                if row.get('exit_status') is not None and type(row['exit_status']) is not int:
                    raise ValueError('phase exit status invalid')
                if type(row.get('duration_ns')) is not int or row['duration_ns'] != row['monotonic_ns'] - started['monotonic_ns'] or row.get('outcome') not in ('returned','raised'):
                    raise ValueError('phase duration/outcome invalid')
                totals[row['phase']] = totals.get(row['phase'], 0) + row['duration_ns']
            elif event == 'child':
                if (not stack or type(row.get('phase_id')) is not int or row['phase_id'] != stack[-1]['phase_id']
                        or type(row.get('pid')) is not int or row['pid'] <= 0
                        or (row.get('kernel_starttime') is not None and (type(row['kernel_starttime']) is not int or row['kernel_starttime'] < 0))
                        or not isinstance(row.get('state'), str)):
                    raise ValueError('phase child identity invalid')
                last_child = {key: row[key] for key in ('pid','kernel_starttime','state','phase_id')}
            else:
                raise ValueError('unknown phase event')
        if last_child is not None:
            last_child['phase_currently_open'] = any(row['phase_id'] == last_child['phase_id'] for row in stack)
        active_context = next((row['context'] for row in reversed(stack) if row['context'].get('argv') or row['context'].get('executable')), None)
        result.update(binding_summary={key: binding[key] for key in ('request_sha256','source_digest','profile_digest','core_source_sha256','expected_targets')},
                      last_entered_phase=stack[-1] if stack else None, active_command_context=active_context,
                      available=True, truncated_tail=truncated, last_complete_record=rows[-1], active_phases=stack,
                      last_child=last_child, cumulative_completed_duration_ns=totals,
                      duration_semantics='nested/inclusive; owner includes source and pin; do not sum overlapping phases',
                      original_owner_binding='matched declared outer descriptor' if outer_descriptor is not None else 'NOT_PROVEN: outer descriptor unavailable')
    except (OSError, ValueError, KeyError, TypeError) as error:
        result['error'] = str(error)
    return result

def report(evidence, mode, root, expected_head=None):
    storage = [json.loads(line) for line in (evidence / 'storage.jsonl').read_text().splitlines()]
    samples = [row for row in storage if row.get('valid') is True]
    result = {'head': (evidence / 'head.txt').read_text().strip(), 'mode': mode,
              'expected_packages': PACKAGES, 'storage': storage[-1],
              'retained_by_root': next((row.get('allocated_by_root') for row in samples if row['kind'] == 'retained'), None),
              'minimum_free_bytes': {device: min(row['filesystems'][device]['free_bytes'] for row in samples if device in row['filesystems'])
                                     for row in samples for device in row['filesystems']},
              'max_sample_gap_seconds': max((b['elapsed_seconds'] - a['elapsed_seconds'] for a, b in zip(samples, samples[1:])), default=0),
              'vanished_entries': sum(row.get('vanished_entries', 0) for row in samples)}
    result['candidate_head_matches_caller'] = (None if expected_head is None else bool(re.fullmatch(r'[0-9a-f]{40}', expected_head)) and result['head'] == expected_head)
    logs = evidence / 'logs' if (evidence / 'logs').exists() else evidence
    stderr = (logs / 'route.stderr').read_text()
    resources = json.loads((evidence / 'resources.json').read_text()) if (evidence / 'resources.json').exists() else None
    binding = terminal_owner_binding(stderr, mode, root, evidence / 'cohort', resources)
    outer_settlement = binding.get('settlement')
    result['terminal_owner_binding'] = binding
    result['qualification_status'] = 'NOT_PROVEN'
    result['qualification_missing_authority'] = ['candidate_source_attempt_attestation']
    try:
        metadata = captured_metadata(evidence, mode)
        if metadata is not None:
            package_inventory(metadata)
    except (OSError, ValueError) as error:
        metadata = None
        result['package_inventory_error'] = str(error)
    result['package_inventory_verified'] = metadata is not None
    if metadata is None:
        result['qualification_missing_authority'].append('captured_Cargo_package_inventory')
    result['outer_settlement'] = outer_settlement
    case_rows = {}
    unit_rows = {}
    complete = False
    case_receipts_verified = True
    receipt_errors = []
    result['phase_diagnostics'] = []
    if mode == 'ordinary':
        stdout = (logs / 'route.stdout').read_bytes()
        try:
            rows = messages(logs / 'route.stdout', framed=True)
            unit_rows = units(rows, root, metadata)
            complete = any(row.get('reason') == 'build-finished' and row.get('success') is True for row in rows)
            case_rows = ordinary_cases(stdout, stderr, rows, root, metadata)
        except (ValueError, Denied) as error:
            result['case_inventory_error'] = str(error)
    else:
        requests = sorted((evidence / 'cohort').glob('*-request.json'))
        for request_path in requests:
            request = json.loads(request_path.read_text())
            compiler = Path(request['evidence_path'])
            # Receipts contain producer-host absolute paths; use the retained filename here.
            compiler = request_path.parent / compiler.name
            rows = messages(compiler) if compiler.exists() else []
            if 'check' in request['producer'] and '--tests' in request['producer']:
                unit_rows = units(rows, root, metadata)
                complete = any(row.get('reason') == 'build-finished' and row.get('success') is True for row in rows)
            result['phase_diagnostics'].append(phase_diagnostics(request_path, binding.get('descriptor')))
            result_path = request_path.with_name(request_path.name.replace('-request.json', '-result.json'))
            try:
                receipt = json.loads(result_path.read_text())
                if (not isinstance(receipt, dict) or type(receipt.get('status')) is not int
                        or not isinstance(receipt.get('settlement'), dict)
                        or type(receipt['settlement'].get('tree_settled')) is not bool
                        or not isinstance(receipt.get('coverage'), list)):
                    raise ValueError('invalid result receipt shape')
                for item in receipt['coverage']:
                    if (not isinstance(item, dict) or not isinstance(item.get('executable'), str)
                            or not isinstance(item.get('cases'), list) or not isinstance(item.get('ignored'), list)
                            or any(not isinstance(name, str) for name in [*item['cases'], *item['ignored']])
                            or not collections.Counter(item['ignored']) <= collections.Counter(item['cases'])):
                        raise ValueError('invalid result coverage shape')
            except (OSError, ValueError) as error:
                case_receipts_verified = False
                receipt_errors.append({'path': result_path.name, 'type': type(error).__name__, 'message': str(error)})
                if 'check' in request['producer'] and '--tests' in request['producer']:
                    complete = False
                continue
            if receipt['status'] != 0 or not successful_receipt(receipt['settlement']):
                case_receipts_verified = False
            if 'check' in request['producer'] and '--tests' in request['producer']:
                complete = complete and receipt.get('status') == 0 and successful_receipt(receipt.get('settlement'))
            for coverage in receipt.get('coverage', []):
                if receipt.get('status') != 0 or not successful_receipt(receipt.get('settlement')):
                    case_receipts_verified = False
                matches = [unit for unit in rows if unit.get('executable') == coverage['executable'] and unit.get('profile', {}).get('test') is True]
                keys = list(units(matches, root, metadata))
                if len(keys) != 1 or keys[0] in case_rows:
                    raise ValueError('ambiguous or repeated cohort case binding')
                case_rows[keys[0]] = {'cases': coverage['cases'], 'ignored': coverage['ignored']}
    observed_packages = {json.loads(key)[0] for key in unit_rows}
    complete = (complete and result['candidate_head_matches_caller'] is not False and metadata is not None and observed_packages == set(PACKAGES) and binding['verified'])
    case_execution_successful = (case_receipts_verified and bool(case_rows) if mode == 'cohort' else bool(case_rows) and all(not row['failed'] and row['footer_status'] == 'ok' and row['footer_consistent'] for row in case_rows.values()))
    case_execution_successful = (case_execution_successful and binding['verified']
                                 and binding.get('settlement', {}).get('cancelled') is False
                                 and (binding.get('product') or {}).get('exit_code') == 0)
    result['case_execution_successful'] = case_execution_successful
    result['measurement_valid'] = (result['storage'].get('measurement_complete') is True
                                   and not any(row.get('valid') is False for row in storage))
    result['observed_reserve_preserved'] = result['storage'].get('observed_reserve_preserved') is True
    # Export the exact observed map already hashed below. Partial or opaque
    # units remain diagnostic; existing completeness/authority fields govern use.
    result.update(unit_count=len(unit_rows), unit_inventory_complete=complete,
                  unit_projection=unit_rows,
                  units_sha256=digest(unit_rows), cases_sha256=digest({key: {field: row[field] for field in ('cases', 'ignored')} for key, row in case_rows.items()}),
                  case_outcomes=case_rows,
                  observed_case_targets=len(case_rows),
                  case_count=sum(len(row['cases']) for row in case_rows.values()),
                  ignored_count=sum(len(row['ignored']) for row in case_rows.values()),
                  observed_case_inventory_complete=complete and set(case_rows) == set(unit_rows),
                  case_receipts_successful_and_settled=case_receipts_verified and case_execution_successful,
                  case_inventory_complete=(complete and set(case_rows) == set(unit_rows)
                                           and case_receipts_verified and case_execution_successful
                                           and type(result['storage'].get('command_status')) is int
                                           and result['storage']['command_status'] == 0),
                  unit_count_by_package=dict(collections.Counter(json.loads(key)[0] for key in unit_rows)))
    result['receipt_errors'] = receipt_errors
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=['ordinary', 'cohort'], required=True)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--candidate-sha')
    args = parser.parse_args()
    print(json.dumps(report(args.evidence, args.mode, args.root, args.candidate_sha), sort_keys=True))
