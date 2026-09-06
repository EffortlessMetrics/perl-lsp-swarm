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
//! consumer surface. That code-action / command gate stays a separate
//! denominator; this leaf does not merge it into the diagnostic-transport set.
//!
//! #13972 makes the unaccounted-candidate ownership check iterate every
//! `MIGRATED_TRANSPORTS` entry instead of indexing two fixed positions, and
//! matches the unaccounted identifier independently of inline-tuple argument
//! shape so a later third consumer cannot silently escape this one negative
//! control.

use std::fs;
use std::path::Path;

const ACCOUNTING_ENTRYPOINT: &str = "native_finding_candidates_with_accounting(";
const POST_MERGE_POLICY_ENTRYPOINT: &str = "normalize_with_native_policy(";
const UNACCOUNTED_IDENT: &str = "native_finding_candidates";

/// Diagnostic transports currently in the native-critic ownership gate.
/// Adding a third migrated path automatically subjects it to the
/// unaccounted-candidate check because that check iterates this denominator
/// rather than indexing `[0]` / `[1]` (#13972).
const MIGRATED_TRANSPORTS: [&str; 2] = ["runtime/diagnostics.rs", "features/diagnostics/pull.rs"];

/// Remaining raw-producer consumers (#11919). Kept as a separate gate so this
/// leaf does not merge code-action / command ownership into the diagnostic
/// transport denominator.
const DIRECT_NATIVE_CONSUMER_SOURCES: [&str; 2] =
    ["runtime/language/code_actions.rs", "execute_command/provider.rs"];

/// Read one production source file of this crate.
///
/// An unreadable instrument is reported as a contextual error, not a panic:
/// an instrument failure must stay distinguishable from a wiring violation.
fn production_source(rel_path: &str) -> Result<String, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("src").join(rel_path);
    fs::read_to_string(&path)
        .map_err(|error| format!("production source {} must be readable: {error}", path.display()))
}

/// Read one production source file of the `perl-lsp-rs-core` workspace crate.
fn core_source(rel_path: &str) -> Result<String, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("..").join("perl-lsp-rs-core").join("src").join(rel_path);
    fs::read_to_string(&path)
        .map_err(|error| format!("production source {} must be readable: {error}", path.display()))
}

fn load_sources<'a>(rel_paths: &'a [&str]) -> Result<Vec<(&'a str, String)>, String> {
    rel_paths
        .iter()
        .copied()
        .map(|rel_path| production_source(rel_path).map(|source| (rel_path, source)))
        .collect()
}

/// Offsets of calls to the unaccounted candidate collector.
///
/// Matches the function identifier as a token, then optional whitespace, then
/// `(`. That catches `native_finding_candidates(args)` and a line break before
/// the argument list, which the exact spelling `native_finding_candidates((`
/// silently accepted. A longer identifier such as
/// `native_finding_candidates_with_accounting` is a different function and is
/// skipped.
fn unaccounted_candidate_offsets(source: &str) -> Vec<usize> {
    source
        .match_indices(UNACCOUNTED_IDENT)
        .filter_map(|(offset, _)| {
            let after_ident = source.get(offset.checked_add(UNACCOUNTED_IDENT.len())?..)?;
            if after_ident.starts_with(|ch: char| ch == '_' || ch.is_ascii_alphanumeric()) {
                return None;
            }
            let after_ws = after_ident.trim_start_matches([' ', '\t', '\n', '\r']);
            after_ws.starts_with('(').then_some(offset)
        })
        .collect()
}

/// Require every source in a migrated-consumer denominator to exclude the
/// unaccounted candidate entrypoint (#13972).
///
/// Taking the denominator as an iterator makes coverage grow with the caller's
/// manifest. A third consumer cannot be added while this check remains fixed to
/// the first two positions.
fn require_no_unguarded_candidate_collection<'a>(
    sources: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<(), String> {
    for (rel_path, source) in sources {
        let offsets = unaccounted_candidate_offsets(source);
        if !offsets.is_empty() {
            return Err(format!(
                "{rel_path} bypasses rejection accounting at byte offsets {offsets:?} (#7475, #13972)"
            ));
        }
    }
    Ok(())
}

