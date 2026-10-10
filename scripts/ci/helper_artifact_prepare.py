#!/usr/bin/env python3
"""Bind actual helper compile artifacts to the existing live preparation owner."""
import json
import os
from pathlib import Path
import sys
sys.dont_write_bytecode = True

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import cargo_admitted as owner

HANDOFF = 'CARGO_ADMITTED_HELPER_ARTIFACTS'
PYTHON = 'CARGO_ADMITTED_HELPER_ARTIFACT_PYTHON'
LIMIT = 64 * 1024 * 1024


def context(env):
    import perllsp_workspace_prepare as product
    binding = product.context(env)
    plan = binding['plan']
    if plan.get('routed_preparation', {}).get('runtime_row') != owner.HELPER_RUNTIME:
        raise owner.Denied('helper artifact bridge requires exact helper owner')
    mapping = owner.routed_mapping(plan)
    binding['helper_validator'] = owner.file_subject(Path(__file__))
    return binding, mapping


def expected(mapping):
    result = {}
    for package, targets in mapping['declared_targets'].items():
        for target in targets:
            result[(package, target['name'], tuple(target['kind']), True)] = target
            if target['kind'] == ['bin']:
                result[(package, target['name'], ('bin',), False)] = target
    return result


def select(message, binding, wanted):
    root = Path(binding['descriptor']['worktree'])
    package = next((p for p in owner.HELPER_PACKAGES
                    if message.get('manifest_path') == str(root / ('xtask' if p == 'xtask' else 'crates/' + p) / 'Cargo.toml')), None)
    if package is None or message.get('executable') is None:
        return None
    target, profile = message.get('target'), message.get('profile')
    if not isinstance(target, dict) or not isinstance(profile, dict) or type(profile.get('test')) is not bool:
        raise owner.Denied('malformed helper executable target/profile')
    key = (package, target.get('name'), tuple(target.get('kind', [])), profile['test'])
    if key not in wanted or target.get('src_path') != str(root / wanted[key]['src_path']):
        raise owner.Denied('unselected or wrong-source helper executable')
    path = owner.native_path(message['executable'])
    directory = Path(binding['descriptor']['resources']['target']) / 'debug'
    suffix = '.exe' if binding['platform'] == 'win32' else ''
    if profile['test']:
        directory = Path(binding['descriptor']['resources']['build']) / 'debug/deps'
        stem = target['name'].replace('-', '_') + '-'
        valid_name = path.name.startswith(stem) and path.name.endswith(suffix)
    else:
        valid_name = path.name == target['name'] + suffix
    if path.parent != directory or not valid_name or str(path) not in message.get('filenames', []):
        raise owner.Denied('helper artifact has wrong private executable/profile/root')
    return key, path


def capture(stdout, env):
    binding, mapping = context(env)
    wanted, found = expected(mapping), {}
    if len(stdout.encode('utf-8')) > LIMIT:
        raise owner.Denied('helper artifact log exceeds bounded reader')
    terminal = False
    for line in stdout.splitlines():
        if not line.startswith('{'):
            continue
        try:
            message = json.loads(line)
        except ValueError as error:
            raise owner.Denied('malformed helper compiler JSON') from error
        if not isinstance(message, dict) or terminal:
            raise owner.Denied('invalid helper JSON or message after terminal')
        if message.get('reason') == 'build-finished':
            if message.get('success') is not True:
                raise owner.Denied('helper compile terminal unsuccessful')
            terminal = True
        elif message.get('reason') == 'compiler-artifact':
            selected = select(message, binding, wanted)
            if selected is not None:
                key, path = selected
                if key in found:
                    raise owner.Denied('duplicate helper executable artifact')
                if os.name != 'nt' and not os.access(path, os.X_OK):
                    raise owner.Denied('helper artifact is not executable')
                found[key] = {'key': [key[0], key[1], list(key[2]), key[3]],
                              'artifact': owner.file_subject(path), 'cargo_artifact': message}
    if not terminal or set(found) != set(wanted):
        raise owner.Denied('missing complete helper harness/normal binary artifact bridge')
    receipt = {'schema_version': 1, 'binding': binding,
               'artifacts': [found[key] for key in sorted(found)]}
    location = Path(binding['descriptor']['resources']['temp']) / ('helper-artifacts-' + str(os.getpid()) + '.json')
    with location.open('x', encoding='utf-8') as output:
        json.dump(receipt, output)
    child = {**env, HANDOFF: json.dumps(owner.file_subject(location)),
             PYTHON: str(Path(sys.executable).resolve(strict=True))}
    validate(child)
    return child


def validate(env, candidate=None):
    try:
        subject = json.loads(env[HANDOFF])
        receipt, actual = owner.bounded_json(subject['path'], limit=1024 * 1024)
        binding, mapping = context(env)
        if (subject != actual or type(receipt['schema_version']) is not int
                or receipt['schema_version'] != 1 or receipt['binding'] != binding
                or env.get(PYTHON) != binding['python']['path']):
            raise owner.Denied('helper artifact source/original owner/interpreter changed')
        wanted, found = expected(mapping), {}
        for entry in receipt['artifacts']:
            selected = select(entry['cargo_artifact'], binding, wanted)
            if selected is None:
                raise owner.Denied('helper receipt has unselected artifact')
            key, path = selected
            if (key in found or entry['key'] != [key[0], key[1], list(key[2]), key[3]]
                    or owner.file_subject(path) != entry['artifact']
                    or (os.name != 'nt' and not os.access(path, os.X_OK))):
                raise owner.Denied('helper artifact changed/duplicated/unspawnable')
            found[key] = path
        if set(found) != set(wanted):
            raise owner.Denied('incomplete frozen helper artifact population')
        xtask = found[('xtask', 'xtask', ('bin',), False)]
        if candidate is not None and owner.native_path(candidate) != xtask:
            raise owner.Denied('Cargo compile-time xtask candidate differs from actual admitted artifact')
        return xtask
    except (KeyError, TypeError, ValueError, OSError) as error:
        raise owner.Denied('invalid/stale helper artifact handoff') from error


if __name__ == '__main__':
    try:
        if len(sys.argv) != 3 or sys.argv[1] != '--resolve-xtask':
            raise owner.Denied('expected --resolve-xtask CARGO_COMPILETIME_CANDIDATE')
        print(validate(dict(os.environ), sys.argv[2]))
    except (owner.Denied, KeyError, TypeError, ValueError, OSError) as error:
        print('helper artifact preparation refused: ' + str(error), file=sys.stderr)
        sys.exit(1)
