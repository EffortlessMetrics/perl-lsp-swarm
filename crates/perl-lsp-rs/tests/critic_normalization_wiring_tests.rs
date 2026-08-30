//! Wiring gate for the #7475 critic normalization accounting and the #9062
//! native CriticService cutover.
//!
//! Both native-critic diagnostic transports must route their evaluation
//! through [`perl_lsp_rs_core::tooling::perl_critic::NativeCriticService`]
//! (#9062), which composes the settled candidate/policy seams and logs and
//! counts every finding rejected for a missing producer disposition (#7475).
//! If a transport reverts to composing its own registry/context/candidate/
//! policy pipeline, two paths can again snapshot configuration at different
//! times or flatten metadata differently — this gate turns that regression
//! red instead.
//!
//! #11919 extends the accounting + post-merge-policy gate to the production
//! sites that still collect raw native candidates directly (the quickfix
//! command surface, pending #6969): each must feed its candidates through
//! the accounting entrypoint AND apply the shared post-merge policy
//! (`normalize_with_native_policy`) so alias-aware exclusion/suppression can
//! never leave a second spelling active on a consumer surface. When such a
//! site cuts over to the service, its obligations move with it into the
//! service-ownership gates below.

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

const ACCOUNTING_ENTRYPOINT: &str = "native_finding_candidates_with_accounting(";
const UNACCOUNTED_ENTRYPOINT: &str = "native_finding_candidates(";
const POST_MERGE_POLICY_ENTRYPOINT: &str = "normalize_with_native_policy(";
const UNGUARDED_CANDIDATE_ENTRYPOINT: &str = "native_finding_candidates((";

/// Production sites still collecting raw native candidates directly (#11919):
/// each must use the accounting entrypoint and the shared post-merge policy
/// until its #9062/#6969 cutover moves it behind the service gates.
const DIRECT_NATIVE_CONSUMER_SOURCES: [&str; 1] = ["execute_command/provider.rs"];

/// Every production site that turns native critic findings into user-visible
/// rows. None may use the unaccounted candidate collection entrypoint.
const NATIVE_CONSUMER_SOURCES: [&str; 4] = [
    "runtime/diagnostics.rs",
    "features/diagnostics/pull.rs",
    "runtime/language/code_actions.rs",
    "execute_command/provider.rs",
];

/// Read one production source file of this crate.
///
/// An unreadable instrument is reported as a contextual error, not a panic:
/// the workspace denies `clippy::panic` in tests as well as production, and an
/// instrument failure must stay distinguishable from a wiring violation.
fn production_source(rel_path: &str) -> Result<String, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("src").join(rel_path);
    fs::read_to_string(&path)
        .map_err(|error| format!("production source {} must be readable: {error}", path.display()))
}

fn core_service_source() -> Result<String, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join(SERVICE_SOURCE_REL);
    fs::read_to_string(&path)
        .map_err(|error| format!("service source {} must be readable: {error}", path.display()))
}

/// Read one production source file of the `perl-lsp-rs-core` workspace crate.
fn core_source(rel_path: &str) -> Result<String, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("..").join("perl-lsp-rs-core").join("src").join(rel_path);
    fs::read_to_string(&path)
        .map_err(|error| format!("production source {} must be readable: {error}", path.display()))
}

