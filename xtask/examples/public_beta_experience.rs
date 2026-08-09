//! Validate one candidate-bound public-beta experience fan-in receipt.
//!
//! The fan-in joins independent child receipts and journey-cell dispositions.
//! It computes a conjunctive ready/blocked/not-proven result: a strong result in
//! one rail cannot compensate for a trust-breaking count, failed journey cell,
//! blocked child receipt, stale candidate identity, or missing proof.

#![allow(clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const CHECK: &str = "public-beta-experience";
const SCHEMA_VERSION: &str = "public_beta_experience.v1";
const VERIFIED_CHILD_SCHEMA_VERSION: &str = "verified_child_receipt.v1";
const REQUIRED_CELLS: [JourneyCellId; 11] = [
    JourneyCellId::InstallUpgrade,
    JourneyCellId::Startup,
    JourneyCellId::Workspace,
    JourneyCellId::CompletionHoverNavigation,
    JourneyCellId::Diagnostics,
    JourneyCellId::EmptyResults,
    JourneyCellId::RenameDelete,
    JourneyCellId::Formatting,
    JourneyCellId::DoctorTrust,
    JourneyCellId::DapPreview,
    JourneyCellId::Shutdown,
];
const REQUIRED_CORE_LOOP: [&str; 9] = [
    "diagnostics",
    "completion",
    "hover",
    "definition",
    "references",
    "symbols",
    "edit_requery",
    "formatting",
    "safe_rename_or_refusal",
];
const REQUIRED_PROJECT_FAMILIES: [&str; 5] = [
    "conventional_modules",
    "test_heavy",
    "framework_shaped",
    "environment_sensitive",
    "dynamic_boundary_control",
];
const EXPECTED_EDITOR: &str = "VS Code is the first-class installed experience.";
const EXPECTED_OTHER_EDITORS: &str =
    "Standard LSP compatibility only; no equivalent UI-polish claim.";
const EXPECTED_DYNAMIC_PERL: &str = "Bounded fallback, degraded answer, or refusal with an intelligible reason; never manufactured certainty.";

