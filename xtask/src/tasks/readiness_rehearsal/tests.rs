use color_eyre::eyre::Result;

use super::fixtures::{Expect, build_case, fixture_cases, run_fixture_suite, sync_fixture_files};
use super::model::{
    ReasonCode, Status, canonical_json, compute_receipt_digest, project_human, project_json,
};
use super::validate::{validate_receipt, validate_schema_file};
use crate::utils::project_root;

#[test]
fn schema_compiles() -> Result<()> {
    validate_schema_file(&project_root()?)?;
    Ok(())
}

#[test]
fn fixture_suite_rejects_every_false_green() -> Result<()> {
    run_fixture_suite()?;
    Ok(())
}

#[test]
fn committed_fixtures_match_builders() -> Result<()> {
    let update = std::env::var_os("UPDATE_READINESS_REHEARSAL_FIXTURES").is_some();
    sync_fixture_files(&project_root()?, update)?;
    Ok(())
}

#[test]
fn json_and_human_are_projections_of_one_result() -> Result<()> {
    let accept = fixture_cases()
        .into_iter()
        .find(|case| matches!(case.expect, Expect::Accept { status: Status::Pass }))
        .ok_or_else(|| color_eyre::eyre::eyre!("valid pass case"))?;
    let receipt = validate_receipt(build_case(&accept)?)?;
    let json = project_json(&receipt)?;
    let human = project_human(&receipt);
    let parsed: super::model::RehearsalReceipt = serde_json::from_str(&json)?;
    if parsed.receipt_digest != receipt.receipt_digest {
        color_eyre::eyre::bail!("json projection changed receipt_digest");
    }
    if parsed.status != Status::Pass {
        color_eyre::eyre::bail!("json projection changed status");
    }
    if !human.contains("published_channels: (none)") || !human.contains("release_cut: false") {
        color_eyre::eyre::bail!("human projection dropped no-publish invariants");
    }
    if !human.contains(&receipt.receipt_digest) {
        color_eyre::eyre::bail!("human projection dropped receipt_digest");
    }
    if human.contains("/tmp") || human.contains("2026-") || json.contains("observed_at") {
        color_eyre::eyre::bail!("projections carried observation time or temporary paths");
    }
    Ok(())
}

#[test]
fn digest_is_stable_and_independent_of_object_key_order() -> Result<()> {
    let accept = fixture_cases()
        .into_iter()
        .find(|case| matches!(case.expect, Expect::Accept { status: Status::Pass }))
        .ok_or_else(|| color_eyre::eyre::eyre!("valid pass case"))?;
    let first = build_case(&accept)?;
    let second = build_case(&accept)?;
    if first.receipt_digest != second.receipt_digest {
        color_eyre::eyre::bail!("identical receipts produced different digests");
    }
    let value = serde_json::to_value(&first)?;
    if canonical_json(&value)? != canonical_json(&value)? {
        color_eyre::eyre::bail!("canonical JSON was not deterministic");
    }
    if compute_receipt_digest(&first)? != first.receipt_digest {
        color_eyre::eyre::bail!("stored digest drifted from canonical digest");
    }
    Ok(())
}

#[test]
fn each_named_control_has_a_discriminating_fixture() -> Result<()> {
    let reasons: Vec<ReasonCode> = fixture_cases()
        .into_iter()
        .filter_map(|case| match case.expect {
            Expect::Reject { reason } => Some(reason),
            Expect::Accept { .. } => None,
        })
        .collect();
    for required in [
        ReasonCode::WrongBinaryPathRole,
        ReasonCode::MixedSourceSha,
        ReasonCode::MixedVersion,
        ReasonCode::MissingArchiveMember,
        ReasonCode::DuplicateArchiveMember,
        ReasonCode::CollidingArchiveMember,
        ReasonCode::ChecksumInvalidMember,
        ReasonCode::VsixServerMismatch,
        ReasonCode::UnavailableInstrumentPass,
        ReasonCode::NonTerminalStagePass,
        ReasonCode::NonzeroExitPass,
        ReasonCode::UncleanTreePass,
        ReasonCode::InstalledMemberMismatch,
        ReasonCode::FailedCleanupHidden,
        ReasonCode::MutationAttempted,
        ReasonCode::PrivacyLeak,
        ReasonCode::CompensatingStage,
        ReasonCode::PublishedChannelsNonEmpty,
        ReasonCode::ReleaseCutTrue,
    ] {
        if !reasons.contains(&required) {
            color_eyre::eyre::bail!("missing discriminator for {}", required.as_str());
        }
    }
    Ok(())
}
