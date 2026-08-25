//! Wiring gate for the #7475 critic normalization accounting.
//!
//! Both native-critic diagnostic production sites must take their candidates
//! from [`perl_lsp_rs_core::tooling::perl_critic::
//! native_finding_candidates_with_accounting`], which logs and counts every
//! finding rejected for a missing producer disposition. If a site reverts to
//! bare `native_finding_candidates`, undeclared emission shapes silently
//! vanish from the product normalized set — this gate turns that regression
//! red instead.
//!
//! #11919 extends the gate to the two remaining raw-producer consumers (the
//! quickfix surface and the `perl.runCritic` command surface): both must feed
//! their candidates through the accounting entrypoint AND apply the shared
//! post-merge policy (`normalize_with_native_policy`) so alias-aware
//! exclusion/suppression can never leave a second spelling active on a
//! consumer surface.

use std::fs;
use std::path::Path;

const ACCOUNTING_ENTRYPOINT: &str = "native_finding_candidates_with_accounting(";
const UNACCOUNTED_ENTRYPOINT: &str = "native_finding_candidates(";
const POST_MERGE_POLICY_ENTRYPOINT: &str = "normalize_with_native_policy(";

/// Every production site that turns native critic findings into user-visible
/// rows must route through the shared accounting + post-merge policy path.
const NATIVE_CONSUMER_SOURCES: [&str; 4] = [
    "runtime/diagnostics.rs",
    "features/diagnostics/pull.rs",
    "runtime/language/code_actions.rs",
    "execute_command/provider.rs",
];

fn production_source(rel_path: &str) -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("src").join(rel_path);
    perl_test_must::must_with(
        fs::read_to_string(&path),
        format!("production source {} must be readable", path.display()),
    )
}

/// Read one production source file of the `perl-lsp-rs-core` workspace crate.
fn core_source(rel_path: &str) -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("..").join("perl-lsp-rs-core").join("src").join(rel_path);
    perl_test_must::must_with(
        fs::read_to_string(&path),
        format!("production source {} must be readable", path.display()),
    )
}

/// The reviewed built-in overlap cohort (#11915/#11918): exactly these seven
/// checked identity constructors may exist. An eighth constructor must turn
/// this gate red until it is consciously admitted here.
const BUILT_IN_IDENTITY_CONSTRUCTORS: [&str; 7] = [
    "built_in_literal_undef_comparison",
    "built_in_potentially_undef_comparison",
    "built_in_backtick_exec",
    "built_in_qx_exec",
    "built_in_readpipe_exec",
    "built_in_system_call",
    "built_in_exec_call",
];

#[test]
fn built_in_identity_constructors_admit_exactly_the_reviewed_overlap_cohort() {
    let source = core_source("tooling/perl_critic/identity.rs");
    let mut declared_constructors: Vec<String> = source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            // Any function visibility/form declaring a built_in_ constructor
            // counts, so a `pub(crate) fn` or non-const variant cannot sneak
            // an eighth cohort member past the pin.
            if !trimmed.starts_with("pub") || !trimmed.contains("fn ") {
                return None;
            }
            let name = trimmed
                .split(['(', '<', ':', ' '])
                .find_map(|token| token.strip_prefix("built_in_"))?;
            Some(name.to_string())
        })
        .collect();
    declared_constructors.sort();

    let mut expected: Vec<String> = BUILT_IN_IDENTITY_CONSTRUCTORS
        .iter()
        .map(|name| name.trim_start_matches("built_in_").to_string())
        .collect();
    expected.sort();

    assert_eq!(
        declared_constructors.len(),
        expected.len(),
        "the reviewed overlap cohort admits exactly {} checked built-in identity constructors; found {declared_constructors:?}",
        expected.len()
    );
    assert_eq!(
        declared_constructors, expected,
        "a new built-in identity constructor must be consciously admitted into the reviewed overlap cohort list"
    );
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
fn code_action_native_path_accounts_for_rejected_producer_identities() {
    let source = production_source("runtime/language/code_actions.rs");
    assert!(
        source.contains(ACCOUNTING_ENTRYPOINT),
        "runtime/language/code_actions.rs must route native candidates through the \
         accounting entrypoint (#7475, #11919)"
    );
}

#[test]
fn execute_command_native_path_accounts_for_rejected_producer_identities() {
    let source = production_source("execute_command/provider.rs");
    assert!(
        source.contains(ACCOUNTING_ENTRYPOINT),
        "execute_command/provider.rs must route native candidates through the \
         accounting entrypoint (#7475, #11919)"
    );
}

#[test]
fn no_production_site_uses_the_unaccounted_candidate_entrypoint() {
    for rel_path in NATIVE_CONSUMER_SOURCES {
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
fn every_native_consumer_applies_the_post_merge_normalized_policy() {
    // #11919: a consumer that filters raw findings by rule ID before (or
    // instead of) the post-merge policy can leave a second registered spelling
    // active after an alias-aware exclusion or suppression — exactly the
    // bullet-7 defect. Every consumer must apply
    // `normalize_with_native_policy` to its candidates and iterate only the
    // admitted normalized rows.
    for rel_path in NATIVE_CONSUMER_SOURCES {
        let source = production_source(rel_path);
        assert!(
            source.contains(POST_MERGE_POLICY_ENTRYPOINT),
            "{rel_path} must apply the shared post-merge policy so alias exclusion/suppression \
             cannot leave a second spelling active (#7475 bullet 7, #11919)"
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
    let dedup_start = perl_test_must::must_some_with(
        source.find("fn dedup_overlapping_diagnostics"),
        "transport dedup must remain defined for non-migrated pairs",
    );
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