#[derive(Debug, Parser)]
#[command(name = "public-beta-experience")]
#[command(about = "Validate one candidate-bound public-beta experience fan-in")]
struct Args {
    /// Fan-in receipt JSON to validate.
    #[arg(long)]
    receipt: PathBuf,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OverallStatus {
    Ready,
    Blocked,
    NotProven,
}

impl OverallStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::NotProven => "not_proven",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReleaseTrack {
    PublicBeta,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DapPosture {
    Preview,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum JourneyCellId {
    InstallUpgrade,
    Startup,
    Workspace,
    CompletionHoverNavigation,
    Diagnostics,
    EmptyResults,
    RenameDelete,
    Formatting,
    DoctorTrust,
    DapPreview,
    Shutdown,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CellDisposition {
    Pass,
    Limited,
    Failed,
    NotProven,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum InputStatus {
    Pass,
    Limited,
    Blocked,
    NotProven,
}

impl InputStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Limited => "limited",
            Self::Blocked => "blocked",
            Self::NotProven => "not_proven",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateIdentity {
    release: String,
    track: ReleaseTrack,
    frozen_product_sha: String,
    candidate_id: String,
    artifact_set_id: String,
    release_topology_sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SupportedEnvelope {
    editor: String,
    other_editors: String,
    project_families: Vec<String>,
    core_loop: Vec<String>,
    dynamic_perl: String,
    dap_posture: DapPosture,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JourneyCell {
    id: JourneyCellId,
    owner_issue: String,
    disposition: CellDisposition,
    evidence_refs: Vec<String>,
    limitation: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ZeroBudgetCounts {
    false_exact: u64,
    stale_exact: u64,
    unsafe_edit: u64,
    unexplained_success_empty: u64,
    silent_startup_failure: u64,
    broken_documented_install: u64,
    wrong_binary_or_version: u64,
    orphaned_server_or_debuggee: u64,
}

impl ZeroBudgetCounts {
    fn total(&self) -> u64 {
        self.false_exact
            + self.stale_exact
            + self.unsafe_edit
            + self.unexplained_success_empty
            + self.silent_startup_failure
            + self.broken_documented_install
            + self.wrong_binary_or_version
            + self.orphaned_server_or_debuggee
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReceiptRef {
    owner_issue: String,
    artifact_path: String,
    schema_version: String,
    sha256: String,
    source_artifact_path: Option<String>,
    source_sha256: Option<String>,
    candidate_id: String,
    status: InputStatus,
    claim_boundary: String,
    limitation: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChildReceipts {
    user_state_presentation: ReceiptRef,
    first_ten_minutes: ReceiptRef,
    install_transition: ReceiptRef,
    installed_acceptance: ReceiptRef,
    first_useful_answer: ReceiptRef,
    representative_workload: ReceiptRef,
    release_topology: ReceiptRef,
    release_integrity: ReceiptRef,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedChildArtifact {
    owner_issue: String,
    schema_version: String,
    receipt_schema_version: String,
    candidate_id: String,
    frozen_product_sha: String,
    artifact_set_id: String,
    source_receipt_sha256: Option<String>,
    status: InputStatus,
    claim_boundary: String,
    limitation: Option<String>,
}

impl ChildReceipts {
    fn iter(&self) -> [(&'static str, &ReceiptRef); 8] {
        [
            ("user_state_presentation", &self.user_state_presentation),
            ("first_ten_minutes", &self.first_ten_minutes),
            ("install_transition", &self.install_transition),
            ("installed_acceptance", &self.installed_acceptance),
            ("first_useful_answer", &self.first_useful_answer),
            ("representative_workload", &self.representative_workload),
            ("release_topology", &self.release_topology),
            ("release_integrity", &self.release_integrity),
        ]
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    check: String,
    schema_version: String,
    status: OverallStatus,
    claim_boundary: String,
    candidate: CandidateIdentity,
    supported_envelope: SupportedEnvelope,
    journey_cells: Vec<JourneyCell>,
    zero_budget: ZeroBudgetCounts,
    child_receipts: ChildReceipts,
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

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn issue_identity(value: &str, field: &str) -> Result<()> {
    if !value.starts_with('#')
        || value.len() < 2
        || !value[1..].bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("{field} must use #<number> identity");
    }
    Ok(())
}

fn validate_candidate(receipt: &Receipt) -> Result<()> {
    exact_hex(&receipt.candidate.frozen_product_sha, 20, "candidate.frozen_product_sha")?;
    exact_hex(&receipt.candidate.release_topology_sha256, 32, "candidate.release_topology_sha256")?;
    for (field, value) in [
        ("candidate.release", receipt.candidate.release.as_str()),
        ("candidate.candidate_id", receipt.candidate.candidate_id.as_str()),
        ("candidate.artifact_set_id", receipt.candidate.artifact_set_id.as_str()),
    ] {
        non_empty(value, field)?;
    }
    Ok(())
}

fn validate_envelope(envelope: &SupportedEnvelope) -> Result<()> {
    if envelope.editor != EXPECTED_EDITOR
        || envelope.other_editors != EXPECTED_OTHER_EDITORS
        || envelope.dynamic_perl != EXPECTED_DYNAMIC_PERL
    {
        bail!("supported_envelope prose must match the accepted v0.18 envelope");
    }
    let observed: BTreeSet<&str> = envelope.project_families.iter().map(String::as_str).collect();
    let required = BTreeSet::from(REQUIRED_PROJECT_FAMILIES);
    if observed != required {
        bail!("supported_envelope.project_families must match the accepted representative set");
    }

    let observed: BTreeSet<&str> = envelope.core_loop.iter().map(String::as_str).collect();
    let required = BTreeSet::from(REQUIRED_CORE_LOOP);
    if observed != required {
        bail!("supported_envelope.core_loop must match the accepted v0.18 core loop");
    }
    Ok(())
}

fn validate_journey_cells(receipt: &Receipt) -> Result<()> {
    let mut observed = BTreeSet::new();
    for cell in &receipt.journey_cells {
        if !observed.insert(cell.id) {
            bail!("duplicate journey cell: {:?}", cell.id);
        }
        issue_identity(&cell.owner_issue, "journey_cells[].owner_issue")?;
        if cell.evidence_refs.is_empty() {
            bail!("journey_cells[].evidence_refs must not be empty");
        }
        for evidence in &cell.evidence_refs {
            non_empty(evidence, "journey_cells[].evidence_refs[]")?;
        }
        match cell.disposition {
            CellDisposition::Limited | CellDisposition::NotProven => {
                let limitation = cell.limitation.as_deref().unwrap_or_default();
                non_empty(limitation, "limited/not-proven journey cell limitation")?;
            }
            CellDisposition::Pass | CellDisposition::Failed => {}
        }
    }

    let required = BTreeSet::from(REQUIRED_CELLS);
    if observed != required {
        bail!("journey_cells must contain each accepted experience cell exactly once");
    }
    Ok(())
}

fn validate_child_receipts(receipt: &Receipt) -> Result<()> {
    for (name, child) in receipt.child_receipts.iter() {
        let (expected_schema, expected_owner) = match name {
            "user_state_presentation" => ("workspace_experience.v1", "#5901"),
            "first_ten_minutes" => ("first_ten_minutes.v1", "#5902"),
            "install_transition" => ("install_transition.v1", "#5903"),
            "installed_acceptance" => ("installed_acceptance.v1", "#4346"),
            "first_useful_answer" => ("first_useful_answer.v1", "#4048"),
            "representative_workload" => ("scenario67.v1", "#4050"),
            "release_topology" => ("release_topology.v1", "#5889"),
            "release_integrity" => ("release_integrity.v1", "#4145"),
            _ => bail!("unknown child receipt slot: {name}"),
        };
        if child.schema_version != expected_schema {
            bail!("child_receipts.{name} must use schema {expected_schema}");
        }
        exact_hex(&child.sha256, 32, &format!("child_receipts.{name}.sha256"))?;
        match (&child.source_artifact_path, &child.source_sha256) {
            (Some(path), Some(digest)) => {
                non_empty(path, &format!("child_receipts.{name}.source_artifact_path"))?;
                exact_hex(digest, 32, &format!("child_receipts.{name}.source_sha256"))?;
                let source_path = Path::new(path);
                if source_path.is_absolute()
                    || source_path.components().any(|component| component == Component::ParentDir)
                {
                    bail!(
                        "child_receipts.{name}.source_artifact_path must stay below the receipt directory"
                    );
                }
            }
            (None, None) => {}
            _ => bail!("child_receipts.{name} source path and digest must be provided together"),
        }
        non_empty(&child.claim_boundary, &format!("child_receipts.{name}.claim_boundary"))?;
        if child.owner_issue != expected_owner {
            bail!("child_receipts.{name} must be owned by {expected_owner}");
        }
        non_empty(&child.artifact_path, &format!("child_receipts.{name}.artifact_path"))?;
        let artifact_path = Path::new(&child.artifact_path);
        if artifact_path.is_absolute()
            || artifact_path.components().any(|component| component == Component::ParentDir)
        {
            bail!("child_receipts.{name}.artifact_path must stay below the receipt directory");
        }
        if child.candidate_id != receipt.candidate.candidate_id {
            bail!("child_receipts.{name} belongs to a different candidate");
        }
        match child.status {
            InputStatus::Limited => {
                let limitation = child.limitation.as_deref().unwrap_or_default();
                non_empty(limitation, &format!("child_receipts.{name}.limitation"))?;
            }
            InputStatus::Pass | InputStatus::Blocked | InputStatus::NotProven => {}
        }
    }

    Ok(())
}

fn load_verified_child_artifact(
    name: &str,
    child: &ReceiptRef,
    receipt: &Receipt,
    artifact_root: &Path,
) -> Result<()> {
    let path = artifact_root.join(&child.artifact_path);
    let bytes = fs::read(&path)
        .with_context(|| format!("reading child receipt artifact {name}: {}", path.display()))?;
    let digest = sha256_hex(&bytes);
    if digest != child.sha256 {
        bail!("child_receipts.{name} digest does not match artifact bytes");
    }
    let artifact: VerifiedChildArtifact = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing child receipt artifact {name}: {}", path.display()))?;
    if artifact.schema_version != VERIFIED_CHILD_SCHEMA_VERSION {
        bail!("child_receipts.{name} artifact is not a verified child envelope");
    }
    if artifact.receipt_schema_version != child.schema_version {
        bail!("child_receipts.{name} artifact schema differs from the declared schema");
    }
    if artifact.owner_issue != child.owner_issue {
        bail!("child_receipts.{name} artifact owner differs from the declared owner");
    }
    if artifact.candidate_id != receipt.candidate.candidate_id {
        bail!("child_receipts.{name} artifact belongs to a different candidate");
    }
    if artifact.frozen_product_sha != receipt.candidate.frozen_product_sha {
        bail!("child_receipts.{name} artifact belongs to a different frozen product");
    }
    if artifact.artifact_set_id != receipt.candidate.artifact_set_id {
        bail!("child_receipts.{name} artifact belongs to a different artifact set");
    }
    if artifact.source_receipt_sha256 != child.source_sha256 {
        bail!("child_receipts.{name} source digest differs from the verified artifact");
    }
    if artifact.status != child.status || artifact.claim_boundary != child.claim_boundary {
        bail!("child_receipts.{name} metadata differs from the verified artifact");
    }
    if artifact.limitation != child.limitation {
        bail!("child_receipts.{name} limitation differs from the verified artifact");
    }
    Ok(())
}

fn validate_source_receipt(
    name: &str,
    child: &ReceiptRef,
    receipt: &Receipt,
    artifact_root: &Path,
) -> Result<()> {
    let (Some(source_path), Some(source_sha256)) =
        (&child.source_artifact_path, &child.source_sha256)
    else {
        return Ok(());
    };
    let path = artifact_root.join(source_path);
    let bytes = fs::read(&path)
        .with_context(|| format!("reading source receipt {name}: {}", path.display()))?;
    let digest = sha256_hex(&bytes);
    if digest != *source_sha256 {
        bail!("child_receipts.{name} source digest does not match artifact bytes");
    }
    let source: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing source receipt {name}: {}", path.display()))?;
    if source.get("schema_version").and_then(serde_json::Value::as_str)
        != Some(child.schema_version.as_str())
    {
        bail!("child_receipts.{name} source schema differs from the declared schema");
    }
    if source.get("status").and_then(serde_json::Value::as_str) != Some(child.status.as_str()) {
        bail!("child_receipts.{name} source status differs from the declared status");
    }
    let source_candidate = source
        .get("candidate")
        .and_then(|candidate| candidate.get("candidate_id"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            color_eyre::eyre::eyre!("child_receipts.{name} source lacks candidate identity")
        })?;
    if source_candidate != receipt.candidate.candidate_id {
        bail!("child_receipts.{name} source belongs to a different candidate");
    }
    Ok(())
}

fn validate_child_artifacts(receipt: &Receipt, artifact_root: &Path) -> Result<()> {
    for (name, child) in receipt.child_receipts.iter() {
        load_verified_child_artifact(name, child, receipt, artifact_root)?;
        validate_source_receipt(name, child, receipt, artifact_root)?;
    }
    Ok(())
}

fn computed_status(receipt: &Receipt) -> OverallStatus {
    let blocked_cell =
        receipt.journey_cells.iter().any(|cell| cell.disposition == CellDisposition::Failed);
    let blocked_child = receipt
        .child_receipts
        .iter()
        .into_iter()
        .any(|(_, child)| child.status == InputStatus::Blocked);
    if receipt.zero_budget.total() > 0 || blocked_cell || blocked_child {
        return OverallStatus::Blocked;
    }

    let unproven_cell =
        receipt.journey_cells.iter().any(|cell| cell.disposition == CellDisposition::NotProven);
    let unproven_child = receipt
        .child_receipts
        .iter()
        .into_iter()
        .any(|(_, child)| child.status == InputStatus::NotProven);
    let missing_source_provenance = receipt
        .child_receipts
        .iter()
        .into_iter()
        .any(|(_, child)| child.source_artifact_path.is_none());
    if unproven_cell || unproven_child || missing_source_provenance {
        return OverallStatus::NotProven;
    }

    OverallStatus::Ready
}

fn validate(receipt: &Receipt) -> Result<OverallStatus> {
    if receipt.check != CHECK {
        bail!("check must be {CHECK}");
    }
    if receipt.schema_version != SCHEMA_VERSION {
        bail!("schema_version must be {SCHEMA_VERSION}");
    }
    non_empty(&receipt.claim_boundary, "claim_boundary")?;
    validate_candidate(receipt)?;
    validate_envelope(&receipt.supported_envelope)?;
    validate_journey_cells(receipt)?;
    validate_child_receipts(receipt)?;
    for limitation in &receipt.limitations {
        non_empty(limitation, "limitations[]")?;
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
    let artifact_root = args
        .receipt
        .parent()
        .ok_or_else(|| color_eyre::eyre::eyre!("receipt path has no parent directory"))?;
    validate_child_artifacts(&receipt, artifact_root)?;
    println!(
        "public-beta-experience: release={} candidate={} status={} zero_budget={} cells={} inputs={}",
        receipt.candidate.release,
        receipt.candidate.candidate_id,
        status.as_str(),
        receipt.zero_budget.total(),
        receipt.journey_cells.len(),
        receipt.child_receipts.iter().len()
    );
    if status != OverallStatus::Ready {
        bail!("public-beta experience is {}", status.as_str());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CellDisposition, OverallStatus, Receipt, validate, validate_child_artifacts};
    use color_eyre::eyre::Result;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn fixture(content: &str) -> Result<Receipt> {
        Ok(serde_json::from_str(content)?)
    }

    #[test]
    fn ready_fixture_is_not_proven_without_source_receipts() -> Result<()> {
        let receipt = fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        assert_eq!(validate(&receipt)?, OverallStatus::NotProven);
        Ok(())
    }

    #[test]
    fn ready_fixture_consumes_hashed_child_artifacts() -> Result<()> {
        let receipt = fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        let artifact_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/experience/public_beta");
        validate_child_artifacts(&receipt, &artifact_root)?;
        Ok(())
    }

    #[test]
    fn source_receipt_provenance_binds_raw_bytes_and_envelope() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        let source = br#"{"schema_version":"install_transition.v1","status":"pass","candidate":{"candidate_id":"v0.18.0-rc1"}}"#;
        let source_digest = super::sha256_hex(source);
        let install = &mut receipt.child_receipts.install_transition;
        install.source_artifact_path = Some("sources/install_transition.json".to_string());
        install.source_sha256 = Some(source_digest.clone());

        let fixture_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/experience/public_beta");
        let directory = tempdir()?;
        let mut install_artifact_digest = None;
        for (_, child) in receipt.child_receipts.iter() {
            let source_path = fixture_root.join(&child.artifact_path);
            let target_path = directory.path().join(&child.artifact_path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let original = fs::read(source_path)?;
            let mut artifact_bytes = original.clone();
            if child.owner_issue == "#5903" {
                let mut artifact: serde_json::Value = serde_json::from_slice(&original)?;
                artifact["source_receipt_sha256"] =
                    serde_json::Value::String(source_digest.clone());
                artifact_bytes = serde_json::to_vec_pretty(&artifact)?;
                install_artifact_digest = Some(super::sha256_hex(&artifact_bytes));
            }
            fs::write(target_path, artifact_bytes)?;
        }
        receipt.child_receipts.install_transition.sha256 = install_artifact_digest
            .ok_or_else(|| color_eyre::eyre::eyre!("missing install artifact"))?;
        let source_path = directory.path().join("sources/install_transition.json");
        let source_parent = source_path
            .parent()
            .ok_or_else(|| color_eyre::eyre::eyre!("source path has no parent"))?;
        fs::create_dir_all(source_parent)?;
        fs::write(&source_path, source)?;

        validate_child_artifacts(&receipt, directory.path())?;
        fs::write(&source_path, b"tampered")?;
        assert!(validate_child_artifacts(&receipt, directory.path()).is_err());
        Ok(())
    }

    #[test]
    fn missing_source_provenance_cannot_claim_ready() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.status = OverallStatus::Ready;
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn child_artifact_digest_mismatch_fails_closed() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.child_receipts.install_transition.sha256 = "0".repeat(64);
        let artifact_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/experience/public_beta");
        assert!(validate_child_artifacts(&receipt, &artifact_root).is_err());
        Ok(())
    }

    #[test]
    fn blocked_fixture_is_blocked() -> Result<()> {
        let receipt = fixture(include_str!("../../fixtures/experience/public_beta/blocked.json"))?;
        assert_eq!(validate(&receipt)?, OverallStatus::Blocked);
        Ok(())
    }

    #[test]
    fn a_nonzero_zero_budget_count_cannot_claim_ready() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.zero_budget.wrong_binary_or_version = 1;
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn cross_candidate_child_evidence_fails_closed() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.child_receipts.install_transition.candidate_id = "another-candidate".to_string();
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn release_topology_envelope_digest_is_not_the_subject_digest() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.child_receipts.release_topology.sha256 = "9".repeat(64);
        let artifact_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/experience/public_beta");
        assert!(validate_child_artifacts(&receipt, &artifact_root).is_err());
        Ok(())
    }

    #[test]
    fn child_slot_schema_and_owner_are_not_substitutable() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.child_receipts.install_transition.schema_version = "other.v1".to_string();
        assert!(validate(&receipt).is_err());

        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.child_receipts.install_transition.owner_issue = "#4048".to_string();
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn limited_child_requires_visible_limitation() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.child_receipts.first_ten_minutes.status = super::InputStatus::Limited;
        receipt.child_receipts.first_ten_minutes.limitation = None;
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn supported_envelope_cannot_shrink_project_family_set() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.supported_envelope.project_families.pop();
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn limited_cell_requires_an_explicit_limitation() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        if let Some(cell) = receipt
            .journey_cells
            .iter_mut()
            .find(|cell| cell.disposition == CellDisposition::Limited)
        {
            cell.limitation = None;
        }
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn missing_journey_cell_fails_closed() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.journey_cells.retain(|cell| cell.id != super::JourneyCellId::Shutdown);
        assert!(validate(&receipt).is_err());
        Ok(())
    }
}
