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
use std::path::{Path, PathBuf};

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

    /// Permit a structurally valid but not-yet-ready fan-in receipt.
    #[arg(long)]
    allow_not_proven: bool,
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
    #[expect(
        dead_code,
        reason = "policy:pending-zero-budget-consumer: reserved for receipt validation"
    )]
    fn has_violations(&self) -> bool {
        self.false_exact > 0
            || self.stale_exact > 0
            || self.unsafe_edit > 0
            || self.unexplained_success_empty > 0
            || self.silent_startup_failure > 0
            || self.broken_documented_install > 0
            || self.wrong_binary_or_version > 0
            || self.orphaned_server_or_debuggee > 0
    }

    fn total(&self) -> u64 {
        [
            self.false_exact,
            self.stale_exact,
            self.unsafe_edit,
            self.unexplained_success_empty,
            self.silent_startup_failure,
            self.broken_documented_install,
            self.wrong_binary_or_version,
            self.orphaned_server_or_debuggee,
        ]
        .iter()
        .try_fold(0u64, |acc, value| acc.checked_add(*value))
        .unwrap_or(1)
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

fn unsafe_serialized_path(value: &str) -> bool {
    value.is_empty()
        || value.contains('\0')
        || value.starts_with('/')
        || value.starts_with('\\')
        || (value.len() >= 2
            && value.as_bytes()[0].is_ascii_alphabetic()
            && value.as_bytes()[1] == b':')
        || value.split(['/', '\\']).any(|component| component == "..")
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
            if let Some((_, candidate_ref)) = evidence.rsplit_once('/')
                && candidate_ref.starts_with('v')
                && candidate_ref != receipt.candidate.candidate_id
            {
                bail!(
                    "journey_cells[].evidence_refs[] must bind to candidate {}",
                    receipt.candidate.candidate_id
                );
            }
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
                if unsafe_serialized_path(path) {
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
        if unsafe_serialized_path(&child.artifact_path) {
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
    match child.schema_version.as_str() {
        "installed_acceptance.v1" => {
            validate_installed_acceptance_source(name, child, receipt, &source)?;
        }
        "release_topology.v1" => {
            validate_release_topology_source(name, child, receipt, &source)?;
        }
        _ => validate_canonical_source_receipt(name, child, receipt, &source)?,
    }
    Ok(())
}

fn validate_canonical_source_receipt(
    name: &str,
    child: &ReceiptRef,
    receipt: &Receipt,
    source: &serde_json::Value,
) -> Result<()> {
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

fn validate_installed_acceptance_source(
    name: &str,
    child: &ReceiptRef,
    receipt: &Receipt,
    source: &serde_json::Value,
) -> Result<()> {
    let schema_version = source.get("schema_version").ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "child_receipts.{name} installed-acceptance source lacks schema_version"
        )
    })?;
    if schema_version.as_i64() != Some(1) {
        bail!("child_receipts.{name} installed-acceptance source must use schema_version 1");
    }
    let outcome = source.get("outcome").and_then(serde_json::Value::as_str).ok_or_else(|| {
        color_eyre::eyre::eyre!("child_receipts.{name} installed-acceptance source lacks outcome")
    })?;
    let known_limitations = source
        .get("known_limitations")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .count()
        })
        .unwrap_or(0);
    let derived_status = match outcome {
        "failed" => InputStatus::Blocked,
        "not_proven" => InputStatus::NotProven,
        "completed" if known_limitations > 0 => InputStatus::Limited,
        "completed" => InputStatus::Pass,
        _ => bail!("child_receipts.{name} installed-acceptance source has unknown outcome"),
    };
    if derived_status != child.status {
        bail!(
            "child_receipts.{name} installed-acceptance source status differs from the declared status"
        );
    }
    let repository_sha =
        source.get("repository_sha").and_then(serde_json::Value::as_str).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "child_receipts.{name} installed-acceptance source lacks repository_sha"
            )
        })?;
    if repository_sha != receipt.candidate.frozen_product_sha {
        bail!(
            "child_receipts.{name} installed-acceptance source belongs to a different frozen product"
        );
    }

    let server_identity =
        source.get("server_identity").and_then(serde_json::Value::as_object).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "child_receipts.{name} installed-acceptance source lacks server_identity"
            )
        })?;
    if server_identity.get("source").and_then(serde_json::Value::as_str)
        != Some("packaged_vsix_bundle")
        || server_identity.get("path").and_then(serde_json::Value::as_str).is_none_or(str::is_empty)
    {
        bail!(
            "child_receipts.{name} installed-acceptance source has incomplete packaged server identity"
        );
    }
    let artifact_hashes =
        source.get("artifact_hashes").and_then(serde_json::Value::as_object).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "child_receipts.{name} installed-acceptance source lacks artifact_hashes"
            )
        })?;
    let bundled_server_sha256 = artifact_hashes
        .get("bundled_server_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "child_receipts.{name} installed-acceptance source lacks bundled_server_sha256"
            )
        })?;
    exact_hex(bundled_server_sha256, 32, &format!("child_receipts.{name}.bundled_server_sha256"))?;
    let vsix_identity =
        source.get("vsix_identity").and_then(serde_json::Value::as_object).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "child_receipts.{name} installed-acceptance source lacks vsix_identity"
            )
        })?;
    for field in ["extension_id", "version", "path"] {
        if vsix_identity.get(field).and_then(serde_json::Value::as_str).is_none_or(str::is_empty) {
            bail!("child_receipts.{name} installed-acceptance source lacks vsix_identity.{field}");
        }
    }
    if source.get("claim_boundary").and_then(serde_json::Value::as_str).is_none_or(str::is_empty) {
        bail!("child_receipts.{name} installed-acceptance source lacks claim_boundary");
    }
    Ok(())
}

