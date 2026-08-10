//! Validate one topology-bound install, upgrade, recovery, or rollback receipt.
//!
//! The validator proves that one documented release path reached its intended
//! disposition without mixed-version readiness, partial-artifact promotion,
//! unsupported-target substitution, loss of the known-good binary, or orphaned
//! candidate processes. It does not download, install, publish, or select the
//! release topology.

#![allow(clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const CHECK: &str = "install-transition";
const SCHEMA_VERSION: &str = "install_transition.v1";
const VERIFIED_CHILD_SCHEMA_VERSION: &str = "verified_child_receipt.v1";
const OWNER_ISSUE: &str = "#5903";

#[derive(Debug, Parser)]
#[command(name = "install-transition")]
#[command(about = "Validate one topology-bound install transition receipt")]
struct Args {
    /// Receipt JSON to validate.
    #[arg(long)]
    receipt: PathBuf,

    /// Optional verified-child envelope output consumed by the public-beta fan-in.
    #[arg(long)]
    verified_output: Option<PathBuf>,

    /// Exact release_topology.v1 JSON consumed by this receipt.
    #[arg(long)]
    topology: PathBuf,

    /// Exact candidate artifact bytes resolved by the transition.
    #[arg(long)]
    artifact: Option<PathBuf>,
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
    artifact_set_id: String,
    frozen_product_sha: String,
    prepared_swarm_sha: String,
    release_repo_sha: String,
    release_topology_sha256: String,
    candidate_id: String,
    previous_version: Option<String>,
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
    artifact_sha256: Option<String>,
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

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct VerifiedChildArtifact<'a> {
    owner_issue: &'static str,
    schema_version: &'static str,
    receipt_schema_version: &'static str,
    candidate_id: &'a str,
    frozen_product_sha: &'a str,
    artifact_set_id: &'a str,
    status: ReceiptStatus,
    claim_boundary: &'a str,
    limitation: Option<String>,
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
    non_empty(&candidate.artifact_set_id, "candidate.artifact_set_id")?;
    exact_hex(&candidate.frozen_product_sha, 20, "candidate.frozen_product_sha")?;
    exact_hex(&candidate.prepared_swarm_sha, 20, "candidate.prepared_swarm_sha")?;
    exact_hex(&candidate.release_repo_sha, 20, "candidate.release_repo_sha")?;
    exact_hex(&candidate.release_topology_sha256, 32, "candidate.release_topology_sha256")?;

    for (field, value) in [
        ("candidate.candidate_id", candidate.candidate_id.as_str()),
        ("candidate.candidate_version", candidate.candidate_version.as_str()),
        ("candidate.extension_version", candidate.extension_version.as_str()),
        ("candidate.server_version", candidate.server_version.as_str()),
        ("candidate.dap_version", candidate.dap_version.as_str()),
        ("candidate.platform", candidate.platform.as_str()),
        ("candidate.target", candidate.target.as_str()),
    ] {
        non_empty(value, field)?;
    }
    if let Some(previous_version) = &candidate.previous_version {
        non_empty(previous_version, "candidate.previous_version")?;
        if previous_version == &candidate.candidate_version {
            bail!("candidate.previous_version must differ from candidate.candidate_version");
        }
    }

    if candidate.extension_version != candidate.candidate_version
        || candidate.server_version != candidate.candidate_version
        || candidate.dap_version != candidate.candidate_version
    {
        bail!("candidate extension, server, and DAP versions must agree");
    }
    Ok(())
}

impl InstallPath {
    fn topology_prefix(self) -> &'static str {
        match self {
            Self::Vsix => "vscode",
            Self::GithubArchive => "github-archive",
            Self::PosixInstaller => "posix",
            Self::PowershellInstaller => "powershell",
            Self::CargoInstall => "cargo",
            Self::ManualArchive => "manual-archive",
        }
    }
}

fn expected_topology_asset(receipt: &Receipt) -> Result<String> {
    let candidate = &receipt.candidate;
    if receipt.transition.path == InstallPath::Vsix {
        return Ok(format!("perl-lsp-rs-{}.vsix", candidate.extension_version));
    }
    let extension = if candidate.platform.starts_with("windows-") { "zip" } else { "tar.gz" };
    Ok(format!("perllsp-{}-{}.{}", candidate.candidate_version, candidate.target, extension))
}

