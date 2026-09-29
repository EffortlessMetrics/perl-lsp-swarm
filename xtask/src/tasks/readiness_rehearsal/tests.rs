use color_eyre::eyre::Result;

use super::fixtures::{Expect, build_case, fixture_cases, run_fixture_suite, sync_fixture_files};
use super::model::{
    FIXTURE_DIR, ReasonCode, RehearsalError, Status, canonical_json, compute_receipt_digest,
    project_human, project_json,
};
use super::validate::{validate_bytes, validate_receipt, validate_schema_file};
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

#[test]
fn committed_json_round_trips_through_validate_bytes() -> Result<()> {
    let root = project_root()?;
    let pass_bytes = std::fs::read(root.join(FIXTURE_DIR).join("valid_pass.json"))?;
    let accepted = validate_bytes(&pass_bytes)?;
    if accepted.status != Status::Pass {
        color_eyre::eyre::bail!("valid_pass.json did not round-trip as pass");
    }
    let reject_bytes = std::fs::read(root.join(FIXTURE_DIR).join("nonzero_exit_pass.json"))?;
    match validate_bytes(&reject_bytes) {
        Err(error) if error.code == ReasonCode::NonzeroExitPass => Ok(()),
        Ok(_) => color_eyre::eyre::bail!("nonzero_exit_pass.json validated through JSON bytes"),
        Err(error) => color_eyre::eyre::bail!(
            "nonzero_exit_pass.json expected nonzero_exit_pass, got {}",
            error.code.as_str()
        ),
    }
}

const PRIVACY_PROBE: &str = "ghp_FAKE_REVIEW_MARKER";

fn load_valid_pass_value() -> Result<serde_json::Value> {
    let bytes = std::fs::read(project_root()?.join(FIXTURE_DIR).join("valid_pass.json"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn assert_diagnostic_hides_probe(error: &RehearsalError) -> Result<()> {
    let text = error.to_string();
    if text.contains(PRIVACY_PROBE) || text.contains("ghp_") {
        color_eyre::eyre::bail!("parse diagnostic echoed a privacy probe: {text}");
    }
    Ok(())
}

#[test]
fn unknown_field_credential_is_privacy_leak_and_not_echoed() -> Result<()> {
    let mut value = load_valid_pass_value()?;
    value
        .as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("valid_pass object"))?
        .insert(PRIVACY_PROBE.to_string(), serde_json::Value::Bool(true));
    let bytes = serde_json::to_vec(&value)?;
    match validate_bytes(&bytes) {
        Err(error) if error.code == ReasonCode::PrivacyLeak => {
            assert_diagnostic_hides_probe(&error)
        }
        Ok(_) => color_eyre::eyre::bail!("unknown credential field validated"),
        Err(error) => color_eyre::eyre::bail!(
            "unknown credential field expected privacy_leak, got {}: {error}",
            error.code.as_str()
        ),
    }
}

#[test]
fn invalid_enum_credential_is_privacy_leak_and_not_echoed() -> Result<()> {
    let mut value = load_valid_pass_value()?;
    value
        .as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("valid_pass object"))?
        .insert("status".to_string(), serde_json::Value::String(PRIVACY_PROBE.to_string()));
    let bytes = serde_json::to_vec(&value)?;
    match validate_bytes(&bytes) {
        Err(error) if error.code == ReasonCode::PrivacyLeak => {
            assert_diagnostic_hides_probe(&error)
        }
        Ok(_) => color_eyre::eyre::bail!("invalid enum credential validated"),
        Err(error) => color_eyre::eyre::bail!(
            "invalid enum credential expected privacy_leak, got {}: {error}",
            error.code.as_str()
        ),
    }
}

#[test]
fn malformed_json_credential_is_redacted() -> Result<()> {
    let bytes =
        format!("{{\"schema_version\":\"readiness_rehearsal.v1\",\"{PRIVACY_PROBE}\"").into_bytes();
    match validate_bytes(&bytes) {
        Err(error) if error.code == ReasonCode::MalformedDocument => {
            assert_diagnostic_hides_probe(&error)
        }
        Ok(_) => color_eyre::eyre::bail!("truncated credential JSON validated"),
        Err(error) => color_eyre::eyre::bail!(
            "truncated credential JSON expected malformed_document, got {}: {error}",
            error.code.as_str()
        ),
    }
}

#[test]
fn validate_path_does_not_echo_unknown_field_credential() -> Result<()> {
    let mut value = load_valid_pass_value()?;
    value
        .as_object_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("valid_pass object"))?
        .insert(PRIVACY_PROBE.to_string(), serde_json::Value::Bool(true));
    let dir = std::env::temp_dir();
    let path = dir.join(format!("readiness-rehearsal-privacy-{}.json", std::process::id()));
    std::fs::write(&path, serde_json::to_vec(&value)?)?;
    let result = super::validate_path(path.clone());
    let _ = std::fs::remove_file(&path);
    match result {
        Err(error) => {
            let text = format!("{error:#}");
            if text.contains(PRIVACY_PROBE) || text.contains("ghp_") {
                color_eyre::eyre::bail!("validate_path echoed a privacy probe: {text}");
            }
            Ok(())
        }
        Ok(()) => color_eyre::eyre::bail!("validate_path accepted an unknown credential field"),
    }
}