fn validate_release_topology_source(
    name: &str,
    child: &ReceiptRef,
    receipt: &Receipt,
    source: &serde_json::Value,
) -> Result<()> {
    if source.get("schema").and_then(serde_json::Value::as_i64) != Some(1) {
        bail!("child_receipts.{name} release-topology source must use schema 1");
    }
    let frozen_product_sha =
        source.get("frozen_product_sha").and_then(serde_json::Value::as_str).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "child_receipts.{name} release-topology source lacks frozen_product_sha"
            )
        })?;
    if frozen_product_sha != receipt.candidate.frozen_product_sha {
        bail!(
            "child_receipts.{name} release-topology source belongs to a different frozen product"
        );
    }
    if child.status != InputStatus::Pass {
        bail!(
            "child_receipts.{name} release-topology source status differs from the declared status"
        );
    }
    Ok(())
}

fn validate_topology_source_binding(receipt: &Receipt) -> Result<()> {
    let all_have_source =
        receipt.child_receipts.iter().into_iter().all(|(_, child)| child.source_sha256.is_some());
    if !all_have_source {
        return Ok(());
    }
    let topology_source = receipt
        .child_receipts
        .release_topology
        .source_sha256
        .as_deref()
        .ok_or_else(|| {
            color_eyre::eyre::eyre!("release_topology source digest is required once all child receipts carry source provenance")
        })?;
    if topology_source != receipt.candidate.release_topology_sha256 {
        bail!(
            "candidate.release_topology_sha256 must match release_topology source receipt digest"
        );
    }
    Ok(())
}

