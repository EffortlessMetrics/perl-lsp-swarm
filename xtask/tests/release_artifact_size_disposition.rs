//! Discriminating proof for #16783 production disposition and repeat confirmation.
//!
//! Native macOS measurements have not been dispatched, so both governed
//! targets remain `not_proven` and `release.yml` must not carry safe-ICF
//! flags. The remaining false-green surface is promotion from the wrong
//! evidence: a dispatcher checkbox, the other macOS triple, a different SHA,
//! a malformed prior, or an already-decided receipt.
//!
//! This target path-includes the instrument model so `disposition.rs` keeps a
//! single type authority. Unused receipt types belong to the example binary.
#![allow(dead_code)]

#[path = "../src/bin/release_artifact_size/disposition.rs"]
mod disposition;
#[path = "../src/bin/release_artifact_size/model.rs"]
mod model;
#[path = "../src/bin/release_artifact_size/policy.rs"]
mod policy;

use std::{collections::BTreeSet, fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use serde_json::json;
use tempfile::TempDir;

use disposition::{
    PriorComparison, PriorReceipt, PriorSubject, RepeatDenial, confirm_repeat, load_prior_receipt,
    production_safe_icf_rustflags, recorded_production_rustflags,
};
use model::{DecisionPolicy, SizeDelta};
use policy::{
    GOVERNED_TARGETS, RELEASE_WORKFLOW_PATH, SAFE_ICF_RUSTFLAGS, SCHEMA_VERSION,
    TARGET_DISPOSITIONS, recorded_disposition,
};

const SHA_A: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA_B: &str = "89abcdef0123456789abcdef0123456789abcdef";
const LOCK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LOCK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ARM: &str = "aarch64-apple-darwin";
const INTEL: &str = "x86_64-apple-darwin";

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn policy() -> DecisionPolicy {
    DecisionPolicy {
        minimum_reduction_basis_points: 50,
        minimum_reduction_bytes: 131_072,
        maximum_component_growth_basis_points: 25,
        maximum_component_growth_bytes: 32_768,
        repeat_required_below_basis_points: 100,
    }
}

fn borderline_delta() -> SizeDelta {
    SizeDelta {
        baseline_bytes: 10_000_000,
        candidate_bytes: 9_940_000,
        reduction_bytes: 60_000,
        reduction_basis_points: 60,
    }
}

fn waiting_borderline(target: &str, git_sha: &str, lock: &str) -> PriorReceipt {
    PriorReceipt {
        schema_version: SCHEMA_VERSION.to_string(),
        recommendation: "not_proven".to_string(),
        subject: PriorSubject {
            git_sha: git_sha.to_string(),
            target: target.to_string(),
            cargo_lock_sha256: lock.to_string(),
            candidate_rustflags: SAFE_ICF_RUSTFLAGS.to_string(),
        },
        policy: policy(),
        comparison: PriorComparison {
            combined: borderline_delta(),
            material_reduction: true,
            repeat_requirement_satisfied: false,
        },
    }
}

fn confirm(target: &str, sha: &str, lock: &str, prior: &PriorReceipt) -> Result<(), RepeatDenial> {
    confirm_repeat(target, sha, lock, SAFE_ICF_RUSTFLAGS, &policy(), prior)
}

#[test]
fn a_matching_waiting_borderline_prior_confirms_the_repeat() {
    let prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    assert_eq!(confirm(ARM, SHA_A, LOCK_A, &prior), Ok(()));
}

#[test]
fn an_intel_prior_cannot_confirm_an_arm64_repeat() -> Result<()> {
    let prior = waiting_borderline(INTEL, SHA_A, LOCK_A);
    match confirm(ARM, SHA_A, LOCK_A, &prior) {
        Err(RepeatDenial::TargetMismatch { prior, current }) => {
            ensure!(prior == INTEL && current == ARM);
            Ok(())
        }
        other => Err(anyhow!("Intel evidence must not satisfy the arm64 row, got {other:?}")),
    }
}

#[test]
fn a_different_source_sha_cannot_confirm_the_repeat() {
    let prior = waiting_borderline(ARM, SHA_B, LOCK_A);
    assert_eq!(confirm(ARM, SHA_A, LOCK_A, &prior), Err(RepeatDenial::SourceMismatch));
}

#[test]
fn a_truncated_or_unknown_sha_cannot_confirm_the_repeat() {
    let mut prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    prior.subject.git_sha = "0123456".to_string();
    assert_eq!(confirm(ARM, SHA_A, LOCK_A, &prior), Err(RepeatDenial::SourceMismatch));
    prior.subject.git_sha = SHA_A.to_string();
    assert_eq!(confirm(ARM, "unknown", LOCK_A, &prior), Err(RepeatDenial::SourceMismatch));
}

#[test]
fn a_different_lock_digest_cannot_confirm_the_repeat() {
    let prior = waiting_borderline(ARM, SHA_A, LOCK_B);
    assert_eq!(confirm(ARM, SHA_A, LOCK_A, &prior), Err(RepeatDenial::LockMismatch));
}

#[test]
fn an_unknown_lock_digest_cannot_confirm_the_repeat() {
    let mut prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    prior.subject.cargo_lock_sha256 = "unknown".to_string();
    assert_eq!(confirm(ARM, SHA_A, LOCK_A, &prior), Err(RepeatDenial::LockMismatch));
}

#[test]
fn mismatched_candidate_flags_cannot_confirm_the_repeat() {
    let mut prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    prior.subject.candidate_rustflags = "-C link-arg=--icf=all".to_string();
    assert_eq!(confirm(ARM, SHA_A, LOCK_A, &prior), Err(RepeatDenial::FlagsMismatch));
}

#[test]
fn a_different_decision_policy_cannot_confirm_the_repeat() {
    let mut prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    prior.policy.minimum_reduction_basis_points = 10;
    assert_eq!(confirm(ARM, SHA_A, LOCK_A, &prior), Err(RepeatDenial::PolicyMismatch));
}

#[test]
fn a_wrong_schema_cannot_confirm_the_repeat() -> Result<()> {
    let mut prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    prior.schema_version = "release_artifact_size.v0".to_string();
    match confirm(ARM, SHA_A, LOCK_A, &prior) {
        Err(RepeatDenial::SchemaMismatch { observed }) => {
            ensure!(observed == "release_artifact_size.v0");
            Ok(())
        }
        other => Err(anyhow!("expected schema mismatch, got {other:?}")),
    }
}

#[test]
fn an_adopt_or_no_adopt_prior_cannot_confirm_a_borderline_repeat() -> Result<()> {
    for recommendation in ["adopt", "do_not_adopt", "reject"] {
        let mut prior = waiting_borderline(ARM, SHA_A, LOCK_A);
        prior.recommendation = recommendation.to_string();
        match confirm(ARM, SHA_A, LOCK_A, &prior) {
            Err(RepeatDenial::PriorWasNotAWaitingBorderline { recommendation: observed }) => {
                ensure!(observed == recommendation);
            }
            other => {
                return Err(anyhow!(
                    "{recommendation} must not confirm a borderline, got {other:?}"
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn a_prior_that_already_satisfied_repeat_cannot_confirm_again() {
    let mut prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    prior.comparison.repeat_requirement_satisfied = true;
    assert!(matches!(
        confirm(ARM, SHA_A, LOCK_A, &prior),
        Err(RepeatDenial::PriorWasNotAWaitingBorderline { .. })
    ));
}

#[test]
fn a_clear_win_prior_cannot_stand_in_for_a_borderline_repeat() {
    let mut prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    prior.comparison.combined.reduction_basis_points = 110;
    assert!(matches!(
        confirm(ARM, SHA_A, LOCK_A, &prior),
        Err(RepeatDenial::PriorWasNotAWaitingBorderline { .. })
    ));
}

#[test]
fn a_small_win_prior_cannot_confirm_a_borderline_current() {
    let mut prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    prior.comparison.material_reduction = false;
    prior.comparison.combined.reduction_basis_points = 20;
    assert!(matches!(
        confirm(ARM, SHA_A, LOCK_A, &prior),
        Err(RepeatDenial::PriorWasNotAWaitingBorderline { .. })
    ));
}

#[test]
fn malformed_or_missing_prior_receipts_fail_closed() -> Result<()> {
    let temp = TempDir::new()?;
    let missing = temp.path().join("absent.json");
    ensure!(load_prior_receipt(&missing).is_err(), "a missing prior must fail closed");

    let empty = temp.path().join("empty.json");
    fs::write(&empty, "")?;
    ensure!(load_prior_receipt(&empty).is_err(), "empty prior JSON must fail closed");

    let garbage = temp.path().join("garbage.json");
    fs::write(&garbage, "{not json")?;
    ensure!(load_prior_receipt(&garbage).is_err(), "malformed JSON must fail closed");

    let wrong_shape = temp.path().join("wrong-shape.json");
    fs::write(&wrong_shape, json!({"schema_version": SCHEMA_VERSION, "ok": true}).to_string())?;
    ensure!(
        load_prior_receipt(&wrong_shape).is_err(),
        "a JSON object that is not a receipt subset must fail closed"
    );
    Ok(())
}

#[test]
fn a_serialized_waiting_borderline_receipt_confirms_after_reload() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("prior.json");
    let prior = waiting_borderline(ARM, SHA_A, LOCK_A);
    fs::write(
        &path,
        json!({
            "schema_version": prior.schema_version,
            "recommendation": prior.recommendation,
            "subject": {
                "git_sha": prior.subject.git_sha,
                "target": prior.subject.target,
                "cargo_lock_sha256": prior.subject.cargo_lock_sha256,
                "candidate_rustflags": prior.subject.candidate_rustflags,
                "host": INTEL,
            },
            "policy": {
                "minimum_reduction_basis_points": prior.policy.minimum_reduction_basis_points,
                "minimum_reduction_bytes": prior.policy.minimum_reduction_bytes,
                "maximum_component_growth_basis_points":
                    prior.policy.maximum_component_growth_basis_points,
                "maximum_component_growth_bytes": prior.policy.maximum_component_growth_bytes,
                "repeat_required_below_basis_points":
                    prior.policy.repeat_required_below_basis_points,
            },
            "comparison": {
                "combined": {
                    "baseline_bytes": prior.comparison.combined.baseline_bytes,
                    "candidate_bytes": prior.comparison.combined.candidate_bytes,
                    "reduction_bytes": prior.comparison.combined.reduction_bytes,
                    "reduction_basis_points": prior.comparison.combined.reduction_basis_points,
                },
                "material_reduction": true,
                "repeat_requirement_satisfied": false,
            },
        })
        .to_string(),
    )?;
    let loaded = load_prior_receipt(&path)
        .map_err(|error| anyhow!("reload a full-shaped prior receipt: {error}"))?;
    ensure!(
        confirm(ARM, SHA_A, LOCK_A, &loaded).is_ok(),
        "a same-subject waiting borderline must confirm after JSON round-trip"
    );
    ensure!(
        confirm(INTEL, SHA_A, LOCK_A, &loaded).is_err(),
        "the extra host field on an arm64 receipt must not let it confirm the Intel row"
    );
    Ok(())
}

#[test]
fn every_governed_target_has_exactly_one_recorded_disposition() -> Result<()> {
    let recorded: BTreeSet<&str> = TARGET_DISPOSITIONS.iter().map(|row| row.target).collect();
    let governed: BTreeSet<&str> = GOVERNED_TARGETS.into_iter().collect();
    ensure!(
        recorded == governed,
        "controller dispositions must cover each governed triple once, found {recorded:?}"
    );
    for target in GOVERNED_TARGETS {
        let row = recorded_disposition(target)
            .ok_or_else(|| anyhow!("missing recorded disposition for {target}"))?;
        ensure!(
            matches!(row.recommendation, "adopt" | "do_not_adopt" | "reject" | "not_proven"),
            "illegal disposition `{}` for {target}",
            row.recommendation
        );
        ensure!(!row.reason.is_empty(), "{target} disposition needs a reason");
    }
    ensure!(recorded_disposition("x86_64-unknown-linux-gnu").is_none());
    Ok(())
}

#[test]
fn unmeasured_targets_do_not_authorize_production_safe_icf_flags() {
    for row in TARGET_DISPOSITIONS {
        let flags = recorded_production_rustflags(row.target);
        if row.recommendation == "adopt" {
            assert_eq!(flags, Some(SAFE_ICF_RUSTFLAGS));
        } else {
            assert_eq!(
                flags, None,
                "{} is `{}` and must not receive production flags",
                row.target, row.recommendation
            );
        }
    }
    assert_eq!(
        TARGET_DISPOSITIONS.map(|row| row.recommendation),
        ["not_proven", "not_proven"],
        "no native measurement has been taken; recording adopt here would be a false closeout"
    );
}

#[test]
fn an_intel_adopt_cannot_authorize_arm64_production_flags() {
    assert_eq!(production_safe_icf_rustflags(INTEL, "adopt"), Some(SAFE_ICF_RUSTFLAGS));
    assert_eq!(
        production_safe_icf_rustflags(ARM, "not_proven"),
        None,
        "the arm64 row must be decided from arm64 evidence"
    );
    assert_eq!(
        recorded_production_rustflags(ARM),
        None,
        "recorded arm64 disposition is not_proven"
    );
    assert!(production_safe_icf_rustflags("x86_64-unknown-linux-gnu", "adopt").is_none());
    assert!(production_safe_icf_rustflags("aarch64-pc-windows-msvc", "adopt").is_none());
}

#[test]
fn reject_and_no_adopt_never_change_production_linker_policy() {
    for recommendation in ["do_not_adopt", "reject", "not_proven", ""] {
        assert_eq!(production_safe_icf_rustflags(ARM, recommendation), None);
        assert_eq!(production_safe_icf_rustflags(INTEL, recommendation), None);
    }
}

#[test]
fn release_workflow_carries_safe_icf_flags_only_for_adopted_targets() -> Result<()> {
    let path = project_root().join(RELEASE_WORKFLOW_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let workflow: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;

    let include = workflow
        .get("jobs")
        .and_then(|jobs| jobs.get("build"))
        .and_then(|job| job.get("strategy"))
        .and_then(|strategy| strategy.get("matrix"))
        .and_then(|matrix| matrix.get("include"))
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| anyhow!("release.yml build matrix include is missing"))?;

    let mut macos_targets = BTreeSet::new();
    let mut other_targets = BTreeSet::new();
    for row in include {
        let target = row
            .get("target")
            .and_then(serde_yaml_ng::Value::as_str)
            .ok_or_else(|| anyhow!("matrix row has no target"))?;
        if GOVERNED_TARGETS.contains(&target) {
            macos_targets.insert(target);
        } else {
            other_targets.insert(target);
        }
        if let Some(rustflags) = row.get("rustflags").and_then(serde_yaml_ng::Value::as_str) {
            let authorized = recorded_production_rustflags(target);
            ensure!(
                authorized.is_some() || !rustflags.contains("icf"),
                "matrix row `{target}` carries ICF rustflags without an adopt disposition"
            );
        }
    }
    let governed: BTreeSet<&str> = GOVERNED_TARGETS.into_iter().collect();
    ensure!(
        macos_targets == governed,
        "release.yml must keep both governed macOS rows, found {macos_targets:?}"
    );
    ensure!(
        other_targets.contains("x86_64-unknown-linux-gnu")
            && other_targets.contains("x86_64-pc-windows-msvc"),
        "Linux and Windows rows must remain in the release matrix"
    );

    let any_adopt = TARGET_DISPOSITIONS.iter().any(|row| row.recommendation == "adopt");
    if !any_adopt {
        for needle in ["icf=safe", "linker=rust-lld", "linker-flavor=ld64"] {
            ensure!(
                !content.contains(needle),
                "no target has earned adopt, so release.yml must not contain `{needle}`"
            );
        }
    }

    Ok(())
}
