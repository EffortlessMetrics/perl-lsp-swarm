//! Product-policy falsifiers for independently owned core/Critic overlap rows (#13798).
//!
//! These tests deliberately describe the adopted v0.18 behavior: Critic
//! severity/include/exclude may filter the Critic contribution, but cannot
//! revoke an independently emitted core security proposition while that
//! contributor remains present. They live in the library test target because
//! the required policy gate runs `cargo test --lib -p perl-lsp-rs-core`;
//! integration targets are only compiled by the compile-all lane.

use crate::tooling::perl_critic::{
    CriticFindingCandidate, CriticFindingOrigin, CriticObservedIdentity, CriticSourceIdentity,
    CriticSuppressionMap, NativeCriticPolicy, NormalizedCriticFinding, Severity,
    normalize_with_native_policy,
};
use perl_parser_core::position::{Position, Range};

const SOURCE_IDENTITY: CriticSourceIdentity = CriticSourceIdentity::new([0x13; 16], 7);

fn system_range() -> Range {
    Range {
        start: Position { byte: 0, line: 0, column: 0 },
        end: Position { byte: 12, line: 0, column: 12 },
    }
}

fn system_candidates(with_core_authority: bool) -> Vec<CriticFindingCandidate> {
    let mut candidates = Vec::new();
    if with_core_authority {
        candidates.push(CriticFindingCandidate::new(
            CriticObservedIdentity::built_in_system_call(),
            SOURCE_IDENTITY,
            Severity::Harsh,
            system_range(),
            "system() executes a shell command.",
            None,
        ));
    }
    candidates.push(CriticFindingCandidate::new(
        CriticObservedIdentity::native_system_call(),
        SOURCE_IDENTITY,
        Severity::Harsh,
        system_range(),
        "Avoid system() where a safer process API is available.",
        None,
    ));
    candidates
}

fn apply_policy_with_source(
    with_core_authority: bool,
    threshold: u8,
    include: &[String],
    exclude: &[String],
    source: &str,
) -> Vec<NormalizedCriticFinding> {
    let suppressions = CriticSuppressionMap::from_source(source);
    let policy = NativeCriticPolicy::new(threshold, include, exclude, &suppressions);
    normalize_with_native_policy(system_candidates(with_core_authority), &policy)
}

fn apply_policy(
    with_core_authority: bool,
    threshold: u8,
    include: &[String],
    exclude: &[String],
) -> Vec<NormalizedCriticFinding> {
    apply_policy_with_source(with_core_authority, threshold, include, exclude, "")
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.into()) }
}

fn require_single_core_authority_row<'a>(
    rows: &'a [NormalizedCriticFinding],
    policy_case: &str,
) -> Result<&'a NormalizedCriticFinding, String> {
    require(
        rows.len() == 1,
        format!(
            "{policy_case}: independently owned PL603 must survive as one logical row; got {}",
            rows.len()
        ),
    )?;
    let row = rows
        .first()
        .ok_or_else(|| format!("{policy_case}: the surviving logical row must be present"))?;
    require(
        row.public_code() == "PL603",
        format!(
            "{policy_case}: built-in presentation authority must remain PL603; got {}",
            row.public_code()
        ),
    )?;
    require(
        row.canonical_id() == Some("critic.security.system_call"),
        format!(
            "{policy_case}: registry-mapped canonical identity must remain \
             critic.security.system_call; got {:?}",
            row.canonical_id()
        ),
    )?;
    require(
        row.contributors().iter().any(|contributor| {
            contributor.identity().origin() == CriticFindingOrigin::BuiltInDiagnostic
        }),
        format!("{policy_case}: survival authority must come from retained contributor provenance"),
    )?;
    Ok(row)
}

fn require_open_overlap_row(
    rows: &[NormalizedCriticFinding],
    policy_case: &str,
) -> Result<(), String> {
    let row = require_single_core_authority_row(rows, policy_case)?;
    require(
        row.contributors().len() == 2,
        format!(
            "{policy_case}: open Critic policy must retain both overlap contributors; got {:?}",
            row.contributors()
        ),
    )?;
    require(
        row.contributors().iter().any(|contributor| {
            contributor.identity().origin() == CriticFindingOrigin::NativeCritic
        }),
        format!("{policy_case}: open policy must retain the native Critic contribution"),
    )
}

