#!/usr/bin/env python3
"""Read persisted campaign evidence; no execution, admission or cleanup authority."""
import argparse
import collections
import hashlib
import json
from pathlib import Path
import re

PACKAGES = ['perl-dap', 'perl-lsp-rs', 'perl-parser-core', 'perl-semantic-analyzer', 'perl-workspace', 'xtask']


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def messages(path):
    rows = []
    for line in path.read_text().splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue  # Libtest's ordinary stdout shares this stream with Cargo JSON.
        if isinstance(row, dict):
            rows.append(row)
    return rows


def units(rows, root):
    result = {}
    for row in rows:
        if row.get('reason') != 'compiler-artifact' or row.get('profile', {}).get('test') is not True:
            continue
        target = row['target']
        relative = Path(target['src_path']).relative_to(root).as_posix()
        parts = relative.split('/')
        package = parts[1] if parts[0] == 'crates' else parts[0]
        if package not in PACKAGES:
            raise ValueError('unexpected package test unit: ' + package)
        key = json.dumps([package, target['name'], target['kind'], relative], separators=(',', ':'))
        value = {'features': sorted(row['features']), 'profile': row['profile']}
        if key in result and result[key] != value:
            raise ValueError('conflicting feature/profile units for ' + key)
        result[key] = value
    return result


def ordinary_cases(stdout, stderr, rows, root):
    by_executable = {}
    for row in rows:
        if row.get('executable') and row.get('profile', {}).get('test') is True:
            for key in units([row], root):
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
                current.append((case[1], case[2].startswith('ignored')))
            if line.startswith('test result:'):
                if len(current) != count or len({name for name, _ in current}) != count:
                    raise ValueError('incomplete or duplicate ordinary case output')
                blocks.append({'cases': sorted(name for name, _ in current),
                               'ignored': sorted(name for name, ignored in current if ignored)})
                current = None
    if current is not None or len(blocks) != len(keys) or len(set(keys)) != len(keys):
        raise ValueError('ordinary harness boundaries incomplete or ambiguous')
    return dict(zip(keys, blocks))


def report(evidence, mode, root):
    storage = [json.loads(line) for line in (evidence / 'storage.jsonl').read_text().splitlines()]
    samples = [row for row in storage if row.get('valid') is True]
    result = {'head': (evidence / 'head.txt').read_text().strip(), 'mode': mode,
              'expected_packages': PACKAGES, 'storage': storage[-1],
              'retained_by_root': next((row.get('allocated_by_root') for row in samples if row['kind'] == 'retained'), None),
              'minimum_free_bytes': {device: min(row['filesystems'][device]['free_bytes'] for row in samples if device in row['filesystems'])
                                     for row in samples for device in row['filesystems']},
              'max_sample_gap_seconds': max((b['elapsed_seconds'] - a['elapsed_seconds'] for a, b in zip(samples, samples[1:])), default=0),
              'vanished_entries': sum(row.get('vanished_entries', 0) for row in samples)}
    logs = evidence / 'logs' if (evidence / 'logs').exists() else evidence
    stderr = (logs / 'route.stderr').read_text()
    settlements = re.findall(r'^cargo-admitted .+ settlement: (\{.*\})$', stderr, re.MULTILINE)
    outer_settlement = json.loads(settlements[-1]) if settlements else None
    result['outer_settlement'] = outer_settlement
    case_rows = {}
    unit_rows = {}
    complete = False
    case_receipts_verified = True
    if mode == 'ordinary':
        stdout = (logs / 'route.stdout').read_text()
        rows = messages(logs / 'route.stdout')
        unit_rows = units(rows, root)
        complete = any(row.get('reason') == 'build-finished' and row.get('success') is True for row in rows)
        try:
            case_rows = ordinary_cases(stdout, stderr, rows, root)
        except ValueError as error:
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
                unit_rows = units(rows, root)
                complete = any(row.get('reason') == 'build-finished' and row.get('success') is True for row in rows)
            result_path = request_path.with_name(request_path.name.replace('-request.json', '-result.json'))
            if not result_path.exists():
                continue
            receipt = json.loads(result_path.read_text())
            if 'check' in request['producer'] and '--tests' in request['producer']:
                complete = complete and receipt.get('status') == 0 and receipt.get('settlement', {}).get('tree_settled') is True
            for coverage in receipt.get('coverage', []):
                if receipt.get('status') != 0 or receipt.get('settlement', {}).get('tree_settled') is not True:
                    case_receipts_verified = False
                matches = [unit for unit in rows if unit.get('executable') == coverage['executable'] and unit.get('profile', {}).get('test') is True]
                keys = list(units(matches, root))
                if len(keys) != 1 or keys[0] in case_rows:
                    raise ValueError('ambiguous or repeated cohort case binding')
                case_rows[keys[0]] = {'cases': coverage['cases'], 'ignored': coverage['ignored']}
    observed_packages = {json.loads(key)[0] for key in unit_rows}
    complete = (complete and observed_packages == set(PACKAGES)
                and outer_settlement is not None and outer_settlement.get('tree_settled') is True)
    result.update(unit_count=len(unit_rows), unit_inventory_complete=complete,
                  units_sha256=digest(unit_rows), cases_sha256=digest(case_rows),
                  observed_case_targets=len(case_rows),
                  case_count=sum(len(row['cases']) for row in case_rows.values()),
                  ignored_count=sum(len(row['ignored']) for row in case_rows.values()),
                  observed_case_inventory_complete=complete and set(case_rows) == set(unit_rows),
                  case_receipts_successful_and_settled=case_receipts_verified,
                  case_inventory_complete=(complete and set(case_rows) == set(unit_rows)
                                           and case_receipts_verified
                                           and result['storage'].get('command_status') == 0),
                  unit_count_by_package=dict(collections.Counter(json.loads(key)[0] for key in unit_rows)))
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=['ordinary', 'cohort'], required=True)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    args = parser.parse_args()
    print(json.dumps(report(args.evidence, args.mode, args.root), sort_keys=True))
