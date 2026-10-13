#!/usr/bin/env python3
"""Bounded, fixture-only real-Perl signature evidence; never a parser pass."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
MATRIX = ROOT / '.ci/signature-conformance/cases.json'
SCHEMA = 'signature-oracle-receipt/v2'
LIMIT = 65536

def digest(data):
    return hashlib.sha256(data).hexdigest()

def text_digest(data):
    """Hash repository text with CRLF normalized to LF; preserve all other bytes."""
    return digest(data.replace(b'\r\n', b'\n'))

def version(text):
    if not re.fullmatch(r'5\.\d+(?:\.\d+)?', text):
        raise ValueError('invalid Perl version: ' + text)
    return tuple(int(x) for x in text.split('.'))[:2]

def load_matrix(path=MATRIX):
    raw = path.read_bytes()
    matrix = json.loads(raw)
    if matrix.get('schema') != 'signature-conformance/v1':
        raise ValueError('matrix schema mismatch')
    rows = matrix.get('cases', [])
    ids = [row['id'] for row in rows]
    if not ids or len(set(ids)) != len(ids):
        raise ValueError('missing or duplicate cases')
    if matrix.get('required_versions') != ['5.36', '5.38', '5.42', '5.44']:
        raise ValueError('required version denominator changed')
    for row in rows:
        source = row['source'].encode('utf-8')
        native = row['native']
        version(row['external']['minimum'])
        if type(native['accepted']) is not bool or type(row['external']['accepted']) is not bool:
            raise ValueError('disposition must be Boolean')
        start, end = native['header_span']
        if not 0 <= start < end <= len(source) or source[start:end][:1] != b'(' or source[start:end][-1:] != b')':
            raise ValueError(row['id'] + ': invalid header geometry')
        for parameter in native['parameters']:
            ps, pe = parameter['span']
            if not start < ps < pe < end:
                raise ValueError(row['id'] + ': invalid parameter geometry')
            for role in ('variable', 'default', 'operator'):
                if role in parameter:
                    field = parameter[role]
                    a, b = field['span']
                    if not ps <= a < b <= pe or source[a:b].decode('utf-8') != field['text']:
                        raise ValueError(row['id'] + ': invalid ' + role + ' geometry')
    return matrix, text_digest(raw)

def execute(command, timeout=3):
    # Fixtures contain no process creation; the owned direct child is killed and reaped.
    env = {'PATH': os.environ.get('PATH', ''), 'LC_ALL': 'C', 'LANG': 'C'}
    if 'SystemRoot' in os.environ:
        env['SystemRoot'] = os.environ['SystemRoot']
    try:
        with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
            child = subprocess.Popen(command, stdout=out, stderr=err, env=env)
            try:
                code = child.wait(timeout=timeout)
                status = 'completed' if code >= 0 else 'signal'
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
                code, status = child.returncode, 'timeout'
            out.seek(0); err.seek(0)
            stdout, stderr = out.read(LIMIT + 1), err.read(LIMIT + 1)
            if len(stdout) > LIMIT or len(stderr) > LIMIT:
                status = 'output_limit'
            return {'status': status, 'exit': code, 'stdout': stdout[:LIMIT].decode('utf-8', 'replace'), 'stderr': stderr[:LIMIT].decode('utf-8', 'replace')}
    except OSError as error:
        return {'status': 'spawn_error', 'exit': None, 'stdout': '', 'stderr': str(error)}

WARNING_PATTERNS = {
    'experimental::signatures': r'The signatures feature is experimental',
    'experimental::signature_named_parameters': r'(?i)named.*(?:signature|parameter).*experimental|(?i:experimental).*named.*(?:signature|parameter)',
    'experimental::args_array_with_signatures': r'Use of @_.*signatured subroutine.*experimental',
}

def expected_warnings(row, actual_version):
    value = version(actual_version)
    expected = []
    if value < (5, 36) and "use feature 'signatures'" in row['source']:
        expected.append('experimental::signatures')
    if row['external']['named_warning'] and value >= (5, 44):
        expected.append('experimental::signature_named_parameters')
    if row['external']['args_array_warning'] and value >= (5, 36):
        expected.append('experimental::args_array_with_signatures')
    return expected

def assess(row, actual_version, compile_result, runtime):
    failures = []
    if compile_result.get('status') != 'completed' or type(compile_result.get('exit')) is not int or not 0 <= compile_result['exit'] <= 255:
        return ['compile instrument invalid or missing process result']
    accepted = row['external']['accepted'] and version(actual_version) >= version(row['external']['minimum'])
    if (compile_result['exit'] == 0) != accepted:
        failures.append('compile disposition')
    if accepted:
        expected = set(expected_warnings(row, actual_version))
        observed = {name for name, pattern in WARNING_PATTERNS.items() if re.search(pattern, compile_result['stderr'], re.S)}
        if observed != expected:
            failures.append('warning identity: expected ' + repr(sorted(expected)) + ', observed ' + repr(sorted(observed)))
        wanted = row['external']['runtime_stdout']
        if wanted is not None and (runtime is None or runtime['status'] != 'completed' or type(runtime.get('exit')) is not int or runtime['exit'] != 0 or runtime['stdout'] != wanted):
            failures.append('runtime observation')
    return failures

def run(perl, expected_version=None):
    matrix, matrix_digest = load_matrix()
    executable = shutil.which(perl)
    if executable is None:
        raise ValueError('NOT_PROVEN: Perl executable missing')
    executable = str(Path(executable).resolve())
    identity = execute([executable, '-e', 'printf "%vd", $^V'])
    if identity['status'] != 'completed' or identity['exit'] != 0:
        raise ValueError('NOT_PROVEN: Perl identity instrument failed: ' + repr(identity))
    actual_version = identity['stdout'].strip()
    version(actual_version)
    if expected_version is not None and version(actual_version) != version(expected_version):
        raise ValueError('NOT_PROVEN: selected and actual version differ')
    receipt = {'schema': SCHEMA, 'matrix_sha256': matrix_digest, 'runner_sha256': text_digest(Path(__file__).read_bytes()), 'executable': executable, 'executable_sha256': digest(Path(executable).read_bytes()), 'identity': identity, 'version': actual_version, 'required_version': '.'.join(map(str, version(actual_version))) in matrix['required_versions'], 'rows': []}
    with tempfile.TemporaryDirectory(prefix='signature-conformance-') as directory:
        for row in matrix['cases']:
            path = Path(directory) / 'fixture.pl'
            path.write_bytes(row['source'].encode('utf-8'))
            compile_result = execute([executable, '-c', str(path)])
            runtime = None
            if compile_result['status'] == 'completed' and compile_result['exit'] == 0 and row['external']['runtime_stdout'] is not None:
                runtime = execute([executable, str(path)])
            receipt['rows'].append({'id': row['id'], 'source_sha256': digest(path.read_bytes()), 'compile': compile_result, 'runtime': runtime, 'failures': assess(row, actual_version, compile_result, runtime)})
    return receipt

def validate_receipts(receipts, required=None):
    matrix, matrix_digest = load_matrix()
    seen = set()
    for receipt in receipts:
        if receipt.get('schema') != SCHEMA or receipt.get('matrix_sha256') != matrix_digest or receipt.get('runner_sha256') != text_digest(Path(__file__).read_bytes()):
            raise ValueError('stale or wrong-subject receipt')
        identity = receipt.get('identity', {})
        if identity.get('status') != 'completed' or type(identity.get('exit')) is not int or identity.get('exit') != 0 or identity.get('stdout', '').strip() != receipt['version']:
            raise ValueError('version identity mismatch')
        if not receipt.get('executable') or not re.fullmatch(r'[0-9a-f]{64}', receipt.get('executable_sha256', '')):
            raise ValueError('missing executable identity')
        actual = '.'.join(map(str, version(receipt['version'])))
        if actual in seen:
            raise ValueError('duplicate version receipt')
        seen.add(actual)
        expected = {row['id']: row for row in matrix['cases']}
        rows = receipt.get('rows', [])
        if len(rows) != len(expected) or {row['id'] for row in rows} != set(expected):
            raise ValueError('missing or duplicate receipt cases')
        for result in rows:
            row = expected[result['id']]
            if result['source_sha256'] != digest(row['source'].encode('utf-8')):
                raise ValueError('stale fixture receipt')
            if assess(row, receipt['version'], result['compile'], result['runtime']):
                raise ValueError('failed oracle row: ' + row['id'])
    missing = set(required if required is not None else matrix['required_versions']) - seen
    if missing:
        raise ValueError('NOT_PROVEN: missing versions ' + ','.join(sorted(missing)))

def check_native_report(path, exit_code):
    matrix, _ = load_matrix()
    report = json.loads(path.read_text(encoding='utf-8'))
    if report.get('schema') != 'signature-native-report/v1' or report.get('matrix') != matrix:
        raise ValueError('NOT_PROVEN: native report subject mismatch')
    expected = {row['id']: row for row in matrix['cases']}
    rows = report.get('rows', [])
    if len(rows) != len(expected) or {row.get('id') for row in rows} != set(expected):
        raise ValueError('NOT_PROVEN: missing native cases')
    for row in rows:
        case = expected[row['id']]
        if row.get('source') != case['source'] or row.get('expected') != case['native'] or not isinstance(row.get('failures'), list):
            raise ValueError('NOT_PROVEN: stale native case')
    mismatches = sum(bool(row['failures']) for row in rows)
    unproven = any(row.get('status') == 'NOT_PROVEN' for row in rows)
    for row in rows:
        observation = row.get('observed')
        if not isinstance(observation, dict) or 'stop_cause' not in observation or 'accepted' not in observation:
            raise ValueError('NOT_PROVEN: missing native observation')
        terminal = observation['stop_cause'] is not None
        if (terminal and (not isinstance(observation['stop_cause'], str) or not observation['stop_cause'])) or (not terminal and type(observation['accepted']) is not bool):
            raise ValueError('NOT_PROVEN: invalid native observation')
        if not terminal and observation['accepted'] != row['expected']['accepted'] and not row['failures']:
            raise ValueError('NOT_PROVEN: unreported native disposition mismatch')
        if row.get('status') != ('NOT_PROVEN' if terminal else 'OBSERVED') or (terminal and (row['observed'].get('accepted') is not None or not row['failures'])):
            raise ValueError('NOT_PROVEN: invalid native disposition')
    status = 'NOT_PROVEN' if unproven else ('CONFORMANCE_MISMATCH' if mismatches else 'PASS')
    if report.get('status') != status or report.get('mismatch_count') != mismatches or exit_code != (101 if mismatches else 0):
        raise ValueError('NOT_PROVEN: native process/report disagreement')
    print(json.dumps({'status': status, 'mismatches': mismatches, 'cases': len(rows)}))
    return 2 if unproven else (1 if mismatches else 0)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--perl', default='perl')
    parser.add_argument('--expected-version')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--validate-receipts', type=Path, nargs='+')
    parser.add_argument('--check-native-report', type=Path)
    parser.add_argument('--native-exit-code', type=int)
    args = parser.parse_args()
    try:
        if args.check_native_report:
            return check_native_report(args.check_native_report, args.native_exit_code)
        if args.validate_receipts:
            validate_receipts([json.loads(path.read_text(encoding='utf-8')) for path in args.validate_receipts])
            print('required signature oracle receipts PASS')
            return 0
        if args.output is None:
            raise ValueError('--output is required')
        receipt = run(args.perl, args.expected_version)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
        failures = [(row['id'], row['failures']) for row in receipt['rows'] if row['failures']]
        print(json.dumps({'version': receipt['version'], 'required_version': receipt['required_version'], 'cases': len(receipt['rows']), 'failures': failures}))
        return 1 if failures else 0
    except (ValueError, KeyError, TypeError, OSError, json.JSONDecodeError) as error:
        print(str(error), file=sys.stderr)
        return 2

if __name__ == '__main__':
    sys.exit(main())
