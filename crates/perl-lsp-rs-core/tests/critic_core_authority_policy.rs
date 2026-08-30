//! Product-policy falsifiers for independently owned core/Critic overlap rows (#13798).
//!
//! These tests deliberately describe the adopted v0.18 behavior before the
//! post-merge policy implementation changes: Critic severity/include/exclude
//! may filter the Critic contribution, but cannot revoke an independently
//! emitted core security proposition while that contributor remains present.

use perl_lsp_rs_core::tooling::perl_critic::{
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

fn apply_policy(
    with_core_authority: bool,
    threshold: u8,
    include: &[String],
    exclude: &[String],
) -> Vec<NormalizedCriticFinding> {
    let suppressions = CriticSuppressionMap::from_source("");
    let policy = NativeCriticPolicy::new(threshold, include, exclude, &suppressions);
    normalize_with_native_policy(system_candidates(with_core_authority), &policy)
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
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
        row.contributors().iter().any(|contributor| {
            contributor.identity().origin() == CriticFindingOrigin::BuiltInDiagnostic
        }),
        format!(
            "{policy_case}: survival authority must come from retained contributor provenance"
        ),
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

fn require_filtered_core_only_row(
    rows: &[NormalizedCriticFinding],
    policy_case: &str,
) -> Result<(), String> {
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
    )
}

#[test]
fn core_security_authority_survives_critic_severity_thresholds() -> Result<(), String> {
    let include = Vec::new();
    let exclude = Vec::new();

    for threshold in [1, 3] {
        let rows = apply_policy(true, threshold, &include, &exclude);
        require_open_overlap_row(&rows, &format!("severity threshold {threshold}"))?;
    }

    let rows = apply_policy(true, 5, &include, &exclude);
    require_filtered_core_only_row(&rows, "severity threshold 5")
}

#[test]
fn core_security_authority_survives_critic_exclude() -> Result<(), String> {
    let include = Vec::new();
    let exclude = vec!["native.security.system_exec".to_string()];
    let rows = apply_policy(true, 1, &include, &exclude);
    require_filtered_core_only_row(&rows, "native alias excluded")
}

#[test]
fn core_security_authority_survives_nonmatching_critic_include() -> Result<(), String> {
    let include = vec!["native.testing.require_use_strict".to_string()];
    let exclude = Vec::new();
    let rows = apply_policy(true, 1, &include, &exclude);
    require_filtered_core_only_row(&rows, "nonmatching include filter")
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
