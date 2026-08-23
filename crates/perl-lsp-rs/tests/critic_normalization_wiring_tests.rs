//! Wiring gate for the #7475 critic normalization accounting and the #9062
//! native CriticService cutover.
//!
//! Both native-critic diagnostic transports must route their evaluation
//! through [`perl_lsp_rs_core::tooling::perl_critic::NativeCriticService`],
//! which takes candidates from `native_finding_candidates_with_accounting`,
//! logging and counting every finding rejected for a missing producer
//! disposition (#7475). If a transport reverts to composing its own
//! registry/context/candidate/policy pipeline (#9062), two paths can again
//! snapshot configuration at different times or flatten metadata differently
//! — this gate turns that regression red instead.
//!
//! The `perl.runCritic` command adapter (`execute_command/provider.rs`) is a
//! pre-migration consumer whose own cutover is #6969; it is deliberately not
//! asserted here.

#![expect(
    clippy::panic,
    reason = "test-only barrier failure is a hard test error, not a production path"
)]

use std::fs;
use std::path::Path;

const SERVICE_ENTRYPOINT: &str = "NativeCriticService::analyze(";
const SERVICE_SOURCE_REL: &str = "../../crates/perl-lsp-rs-core/src/tooling/perl_critic/service.rs";
const MIGRATED_TRANSPORTS: [&str; 2] = ["runtime/diagnostics.rs", "features/diagnostics/pull.rs"];

/// Composition entry points that only the service may call in production.
/// Anything below would let a consumer rebuild its own producer/filter/
/// suppression pipeline instead of consuming one shared run.
const SERVICE_ONLY_COMPOSITION: [&str; 6] = [
    "native_finding_candidates(",
    "normalize_with_native_policy(",
    "NativeCriticPolicy::new(",
    "for_profile_with_config(",
    ".check_unfiltered(",
    "built_in_observation_candidates(",
];

fn production_source(rel_path: &str) -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("src").join(rel_path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("production source {} must be readable: {error}", path.display())
    })
}

fn core_service_source() -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join(SERVICE_SOURCE_REL);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("service source {} must be readable: {error}", path.display())
    })
}

#[test]
fn both_transports_route_native_evaluation_through_the_service() {
    for rel_path in MIGRATED_TRANSPORTS {
        let source = production_source(rel_path);
        assert!(
            source.contains(SERVICE_ENTRYPOINT),
            "{rel_path} must route native critic evaluation through NativeCriticService (#9062)"
        );
    }
}

#[test]
fn the_service_owns_candidate_collection_and_policy_composition() {
    let service = core_service_source();
    assert!(
        service.contains(SERVICE_ENTRYPOINT),
        "NativeCriticService must expose the analyze seam (#9062)"
    );
    for composition in SERVICE_ONLY_COMPOSITION {
        assert!(
            service.contains(composition),
            "the service must compose through the settled {composition} seam"
        );
    }
}

#[test]
fn no_migrated_transport_composes_its_own_native_pipeline() {
    for rel_path in MIGRATED_TRANSPORTS {
        let source = production_source(rel_path);
        for composition in SERVICE_ONLY_COMPOSITION {
            assert!(
                !source.contains(composition),
                "{rel_path} bypasses the one native critic service with `{composition}` (#9062)"
            );
        }
    }
}

#[test]
fn rejected_producer_identities_stay_accounted_inside_the_service() {
    // #7475: findings without a registered producer disposition are logged and
    // counted, never silently dropped. After the #9062 cutover this accounting
    // lives exactly once, inside the service every transport shares.
    let service = core_service_source();
    assert!(
        service.contains("account_unresolved_native_identities("),
        "the service must account rejected producer identities (#7475)"
    );
    let unguarded = "native_finding_candidates((";
    assert!(
        !production_source(MIGRATED_TRANSPORTS[0]).contains(unguarded)
            && !production_source(MIGRATED_TRANSPORTS[1]).contains(unguarded),
        "no migrated transport may collect candidates outside the accounted seam"
    );
}

#[test]
fn both_transports_feed_built_in_overlap_observations_into_the_service() {
    // #11918/#9062: the reviewed core-overlap producers reach canonical
    // normalization only if each transport extracts emitter-owned
    // observations from its core rows and hands them to the service subject.
    // A transport that stops consuming them reverts to duplicate or unmerged
    // rows end-to-end.
    for rel_path in MIGRATED_TRANSPORTS {
        let source = production_source(rel_path);
        assert!(
            source.contains("take_critic_overlap_observations("),
            "{rel_path} must consume emitter-declared overlap observations (#11918)"
        );
        assert!(
            source.contains("overlap_observations"),
            "{rel_path} must pass overlap observations into the service subject (#9062)"
        );
    }
}

#[test]
fn superseded_runs_cannot_populate_current_result_storage() {
    // #9062 publication boundary: every migrated transport must consult the
    // run's publishability before projecting rows, so late/stale work can
    // never surface as current.
    for rel_path in MIGRATED_TRANSPORTS {
        let source = production_source(rel_path);
        assert!(
            source.contains("is_publishable()"),
            "{rel_path} must check run publishability before publishing (#9062)"
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
