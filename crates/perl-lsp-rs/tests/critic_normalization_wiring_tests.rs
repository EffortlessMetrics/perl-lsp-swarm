//! Wiring gate for the #7475 critic normalization accounting.
//!
//! Both native-critic diagnostic production sites must take their candidates
//! from [`perl_lsp_rs_core::tooling::perl_critic::
//! native_finding_candidates_with_accounting`], which logs and counts every
//! finding rejected for a missing producer disposition. If a site reverts to
//! bare `native_finding_candidates`, undeclared emission shapes silently
//! vanish from the product normalized set — this gate turns that regression
//! red instead.

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

#[test]
fn both_transports_feed_built_in_overlap_observations_into_the_seam() {
    // #11918: the reviewed core-overlap producers reach
    // `normalize_critic_findings` only if both diagnostic transports extract
    // emitter-owned observations and chain them with the native candidates.
    // A transport that stops consuming them reverts to duplicate or unmerged
    // rows end-to-end.
    for rel_path in ["runtime/diagnostics.rs", "features/diagnostics/pull.rs"] {
        let source = production_source(rel_path);
        assert!(
            source.contains("take_critic_overlap_observations("),
            "{rel_path} must consume emitter-declared overlap observations (#11918)"
        );
        assert!(
            source.contains("built_in_observation_candidates("),
            "{rel_path} must convert overlap observations into seam candidates (#11918)"
        );
    }
}

#[test]
fn transport_coincidence_dedup_stays_retired_for_upstream_merged_aliases() {
    // #11918: duplicate prevention for the reviewed core/native alias pairs
    // moved upstream into the normalized seam. The transport-level #5088 XOR
    // dedup must keep exempting exactly those pairs; restoring the collapse
    // here would silently mask a merge regression as "no duplicates".
    let source = production_source("runtime/diagnostics.rs");
    let dedup_start = source
        .find("fn dedup_overlapping_diagnostics")
        .unwrap_or_else(|| panic!("transport dedup must remain defined for non-migrated pairs"));
    let dedup_body = &source[dedup_start..dedup_start + 2000];
    assert!(
        dedup_body.contains("is_upstream_merged_alias_pair("),
        "the dedup predicate must exempt the upstream-merged alias pairs (#11918)"
    );
    assert!(
        source.contains("fn is_upstream_merged_alias_pair"),
        "the exemption table must stay next to the transport dedup (#11918)"
    );
    for retired in [
        "\"PL404\"",
        "\"PL601\"",
        "\"PL603\"",
        "\"PL604\"",
        "\"PL606\"",
        "\"native.common.undef_comparison\"",
        "\"native.security.backtick_exec\"",
        "\"native.security.qx_readpipe\"",
        "\"native.security.system_exec\"",
    ] {
        assert!(source.contains(retired), "the exemption must keep covering {retired} (#11918)");
    }
}