fn validate_topology_path(receipt: &Receipt) -> Result<()> {
    let expected_path_id =
        format!("{}-{}", receipt.transition.path.topology_prefix(), receipt.candidate.platform);
    if receipt.transition.topology_path_id != expected_path_id {
        bail!("receipt topology_path_id is not the canonical path for {}", expected_path_id);
    }
    let expected_asset = expected_topology_asset(receipt)?;
    if receipt.transition.expected_asset != expected_asset {
        bail!("receipt expected_asset is not the canonical asset: expected {expected_asset}");
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
    if let Some(digest) = &transition.artifact_sha256 {
        exact_hex(digest, 32, "transition.artifact_sha256")?;
    }
    for limitation in transition.limitations.iter().chain(receipt.limitations.iter()) {
        non_empty(limitation, "limitations[]")?;
    }

    match transition.class {
        TransitionClass::CleanInstall if receipt.candidate.previous_version.is_some() => {
            bail!("clean_install must not invent a previous version");
        }
        TransitionClass::NormalUpgrade
        | TransitionClass::CachedOldBinary
        | TransitionClass::Rollback
            if receipt.candidate.previous_version.is_none() =>
        {
            bail!("upgrade, cached-binary, and rollback transitions require a previous version");
        }
        TransitionClass::CleanInstall
        | TransitionClass::VersionOrTargetMismatch
        | TransitionClass::InterruptedInstall
        | TransitionClass::ChannelIdentity
        | TransitionClass::NormalUpgrade
        | TransitionClass::CachedOldBinary
        | TransitionClass::Rollback => {}
    }
    Ok(())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn verify_topology_binding(receipt: &Receipt, path: &Path) -> Result<()> {
    let bytes =
        fs::read(path).with_context(|| format!("reading release topology {}", path.display()))?;
    let digest = sha256_bytes(&bytes);
    if digest != receipt.candidate.release_topology_sha256.to_ascii_lowercase() {
        bail!(
            "release topology digest mismatch: receipt={}, actual={digest}",
            receipt.candidate.release_topology_sha256
        );
    }
    let topology: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing release topology {}", path.display()))?;
    if topology["schema"] != 1 || topology["track"] != "public-beta" {
        bail!("release topology is not a public-beta schema v1 manifest");
    }
    if topology["release"] != receipt.candidate.candidate_version {
        bail!("release topology version does not match candidate version");
    }
    if topology["frozen_product_sha"] != receipt.candidate.frozen_product_sha {
        bail!("release topology frozen product SHA does not match candidate");
    }
    if topology["prepared_swarm_sha"] != receipt.candidate.prepared_swarm_sha {
        bail!("release topology prepared swarm SHA does not match candidate");
    }

    if receipt.transition.path == InstallPath::Vsix {
        if topology["vsix"]["version"] != receipt.candidate.extension_version
            || topology["vsix"]["package_path"] != "vscode-extension"
            || topology["vsix"]["asset_name"]
                != format!("perl-lsp-rs-{}.vsix", receipt.candidate.extension_version)
        {
            bail!("release topology VS Code package or asset does not match candidate");
        }
        return Ok(());
    }

    let targets = topology["binary_targets"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("release topology has no binary_targets array"))?;
    let target = targets
        .iter()
        .find(|entry| entry["target"] == receipt.candidate.target)
        .ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "release topology does not publish candidate target {}",
                receipt.candidate.target
            )
        })?;
    if target["archive_name"] != receipt.transition.expected_asset {
        bail!("release topology archive does not match expected asset");
    }
    let members = target["required_members"].as_array().ok_or_else(|| {
        color_eyre::eyre::eyre!("release topology target has no required_members array")
    })?;
    for required in [
        if receipt.candidate.platform.starts_with("windows-") { "perllsp.exe" } else { "perllsp" },
        if receipt.candidate.platform.starts_with("windows-") {
            "perl-dap.exe"
        } else {
            "perl-dap"
        },
    ] {
        if !members.iter().any(|member| member == required) {
            bail!("release topology target omits required member {required}");
        }
    }
    Ok(())
}