fn require_filtered_core_only_row<'a>(
    rows: &'a [NormalizedCriticFinding],
    policy_case: &str,
) -> Result<&'a NormalizedCriticFinding, String> {
    let row = require_single_core_authority_row(rows, policy_case)?;
    require(
        row.contributors().len() == 1,
        format!(
            "{policy_case}: filtered Critic contribution must leave one core contributor; got {:?}",
            row.contributors()
        ),
    )?;
    require(
        !row.contributors().iter().any(|contributor| {
            contributor.identity().origin() == CriticFindingOrigin::NativeCritic
        }),
        format!(
            "{policy_case}: Critic policy may not keep an active native contribution after filtering it"
        ),
    )?;
    Ok(row)
}

#[test]
fn core_security_authority_survives_critic_severity_thresholds() -> Result<(), String> {
    let include = Vec::new();
    let exclude = Vec::new();

    for threshold in [1, 3] {
        let rows = apply_policy(true, threshold, &include, &exclude);
        require_open_overlap_row(&rows, &format!("severity threshold {threshold}"))?;
    }

    // The universal claim is that Critic severity cannot revoke the core
    // proposition, so every threshold above the producers' severities must
    // leave exactly the core-only row, not just the 5-vs-3 boundary.
    for threshold in [4, 5, 6, 10, u8::MAX] {
        let rows = apply_policy(true, threshold, &include, &exclude);
        require_filtered_core_only_row(&rows, &format!("severity threshold {threshold}"))?;
    }

    Ok(())
}

#[test]
fn below_threshold_critic_severity_cannot_ride_a_stricter_core_severity() -> Result<(), String> {
    // The threshold is evaluated over the Critic-owned contributors' own
    // severities: a stricter built-in severity must never admit a
    // below-threshold Critic contribution (#13798).
    let include = Vec::new();
    let exclude = Vec::new();
    let suppressions = CriticSuppressionMap::from_source("");
    let policy = NativeCriticPolicy::new(4, &include, &exclude, &suppressions);
    let candidates = vec![
        CriticFindingCandidate::new(
            CriticObservedIdentity::built_in_system_call(),
            SOURCE_IDENTITY,
            Severity::Gentle,
            system_range(),
            "system() executes a shell command.",
            None,
        ),
        CriticFindingCandidate::new(
            CriticObservedIdentity::native_system_call(),
            SOURCE_IDENTITY,
            Severity::Harsh,
            system_range(),
            "Avoid system() where a safer process API is available.",
            None,
        ),
    ];
    let rows = normalize_with_native_policy(candidates, &policy);
    let row = require_single_core_authority_row(&rows, "below-threshold Critic severity")?;
    require(
        !row.has_severity_conflict(),
        "the surviving core-only row reports only the built-in severity",
    )?;
    require(
        row.severity() == Severity::Gentle,
        "the surviving row severity comes from the retained built-in contributor",
    )
}

#[test]
fn core_security_authority_survives_critic_exclude() -> Result<(), String> {
    let include = Vec::new();
    let exclude = vec!["native.security.system_exec".to_string()];
    let rows = apply_policy(true, 1, &include, &exclude);
    require_filtered_core_only_row(&rows, "native alias excluded");
    Ok(())
}

#[test]
fn core_security_authority_survives_exclusion_by_the_pl603_selector() -> Result<(), String> {
    // Production accepts the built-in compatibility code as a Critic-policy
    // selector. Excluding `PL603` strips the Critic contribution but may not
    // revoke the independently owned core proposition; removal of the core
    // proposition itself is a built-in-policy decision (#13798).
    let include = Vec::new();
    let exclude = vec!["PL603".to_string()];
    let rows = apply_policy(true, 1, &include, &exclude);
    require_filtered_core_only_row(&rows, "PL603 selector excluded");
    Ok(())
}

#[test]
fn core_security_authority_survives_nonmatching_critic_include() -> Result<(), String> {
    let include = vec!["native.testing.require_use_strict".to_string()];
    let exclude = Vec::new();
    let rows = apply_policy(true, 1, &include, &exclude);
    require_filtered_core_only_row(&rows, "nonmatching include filter");
    Ok(())
}

