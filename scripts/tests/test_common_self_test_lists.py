"""Synthetic negative controls for the captured-registration comparator.

These fixtures are not captured Rust execution evidence.
"""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from check_common_self_test_lists import CASES, DAP_PRESERVED, compare, parse_listing


def listing(names):
    return ''.join(f'{name}: test\n' for name in names) + f'\n{len(names)} tests, 0 benchmarks\n'


def fixture(package, platform):
    owner = Path(CASES[package]['owner']).stem
    opposite = 'windows' if platform == 'unix' else 'unix'
    names = [name for name, attrs in CASES[package]['cases'].items() if f'cfg({opposite})' not in attrs]
    retained = dict(DAP_PRESERVED) if package == 'perl-dap' else {owner: ['existing_owner_case']}
    retained['unrelated'] = ['common::unrelated_case', 'some_ignored_case']
    captures = []
    for prefix in ['common::', 'common_self_tests::']:
        suites = {}
        for suite, cases in retained.items():
            self_tests = names if suite == owner or (suite != 'unrelated' and prefix == 'common::') else []
            suites[suite] = {'all': listing(cases + [prefix + name for name in self_tests]),
                             'ignored': listing(['some_ignored_case'] if suite == 'unrelated' else [])}
        captures.append({'package': package, 'platform': platform,
                         'revision': ('a' if prefix == 'common::' else 'b') * 40,
                         'configuration': {'target': 'test-triple', 'features': [],
                                           'no_default_features': False, 'profile': 'test'},
                         'expected_targets': sorted(suites), 'suites': suites})
    return captures


class CapturedRegistrationTests(unittest.TestCase):
    def test_platform_fixtures_pass_without_claiming_execution(self):
        for package in CASES:
            for platform in ['unix', 'windows']:
                result = compare(*fixture(package, platform))
                self.assertEqual(result['execution'], 'NOT_PROVEN')
                self.assertEqual(result['ci_selection'], 'NOT_PROVEN')

    def test_omitted_owner_and_missing_case_fail(self):
        for package in CASES:
            for omit_all in [False, True]:
                before, after = fixture(package, 'windows')
                owner = Path(CASES[package]['owner']).stem
                names = [name for name, _ in parse_listing(after['suites'][owner]['all'])]
                moved = [name for name in names if name.startswith('common_self_tests::')]
                removed = moved if omit_all else moved[:1]
                after['suites'][owner]['all'] = listing([name for name in names if name not in removed])
                with self.assertRaisesRegex(AssertionError, 'self-test selectors'):
                    compare(before, after)

    def test_restored_duplicate_module_and_second_owner_fail(self):
        for package in CASES:
            for prefix in ['common::', 'common_self_tests::']:
                before, after = fixture(package, 'windows')
                names = [name for name, _ in parse_listing(after['suites']['unrelated']['all'])]
                moved = [prefix + name for name, attrs in CASES[package]['cases'].items()
                         if 'cfg(unix)' not in attrs]
                after['suites']['unrelated']['all'] = listing(names + moved)
                with self.assertRaisesRegex(AssertionError, 'unexpected old/new|duplicate self-test owner'):
                    compare(before, after)

    def test_empty_override_contract_is_rejected_on_either_side(self):
        for side in [0, 1]:
            captures = fixture('perl-dap', 'windows')
            captures[side]['suites']['debuggee_perl_override_contract']['all'] = listing([])
            with self.assertRaisesRegex(AssertionError, 'preserved selectors changed or empty'):
                compare(*captures)

    def test_extra_smoke_selector_is_rejected_even_if_both_sides_match(self):
        captures = fixture('perl-dap', 'windows')
        for capture in captures:
            output = capture['suites']['dap_smoke_e2e']
            output['all'] = listing([name for name, _ in parse_listing(output['all'])] + ['extra_smoke'])
        with self.assertRaisesRegex(AssertionError, 'preserved selectors changed or empty'):
            compare(*captures)

    def test_duplicated_hygiene_selector_is_rejected(self):
        captures = fixture('perl-dap', 'windows')
        output = captures[1]['suites']['debuggee_probe_hygiene']
        output['all'] = listing(DAP_PRESERVED['debuggee_probe_hygiene'] +
                                ['probe_workspace_cleanup_covers_each_child_exit_path'])
        with self.assertRaisesRegex(AssertionError, 'duplicate listing selector'):
            compare(*captures)

    def test_non_self_test_and_ignore_drift_fail(self):
        for field in ['all', 'ignored']:
            captures = fixture('perl-dap', 'windows')
            captures[1]['suites']['unrelated'][field] = listing(['some_ignored_case']) if field == 'all' else listing([])
            with self.assertRaisesRegex(AssertionError, 'non-self-test registrations or ignored sets changed'):
                compare(*captures)

    def test_self_test_ignore_and_platform_drift_fail(self):
        for package in CASES:
            captures = fixture(package, 'windows')
            owner = Path(CASES[package]['owner']).stem
            name = next(iter(CASES[package]['cases']))
            captures[1]['suites'][owner]['ignored'] = listing(['common_self_tests::' + name])
            with self.assertRaisesRegex(AssertionError, 'self-test ignored set changed'):
                compare(*captures)
            captures = fixture(package, 'windows')
            unix_name = next(name for name, attrs in CASES[package]['cases'].items() if 'cfg(unix)' in attrs)
            output = captures[1]['suites'][owner]
            output['all'] = listing([name for name, _ in parse_listing(output['all'])] + ['common_self_tests::' + unix_name])
            with self.assertRaisesRegex(AssertionError, 'self-test selectors'):
                compare(*captures)

    def test_missing_suite_in_both_captures_fails(self):
        captures = fixture('perl-dap', 'windows')
        for capture in captures:
            del capture['suites']['unrelated']
        with self.assertRaisesRegex(AssertionError, 'incomplete package target inventory'):
            compare(*captures)

    def test_missing_truncated_and_failed_listing_output_fails(self):
        for output in ['', 'example: test', 'example: test\n0 tests, 0 benchmarks',
                       'error: test executable failed\n0 tests, 0 benchmarks']:
            with self.assertRaises(AssertionError):
                parse_listing(output)

    def test_configuration_and_revision_are_required(self):
        captures = fixture('perl-dap', 'windows')
        captures[1]['configuration']['features'] = ['some-feature']
        with self.assertRaisesRegex(AssertionError, 'configuration changed'):
            compare(*captures)
        captures = fixture('perl-dap', 'windows')
        captures[1]['revision'] = 'HEAD'
        with self.assertRaisesRegex(AssertionError, 'exact revision'):
            compare(*captures)


if __name__ == '__main__':
    unittest.main()
