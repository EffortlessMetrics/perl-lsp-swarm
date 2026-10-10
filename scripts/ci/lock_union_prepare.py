#!/usr/bin/env python3
"""One successful lock-union measurement under the existing admission owner."""
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


def diagnostic_success(stdout, returncode, root):
    if returncode != 0 or len(stdout.encode('utf-8')) > shared.LIMIT:
        raise owner.Denied('lock-union instrument must compile successfully with bounded JSON')
    found, terminal = {}, False
    source = root / 'src/lib.rs'
    source_lines = source.read_text().splitlines()
    for line in stdout.splitlines():
        if not line or terminal:
            raise owner.Denied('blank or post-terminal lock-union JSON')
        event = json.loads(line)
        if not isinstance(event, dict):
            raise owner.Denied('lock-union JSON must contain objects')
        if event.get('reason') == 'build-finished':
            if event.get('success') is not True:
                raise owner.Denied('lock-union compilation has no successful terminal')
            terminal = True
        if event.get('reason') != 'compiler-message':
            continue
        message = event.get('message')
        if not isinstance(message, dict):
            raise owner.Denied('invalid lock-union diagnostic')
        if message.get('level') == 'error':
            raise owner.Denied('lock-union instrument reported a compiler error')
        code = message.get('code') or {}
        if not isinstance(code, dict):
            raise owner.Denied('invalid lock-union lint code')
        lint = code.get('code')
        if lint not in set(EXPECTED.values()):
            continue
        target = event.get('target')
        if (event.get('manifest_path') != str(root / 'Cargo.toml')
                or not isinstance(target, dict) or target.get('src_path') != str(source)
                or message.get('level') != 'warning'):
            raise owner.Denied('lock-union warning has wrong manifest/source/level')
        spans = message.get('spans')
        if not isinstance(spans, list):
            raise owner.Denied('missing lock-union primary span')
        primary = [span for span in spans if isinstance(span, dict) and span.get('is_primary') is True]
        if len(primary) != 1:
            raise owner.Denied('missing/ambiguous lock-union primary span')
        span = primary[0]; number = span.get('line_start'); text = span.get('text')
        if (span.get('file_name') not in ('src/lib.rs', str(source))
                or type(number) is not int or not 1 <= number <= len(source_lines)
                or not isinstance(text, list) or len(text) != 1
                or not isinstance(text[0], dict) or text[0].get('text') != source_lines[number - 1]):
            raise owner.Denied('lock-union diagnostic span is not current source')
        statement = source_lines[number - 1].strip()
        if EXPECTED.get(statement) != lint or statement in found:
            raise owner.Denied('unexpected/duplicate lock-union site or wrong lint ownership')
        found[statement] = lint
    if found != EXPECTED or not terminal:
        raise owner.Denied('missing current lock-union positives or successful terminal')


def fixture(env=None, invoke=subprocess.run):
    env = dict(os.environ if env is None else env)
    command, child, cwd = owner.nested_command(owner.LOCK_UNION_ROW, env)
    descriptor = json.loads(env['CARGO_ADMITTED_RESOURCES'])
    snapshot, _ = owner.bounded_json(descriptor['nested_snapshot']['path'])
    binding = snapshot['plan']['lock_union_fixture']
    if owner.file_subject(Path(sys.executable).resolve(strict=True)) != binding['python']:
        raise owner.Denied('lock-union interpreter differs from admitted native interpreter')
    result = invoke(command, env=child, cwd=cwd, capture_output=True, text=True,
                    encoding='utf-8', errors='strict')
    owner.nested_command(owner.LOCK_UNION_ROW, env)
    diagnostic_success(result.stdout, result.returncode, cwd)
    print(result.stdout, end='', flush=True)
    print(result.stderr, end='', file=sys.stderr, flush=True)
    return result.returncode


if __name__ == '__main__':
    try:
        if sys.argv[1:] == ['--prepare']:
            shared.prepare(label='lock-union')
        elif sys.argv[1:] in (['--direct'], ['--fixture']):
            sys.exit(fixture())
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