#[test]
fn severity_conflict_is_flagged_and_resolves_to_the_more_severe_producer() -> Result<(), String> {
    let include = Vec::new();
    let exclude = Vec::new();
    let suppressions = CriticSuppressionMap::from_source("");
    let policy = NativeCriticPolicy::new(1, &include, &exclude, &suppressions);
    let candidates = vec![
        CriticFindingCandidate::new(
            CriticObservedIdentity::built_in_system_call(),
            SOURCE_IDENTITY,
            Severity::Harsh,
            system_range(),
            "system() executes a shell command.",
            None,
        ),
        CriticFindingCandidate::new(
            CriticObservedIdentity::native_system_call(),
            SOURCE_IDENTITY,
            Severity::Gentle,
            system_range(),
            "Avoid system() where a safer process API is available.",
            None,
        ),
    ];
    let rows = normalize_with_native_policy(candidates, &policy);
    let row = require_single_core_authority_row(&rows, "conflicting producer severities")?;
    require(
        row.contributors().len() == 2,
        "open policy must retain both overlap contributors under a severity conflict",
    )?;
    require(
        row.has_severity_conflict(),
        "the merged PL603 row must flag disagreeing producer severities",
    )?;
    require(
        row.severity() == Severity::Gentle,
        "conflicting severities must resolve to the more severe producer (Gentle = perlcritic 5)",
    )
}

#[test]
fn filtered_critic_fix_availability_is_not_advertised_on_the_core_row() -> Result<(), String> {
    // A stripped Critic producer's fix advertisement must not survive on the
    // core-only row: diagnostics advertise what the code-action path can
    // actually provide (#13798).
    let include = Vec::new();
    let exclude = Vec::new();
    let suppressions = CriticSuppressionMap::from_source("");
    let policy = NativeCriticPolicy::new(5, &include, &exclude, &suppressions);
    let candidates = vec![
        CriticFindingCandidate::new(
            CriticObservedIdentity::built_in_system_call(),
            SOURCE_IDENTITY,
            Severity::Harsh,
            system_range(),
            "system() executes a shell command.",
            None,
        ),
        CriticFindingCandidate::with_fix_availability(
            CriticObservedIdentity::native_system_call(),
            SOURCE_IDENTITY,
            Severity::Harsh,
            system_range(),
            "Avoid system() where a safer process API is available.",
            None,
            true,
        ),
    ];
    let rows = normalize_with_native_policy(candidates, &policy);
    let row = require_filtered_core_only_row(&rows, "fixable Critic contribution filtered")?;
    require(
        !row.has_available_fix(),
        "a stripped Critic fix may not be advertised on the surviving core-only row",
    )
}

#[test]
fn scoped_no_critic_suppression_removes_the_whole_overlap_row_today() -> Result<(), String> {
    // Pinned pre-#14021 behavior: a `## no critic PL603` directive suppresses
    // the entire merged row, including the independently owned core
    // contributor. #14021 owns the source-directive ruling; this pin makes any
    // future change to that boundary an intentional, reviewed decision rather
    // than a silent side effect of the merge restructuring.
    let source = "system(\"ls\") or die; ## no critic PL603\n";
    let include = Vec::new();
    let exclude = Vec::new();
    require(
        apply_policy_with_source(true, 1, &include, &exclude, source).is_empty(),
        "scoped `## no critic PL603` must suppress the whole merged row (ruling deferred to #14021)",
    )
}

#[test]
fn critic_only_row_still_obeys_critic_policy() -> Result<(), String> {
    let empty = Vec::new();
    require(
        apply_policy(false, 5, &empty, &empty).is_empty(),
        "a native-only severity-3 row must remain filtered at threshold 5",
    )?;

    let exclude = vec!["native.security.system_exec".to_string()];
    require(
        apply_policy(false, 1, &empty, &exclude).is_empty(),
        "a native-only row must remain removable by its native alias",
    )?;

    let include = vec!["native.testing.require_use_strict".to_string()];
    require(
        apply_policy(false, 1, &include, &empty).is_empty(),
        "a native-only row must remain filtered by a nonmatching include set",
    )
}

#[test]
fn open_policy_keeps_exactly_one_overlap_row() -> Result<(), String> {
    let empty = Vec::new();
    let rows = apply_policy(true, 1, &empty, &empty);
    require_open_overlap_row(&rows, "open policy")
}
