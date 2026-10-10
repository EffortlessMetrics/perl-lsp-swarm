"""Compile the actual shared std-only resolver seam, without workspace Cargo.

This proves the seam's refusal/return contract, not full owning LSP harnesses
or native Windows execution. Platform artifact contracts live in adapter tests.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'crates/perl-lsp-rs/tests/common/admitted_binary.rs'
TESTS = r'''
#[cfg(test)]
mod contract_tests {
    use super::*;
    fn env(name: &str) -> Option<OsString> {
        match name {
            "CARGO_ADMITTED_RESOURCES" => Some("descriptor".into()),
            "CARGO_ADMITTED_PERLLSP_HANDOFF" => Some("receipt".into()),
            "CARGO_ADMITTED_PYTHON" => Some("python".into()),
            "PERL_LSP_BIN" => Some(std::env::temp_dir().join("perllsp").into_os_string()),
            _ => None,
        }
    }
    #[test]
    fn developer_mode_leaves_existing_resolution_in_charge() {
        assert!(resolve_with("debug", |_| None, |_, _| panic!("must not validate")).is_none());
    }
    #[test]
    fn partial_admission_never_reaches_fallback_or_validator() {
        for absent in ["CARGO_ADMITTED_RESOURCES", "CARGO_ADMITTED_PERLLSP_HANDOFF",
                       "CARGO_ADMITTED_PYTHON", "PERL_LSP_BIN"] {
            let result = resolve_with("debug", |name| if name == absent {None} else {env(name)},
                                      |_, _| panic!("incomplete binding must refuse"));
            assert!(result.expect("admitted mode").is_err(), "{absent}");
        }
    }
    #[test]
    fn successful_binding_has_one_exact_path_and_active_profile() {
        for profile in ["debug", "release"] {
            let path = std::env::temp_dir().join("perllsp");
            let result = resolve_with(profile, env, |python, observed| {
                assert_eq!(python, OsString::from("python"));
                assert_eq!(observed, profile);
                Ok(format!("{}\r\n", path.display()))
            });
            assert_eq!(result.expect("admitted").expect("valid"), path);
        }
    }
    #[test]
    fn validator_absent_stale_mismatched_and_unspawnable_errors_propagate() {
        for reason in ["absent", "stale lease", "wrong source", "wrong profile", "unspawnable"] {
            let result = resolve_with("debug", env, |_, _| Err(reason.to_owned()));
            assert_eq!(result.expect("admitted").expect_err("refused"), reason);
        }
    }
    #[test]
    fn malformed_validator_success_cannot_become_candidate() {
        for output in ["", "perllsp", "/different/perllsp", "/one\n/two", "/one\0"] {
            let result = resolve_with("debug", env, |_, _| Ok(output.into()));
            assert!(result.expect("admitted").is_err(), "{output:?}");
        }
    }
    #[test]
    fn handoff_alone_cannot_silently_become_developer_mode() {
        let result = resolve_with("debug", |name| {
            (name == "CARGO_ADMITTED_PERLLSP_HANDOFF").then(|| "receipt".into())
        }, |_, _| panic!("must refuse missing owner"));
        assert!(result.expect("admitted mode").is_err());
    }
}
'''


class ResolverControls(unittest.TestCase):
    def test_actual_shared_resolver_without_product_cargo(self):
        rustc = os.environ.get('RESOLVER_CONTRACT_RUSTC')
        if not rustc:
            self.fail('set RESOLVER_CONTRACT_RUSTC to the installed native compiler')
        with tempfile.TemporaryDirectory(prefix='perllsp-resolver-contract-') as directory:
            root = Path(directory)
            source = root / 'resolver.rs'
            source.write_text(SOURCE.read_text() + TESTS)
            binary = root / ('resolver.exe' if os.name == 'nt' else 'resolver')
            env = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'crates/perl-lsp-rs'))
            subprocess.run([rustc, '--edition=2024', '--test', str(source), '-o', str(binary)],
                           env=env, check=True)
            subprocess.run([str(binary), '--nocapture'], env=env, check=True)

    def test_both_consumers_validate_before_developer_candidates(self):
        common = (ROOT / 'crates/perl-lsp-rs/tests/common/binary_resolution.rs').read_text()
        entry = common[common.index('pub(crate) fn resolve_perl_lsp_cmds()'):]
        self.assertLess(entry.index('admitted_binary::resolve(active_profile())'), entry.index('resolve_explicit_candidates('))
        self.assertIn('Command::new(must(path))', entry)
        self.assertIn('return vec![command].into_iter();', entry)
        support = (ROOT / 'crates/perl-lsp-rs/tests/support/mod.rs').read_text()
        entry = support[support.index('pub fn product_binary_path()'):]
        self.assertLess(entry.index('admitted_binary::resolve('), entry.index('std::env::var("PERL_LSP_BIN")'))
        self.assertIn('return path.map(', entry)
        self.assertIn('get_or_init(build_canonical_perllsp)', entry)  # developer freshness retained


if __name__ == '__main__':
    unittest.main()
