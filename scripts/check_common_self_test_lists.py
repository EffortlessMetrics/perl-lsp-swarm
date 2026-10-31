#!/usr/bin/env python3
"""Compare complete package integration-test listings before/after extraction.

Usage: python scripts/check_common_self_test_lists.py before.json after.json

Each capture has package, revision (40-digit commit), platform (unix/windows),
configuration (target triple, sorted resolved features, no_default_features,
profile), expected_targets
(every integration target selected by Cargo metadata for that configuration),
and suites. Each suite is keyed by Cargo target name and contains the verbatim
`--list --format pretty --color never` stdout as `all`, and
`--list --ignored --format pretty --color never` stdout as `ignored`.
Pretty output includes the required summary, including `0 tests, 0 benchmarks`
when no ignored cases are selected. Terse output omits it and is not accepted.
Do not synthesize a summary or convert truncated output into a valid capture.
Capture each compiled integration executable separately;
retain the commands, metadata, executable hashes and stderr alongside the JSON.
Do not infer expected_targets from the list of binaries that happened to run.

This consumes evidence; it neither builds nor runs Cargo. It proves registration
equivalence within the supplied complete inventory, not execution, CI routing,
or the provenance/completeness of that inventory. Execution/typed-skip logs are
separate evidence, particularly for debuggee_probe_hygiene on Windows.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent / 'tests'))
from test_common_self_test_ownership import CASES


DAP_PRESERVED = {
    'dap_smoke_e2e': ['dap_smoke_e2e'],
    'debuggee_perl_pin_selection': ['valid_pin_selects_the_pinned_usable_identity'],
    'debuggee_perl_override_contract': [
        'debuggee_pin_is_the_single_availability_source_of_truth',
        'attach_does_not_resolve_launch_pin_during_initialization',
    ],
    'debuggee_probe_hygiene': [
        'probe_workspace_cleanup_covers_each_child_exit_path',
        'cleanup_command_wait_error_kills_and_reaps_helper',
        'pid_publication_helpers_hold_past_configured_timeout_on_windows_hosts',
    ],
}


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def parse_listing(output):
    """Retain multiplicity and reject truncated/foreign output, never use a set."""
    cases = Counter()
    lines = output.strip().splitlines()
    require(bool(lines), 'empty listing (missing libtest summary)')
    summary = re.fullmatch(r'(\d+) tests?, (\d+) benchmarks?', lines[-1])
    require(summary is not None, 'missing libtest summary')
    tests = benchmarks = 0
    for line in lines[:-1]:
        if not line.strip():
            continue
        match = re.fullmatch(r'(.+): (test|benchmark)', line)
        require(match is not None, f'unrecognized listing line: {line!r}')
        kind = match[2]
        cases[(match[1], kind)] += 1
        tests += kind == 'test'
        benchmarks += kind == 'benchmark'
    require((tests, benchmarks) == tuple(map(int, summary.groups())), 'listing count mismatch')
    require(all(count == 1 for count in cases.values()), 'duplicate listing selector')
    return cases


def compare(before, after):
    for field in ['package', 'platform', 'configuration', 'expected_targets']:
        require(before[field] == after[field], f'capture {field} changed')
    package, platform = before['package'], before['platform']
    require(package in CASES, 'unsupported package')
    require(platform in ['unix', 'windows'], 'unsupported platform')
    config = before['configuration']
    require(bool(config['target']) and isinstance(config['features'], list)
            and isinstance(config['no_default_features'], bool) and bool(config['profile']),
            'missing configuration')
    require(config['features'] == sorted(set(config['features'])), 'features must be sorted and unique')
    targets = before['expected_targets']
    require(bool(targets) and targets == sorted(set(targets)), 'targets must be nonempty, sorted and unique')
    contract = CASES[package]
    owner = Path(contract['owner']).stem
    expected = Counter({(name, 'test'): 1 for name, attrs in contract['cases'].items()
                        if f'cfg({"windows" if platform == "unix" else "unix"})' not in attrs})
    expected_ignored = Counter({(name, kind): count for (name, kind), count in expected.items()
                                if any(a.startswith('ignore') for a in contract['cases'][name])})
    old_names = {'common::' + name for name in contract['cases']}
    captures = []
    for capture, prefix, other_prefix in [(before, 'common::', 'common_self_tests::'),
                                         (after, 'common_self_tests::', 'common::')]:
        require(re.fullmatch(r'[0-9a-f]{40}', capture['revision']) is not None, 'missing exact revision')
        require(sorted(capture['suites']) == targets, 'incomplete package target inventory')
        require(owner in targets, 'owner target missing')
        if package == 'perl-dap':
            require(set(DAP_PRESERVED) <= set(targets), 'preserved DAP target missing')
        parsed = {}
        for suite, listings in capture['suites'].items():
            all_cases, ignored = parse_listing(listings['all']), parse_listing(listings['ignored'])
            require(not (ignored - all_cases), f'{suite}: ignored selectors absent from all listing')
            require(not any((name.startswith(other_prefix) if capture is before else name in old_names)
                            for name, _ in all_cases),
                    f'{suite}: unexpected old/new self-test registration')
            is_moved = lambda name: (name in old_names if capture is before else name.startswith(prefix))
            moved = Counter({(name.removeprefix(prefix), kind): count
                             for (name, kind), count in all_cases.items() if is_moved(name)})
            moved_ignored = Counter({(name.removeprefix(prefix), kind): count
                                     for (name, kind), count in ignored.items() if is_moved(name)})
            if moved or suite == owner:
                require(moved == expected, f'{suite}: missing/extra/platform self-test selectors')
                require(moved_ignored == expected_ignored, f'{suite}: self-test ignored set changed')
            if capture is after and suite != owner:
                require(not moved, f'{suite}: duplicate self-test owner')
            retained = Counter({key: n for key, n in all_cases.items() if not is_moved(key[0])})
            retained_ignored = Counter({key: n for key, n in ignored.items() if not is_moved(key[0])})
            if package == 'perl-dap' and suite in DAP_PRESERVED:
                require(retained == Counter({(name, 'test'): 1 for name in DAP_PRESERVED[suite]}),
                        f'{suite}: preserved selectors changed or empty')
                require(not retained_ignored, f'{suite}: preserved selectors unexpectedly ignored')
            parsed[suite] = (retained, retained_ignored)
        captures.append(parsed)
    require(captures[0] == captures[1], 'non-self-test registrations or ignored sets changed')
    return {'package': package, 'platform': platform, 'before': before['revision'],
            'after': after['revision'], 'integration_targets': len(targets),
            'owner_self_tests': sum(expected.values()), 'registration_equivalence': 'PASS',
            'execution': 'NOT_PROVEN', 'ci_selection': 'NOT_PROVEN'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('before', type=Path)
    parser.add_argument('after', type=Path)
    args = parser.parse_args()
    print(json.dumps(compare(json.loads(args.before.read_text(encoding='utf-8')),
                             json.loads(args.after.read_text(encoding='utf-8'))), indent=2))
