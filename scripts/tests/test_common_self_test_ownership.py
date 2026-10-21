"""Source registration guard; run before package-wide Cargo test/list comparison.

This checks the deliberately small common_self_tests module graph, not Rust cfg
evaluation or runtime coverage. Platform-specific Cargo --list/--ignored evidence
and the exact prepared PERL_LSP_BIN are separate validation requirements.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
CASES = {'perl-dap': {'owner': 'dap_smoke_e2e.rs',
              'modules': ['mod.rs'],
              'cases': {'launch_argument_tests::launch_arguments_preserve_exact_pinned_interpreter': ['test'],
                        'explicit_pin_tests::pinned_identity_decodes_rendered_dap_string': ['test'],
                        'explicit_pin_tests::pinned_identity_decodes_perl_debugger_ordinal': ['test'],
                        'explicit_pin_tests::pinned_identity_decodes_raw_perl_debugger_escapes': ['test'],
                        'explicit_pin_tests::plain_path_ending_in_double_quote_is_not_json_decoded': ['cfg(unix)',
                                                                                                      'test'],
                        'explicit_pin_tests::pinned_identity_rejects_rendered_ambient_path': ['test'],
                        'explicit_pin_tests::pinned_identity_rejects_ambiguous_unclosed_rendered_string': ['test'],
                        'explicit_pin_tests::nested_relative_pin_is_frozen_before_different_launch_cwd': ['test'],
                        'explicit_pin_tests::path_only_candidate_is_frozen_to_absolute_path': ['test'],
                        'explicit_pin_tests::path_lookup_skips_non_executable_candidate': ['cfg(unix)',
                                                                                           'test'],
                        'explicit_pin_tests::path_only_candidate_honors_pathext_for_perl_exe': ['cfg(windows)',
                                                                                                'test'],
                        'explicit_pin_tests::path_candidate_with_extension_does_not_gain_pathext_suffix': ['cfg(windows)',
                                                                                                           'test'],
                        'explicit_pin_tests::uncached_resolver_continues_after_a_failed_path_probe': ['test'],
                        'explicit_pin_tests::bare_name_lookup_uses_only_exe_as_the_implicit_extension': ['cfg(windows)',
                                                                                                         'test'],
                        'explicit_pin_tests::non_utf8_pin_is_rejected_before_launch': ['cfg(unix)',
                                                                                       'test'],
                        'explicit_pin_tests::bare_candidate_absent_from_search_path_fails_closed': ['test'],
                        'explicit_pin_tests::bare_candidate_without_search_path_fails_closed': ['test'],
                        'explicit_pin_tests::absolute_candidate_missing_on_disk_fails_closed': ['test']}}}


def check_ownership(sources, package):
    contract = CASES[package]
    owner = contract['owner']
    for path, text in sources.items():
        if re.search(r'(?:#\[\s*path\s*=\s*|include!\s*\(\s*)[r#]*"[^"]*common_self_tests', text):
            raise AssertionError(f'{package}: aliased self-test registration: {path}')
        if path == owner or path.startswith('common_self_tests/'):
            # This fixed module graph has no block comments. Fail closed if a
            # wrapper could make the regex inventory see commented-out tests.
            if '/*' in text or '*/' in text:
                raise AssertionError(f'{package}: block comments require registration review: {path}')
            if re.search(r'(?m)^\s*#!\[cfg(?:_attr)?\b', text):
                raise AssertionError(f'{package}: conditional owner/module file: {path}')
    if '\nmod common;\nmod common_self_tests;\n' not in sources.get(owner, ''):
        raise AssertionError(f'{package}: missing unconditional owner registration')
    registrations = [path for path, text in sources.items()
                     if re.search(r'(?m)^\s*mod common_self_tests\s*;', text)]
    if registrations != [owner]:
        raise AssertionError(f'{package}: duplicate owner registration: {registrations}')
    for child in contract['modules']:
        if child != 'mod.rs':
            declaration = re.search(r'(?m)^mod ' + re.escape(Path(child).stem) + r';$', sources['common_self_tests/mod.rs'])
            if declaration is None:
                raise AssertionError(f'{package}: missing child registration: {child}')
            if sources['common_self_tests/mod.rs'][:declaration.start()].rstrip().endswith(']'):
                raise AssertionError(f'{package}: conditional child registration: {child}')
    for path, text in sources.items():
        if path.startswith('common/') and re.search(r'(?m)^\s*#\[(?:test|tokio::test)', text):
            raise AssertionError(f'{package}: duplicate self-test in {path}')
    found = {}
    for path, text in sources.items():
        if not path.startswith('common_self_tests/'):
            continue
        for block in re.finditer(r'(?ms)^#\[cfg\(test\)\]\nmod (\w+) \{\n(.*?)^\}', text):
            if text[:block.start()].rstrip().endswith(']'):
                raise AssertionError(f'{package}: unexpected module registration attributes in {path}')
            prefix = '' if path == 'common_self_tests/mod.rs' else Path(path).stem + '::'
            for case in re.finditer(r'(?m)^((?:    #\[[^\n]*\]\n)+)    fn (\w+)\(', block[2]):
                if '#[test]' not in case[1]:
                    continue
                identity = prefix + block[1] + '::' + case[2]
                if identity in found:
                    raise AssertionError(f'{package}: duplicate case {identity}')
                found[identity] = re.findall(r'#\[([^\n]*)\]', case[1])
    if found != contract['cases']:
        raise AssertionError(f'{package}: self-test identity/cfg/ignore set changed: '
                             f'missing={set(contract["cases"]) - set(found)}, '
                             f'extra={set(found) - set(contract["cases"])}')


class CommonSelfTestOwnershipTests(unittest.TestCase):
    def sources(self, package):
        root = ROOT / 'crates' / package / 'tests'
        return {p.relative_to(root).as_posix(): p.read_text(encoding='utf-8')
                for p in sorted(root.rglob('*.rs'))}

    def test_current_registration(self):
        for package in CASES:
            with self.subTest(package=package):
                check_ownership(self.sources(package), package)

    def test_omitted_owner_is_rejected(self):
        for package, contract in CASES.items():
            sources = self.sources(package)
            sources[contract['owner']] = sources[contract['owner']].replace('mod common_self_tests;', '')
            with self.assertRaisesRegex(AssertionError, 'missing unconditional owner'):
                check_ownership(sources, package)

    def test_duplicate_module_is_rejected(self):
        for package in CASES:
            sources = self.sources(package)
            restored = re.search(r'(?ms)^#\[cfg\(test\)\]\nmod \w+ \{\n.*?^\}',
                                 sources['common_self_tests/mod.rs'])[0]
            sources['common/mod.rs'] += '\n' + restored + '\n'
            with self.assertRaisesRegex(AssertionError, 'duplicate self-test'):
                check_ownership(sources, package)

    def test_omitted_child_is_rejected(self):
        for package, contract in CASES.items():
            for child in contract['modules']:
                if child == 'mod.rs':
                    continue
                sources = self.sources(package)
                sources['common_self_tests/mod.rs'] = sources['common_self_tests/mod.rs'].replace(f'mod {Path(child).stem};', '')
                with self.assertRaisesRegex(AssertionError, 'missing child registration'):
                    check_ownership(sources, package)

    def test_missing_case_is_rejected(self):
        for package in CASES:
            sources = self.sources(package)
            path = 'common_self_tests/mod.rs'
            sources[path] = sources[path].replace('    #[test]', '    #[allow(dead_code)]', 1)
            with self.assertRaisesRegex(AssertionError, 'identity/cfg/ignore set changed'):
                check_ownership(sources, package)

    def test_extra_owner_is_rejected(self):
        for package in CASES:
            sources = self.sources(package)
            sources['unrelated.rs'] = 'mod common_self_tests;\n'
            with self.assertRaisesRegex(AssertionError, 'duplicate owner'):
                check_ownership(sources, package)

    def test_aliased_duplicate_is_rejected(self):
        for package in CASES:
            for declaration in [
                '#[path = "../common_self_tests/mod.rs"]\nmod duplicate;',
                'include!("../common_self_tests/mod.rs");',
            ]:
                sources = self.sources(package)
                sources['common/mod.rs'] += '\n' + declaration + '\n'
                with self.assertRaisesRegex(AssertionError, 'aliased self-test registration'):
                    check_ownership(sources, package)

    def test_conditional_module_is_rejected(self):
        for package, contract in CASES.items():
            for path in [contract['owner'], 'common_self_tests/mod.rs']:
                sources = self.sources(package)
                sources[path] = '#![cfg(unix)]\n' + sources[path]
                with self.assertRaisesRegex(AssertionError, 'conditional owner/module file'):
                    check_ownership(sources, package)
            sources = self.sources(package)
            sources['common_self_tests/mod.rs'] = sources['common_self_tests/mod.rs'].replace(
                '#[cfg(test)]', '#[cfg(unix)]\n#[cfg(test)]', 1)
            with self.assertRaisesRegex(AssertionError, 'unexpected module registration attributes'):
                check_ownership(sources, package)
            for child in contract['modules']:
                if child == 'mod.rs':
                    continue
                sources = self.sources(package)
                sources['common_self_tests/mod.rs'] = sources['common_self_tests/mod.rs'].replace(
                    f'mod {Path(child).stem};', f'#[cfg(unix)]\nmod {Path(child).stem};')
                with self.assertRaisesRegex(AssertionError, 'conditional child registration'):
                    check_ownership(sources, package)

    def test_platform_or_ignore_drift_is_rejected(self):
        for package in CASES:
            for replacement in ['#[cfg(windows)]', '#[ignore]']:
                sources = self.sources(package)
                path = next(p for p, t in sources.items()
                            if p.startswith('common_self_tests/') and '    #[cfg(unix)]' in t)
                sources[path] = re.sub(r'(?m)^    #\[cfg\(unix\)\]$', '    ' + replacement, sources[path], count=1)
                with self.assertRaisesRegex(AssertionError, 'identity/cfg/ignore set changed'):
                    check_ownership(sources, package)

    def test_inner_cfg_cannot_disable_a_moved_module(self):
        for package, contract in CASES.items():
            for child in contract['modules']:
                for attribute in ['#![cfg(any())]', '#![cfg_attr(test, cfg(any()))]']:
                    sources = self.sources(package)
                    path = 'common_self_tests/' + child
                    sources[path] = re.sub(
                        r'(mod \w+ \{\n)', r'\1    ' + attribute + '\n',
                        sources[path], count=1)
                    with self.assertRaisesRegex(AssertionError, 'conditional owner/module file'):
                        check_ownership(sources, package)

    def test_commented_module_is_rejected(self):
        for package, contract in CASES.items():
            for child in contract['modules']:
                sources = self.sources(package)
                path = 'common_self_tests/' + child
                sources[path] = '/*\n' + sources[path] + '\n*/\n'
                with self.assertRaisesRegex(AssertionError, 'block comments require registration review'):
                    check_ownership(sources, package)


if __name__ == '__main__':
    unittest.main()