fn validate_child_artifacts(receipt: &Receipt, artifact_root: &Path) -> Result<()> {
    for (name, child) in receipt.child_receipts.iter() {
        load_verified_child_artifact(name, child, receipt, artifact_root)?;
        validate_source_receipt(name, child, receipt, artifact_root)?;
    }
    validate_topology_source_binding(receipt)?;
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
    if status != OverallStatus::Ready && !args.allow_not_proven {
        bail!("public-beta experience is {}", status.as_str());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CellDisposition, OverallStatus, Receipt, sha256_hex, validate, validate_child_artifacts,
    };
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
    fn release_topology_source_digest_binds_candidate_topology_sha() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        let digest = "a".repeat(64);
        receipt.child_receipts.user_state_presentation.source_sha256 = Some(digest.clone());
        receipt.child_receipts.first_ten_minutes.source_sha256 = Some(digest.clone());
        receipt.child_receipts.install_transition.source_sha256 = Some(digest.clone());
        receipt.child_receipts.installed_acceptance.source_sha256 = Some(digest.clone());
        receipt.child_receipts.first_useful_answer.source_sha256 = Some(digest.clone());
        receipt.child_receipts.representative_workload.source_sha256 = Some(digest.clone());
        receipt.child_receipts.release_topology.source_sha256 = Some(digest.clone());
        receipt.child_receipts.release_integrity.source_sha256 = Some(digest.clone());
        receipt.candidate.release_topology_sha256 = digest;
        super::validate_topology_source_binding(&receipt)?;
        receipt.candidate.release_topology_sha256 = "b".repeat(64);
        assert!(super::validate_topology_source_binding(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn installed_acceptance_source_uses_runtime_receipt_shape() -> Result<()> {
        let receipt = fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        let source: serde_json::Value = serde_json::from_slice(
            br#"{
                "schema_version":1,
                "outcome":"completed",
                "repository_sha":"0123456789abcdef0123456789abcdef01234567",
                "known_limitations":[],
                "claim_boundary":"Packaged VSIX and bundled-server journey.",
                "server_identity":{"source":"packaged_vsix_bundle","path":"bin/linux-x64/perl-lsp"},
                "artifact_hashes":{"bundled_server_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
                "vsix_identity":{"extension_id":"EffortlessMetrics.perl-lsp-rs","version":"0.18.0","path":"extension"}
            }"#,
        )?;
        super::validate_installed_acceptance_source(
            "installed_acceptance",
            &receipt.child_receipts.installed_acceptance,
            &receipt,
            &source,
        )?;
        Ok(())
    }

    #[test]
    fn installed_acceptance_not_proven_source_cannot_be_declared_pass() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.child_receipts.installed_acceptance.status = super::InputStatus::NotProven;
        let source: serde_json::Value = serde_json::from_slice(
            br#"{
                "schema_version":1,
                "outcome":"not_proven",
                "repository_sha":"0123456789abcdef0123456789abcdef01234567",
                "known_limitations":["DAP preview is not exercised"],
                "claim_boundary":"Packaged VSIX and bundled-server journey.",
                "server_identity":{"source":"packaged_vsix_bundle","path":"bin/linux-x64/perl-lsp"},
                "artifact_hashes":{"bundled_server_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
                "vsix_identity":{"extension_id":"EffortlessMetrics.perl-lsp-rs","version":"0.18.0","path":"extension"}
            }"#,
        )?;
        super::validate_installed_acceptance_source(
            "installed_acceptance",
            &receipt.child_receipts.installed_acceptance,
            &receipt,
            &source,
        )?;
        receipt.child_receipts.installed_acceptance.status = super::InputStatus::Pass;
        assert!(
            super::validate_installed_acceptance_source(
                "installed_acceptance",
                &receipt.child_receipts.installed_acceptance,
                &receipt,
                &source,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn installed_acceptance_missing_source_receipt_fails_closed() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.child_receipts.installed_acceptance.source_artifact_path =
            Some("sources/packaged_bundle_journey_receipt.json".to_string());
        receipt.child_receipts.installed_acceptance.source_sha256 = Some("a".repeat(64));
        let artifact_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/experience/public_beta");
        assert!(validate_child_artifacts(&receipt, &artifact_root).is_err());
        Ok(())
    }

    #[test]
    fn installed_acceptance_source_path_cannot_escape_receipt_root() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        receipt.child_receipts.installed_acceptance.source_artifact_path =
            Some("../packaged_bundle_journey_receipt.json".to_string());
        receipt.child_receipts.installed_acceptance.source_sha256 = Some("a".repeat(64));
        assert!(validate(&receipt).is_err());

        for path in [
            "C:/outside/packaged_bundle_journey_receipt.json",
            r#"C:\outside\packaged_bundle_journey_receipt.json"#,
            r#"\\server\share\packaged_bundle_journey_receipt.json"#,
            "//server/share/packaged_bundle_journey_receipt.json",
            "/tmp/packaged_bundle_journey_receipt.json",
            r#"\tmp\packaged_bundle_journey_receipt.json"#,
            r#"nested\..\packaged_bundle_journey_receipt.json"#,
        ] {
            receipt.child_receipts.installed_acceptance.source_artifact_path =
                Some(path.to_string());
            assert!(validate(&receipt).is_err(), "path should be rejected: {path}");
        }
        Ok(())
    }

    #[test]
    fn installed_acceptance_source_rejects_forged_minimal_receipt() -> Result<()> {
        let receipt = fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        let forged: serde_json::Value = serde_json::from_slice(
            br#"{"schema_version":1,"outcome":"completed","repository_sha":"0123456789abcdef0123456789abcdef01234567","known_limitations":[]}"#,
        )?;
        assert!(
            super::validate_installed_acceptance_source(
                "installed_acceptance",
                &receipt.child_receipts.installed_acceptance,
                &receipt,
                &forged,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn parent_fan_in_consumes_candidate_bound_installed_pair() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        let fixture_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/experience/public_beta");
        let directory = tempdir()?;
        for (_, child) in receipt.child_receipts.iter() {
            let source_path = fixture_root.join(&child.artifact_path);
            let target_path = directory.path().join(&child.artifact_path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(target_path, fs::read(source_path)?)?;
        }
        {
            let claim_boundary =
                "Packaged VSIX and bundled-server journey exercised by the VS Code extension host.";
            let source = serde_json::json!({
                "schema_version": 1,
                "outcome": "not_proven",
                "repository_sha": receipt.candidate.frozen_product_sha,
                "known_limitations": ["DAP preview is not exercised by this slice."],
                "claim_boundary": claim_boundary,
                "server_identity": {"source": "packaged_vsix_bundle", "path": "bin/linux-x64/perl-lsp"},
                "artifact_hashes": {"vsix_sha256": "a".repeat(64), "bundled_server_sha256": "b".repeat(64)},
                "vsix_identity": {"extension_id": "EffortlessMetrics.perl-lsp-rs", "version": "0.18.0", "path": "extension"}
            });
            let source_bytes = serde_json::to_vec_pretty(&source)?;
            let source_path = directory.path().join("sources/packaged_bundle_journey_receipt.json");
            fs::create_dir_all(
                source_path
                    .parent()
                    .ok_or_else(|| color_eyre::eyre::eyre!("missing source parent"))?,
            )?;
            fs::write(&source_path, &source_bytes)?;

            let verified = serde_json::json!({
                "owner_issue": "#4346",
                "schema_version": "verified_child_receipt.v1",
                "receipt_schema_version": "installed_acceptance.v1",
                "candidate_id": receipt.candidate.candidate_id,
                "frozen_product_sha": receipt.candidate.frozen_product_sha,
                "artifact_set_id": receipt.candidate.artifact_set_id,
                "status": "not_proven",
                "claim_boundary": claim_boundary,
                "limitation": "DAP preview is not exercised by this slice.",
                "source_receipt_sha256": sha256_hex(&source_bytes)
            });
            let verified_bytes = serde_json::to_vec_pretty(&verified)?;
            let verified_path = directory.path().join("verified/installed_acceptance.json");
            fs::create_dir_all(
                verified_path
                    .parent()
                    .ok_or_else(|| color_eyre::eyre::eyre!("missing verified parent"))?,
            )?;
            fs::write(&verified_path, &verified_bytes)?;

            let installed = &mut receipt.child_receipts.installed_acceptance;
            installed.source_artifact_path =
                Some("sources/packaged_bundle_journey_receipt.json".to_string());
            installed.source_sha256 = Some(sha256_hex(&source_bytes));
            installed.artifact_path = "verified/installed_acceptance.json".to_string();
            installed.sha256 = sha256_hex(&verified_bytes);
            installed.status = super::InputStatus::NotProven;
            installed.claim_boundary = claim_boundary.to_string();
            installed.limitation = Some("DAP preview is not exercised by this slice.".to_string());
        }
        validate_child_artifacts(&receipt, directory.path())?;
        Ok(())
    }

    #[test]
    fn installed_acceptance_verified_status_must_match_fan_in() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/public_beta/ready.json"))?;
        let fixture_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/experience/public_beta");
        let directory = tempdir()?;
        let mut installed_digest = None;
        for (name, child) in receipt.child_receipts.iter() {
            let source_path = fixture_root.join(&child.artifact_path);
            let target_path = directory.path().join(&child.artifact_path);
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut bytes = fs::read(source_path)?;
            if name == "installed_acceptance" {
                let mut artifact: serde_json::Value = serde_json::from_slice(&bytes)?;
                artifact["status"] = serde_json::Value::String("not_proven".to_string());
                bytes = serde_json::to_vec_pretty(&artifact)?;
                installed_digest = Some(super::sha256_hex(&bytes));
            }
            fs::write(target_path, bytes)?;
        }
        receipt.child_receipts.installed_acceptance.sha256 = installed_digest
            .ok_or_else(|| color_eyre::eyre::eyre!("missing installed artifact"))?;

        assert!(validate_child_artifacts(&receipt, directory.path()).is_err());
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