/// Require every source in a migrated-consumer denominator to exclude the
/// direct, unaccounted candidate entrypoint (#13972).
///
/// Taking the denominator as an iterator makes coverage grow with the caller's
/// manifest. A third consumer cannot be added while this check remains fixed to
/// the first two positions.
fn require_no_unguarded_candidate_collection<'a>(
    sources: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), String> {
    for (rel_path, source) in sources {
        if source.contains(UNGUARDED_CANDIDATE_ENTRYPOINT) {
            return Err(format!(
                "{rel_path} may not collect native candidates outside the accounted service seam"
            ));
        }
    }
    Ok(())
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
fn built_in_identity_constructors_admit_exactly_the_reviewed_overlap_cohort() -> Result<(), String>
{
    let source = core_source("tooling/perl_critic/identity.rs")?;
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
    Ok(())
}

#[test]
fn both_transports_route_native_evaluation_through_the_service() -> Result<(), String> {
    for rel_path in MIGRATED_TRANSPORTS {
        let source = production_source(rel_path)?;
        assert!(
            source.contains(SERVICE_ENTRYPOINT),
            "{rel_path} must route native critic evaluation through NativeCriticService (#9062)"
        );
    }
    Ok(())
}

#[test]
fn direct_native_consumers_account_for_rejected_producer_identities() -> Result<(), String> {
    // #7475/#11919: sites still collecting raw candidates directly must use
    // the accounting entrypoint. Service-routed surfaces carry the identical
    // guarantee inside the shared service via
    // `rejected_producer_identities_stay_accounted_inside_the_service`.
    for rel_path in DIRECT_NATIVE_CONSUMER_SOURCES {
        let source = production_source(rel_path)?;
        assert!(
            source.contains(ACCOUNTING_ENTRYPOINT),
            "{rel_path} must route native candidates through the accounting entrypoint \
             (#7475, #11919)"
        );
    }
    Ok(())
}

#[test]
fn no_production_site_uses_the_unaccounted_candidate_entrypoint() -> Result<(), String> {
    for rel_path in NATIVE_CONSUMER_SOURCES {
        let source = production_source(rel_path)?;
        assert!(
            !source.contains(UNACCOUNTED_ENTRYPOINT),
            "{rel_path} must not collect native candidates outside an accounted seam (#7475)"
        );
    }
    Ok(())
}

#[test]
fn the_service_owns_candidate_collection_and_policy_composition() -> Result<(), String> {
    let service = core_service_source()?;
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
    Ok(())
}

#[test]
fn every_direct_native_consumer_applies_the_post_merge_normalized_policy() -> Result<(), String> {
    // #11919: a consumer that filters raw findings by rule ID before (or
    // instead of) the post-merge policy can leave a second registered spelling
    // active after an alias-aware exclusion or suppression — exactly the
    // bullet-7 defect. Every site still collecting raw candidates must apply
    // `normalize_with_native_policy` itself; service-routed consumers inherit
    // the identical guarantee from
    // `the_service_owns_candidate_collection_and_policy_composition`.
    for rel_path in DIRECT_NATIVE_CONSUMER_SOURCES {
        let source = production_source(rel_path)?;
        assert!(
            source.contains(POST_MERGE_POLICY_ENTRYPOINT),
            "{rel_path} must apply the shared post-merge policy so alias exclusion/suppression \
             cannot leave a second spelling active (#7475 bullet 7, #11919)"
        );
    }
    Ok(())
}

#[test]
fn no_migrated_transport_composes_its_own_native_pipeline() -> Result<(), String> {
    for rel_path in MIGRATED_TRANSPORTS {
        let source = production_source(rel_path)?;
        for composition in SERVICE_ONLY_COMPOSITION {
            assert!(
                !source.contains(composition),
                "{rel_path} bypasses the one native critic service with `{composition}` (#9062)"
            );
        }
    }
    Ok(())
}

#[test]
fn rejected_producer_identities_stay_accounted_inside_the_service() -> Result<(), String> {
    // #7475: findings without a registered producer disposition are logged and
    // counted, never silently dropped. After the #9062 cutover this accounting
    // lives exactly once, inside the service every transport shares.
    let service = core_service_source()?;
    assert!(
        service.contains("account_unresolved_native_identities("),
        "the service must account rejected producer identities (#7475)"
    );
    let sources = MIGRATED_TRANSPORTS
        .iter()
        .map(|rel_path| production_source(rel_path).map(|source| (*rel_path, source)))
        .collect::<Result<Vec<_>, _>>()?;
    require_no_unguarded_candidate_collection(
        sources.iter().map(|(rel_path, source)| (*rel_path, source.as_str())),
    )
}

#[test]
fn unaccounted_candidate_guard_checks_a_third_denominator_entry() -> Result<(), String> {
    let error = require_no_unguarded_candidate_collection([
        ("push", "NativeCriticService::analyze(subject)"),
        ("pull", "NativeCriticService::analyze(subject)"),
        ("future-command", "native_finding_candidates((source, findings))"),
    ])
    .err()
    .ok_or_else(|| "a forbidden third denominator entry must fail the guard".to_string())?;

    if error.contains("future-command") {
        Ok(())
    } else {
        Err(format!("the guard must identify the third denominator entry; got: {error}"))
    }
}

#[test]
fn both_transports_feed_built_in_overlap_observations_into_the_service() -> Result<(), String> {
    // #11918/#9062: the reviewed core-overlap producers reach canonical
    // normalization only if each transport extracts emitter-owned
    // observations from its core rows and hands them to the service subject.
    // A transport that stops consuming them reverts to duplicate or unmerged
    // rows end-to-end.
    for rel_path in MIGRATED_TRANSPORTS {
        let source = production_source(rel_path)?;
        assert!(
            source.contains("take_critic_overlap_observations("),
            "{rel_path} must consume emitter-declared overlap observations (#11918)"
        );
        assert!(
            source.contains("overlap_observations"),
            "{rel_path} must pass overlap observations into the service subject (#9062)"
        );
    }
    Ok(())
}

#[test]
fn superseded_runs_cannot_populate_current_result_storage() -> Result<(), String> {
    // #9062 publication boundary: every migrated transport must consult the
    // run's publishability before projecting rows, so late/stale work can
    // never surface as current.
    for rel_path in MIGRATED_TRANSPORTS {
        let source = production_source(rel_path)?;
        assert!(
            source.contains("is_publishable()"),
            "{rel_path} must check run publishability before publishing (#9062)"
        );
    }
    Ok(())
}

#[test]
fn transport_coincidence_dedup_stays_retired_for_upstream_merged_aliases() -> Result<(), String> {
    // #11918: duplicate prevention for the reviewed core/native alias pairs
    // moved upstream into the normalized seam. The transport-level #5088 XOR
    // dedup must keep exempting exactly those pairs; restoring the collapse
    // here would silently mask a merge regression as "no duplicates".
    let source = production_source("runtime/diagnostics.rs")?;
    let dedup_start = source
        .find("fn dedup_overlapping_diagnostics")
        .ok_or("transport dedup must remain defined for non-migrated pairs")?;
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
    Ok(())
}
