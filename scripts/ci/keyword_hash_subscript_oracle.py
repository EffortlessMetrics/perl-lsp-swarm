#!/usr/bin/env python3
"""Compile/Deparse only the checked-in #14059 slice; never execute fixture bodies."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
MATRIX = ROOT / '.ci/keyword-hash-subscript/cases.json'
LIMIT = 65536
PREFIX = 'use strict; use warnings; our (%h, $h, $x); sub probe { while (1) { '
SUFFIX = '; } }\n'

def subject(case_id):
    parts = case_id.split('-')
    if parts[0] in ('next', 'last', 'redo', 'return'):
        word, form, context = parts
        key = word + (', $x' if context == 'comma' else '')
    elif case_id == 'ordinary-next-label':
        return 'next OUTER'
    elif case_id == 'ordinary-return-operand':
        return 'return $x'
    else:
        form, control = case_id.split('-', 1)
        key = {'ordinary-terminal': 'ordinary', 'ordinary-comma': 'ordinary, $x',
               'quoted-terminal': "'next'", 'quoted-comma': "'next', $x",
               'return-list-operands': 'return $x, 7', 'next-label-comma': 'next OUTER, 7'}[control]
    return {'scalar': '$h', 'arrow': '$h->', 'slice': '@h'}[form] + '{' + key + '}'

def digest(data):
    return hashlib.sha256(data).hexdigest()

def load_matrix(path=MATRIX):
    raw = path.read_bytes()
    m = json.loads(raw)
    if m['schema'] != 'keyword-hash-subscript-oracle/v1':
        raise ValueError('matrix schema')
    required = {f'{w}-{f}-{c}' for w in ('next', 'last', 'redo', 'return')
                for f in ('scalar', 'arrow', 'slice') for c in ('terminal', 'comma')}
    controls = {f'{f}-{c}' for f in ('scalar', 'arrow', 'slice') for c in (
        'ordinary-terminal', 'ordinary-comma', 'quoted-terminal', 'quoted-comma',
        'return-list-operands', 'next-label-comma')} | {'ordinary-next-label', 'ordinary-return-operand'}
    known = {f'{f}-{c}': 14065 for f in ('scalar', 'arrow', 'slice')
             for c in ('ordinary-comma', 'return-list-operands')}
    rows = m['cases']
    ids = [r['id'] for r in rows]
    if len(ids) != len(set(ids)) or set(ids) != required | controls:
        raise ValueError('missing/duplicate matrix or control case')
    if set(m['denominator']['matrix_ids']) != required or set(m['denominator']['control_ids']) != controls:
        raise ValueError('declared denominator differs')
    if {r['id']: r['mismatch_owner'] for r in rows if r['mismatch_owner'] is not None} != known:
        raise ValueError('known native mismatch ownership changed')
    profiles = {p['id']: p for p in m['profiles']}
    if len(profiles) != 1 or next(iter(profiles.values()))['version'] != '5.40.1':
        raise ValueError('version/profile denominator changed')
    for r in rows:
        source = r['source'].encode('utf-8')
        if digest(source) != r['source_sha256']:
            raise ValueError(r['id'] + ': stale source bytes')
        p = profiles[r['profile']]
        if p['pragmas'] != ['strict', 'warnings'] or p['features'] or p['other_versions'] != 'NOT_PROVEN':
            raise ValueError('profile policy changed')
        if source != (PREFIX + subject(r['id']) + SUFFIX).encode('utf-8'):
            raise ValueError(r['id'] + ': source/profile mismatch')
        a, b = r['subject_span']
        if [a, b] != [len(PREFIX), len(PREFIX) + len(subject(r['id']))]:
            raise ValueError('subject geometry')
        def geometry(n, lower, upper):
            a, b = n['span']
            if not lower <= a < b <= upper:
                raise ValueError(r['id'] + ': AST geometry')
            for value in n.values():
                for item in value if isinstance(value, list) else [value]:
                    if isinstance(item, dict) and 'kind' in item:
                        geometry(item, a, b)
        if r['expected']['disposition'] == 'clean':
            n = r['expected']['ast']
            if n['span'] != r['subject_span']:
                raise ValueError('whole geometry')
            geometry(n, a, b)
        elif r['expected']['disposition'] != 'rejected' or r['expected']['ast'] is not None:
            raise ValueError('unknown disposition')
    return m, digest(raw)

def execute(command, source=b'', timeout=5):
    # Trusted literal fixtures create no descendants; kill and reap the owned direct child on timeout.
    env = {'PATH': os.environ.get('PATH', ''), 'LC_ALL': 'C', 'LANG': 'C'}
    if 'SystemRoot' in os.environ:
        env['SystemRoot'] = os.environ['SystemRoot']
    try:
        with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
            child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=out, stderr=err, env=env)
            try:
                child.communicate(source, timeout=timeout)
                status = 'completed' if child.returncode >= 0 else 'signal'
            except subprocess.TimeoutExpired:
                child.kill()
                child.communicate()
                status = 'timeout'
            out.seek(0); err.seek(0)
            stdout, stderr = out.read(LIMIT + 1), err.read(LIMIT + 1)
            if len(stdout) > LIMIT or len(stderr) > LIMIT:
                status = 'output_limit'
            return {'status': status, 'exit': child.returncode,
                    'stdout': stdout[:LIMIT].decode('utf-8', 'replace'),
                    'stderr': stderr[:LIMIT].decode('utf-8', 'replace')}
    except OSError as error:
        return {'status': 'spawn_error', 'exit': None, 'stdout': '', 'stderr': str(error)}

def assess(row, compiled, deparsed):
    for name, result in [('compile', compiled), ('deparse', deparsed)]:
        if result['status'] != 'completed' or type(result['exit']) is not int or not 0 <= result['exit'] <= 255:
            return 'NOT_PROVEN', [name + ': invalid instrument result']
    failures = []
    for name, result in [('compile', compiled), ('deparse', deparsed)]:
        if result['exit'] != row['oracle'][name + '_exit']:
            failures.append(name + ': disposition')
    if deparsed['stdout'] != row['oracle']['deparse_stdout']:
        failures.append('exact Deparse output')
    return ('ORACLE_MISMATCH' if failures else 'PROVEN'), failures

def run(perl='perl'):
    m, matrix_hash = load_matrix()
    receipt = {'schema': 'keyword-hash-subscript-perl-receipt/v1', 'matrix_sha256': matrix_hash,
               'runner_sha256': digest(Path(__file__).read_bytes()), 'status': 'NOT_PROVEN', 'rows': []}
    executable = shutil.which(perl)
    if executable is None:
        receipt['reason'] = 'Perl executable missing'
        return receipt
    executable = str(Path(executable).resolve())
    identity = execute([executable, '-V:version', '-V:archname'])
    receipt.update(executable=executable, executable_sha256=digest(Path(executable).read_bytes()), identity=identity)
    version = re.search(r"version='([^']+)'", identity['stdout'])
    archname = re.search(r"archname='([^']+)'", identity['stdout'])
    if (identity['status'] != 'completed' or identity['exit'] != 0 or version is None
            or version[1] != m['profiles'][0]['version'] or archname is None
            or archname[1] != m['profiles'][0]['archname']):
        receipt['reason'] = 'exact Perl version/profile not available'
        return receipt
    receipt['version'] = version[1]
    for row in m['cases']:
        source = row['source'].encode('utf-8')
        compiled = execute([executable, '-c'], source)
        deparsed = execute([executable, '-MO=Deparse,-p'], source)
        status, failures = assess(row, compiled, deparsed)
        receipt['rows'].append({'id': row['id'], 'source_sha256': digest(source), 'profile': row['profile'],
                                'compile': compiled, 'deparse': deparsed, 'status': status, 'failures': failures})
    statuses = {r['status'] for r in receipt['rows']}
    receipt['status'] = 'NOT_PROVEN' if 'NOT_PROVEN' in statuses else 'ORACLE_MISMATCH' if 'ORACLE_MISMATCH' in statuses else 'PROVEN'
    return receipt

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--perl', default='perl')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = run(args.perl)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(result['status'], len(result['rows']), 'rows:', args.output)
    raise SystemExit(0 if result['status'] == 'PROVEN' else 1)