fn verify_artifact_binding(receipt: &Receipt, path: &Path) -> Result<()> {
    if !receipt.transition.artifact_verified {
        return Ok(());
    }
    let expected =
        receipt.transition.artifact_sha256.as_deref().ok_or_else(|| {
            color_eyre::eyre::eyre!("verified artifact is missing artifact_sha256")
        })?;
    let bytes =
        fs::read(path).with_context(|| format!("reading candidate artifact {}", path.display()))?;
    let actual = sha256_bytes(&bytes);
    if actual != expected.to_ascii_lowercase() {
        bail!("candidate artifact digest mismatch: receipt={expected}, actual={actual}");
    }
    Ok(())
}

/// A resolved asset that is not the expected one means the install path
/// constructed an artifact name the release did not publish.
///
/// This is the historical PowerShell incident shape: the documented primary
/// installer built an obsolete asset name, so nothing installed. Refusing
/// safely is the correct *runtime* behavior, but the release path is still
/// broken — a user following the documented instructions cannot install. So
/// the mismatch is a safety violation in its own right, independent of the
/// disposition or outcome the receipt claims.
///
/// `None` is not a violation: it means nothing was resolved at all, which the
/// disposition checks already cover.
fn resolved_asset_is_wrong(transition: &TransitionEvidence) -> bool {
    transition
        .resolved_asset
        .as_deref()
        .is_some_and(|resolved| resolved != transition.expected_asset)
}

fn has_safety_violation(transition: &TransitionEvidence) -> bool {
    transition.partial_artifact_promoted
        || transition.mixed_version_reported_ready
        || transition.unsupported_target_selected
        || transition.candidate_process_left_running
        || resolved_asset_is_wrong(transition)
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
    let asset_resolution_is_safe = !transition.documented_primary
        || transition.resolved_asset.is_none()
        || transition.resolved_asset.as_deref() == Some(transition.expected_asset.as_str());
    let observed_identity_is_safe = match &receipt.candidate.previous_version {
        Some(previous_version) => {
            transition.observed_release_identity.is_none()
                || transition.observed_release_identity.as_deref()
                    == Some(previous_version.as_str())
        }
        None => transition.observed_release_identity.is_none(),
    };
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
        && asset_resolution_is_safe
        && observed_identity_is_safe
        && mismatch_contract_holds
}

fn rollback_is_valid(receipt: &Receipt) -> bool {
    let transition = &receipt.transition;
    let Some(previous_version) = &receipt.candidate.previous_version else {
        return false;
    };
    transition.observed_disposition == Disposition::RolledBack
        && transition.outcome == TransitionOutcome::Completed
        && transition.observed_release_identity.as_deref() == Some(previous_version.as_str())
        && transition.known_good_preserved
        && transition.rollback_completed
        && !transition.partial_artifact_promoted
        && !transition.mixed_version_reported_ready
        && !transition.unsupported_target_selected
        && !transition.candidate_process_left_running
}