fn require_each_contains<'a>(
    sources: impl IntoIterator<Item = (&'a str, &'a str)>,
    token: &str,
    obligation: &str,
) -> Result<(), String> {
    for (rel_path, source) in sources {
        if !source.contains(token) {
            return Err(format!("{rel_path} {obligation}"));
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

    if declared_constructors.len() != expected.len() {
        return Err(format!(
            "the reviewed overlap cohort admits exactly {} checked built-in identity constructors; found {declared_constructors:?}",
            expected.len()
        ));
    }
    if declared_constructors != expected {
        return Err(format!(
            "a new built-in identity constructor must be consciously admitted into the reviewed overlap cohort list; found {declared_constructors:?}"
        ));
    }
    Ok(())
}

#[test]
fn migrated_transports_account_for_rejected_producer_identities() -> Result<(), String> {
    let sources = load_sources(MIGRATED_TRANSPORTS.as_slice())?;
    require_each_contains(
        sources.iter().map(|(rel_path, source)| (*rel_path, source.as_str())),
        ACCOUNTING_ENTRYPOINT,
        "must route native candidates through the accounting entrypoint (#7475)",
    )
}

#[test]
fn direct_native_consumers_account_for_rejected_producer_identities() -> Result<(), String> {
    let sources = load_sources(DIRECT_NATIVE_CONSUMER_SOURCES.as_slice())?;
    require_each_contains(
        sources.iter().map(|(rel_path, source)| (*rel_path, source.as_str())),
        ACCOUNTING_ENTRYPOINT,
        "must route native candidates through the accounting entrypoint (#7475, #11919)",
    )
}

#[test]
fn rejected_producer_identities_stay_accounted_on_migrated_transports() -> Result<(), String> {
    let sources = load_sources(MIGRATED_TRANSPORTS.as_slice())?;
    require_no_unguarded_candidate_collection(
        sources.iter().map(|(rel_path, source)| (*rel_path, source.as_str())),
    )
}

#[test]
fn no_direct_native_consumer_uses_the_unaccounted_candidate_entrypoint() -> Result<(), String> {
    let sources = load_sources(DIRECT_NATIVE_CONSUMER_SOURCES.as_slice())?;
    require_no_unguarded_candidate_collection(
        sources.iter().map(|(rel_path, source)| (*rel_path, source.as_str())),
    )
}

#[test]
fn unaccounted_candidate_check_does_not_index_migrated_transports_by_position() -> Result<(), String>
{
    let source = include_str!("critic_normalization_wiring_tests.rs");
    for index in 0..2 {
        let needle = format!("MIGRATED_TRANSPORTS[{index}]");
        if source.contains(&needle) {
            return Err(format!(
                "unaccounted-candidate check must iterate MIGRATED_TRANSPORTS instead of indexing {needle} (#13972)"
            ));
        }
    }
    Ok(())
}

#[test]
fn unaccounted_candidate_guard_checks_a_third_denominator_entry() -> Result<(), String> {
    let error = require_no_unguarded_candidate_collection([
        ("push", "native_finding_candidates_with_accounting(subject, findings, id)"),
        ("pull", "native_finding_candidates_with_accounting(subject, findings, id)"),
        ("future-command", "let args = (subject, findings, id);\nnative_finding_candidates(args)"),
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
fn unaccounted_candidate_guard_still_checks_the_first_denominator_entry() -> Result<(), String> {
    let error = require_no_unguarded_candidate_collection([
        ("push", "native_finding_candidates(args)"),
        ("pull", "native_finding_candidates_with_accounting(subject, findings, id)"),
        ("future-command", "native_finding_candidates_with_accounting(subject, findings, id)"),
    ])
    .err()
    .ok_or_else(|| "a forbidden first denominator entry must fail the guard".to_string())?;

    if error.contains("push") {
        Ok(())
    } else {
        Err(format!("the guard must identify the first denominator entry; got: {error}"))
    }
}

#[test]
fn unaccounted_candidate_guard_catches_a_line_broken_unaccounted_call() -> Result<(), String> {
    let error =
        require_no_unguarded_candidate_collection([("push", "native_finding_candidates\n(args)")])
            .err()
            .ok_or_else(|| "a line-broken unaccounted call must fail the guard".to_string())?;

    if error.contains("push") {
        Ok(())
    } else {
        Err(format!("the guard must identify the line-broken call site; got: {error}"))
    }
}

#[test]
fn unaccounted_candidate_guard_accepts_the_accounting_entrypoint() -> Result<(), String> {
    require_no_unguarded_candidate_collection([
        ("push", "native_finding_candidates_with_accounting(subject, findings, id)"),
        (
            "pull",
            "native_finding_candidates_with_accounting(\n    subject,\n    findings,\n    id,\n)",
        ),
    ])
}

#[test]
fn migrated_transports_apply_the_post_merge_normalized_policy() -> Result<(), String> {
    let sources = load_sources(MIGRATED_TRANSPORTS.as_slice())?;
    require_each_contains(
        sources.iter().map(|(rel_path, source)| (*rel_path, source.as_str())),
        POST_MERGE_POLICY_ENTRYPOINT,
        "must apply the shared post-merge policy so alias exclusion/suppression \
         cannot leave a second spelling active (#7475 bullet 7, #11919)",
    )
}

#[test]
fn direct_native_consumers_apply_the_post_merge_normalized_policy() -> Result<(), String> {
    let sources = load_sources(DIRECT_NATIVE_CONSUMER_SOURCES.as_slice())?;
    require_each_contains(
        sources.iter().map(|(rel_path, source)| (*rel_path, source.as_str())),
        POST_MERGE_POLICY_ENTRYPOINT,
        "must apply the shared post-merge policy so alias exclusion/suppression \
         cannot leave a second spelling active (#7475 bullet 7, #11919)",
    )
}

#[test]
fn both_transports_feed_built_in_overlap_observations_into_the_seam() -> Result<(), String> {
    // #11918: the reviewed core-overlap producers reach
    // `normalize_critic_findings` only if both diagnostic transports extract
    // emitter-owned observations and chain them with the native candidates.
    // A transport that stops consuming them reverts to duplicate or unmerged
    // rows end-to-end.
    let sources = load_sources(MIGRATED_TRANSPORTS.as_slice())?;
    let views: Vec<(&str, &str)> =
        sources.iter().map(|(rel_path, source)| (*rel_path, source.as_str())).collect();
    require_each_contains(
        views.iter().copied(),
        "take_critic_overlap_observations(",
        "must consume emitter-declared overlap observations (#11918)",
    )?;
    require_each_contains(
        views.iter().copied(),
        "built_in_observation_candidates(",
        "must convert overlap observations into seam candidates (#11918)",
    )
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
        .ok_or_else(|| "transport dedup must remain defined for non-migrated pairs".to_string())?;
    let dedup_end = dedup_start.saturating_add(2000).min(source.len());
    let dedup_body = source.get(dedup_start..dedup_end).ok_or_else(|| {
        "transport dedup body must remain readable after the definition site".to_string()
    })?;
    if !dedup_body.contains("is_upstream_merged_alias_pair(") {
        return Err(
            "the dedup predicate must exempt the upstream-merged alias pairs (#11918)".to_string()
        );
    }
    if !source.contains("fn is_upstream_merged_alias_pair") {
        return Err(
            "the exemption table must stay next to the transport dedup (#11918)".to_string()
        );
    }
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
        if !source.contains(retired) {
            return Err(format!("the exemption must keep covering {retired} (#11918)"));
        }
    }
    Ok(())
}
