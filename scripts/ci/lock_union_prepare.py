#!/usr/bin/env python3
"""Finite lock measurements under one existing admission owner."""
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import disallowed_fields_prepare as shared
owner = shared.owner

EXPECTED = {
    'let _ = dropped_std_mutex.lock();': 'let_underscore_lock',
    'let _ = dropped_std_rwlock.read();': 'let_underscore_lock',
    'let _ = dropped_pl_mutex.lock();': 'clippy::let_underscore_lock',
    'let _ = dropped_pl_rwlock.write();': 'clippy::let_underscore_lock',
}


STD = tuple(statement for statement, lint in EXPECTED.items() if lint == 'let_underscore_lock')
PL = tuple(statement for statement, lint in EXPECTED.items() if lint == 'clippy::let_underscore_lock')
OWNED = ('let _ = dropped_arc_mutex.lock_arc();', 'let _ = dropped_arc_rwlock.write_arc();',
         'let _ = PlMutexGuard::map(dropped_mapped_pl_guard, |value| &mut value.0);')
DROP = ('drop(dropped_via_std_drop.lock());', 'drop(dropped_via_pl_drop.lock());')
MEASUREMENTS = {'union': owner.LOCK_UNION_ROW, 'rustc': 'xtask-lock-rustc-fixture',
                'clippy': 'xtask-lock-clippy-fixture', 'must-use': 'xtask-lock-must-use-fixture',
                'sweep': 'xtask-lock-sweep-fixture'}


def diagnostic_success(stdout, returncode, root, measurement='union'):
    if measurement not in MEASUREMENTS:
        raise owner.Denied('unknown finite lock measurement')
    if returncode != 0 or len(stdout.encode('utf-8')) > shared.LIMIT:
        raise owner.Denied('lock instrument must compile successfully with bounded JSON')
    required = dict(EXPECTED)
    if measurement == 'rustc': required = {s: 'let_underscore_lock' for s in STD}
    if measurement == 'clippy': required = {s: 'clippy::let_underscore_lock' for s in PL}
    if measurement == 'must-use': required = {s: 'clippy::let_underscore_must_use' for s in OWNED}
    if measurement == 'sweep': required.update({s: 'clippy::let_underscore_must_use' for s in OWNED})
    allowed = set(required.items())
    if measurement in ('must-use', 'sweep'):
        allowed.update((s, 'clippy::let_underscore_must_use') for s in STD)
    governed = ({'clippy::let_underscore_must_use'} if measurement == 'must-use'
                else set(EXPECTED.values()) | ({'clippy::let_underscore_must_use'} if measurement == 'sweep' else set()))
    found, terminal = set(), False
    source = root / 'src/lib.rs'
    source_lines = source.read_text().splitlines()
    for line in stdout.splitlines():
        if not line or terminal:
            raise owner.Denied('blank or post-terminal lock JSON')
        event = json.loads(line)
        if not isinstance(event, dict):
            raise owner.Denied('lock JSON must contain objects')
        if event.get('reason') == 'build-finished':
            if event.get('success') is not True:
                raise owner.Denied('lock compilation has no successful terminal')
            terminal = True
        if event.get('reason') != 'compiler-message': continue
        message = event.get('message')
        if not isinstance(message, dict) or message.get('level') == 'error':
            raise owner.Denied('invalid/error lock diagnostic')
        code = message.get('code') or {}
        if not isinstance(code, dict): raise owner.Denied('invalid lock lint code')
        lint = code.get('code')
        if lint not in governed and measurement != 'sweep': continue
        target = event.get('target')
        if (event.get('manifest_path') != str(root / 'Cargo.toml')
                or not isinstance(target, dict) or target.get('src_path') != str(source)
                or message.get('level') != 'warning'):
            raise owner.Denied('lock warning has wrong manifest/source/level')
        spans = message.get('spans')
        if not isinstance(spans, list): raise owner.Denied('missing lock span')
        primary = [s for s in spans if isinstance(s, dict) and s.get('is_primary') is True]
        if lint in governed and len(primary) != 1:
            raise owner.Denied('missing/ambiguous lock primary span')
        if not spans:
            # Group-level advice has no fixture line and cannot supply liveness.
            continue
        span = primary[0] if lint in governed else spans[0]
        if not isinstance(span, dict): raise owner.Denied('invalid lock span')
        number, text = span.get('line_start'), span.get('text')
        if lint not in governed and text == []:
            # Command-line group advice has a dummy span, no source finding.
            continue
        if (span.get('file_name') not in ('src/lib.rs', str(source))
                or type(number) is not int or not isinstance(text, list) or not text
                or not 1 <= number <= len(source_lines)
                or number + len(text) - 1 > len(source_lines)
                or any(not isinstance(t, dict) or t.get('text') != source_lines[number+i-1] for i,t in enumerate(text))):
            raise owner.Denied('lock diagnostic span is not current source')
        statement = source_lines[number-1].strip()
        if measurement == 'sweep' and statement in DROP:
            raise owner.Denied('Clippy now covers explicit drop; revisit the measured ruling')
        if lint not in governed: continue
        pair = (statement, lint)
        if len(text) != 1 or pair not in allowed or pair in found:
            raise owner.Denied('unexpected/duplicate lock site or wrong lint ownership')
        found.add(pair)
    if not set(required.items()).issubset(found) or not terminal:
        raise owner.Denied('missing current lock positives or successful terminal')


