//! Wiring gate for the #7475 critic normalization accounting.
//!
//! Both native-critic diagnostic production sites must take their candidates
//! from [`perl_lsp_rs_core::tooling::perl_critic::
//! native_finding_candidates_with_accounting`], which logs and counts every
//! finding rejected for a missing producer disposition. If a site reverts to
//! bare `native_finding_candidates`, undeclared emission shapes silently
//! vanish from the product normalized set — this gate turns that regression
//! red instead.

#![expect(
    clippy::panic,
    reason = "test-only barrier failure is a hard test error, not a production path"
)]

use std::fs;
use std::path::Path;

const ACCOUNTING_ENTRYPOINT: &str = "native_finding_candidates_with_accounting(";
const UNACCOUNTED_ENTRYPOINT: &str = "native_finding_candidates(";

fn production_source(rel_path: &str) -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("src").join(rel_path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("production source {} must be readable: {error}", path.display())
    })
}

#[test]
fn push_diagnostics_native_path_accounts_for_rejected_producer_identities() {
    let source = production_source("runtime/diagnostics.rs");
    assert!(
        source.contains(ACCOUNTING_ENTRYPOINT),
        "runtime/diagnostics.rs must route native candidates through the accounting entrypoint (#7475)"
    );
}

#[test]
fn pull_diagnostics_native_path_accounts_for_rejected_producer_identities() {
    let source = production_source("features/diagnostics/pull.rs");
    assert!(
        source.contains(ACCOUNTING_ENTRYPOINT),
        "features/diagnostics/pull.rs must route native candidates through the accounting entrypoint (#7475)"
    );
}

#[test]
fn no_production_site_uses_the_unaccounted_candidate_entrypoint() {
    for rel_path in ["runtime/diagnostics.rs", "features/diagnostics/pull.rs"] {
        let source = production_source(rel_path);
        let unaccounted_sites: Vec<usize> = source
            .match_indices(UNACCOUNTED_ENTRYPOINT)
            .map(|(offset, _)| offset)
            .filter(|offset| !source[*offset..].starts_with(ACCOUNTING_ENTRYPOINT))
            .collect();
        assert!(
            unaccounted_sites.is_empty(),
            "{rel_path} bypasses rejection accounting at byte offsets {unaccounted_sites:?} (#7475)"
        );
    }
}
