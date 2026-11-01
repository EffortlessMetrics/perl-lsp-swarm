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
                        'explicit_pin_tests::absolute_candidate_missing_on_disk_fails_closed': ['test']}},
 'perl-lsp-rs': {'owner': 'lsp_test_infrastructure_validation.rs',
                 'modules': ['mod.rs',
                             'binary_resolution.rs',
                             'handshake.rs',
                             'protocol_io.rs',
                             'test_reliability.rs',
                             'timeout_scaler.rs'],
                 'cases': {'stderr_tests::stderr_fragments_reconstruct_across_reads': ['test'],
                           'stderr_tests::stderr_preserves_utf8_code_points_split_across_reads': ['test'],
                           'stderr_tests::unterminated_stderr_keeps_newest_bounded_bytes': ['test'],
                           'stderr_tests::stderr_echo_lines_survive_tail_eviction': ['test'],
                           'binary_resolution::tests::target_directory_resolves_relative_and_absolute_values': ['test'],
                           'binary_resolution::tests::missing_prebuilt_binary_refuses_cargo_run_fallback': ['test'],
                           'binary_resolution::tests::cargo_classifier_uses_the_executable_stem_not_a_string_prefix': ['test'],
                           'binary_resolution::tests::resolution_never_offers_a_cargo_candidate': ['test'],
                           'binary_resolution::tests::probed_paths_carry_the_executable_suffix_and_profile_order': ['test'],
                           'binary_resolution::tests::current_executable_is_accepted_as_a_real_binary': ['test'],
                           'binary_resolution::tests::valid_explicit_candidate_short_circuits_lower_priority_resolution': ['test'],
                           'binary_resolution::tests::invalid_explicit_candidate_is_skipped_for_a_valid_lower_priority_candidate': ['test'],
                           'binary_resolution::tests::non_executable_explicit_candidate_is_rejected': ['cfg(unix)',
                                                                                                       'test'],
                           'binary_resolution::tests::missing_relative_explicit_candidate_is_rejected': ['test'],
                           'binary_resolution::tests::stale_opposite_profile_artifact_is_refused_not_reused': ['test'],
                           'binary_resolution::tests::failed_prebuild_names_a_planted_leftover_without_reusing_it': ['test'],
                           'binary_resolution::tests::empty_target_directory_requires_a_build_without_opposite_context': ['test'],
                           'binary_resolution::tests::non_regular_and_non_executable_paths_refuse_cargo_run_fallback': ['cfg(unix)',
                                                                                                                        'test'],
                           'handshake::initialize_lsp_with_root_path_tests::initializes_server_against_an_explicit_empty_root': ['test'],
                           'protocol_io::read_notification_for_uri_tests::returns_matching_notification_and_requeues_the_rest': ['test'],
                           'protocol_io::read_notification_for_uri_tests::scans_buffered_pending_before_polling': ['test'],
                           'protocol_io::read_notification_for_uri_tests::mismatched_shapes_never_match_and_empty_transport_times_out': ['test'],
                           'test_reliability::tests::test_environment_validation': ['test'],
                           'test_reliability::tests::test_timeout_multiplier_reasonable': ['test'],
                           'test_reliability::tests::test_resource_monitor_tracks_time': ['test'],
                           'test_reliability::tests::test_graceful_degradation_succeeds_eventually': ['test'],
                           'test_reliability::tests::test_truncate_long_strings': ['test'],
                           'timeout_scaler::tests::test_effective_thread_count_returns_at_least_one': ['test'],
                           'timeout_scaler::tests::test_adaptive_timeout_returns_reasonable_duration': ['test'],
                           'timeout_scaler::tests::test_scaled_timeout_applies_factor': ['test'],
                           'timeout_scaler::tests::test_short_timeout_is_shorter_than_adaptive': ['test'],
                           'timeout_scaler::tests::test_init_timeout_is_longer_than_adaptive': ['test'],
                           'timeout_scaler::tests::test_timeout_profiles_have_correct_ordering': ['test'],
                           'timeout_scaler::tests::test_retry_config_exponential_backoff': ['test']}}}


def registration_text(text):
    # Ignore line-comment trivia without treating // inside Rust strings as a
    # comment. Block comments are outside this fixed graph and rejected below.
    tokens = r'''r(?P<hashes>\#{0,16})".*?"(?P=hashes)|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])'|//[^\n]*'''
    text = re.sub(tokens, lambda m: '' if m[0].startswith('//') else m[0], text, flags=re.S)
    return '\n'.join(line.rstrip() for line in text.splitlines() if line.strip()) + '\n'


def check_ownership(sources, package):
    contract = CASES[package]
    owner = contract['owner']
    sources = {path: registration_text(text) for path, text in sources.items()}
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

    def test_comments_cannot_hide_disabling_attributes(self):
        for package, contract in CASES.items():
            for child in contract['modules']:
                for case_attribute in [False, True]:
                    for comment in [' // explanation', '\n// explanation\n']:
                        sources = self.sources(package)
                        path = 'common_self_tests/' + child
                        anchor = '    #[test]' if case_attribute else '#[cfg(test)]'
                        indent = '    ' if case_attribute else ''
                        sources[path] = sources[path].replace(
                            anchor, indent + '#[cfg(any())]' + comment + '\n' + anchor, 1)
                        with self.assertRaisesRegex(AssertionError, 'registration attributes|identity/cfg/ignore'):
                            check_ownership(sources, package)


if __name__ == '__main__':
    unittest.main()
