//! Validate one topology-bound install, upgrade, recovery, or rollback receipt.
//!
//! The validator proves that one documented release path reached its intended
//! disposition without mixed-version readiness, partial-artifact promotion,
//! unsupported-target substitution, loss of the known-good binary, or orphaned
//! candidate processes. It does not download, install, publish, or select the
//! release topology.

#![allow(clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const CHECK: &str = "install-transition";
const SCHEMA_VERSION: &str = "install_transition.v1";

#[derive(Debug, Parser)]
#[command(name = "install-transition")]
#[command(about = "Validate one topology-bound install transition receipt")]
struct Args {
    /// Receipt JSON to validate.
    #[arg(long)]
    receipt: PathBuf,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReceiptStatus {
    Pass,
    Blocked,
    NotProven,
}

impl ReceiptStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Blocked => "blocked",
            Self::NotProven => "not_proven",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TransitionClass {
    CleanInstall,
    NormalUpgrade,
    CachedOldBinary,
    VersionOrTargetMismatch,
    InterruptedInstall,
    Rollback,
    ChannelIdentity,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum InstallPath {
    Vsix,
    GithubArchive,
    PosixInstaller,
    PowershellInstaller,
    CargoInstall,
    ManualArchive,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Disposition {
    Applied,
    Rejected,
    RolledBack,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TransitionOutcome {
    Completed,
    Failed,
    NotProven,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateIdentity {
    frozen_product_sha: String,
    prepared_swarm_sha: String,
    release_repo_sha: String,
    release_topology_sha256: String,
    candidate_id: String,
    previous_version: String,
    candidate_version: String,
    extension_version: String,
    server_version: String,
    dap_version: String,
    platform: String,
    target: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransitionEvidence {
    topology_path_id: String,
    path: InstallPath,
    class: TransitionClass,
    documented_primary: bool,
    expected_asset: String,
    resolved_asset: Option<String>,
    intended_disposition: Disposition,
    observed_disposition: Disposition,
    outcome: TransitionOutcome,
    observed_release_identity: Option<String>,
    artifact_verified: bool,
    known_good_preserved: bool,
    partial_artifact_promoted: bool,
    mixed_version_reported_ready: bool,
    unsupported_target_selected: bool,
    rollback_completed: bool,
    candidate_process_left_running: bool,
    compatibility_mismatch_detected: bool,
    user_action: String,
    limitations: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    check: String,
    schema_version: String,
    status: ReceiptStatus,
    claim_boundary: String,
    candidate: CandidateIdentity,
    transition: TransitionEvidence,
    limitations: Vec<String>,
}

fn non_empty(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn exact_hex(value: &str, bytes: usize, field: &str) -> Result<()> {
    if value.len() != bytes * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{field} must be exactly {} hexadecimal characters", bytes * 2);
    }
    Ok(())
}

fn validate_candidate(candidate: &CandidateIdentity) -> Result<()> {
    exact_hex(&candidate.frozen_product_sha, 20, "candidate.frozen_product_sha")?;
    exact_hex(&candidate.prepared_swarm_sha, 20, "candidate.prepared_swarm_sha")?;
    exact_hex(&candidate.release_repo_sha, 20, "candidate.release_repo_sha")?;
    exact_hex(
        &candidate.release_topology_sha256,
        32,
        "candidate.release_topology_sha256",
    )?;

    for (field, value) in [
        ("candidate.candidate_id", candidate.candidate_id.as_str()),
        ("candidate.previous_version", candidate.previous_version.as_str()),
        ("candidate.candidate_version", candidate.candidate_version.as_str()),
        ("candidate.extension_version", candidate.extension_version.as_str()),
        ("candidate.server_version", candidate.server_version.as_str()),
        ("candidate.dap_version", candidate.dap_version.as_str()),
        ("candidate.platform", candidate.platform.as_str()),
        ("candidate.target", candidate.target.as_str()),
    ] {
        non_empty(value, field)?;
    }

    if candidate.extension_version != candidate.candidate_version
        || candidate.server_version != candidate.candidate_version
        || candidate.dap_version != candidate.candidate_version
    {
        bail!("candidate extension, server, and DAP versions must agree");
    }
    if candidate.previous_version == candidate.candidate_version {
        bail!("candidate.previous_version must differ from candidate.candidate_version");
    }
    Ok(())
}

fn validate_transition_identity(receipt: &Receipt) -> Result<()> {
    let transition = &receipt.transition;
    non_empty(&transition.topology_path_id, "transition.topology_path_id")?;
    non_empty(&transition.expected_asset, "transition.expected_asset")?;
    non_empty(&transition.user_action, "transition.user_action")?;
    if let Some(asset) = &transition.resolved_asset {
        non_empty(asset, "transition.resolved_asset")?;
    }
    if let Some(identity) = &transition.observed_release_identity {
        non_empty(identity, "transition.observed_release_identity")?;
    }
    for limitation in transition.limitations.iter().chain(receipt.limitations.iter()) {
        non_empty(limitation, "limitations[]")?;
    }
    Ok(())
}

fn has_safety_violation(transition: &TransitionEvidence) -> bool {
    transition.partial_artifact_promoted
        || transition.mixed_version_reported_ready
        || transition.unsupported_target_selected
        || transition.candidate_process_left_running
}

fn applied_is_valid(receipt: &Receipt) -> bool {
    let transition = &receipt.transition;
    transition.observed_disposition == Disposition::Applied
        && transition.outcome == TransitionOutcome::Completed
        && transition.resolved_asset.as_deref() == Some(transition.expected_asset.as_str())
        && transition.observed_release_identity.as_deref()
            == Some(receipt.candidate.candidate_version.as_str())
        && transition.artifact_verified
        && transition.known_good_preserved
        && !transition.rollback_completed
}

fn rejected_is_valid(receipt: &Receipt) -> bool {
    let transition = &receipt.transition;
    let observed_identity_is_safe = transition.observed_release_identity.is_none()
        || transition.observed_release_identity.as_deref()
            == Some(receipt.candidate.previous_version.as_str());
    let mismatch_contract_holds = transition.class != TransitionClass::VersionOrTargetMismatch
        || transition.compatibility_mismatch_detected;

    transition.observed_disposition == Disposition::Rejected
        && transition.outcome == TransitionOutcome::Completed
        && transition.known_good_preserved
        && !transition.partial_artifact_promoted
        && !transition.mixed_version_reported_ready
        && !transition.unsupported_target_selected
        && !transition.rollback_completed
        && !transition.candidate_process_left_running
        && observed_identity_is_safe
        && mismatch_contract_holds
}

fn rollback_is_valid(receipt: &Receipt) -> bool {
    let transition = &receipt.transition;
    transition.observed_disposition == Disposition::RolledBack
        && transition.outcome == TransitionOutcome::Completed
        && transition.observed_release_identity.as_deref()
            == Some(receipt.candidate.previous_version.as_str())
        && transition.known_good_preserved
        && transition.rollback_completed
        && !transition.partial_artifact_promoted
        && !transition.mixed_version_reported_ready
        && !transition.unsupported_target_selected
        && !transition.candidate_process_left_running
}

fn computed_status(receipt: &Receipt) -> ReceiptStatus {
    if receipt.transition.outcome == TransitionOutcome::NotProven {
        return ReceiptStatus::NotProven;
    }
    if receipt.transition.outcome == TransitionOutcome::Failed
        || has_safety_violation(&receipt.transition)
    {
        return ReceiptStatus::Blocked;
    }

    let disposition_valid = match receipt.transition.intended_disposition {
        Disposition::Applied => applied_is_valid(receipt),
        Disposition::Rejected => rejected_is_valid(receipt),
        Disposition::RolledBack => rollback_is_valid(receipt),
    };
    if disposition_valid {
        ReceiptStatus::Pass
    } else {
        ReceiptStatus::Blocked
    }
}

fn validate(receipt: &Receipt) -> Result<ReceiptStatus> {
    if receipt.check != CHECK {
        bail!("check must be {CHECK}");
    }
    if receipt.schema_version != SCHEMA_VERSION {
        bail!("schema_version must be {SCHEMA_VERSION}");
    }
    non_empty(&receipt.claim_boundary, "claim_boundary")?;
    validate_candidate(&receipt.candidate)?;
    validate_transition_identity(receipt)?;

    if receipt.transition.documented_primary
        && receipt.transition.resolved_asset.is_none()
        && receipt.transition.intended_disposition == Disposition::Applied
    {
        bail!("a documented primary path intended to apply must resolve an asset");
    }
    if receipt.transition.class == TransitionClass::Rollback
        && receipt.transition.intended_disposition != Disposition::RolledBack
    {
        bail!("rollback transition must intend rolled_back");
    }
    if receipt.transition.class != TransitionClass::Rollback
        && receipt.transition.intended_disposition == Disposition::RolledBack
    {
        bail!("only rollback transition may intend rolled_back");
    }

    let computed = computed_status(receipt);
    if receipt.status != computed {
        bail!(
            "declared status {} disagrees with computed status {}",
            receipt.status.as_str(),
            computed.as_str()
        );
    }
    Ok(computed)
}

fn load(path: &Path) -> Result<Receipt> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let receipt = load(&args.receipt)?;
    let status = validate(&receipt)?;
    println!(
        "install-transition: status={} class={:?} path={:?} intended={:?} observed={:?}",
        status.as_str(),
        receipt.transition.class,
        receipt.transition.path,
        receipt.transition.intended_disposition,
        receipt.transition.observed_disposition
    );
    if status != ReceiptStatus::Pass {
        bail!("install-transition receipt is {}", status.as_str());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Receipt, ReceiptStatus, validate};
    use color_eyre::eyre::Result;

    fn fixture(content: &str) -> Result<Receipt> {
        Ok(serde_json::from_str(content)?)
    }

    #[test]
    fn normal_upgrade_fixture_passes() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/normal_upgrade.json"
        ))?;
        assert_eq!(validate(&receipt)?, ReceiptStatus::Pass);
        Ok(())
    }

    #[test]
    fn corrupted_candidate_rejected_fixture_passes() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/corrupt_rejected.json"
        ))?;
        assert_eq!(validate(&receipt)?, ReceiptStatus::Pass);
        Ok(())
    }

    #[test]
    fn historical_powershell_asset_mismatch_is_blocked() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/powershell_404.json"
        ))?;
        assert_eq!(validate(&receipt)?, ReceiptStatus::Blocked);
        Ok(())
    }

    #[test]
    fn mixed_version_ready_state_cannot_claim_pass() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/normal_upgrade.json"
        ))?;
        receipt.transition.mixed_version_reported_ready = true;
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn rollback_must_restore_the_previous_pair() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/normal_upgrade.json"
        ))?;
        receipt.transition.class = super::TransitionClass::Rollback;
        receipt.transition.intended_disposition = super::Disposition::RolledBack;
        receipt.transition.observed_disposition = super::Disposition::RolledBack;
        receipt.transition.rollback_completed = true;
        receipt.transition.observed_release_identity = Some("0.18.0".to_string());
        assert!(validate(&receipt).is_err());
        Ok(())
    }
}