fn computed_status(receipt: &Receipt) -> ReceiptStatus {
    // A concrete safety violation is stronger evidence than an incomplete
    // receipt: it must remain a hard block instead of being downgraded to
    // `not_proven` by the declared outcome.
    if has_safety_violation(&receipt.transition)
        || receipt.transition.outcome == TransitionOutcome::Failed
    {
        return ReceiptStatus::Blocked;
    }
    if receipt.transition.outcome == TransitionOutcome::NotProven {
        return ReceiptStatus::NotProven;
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
    validate_topology_path(receipt)?;
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
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading install-transition receipt {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parsing install-transition receipt {}", path.display()))
}

fn write_verified_child_artifact(
    receipt: &Receipt,
    status: ReceiptStatus,
    path: &Path,
) -> Result<()> {
    let artifact = VerifiedChildArtifact {
        owner_issue: OWNER_ISSUE,
        schema_version: VERIFIED_CHILD_SCHEMA_VERSION,
        receipt_schema_version: SCHEMA_VERSION,
        candidate_id: &receipt.candidate.candidate_id,
        frozen_product_sha: &receipt.candidate.frozen_product_sha,
        artifact_set_id: &receipt.candidate.artifact_set_id,
        status,
        claim_boundary: &receipt.claim_boundary,
        limitation: receipt.limitations.first().cloned(),
    };
    let content = serde_json::to_vec_pretty(&artifact)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("creating verified artifact directory {}", parent.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary verified artifact near {}", path.display()))?;
    std::io::Write::write_all(&mut temporary, &content)
        .with_context(|| format!("writing temporary verified artifact near {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("flushing temporary verified artifact near {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("publishing verified child artifact {}", path.display()))?;
    Ok(())
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let receipt = load(&args.receipt)?;
    let status = validate(&receipt)?;
    verify_topology_binding(&receipt, &args.topology)?;
    if receipt.transition.artifact_verified {
        let artifact = args
            .artifact
            .as_deref()
            .ok_or_else(|| color_eyre::eyre::eyre!("a passing receipt requires --artifact"))?;
        verify_artifact_binding(&receipt, artifact)?;
    }
    if let Some(path) = &args.verified_output {
        write_verified_child_artifact(&receipt, status, path)?;
    }
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
    use super::{
        sha256_bytes, validate, verify_artifact_binding, verify_topology_binding,
        write_verified_child_artifact, Receipt, ReceiptStatus,
    };
    use color_eyre::eyre::Result;
    use std::fs;
    use tempfile::tempdir;

    fn fixture(content: &str) -> Result<Receipt> {
        Ok(serde_json::from_str(content)?)
    }

    fn topology_for(receipt: &Receipt) -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "release": receipt.candidate.candidate_version,
            "track": "public-beta",
            "frozen_product_sha": receipt.candidate.frozen_product_sha,
            "prepared_swarm_sha": receipt.candidate.prepared_swarm_sha,
            "binary_targets": [{
                "target": receipt.candidate.target,
                "archive_name": receipt.transition.expected_asset,
                "required_members": ["perllsp.exe", "perl-dap.exe"]
            }],
            "vsix": {
                "version": receipt.candidate.extension_version,
                "asset_name": format!("perl-lsp-rs-{}.vsix", receipt.candidate.extension_version),
                "package_path": "vscode-extension"
            }
        })
    }

    #[test]
    fn pass_requires_exact_topology_and_artifact_bytes() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        let directory = tempdir()?;
        let topology = serde_json::to_vec(&topology_for(&receipt))?;
        let topology_path = directory.path().join("release-topology.json");
        fs::write(&topology_path, &topology)?;
        receipt.candidate.release_topology_sha256 = sha256_bytes(&topology);

        let artifact_path = directory.path().join(&receipt.transition.expected_asset);
        let artifact = b"candidate archive bytes";
        fs::write(&artifact_path, artifact)?;
        receipt.transition.artifact_sha256 = Some(sha256_bytes(artifact));

        verify_topology_binding(&receipt, &topology_path)?;
        verify_artifact_binding(&receipt, &artifact_path)?;
        Ok(())
    }

    #[test]
    fn topology_digest_mismatch_is_rejected() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        let directory = tempdir()?;
        let topology_path = directory.path().join("release-topology.json");
        fs::write(&topology_path, br#"{"schema":1,"track":"public-beta"}"#)?;
        match verify_topology_binding(&receipt, &topology_path) {
            Ok(()) => bail!("topology digest mismatch unexpectedly passed"),
            Err(error) => {
                assert!(format!("{error:#}").contains("digest mismatch"));
            }
        }
        Ok(())
    }

    #[test]
    fn prepared_swarm_identity_mismatch_is_rejected() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        let directory = tempdir()?;
        let mut topology = topology_for(&receipt);
        topology["prepared_swarm_sha"] = serde_json::Value::String("4".repeat(40));
        let bytes = serde_json::to_vec(&topology)?;
        let topology_path = directory.path().join("release-topology.json");
        fs::write(&topology_path, &bytes)?;
        let mut receipt = receipt;
        receipt.candidate.release_topology_sha256 = sha256_bytes(&bytes);
        match verify_topology_binding(&receipt, &topology_path) {
            Ok(()) => bail!("prepared swarm identity mismatch unexpectedly passed"),
            Err(error) => {
                assert!(format!("{error:#}").contains("prepared swarm SHA"));
            }
        }
        Ok(())
    }

    #[test]
    fn vsix_asset_identity_mismatch_is_rejected() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        receipt.transition.path = super::InstallPath::Vsix;
        receipt.transition.topology_path_id = "vscode-windows-x86_64".to_string();
        receipt.transition.expected_asset = "perl-lsp-rs-0.18.0.vsix".to_string();
        let mut topology = topology_for(&receipt);
        topology["vsix"]["asset_name"] = serde_json::Value::String("wrong.vsix".to_string());
        let directory = tempdir()?;
        let bytes = serde_json::to_vec(&topology)?;
        let topology_path = directory.path().join("release-topology.json");
        fs::write(&topology_path, &bytes)?;
        receipt.candidate.release_topology_sha256 = sha256_bytes(&bytes);
        match verify_topology_binding(&receipt, &topology_path) {
            Ok(()) => bail!("VSIX asset identity mismatch unexpectedly passed"),
            Err(error) => {
                assert!(format!("{error:#}").contains("VS Code package or asset"));
            }
        }
        Ok(())
    }

    #[test]
    fn verified_child_output_carries_transition_identity() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        let status = validate(&receipt)?;
        let directory = tempdir()?;
        let output = directory.path().join("child.json");
        write_verified_child_artifact(&receipt, status, &output)?;
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(output)?)?;
        assert_eq!(value["schema_version"], "verified_child_receipt.v1");
        assert_eq!(value["receipt_schema_version"], "install_transition.v1");
        assert_eq!(value["candidate_id"], "v0.18.0-rc1");
        assert_eq!(value["artifact_set_id"], "v0.18.0-rc1-primary");
        assert_eq!(value["status"], ReceiptStatus::Pass.as_str());
        Ok(())
    }

    #[test]
    fn failed_publish_keeps_existing_artifact_directory_intact() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        let status = validate(&receipt)?;
        let directory = tempdir()?;
        let output = directory.path().join("existing-artifact");
        fs::create_dir(&output)?;
        let marker = output.join("previous.json");
        fs::write(&marker, b"previous verified artifact")?;

        let error = write_verified_child_artifact(&receipt, status, &output)
            .expect_err("publishing over a directory must fail");

        assert!(
            format!("{error:#}").contains("publishing verified child artifact"),
            "publish failure should identify the destination: {error:#}"
        );
        assert_eq!(fs::read(&marker)?, b"previous verified artifact");
        Ok(())
    }

    #[test]
    fn clean_install_fixture_passes() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        assert_eq!(validate(&receipt)?, ReceiptStatus::Pass);
        Ok(())
    }

    #[test]
    fn receipt_must_match_canonical_topology_path() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        validate_topology_path(&receipt)?;
        Ok(())
    }

    #[test]
    fn fabricated_topology_path_id_is_rejected() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        receipt.transition.topology_path_id = "fabricated-path".to_string();
        match validate_topology_path(&receipt) {
            Ok(()) => bail!("fabricated topology path unexpectedly validated"),
            Err(error) => {
                let rendered = format!("{error:#}");
                if !rendered.contains("receipt topology_path_id is not the canonical path") {
                    bail!("unexpected topology rejection: {rendered}");
                }
            }
        }
        Ok(())
    }

    #[test]
    fn every_install_path_has_one_canonical_path_and_asset() -> Result<()> {
        let cases = [
            (InstallPath::Vsix, "vscode-linux-x86_64", "perl-lsp-rs-0.18.0.vsix"),
            (
                InstallPath::GithubArchive,
                "github-archive-linux-x86_64",
                "perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz",
            ),
            (
                InstallPath::PosixInstaller,
                "posix-linux-x86_64",
                "perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz",
            ),
            (
                InstallPath::PowershellInstaller,
                "powershell-windows-x86_64",
                "perllsp-0.18.0-x86_64-pc-windows-msvc.zip",
            ),
            (
                InstallPath::CargoInstall,
                "cargo-linux-x86_64",
                "perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz",
            ),
            (
                InstallPath::ManualArchive,
                "manual-archive-linux-x86_64",
                "perllsp-0.18.0-x86_64-unknown-linux-gnu.tar.gz",
            ),
        ];
        for (path, path_id, asset) in cases {
            let mut receipt = fixture(include_str!(
                "../../fixtures/experience/install_transition/clean_install.json"
            ))?;
            receipt.candidate.platform = if path == InstallPath::PowershellInstaller {
                "windows-x86_64".to_string()
            } else {
                "linux-x86_64".to_string()
            };
            receipt.candidate.target = if path == InstallPath::PowershellInstaller {
                "x86_64-pc-windows-msvc".to_string()
            } else {
                "x86_64-unknown-linux-gnu".to_string()
            };
            receipt.transition.path = path;
            receipt.transition.topology_path_id = path_id.to_string();
            receipt.transition.expected_asset = asset.to_string();
            validate_topology_path(&receipt)?;
        }
        Ok(())
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

    /// The historical PowerShell packet escapes only through `outcome: failed`.
    /// A receipt that reports the *same* wrong-asset construction as a clean,
    /// completed safe refusal must not be able to claim `pass` — the documented
    /// install path is still broken for the user.
    #[test]
    fn wrong_asset_cannot_claim_pass_by_reporting_a_clean_refusal() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/powershell_404.json"
        ))?;
        receipt.transition.intended_disposition = super::Disposition::Rejected;
        receipt.transition.observed_disposition = super::Disposition::Rejected;
        receipt.transition.outcome = super::TransitionOutcome::Completed;
        receipt.transition.artifact_verified = true;

        assert_eq!(
            validate(&receipt)?,
            ReceiptStatus::Blocked,
            "a documented-primary path that resolved an unpublished asset must be blocked \
             even when the transition itself completed as a safe refusal"
        );
        Ok(())
    }

    /// Guards the direction of the check above: matching assets must still be
    /// able to reach `pass`, so the new invariant is not blocking everything.
    #[test]
    fn matching_resolved_asset_still_reaches_pass() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        assert_eq!(
            receipt.transition.resolved_asset.as_deref(),
            Some(receipt.transition.expected_asset.as_str())
        );
        assert_eq!(validate(&receipt)?, ReceiptStatus::Pass);
        Ok(())
    }

    /// Assert that validation rejected the receipt *for the intended reason*.
    ///
    /// A bare `is_err()` passes for any error, including a schema or parse
    /// failure that never reaches the invariant under test — so it cannot
    /// distinguish "the guard fired" from "the fixture stopped deserializing".
    #[track_caller]
    fn assert_rejected_because(receipt: &Receipt, expected: &str) {
        match validate(receipt) {
            Ok(status) => {
                panic!(
                    "expected rejection mentioning {expected:?}, but validation returned {status:?}"
                )
            }
            Err(error) => {
                let rendered = format!("{error:#}");
                assert!(
                    rendered.contains(expected),
                    "expected rejection mentioning {expected:?}, got: {rendered}"
                );
            }
        }
    }

    #[test]
    fn mixed_version_ready_state_cannot_claim_pass() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/normal_upgrade.json"
        ))?;
        receipt.transition.mixed_version_reported_ready = true;
        assert_rejected_because(&receipt, "disagrees with computed status blocked");
        Ok(())
    }

    #[test]
    fn safety_violation_cannot_be_downgraded_to_not_proven() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/normal_upgrade.json"
        ))?;
        receipt.status = ReceiptStatus::Blocked;
        receipt.transition.outcome = super::TransitionOutcome::NotProven;
        receipt.transition.candidate_process_left_running = true;
        assert_eq!(validate(&receipt)?, ReceiptStatus::Blocked);
        Ok(())
    }

    #[test]
    fn rollback_requires_a_named_previous_pair() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/install_transition/clean_install.json"
        ))?;
        receipt.transition.class = super::TransitionClass::Rollback;
        receipt.transition.intended_disposition = super::Disposition::RolledBack;
        receipt.transition.observed_disposition = super::Disposition::RolledBack;
        receipt.transition.rollback_completed = true;
        assert_rejected_because(&receipt, "require a previous version");
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
        assert_rejected_because(&receipt, "disagrees with computed status blocked");
        Ok(())
    }
}