def fixture(env=None, invoke=subprocess.run, measurement='union'):
    env = dict(os.environ if env is None else env)
    if measurement not in MEASUREMENTS:
        raise owner.Denied('unknown finite lock measurement')
    row = MEASUREMENTS[measurement]
    command, child, cwd = owner.nested_command(row, env)
    descriptor = json.loads(env['CARGO_ADMITTED_RESOURCES'])
    snapshot, _ = owner.bounded_json(descriptor['nested_snapshot']['path'])
    binding = snapshot['plan']['lock_union_fixture']
    if owner.file_subject(Path(sys.executable).resolve(strict=True)) != binding['python']:
        raise owner.Denied('lock-union interpreter differs from admitted native interpreter')
    result = shared.prepared_output(row, env)
    replayed = result is not None
    if result is None:
        result = invoke(command, env=child, cwd=cwd, capture_output=True, text=True,
                        encoding='utf-8', errors='strict')
    owner.nested_command(row, env)
    diagnostic_error = None
    try:
        diagnostic_success(result.stdout, result.returncode, cwd, measurement)
    except (owner.Denied, ValueError, TypeError, KeyError) as error:
        diagnostic_error = error
    if not replayed and owner.LOCK_PARTITION_TEST_ROW in snapshot['plan']['request']['rows']:
        evidence = {'schema_version':1, 'measurement':measurement, 'tested_source':snapshot['plan']['source']['head'],
                    'row':row, 'argv':command, 'cwd':str(cwd), 'exit_code':result.returncode,
                    'stdout':result.stdout[:shared.LIMIT], 'stderr':result.stderr[:shared.LIMIT],
                    'stdout_truncated':len(result.stdout)>shared.LIMIT, 'stderr_truncated':len(result.stderr)>shared.LIMIT,
                    'diagnostic_validation':{'passed':diagnostic_error is None, 'error':str(diagnostic_error) if diagnostic_error else None},
                    'snapshot':descriptor['nested_snapshot'],
                    'owner_process':descriptor['owner_process'], 'lease_identity':descriptor['lease_identity'],
                    'marker_identity':descriptor['marker_identity'], 'recorded_under_original_live_owner':True}
        path = Path(descriptor['resources']['temp']) / ('lock-measurement-' + str(os.getpid()) + '-' + measurement + '.json')
        with path.open('x', encoding='utf-8') as record: json.dump(evidence, record)
    if diagnostic_error is not None:
        raise diagnostic_error
    print(result.stdout, end='', flush=True)
    print(result.stderr, end='', file=sys.stderr, flush=True)
    return result.returncode


if __name__ == '__main__':
    try:
        if sys.argv[1:] == ['--prepare']:
            shared.prepare(label='lock-union')
        elif sys.argv[1:] in (['--direct'], ['--fixture']):
            sys.exit(fixture())
        elif len(sys.argv) == 3 and sys.argv[1] == '--measurement':
            sys.exit(fixture(measurement=sys.argv[2]))
        elif sys.argv[1:] == ['--remaining']:
            sys.exit(shared.owning_test(fixture_row=owner.LOCK_FIXTURE_ROWS,
                                      test_row=owner.LOCK_PARTITION_TEST_ROW,
                                      test_name=owner.LOCK_PARTITION_TESTS,
                                      copy_prefix='lock-remaining-owning'))
        elif sys.argv[1:] == ['--test']:
            sys.exit(shared.owning_test(fixture_row=owner.LOCK_UNION_ROW,
                                      test_row=owner.LOCK_UNION_TEST_ROW,
                                      test_name=owner.LOCK_UNION_TEST,
                                      copy_prefix='lock-union-owning'))
        elif sys.argv[1:] == ['--control']:
            sys.exit(shared.owning_test(fixture_row=owner.LOCK_UNION_ROW,
                                      test_row=owner.PREPARATION_CONTROL_ROW,
                                      test_name=owner.PREPARATION_CONTROL_TEST,
                                      copy_prefix='preparation-control-owning'))
        else:
            raise owner.Denied('expected --prepare, --direct, --fixture, --test or --control')
    except (owner.Denied, KeyError, TypeError, ValueError, OSError) as error:
        print('lock-union fixture refused: ' + str(error), file=sys.stderr)
        sys.exit(75)
