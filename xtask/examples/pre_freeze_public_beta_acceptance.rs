//! Validate the machine-readable pre-freeze public-beta acceptance packet.
//!
//! This packet is the execution handoff for #6056. It composes candidate-bound
//! artifact, platform, journey, and mechanism evidence without claiming that
//! the packet itself proves the underlying runtime observations.

#![expect(
    clippy::print_stdout,
    reason = "packet validator emits one concise machine-readable result"
)]

use clap::Parser;
use color_eyre::eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

const CHECK: &str = "pre-freeze-public-beta-acceptance";
const SCHEMA_VERSION: &str = "pre_freeze_public_beta_acceptance.v1";
const REQUIRED_MECHANISMS: [&str; 4] = ["#5900", "#5901", "#5902", "#5903"];
const REQUIRED_JOURNEY_CELLS: [&str; 11] = [
    "install_upgrade",
    "startup",
    "workspace",
    "completion_hover_navigation",
    "diagnostics",
    "empty_results",
    "rename_delete",
    "formatting",
    "doctor_trust",
    "dap_preview",
    "shutdown",
];

#[derive(Debug, Parser)]
#[command(name = "pre-freeze-public-beta-acceptance")]
#[command(about = "Validate one pre-freeze public-beta acceptance packet")]
struct Args {
    /// Machine-readable packet JSON to validate.
    #[arg(long)]
    packet: PathBuf,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum EvidenceStatus {
    Pass,
    Limited,
    Blocked,
    NotProven,
}

impl EvidenceStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Limited => "limited",
            Self::Blocked => "blocked",
            Self::NotProven => "not_proven",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FreezeRecommendation {
    Ready,
    Blocked,
    NotProven,
}

impl FreezeRecommendation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::NotProven => "not_proven",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactEvidence {
    path: String,
    sha256: String,
    repository_sha: String,
    artifact_set_id: String,
    provenance: ArtifactProvenance,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ArtifactProvenance {
    ReleaseShaped,
    WorkspaceOutput,
    Unknown,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Artifacts {
    perllsp: ArtifactEvidence,
    perl_dap: ArtifactEvidence,
    vsix: ArtifactEvidence,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EvidenceProvenance {
    candidate_id: String,
    repository_sha: String,
    artifact_set_id: String,
    topology_digest: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlatformEvidence {
    status: EvidenceStatus,
    evidence_refs: Vec<String>,
    provenance: EvidenceProvenance,
    claim_boundary: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Platforms {
    linux: PlatformEvidence,
    macos: PlatformEvidence,
    windows: PlatformEvidence,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JourneyCell {
    id: String,
    status: EvidenceStatus,
    evidence_refs: Vec<String>,
    provenance: EvidenceProvenance,
    limitation: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ZeroBudgetCounts {
    wrong_binary_or_artifact: u64,
    partial_or_checksum_invalid_install: u64,
    false_exact: u64,
    stale_exact: u64,
    unsafe_edit: u64,
    unexplained_successful_empty: u64,
    mixed_generation_result: u64,
    cross_root_leakage: u64,
    orphaned_candidate_process: u64,
    silent_product_failure: u64,
}

impl ZeroBudgetCounts {
    fn total(&self) -> u64 {
        [
            self.wrong_binary_or_artifact,
            self.partial_or_checksum_invalid_install,
            self.false_exact,
            self.stale_exact,
            self.unsafe_edit,
            self.unexplained_successful_empty,
            self.mixed_generation_result,
            self.cross_root_leakage,
            self.orphaned_candidate_process,
            self.silent_product_failure,
        ]
        .into_iter()
        .try_fold(0u64, |acc, value| acc.checked_add(value))
        .unwrap_or(u64::MAX)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MechanismDisposition {
    issue: String,
    status: EvidenceStatus,
    candidate_id: String,
    claim_boundary: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    check: String,
    schema_version: String,
    repository_sha: String,
    candidate_id: String,
    source_version: String,
    target_release: String,
    topology_digest: String,
    artifact_set_id: String,
    artifacts: Artifacts,
    platforms: Platforms,
    journey_cells: Vec<JourneyCell>,
    zero_budget_counts: ZeroBudgetCounts,
    product_blockers: Vec<String>,
    expected_beta_limitations: Vec<String>,
    friction_findings: Vec<String>,
    mechanism_dispositions: Vec<MechanismDisposition>,
    freeze_recommendation: FreezeRecommendation,
    claim_boundary: String,
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

fn validate_artifact(name: &str, artifact: &ArtifactEvidence, packet: &Packet) -> Result<()> {
    non_empty(&artifact.path, &format!("artifacts.{name}.path"))?;
    exact_hex(&artifact.sha256, 32, &format!("artifacts.{name}.sha256"))?;
    if artifact.repository_sha != packet.repository_sha {
        bail!("artifacts.{name} belongs to a different repository SHA");
    }
    if artifact.artifact_set_id != packet.artifact_set_id {
        bail!("artifacts.{name} belongs to a different artifact set");
    }
    if artifact.provenance != ArtifactProvenance::ReleaseShaped {
        bail!("artifacts.{name} is not declared as release-shaped provenance");
    }
    Ok(())
}

fn validate_provenance(
    field: &str,
    provenance: &EvidenceProvenance,
    packet: &Packet,
) -> Result<()> {
    if provenance.candidate_id != packet.candidate_id {
        bail!("{field}.provenance belongs to a different candidate");
    }
    if provenance.repository_sha != packet.repository_sha {
        bail!("{field}.provenance belongs to a different repository SHA");
    }
    if provenance.artifact_set_id != packet.artifact_set_id {
        bail!("{field}.provenance belongs to a different artifact set");
    }
    if provenance.topology_digest != packet.topology_digest {
        bail!("{field}.provenance belongs to a different topology");
    }
    Ok(())
}

fn validate_platform(name: &str, platform: &PlatformEvidence, packet: &Packet) -> Result<()> {
    if platform.evidence_refs.is_empty() {
        bail!("platforms.{name}.evidence_refs must not be empty");
    }
    for evidence in &platform.evidence_refs {
        non_empty(evidence, &format!("platforms.{name}.evidence_refs[]"))?;
    }
    validate_provenance(&format!("platforms.{name}"), &platform.provenance, packet)?;
    non_empty(&platform.claim_boundary, &format!("platforms.{name}.claim_boundary"))?;
    if platform.status == EvidenceStatus::Pass && name != "linux" {
        // A pass is allowed for release-preparation smoke on these platforms;
        // semantic acceptance is still computed from Linux below.
    }
    Ok(())
}

fn validate_journey_cells(cells: &[JourneyCell], packet: &Packet) -> Result<()> {
    let mut observed = BTreeSet::new();
    for cell in cells {
        if !observed.insert(cell.id.as_str()) {
            bail!("duplicate journey cell: {}", cell.id);
        }
        if !REQUIRED_JOURNEY_CELLS.contains(&cell.id.as_str()) {
            bail!("unknown journey cell: {}", cell.id);
        }
        validate_provenance(&format!("journey_cells.{}", cell.id), &cell.provenance, packet)?;
        if cell.evidence_refs.is_empty() {
            bail!("journey cell {} has no evidence references", cell.id);
        }
        for evidence in &cell.evidence_refs {
            non_empty(evidence, &format!("journey_cells.{}.evidence_refs[]", cell.id))?;
        }
        match cell.status {
            EvidenceStatus::Limited | EvidenceStatus::NotProven => {
                non_empty(
                    cell.limitation.as_deref().unwrap_or_default(),
                    &format!("journey_cells.{}.limitation", cell.id),
                )?;
            }
            EvidenceStatus::Pass | EvidenceStatus::Blocked => {}
        }
    }
    let required: BTreeSet<&str> = REQUIRED_JOURNEY_CELLS.into_iter().collect();
    if observed != required {
        bail!("journey_cells must contain each required cell exactly once");
    }
    Ok(())
}

fn validate_mechanisms(dispositions: &[MechanismDisposition], packet: &Packet) -> Result<()> {
    let mut observed = BTreeSet::new();
    for disposition in dispositions {
        if !observed.insert(disposition.issue.as_str()) {
            bail!("duplicate mechanism disposition: {}", disposition.issue);
        }
        if !REQUIRED_MECHANISMS.contains(&disposition.issue.as_str()) {
            bail!("unknown mechanism disposition: {}", disposition.issue);
        }
        if disposition.candidate_id != packet.candidate_id {
            bail!(
                "mechanism {} candidate identity does not match packet candidate",
                disposition.issue
            );
        }
        non_empty(
            &disposition.claim_boundary,
            &format!("mechanism_dispositions.{}.claim_boundary", disposition.issue),
        )?;
    }
    let required: BTreeSet<&str> = REQUIRED_MECHANISMS.into_iter().collect();
    if observed != required {
        bail!("mechanism_dispositions must cover #5900 through #5903 exactly once");
    }
    Ok(())
}

fn computed_recommendation(packet: &Packet) -> FreezeRecommendation {
    let blocked = !packet.product_blockers.is_empty()
        || packet.zero_budget_counts.total() > 0
        || packet.platforms.linux.status == EvidenceStatus::Blocked
        || packet.platforms.macos.status == EvidenceStatus::Blocked
        || packet.platforms.windows.status == EvidenceStatus::Blocked
        || packet.journey_cells.iter().any(|cell| cell.status == EvidenceStatus::Blocked)
        || packet.mechanism_dispositions.iter().any(|item| item.status == EvidenceStatus::Blocked);
    if blocked {
        return FreezeRecommendation::Blocked;
    }

    let not_proven = packet.platforms.linux.status == EvidenceStatus::NotProven
        || packet.platforms.macos.status == EvidenceStatus::NotProven
        || packet.platforms.windows.status == EvidenceStatus::NotProven
        || packet.journey_cells.iter().any(|cell| cell.status == EvidenceStatus::NotProven)
        || packet
            .mechanism_dispositions
            .iter()
            .any(|item| item.status == EvidenceStatus::NotProven);
    if not_proven || packet.platforms.linux.status != EvidenceStatus::Pass {
        return FreezeRecommendation::NotProven;
    }

    FreezeRecommendation::Ready
}

fn validate(packet: &Packet) -> Result<FreezeRecommendation> {
    if packet.check != CHECK {
        bail!("check must be {CHECK}");
    }
    if packet.schema_version != SCHEMA_VERSION {
        bail!("schema_version must be {SCHEMA_VERSION}");
    }
    exact_hex(&packet.repository_sha, 20, "repository_sha")?;
    if !packet.topology_digest.starts_with("sha256:") {
        bail!("topology_digest must use sha256:<64 hex characters>");
    }
    exact_hex(&packet.topology_digest[7..], 32, "topology_digest")?;
    for (field, value) in [
        ("source_version", packet.source_version.as_str()),
        ("target_release", packet.target_release.as_str()),
        ("candidate_id", packet.candidate_id.as_str()),
        ("artifact_set_id", packet.artifact_set_id.as_str()),
        ("claim_boundary", packet.claim_boundary.as_str()),
    ] {
        non_empty(value, field)?;
    }
    validate_artifact("perllsp", &packet.artifacts.perllsp, packet)?;
    validate_artifact("perl_dap", &packet.artifacts.perl_dap, packet)?;
    validate_artifact("vsix", &packet.artifacts.vsix, packet)?;
    validate_platform("linux", &packet.platforms.linux, packet)?;
    validate_platform("macos", &packet.platforms.macos, packet)?;
    validate_platform("windows", &packet.platforms.windows, packet)?;
    validate_journey_cells(&packet.journey_cells, packet)?;
    validate_mechanisms(&packet.mechanism_dispositions, packet)?;
    for (field, values) in [
        ("product_blockers", &packet.product_blockers),
        ("expected_beta_limitations", &packet.expected_beta_limitations),
        ("friction_findings", &packet.friction_findings),
    ] {
        for value in values {
            non_empty(value, &format!("{field}[]"))?;
        }
    }
    let computed = computed_recommendation(packet);
    if packet.freeze_recommendation != computed {
        bail!(
            "freeze_recommendation {} disagrees with computed recommendation {}",
            packet.freeze_recommendation.as_str(),
            computed.as_str()
        );
    }
    Ok(computed)
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let packet: Packet = serde_json::from_str(&fs::read_to_string(args.packet)?)?;
    let recommendation = validate(&packet)?;
    println!(
        "pre-freeze-public-beta-acceptance: sha={} artifact_set={} recommendation={}",
        packet.repository_sha,
        packet.artifact_set_id,
        recommendation.as_str()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactEvidence, ArtifactProvenance, Artifacts, EvidenceProvenance, EvidenceStatus,
        FreezeRecommendation, JourneyCell, MechanismDisposition, Packet, PlatformEvidence,
        Platforms, ZeroBudgetCounts, computed_recommendation, validate,
    };
    use color_eyre::eyre::Result;

    fn ready_packet() -> Packet {
        let artifact = |name: &str| ArtifactEvidence {
            path: format!("isolated/{name}"),
            sha256: "a".repeat(64),
            repository_sha: "0123456789abcdef0123456789abcdef01234567".to_string(),
            artifact_set_id: "candidate-v0.18.0".to_string(),
            provenance: ArtifactProvenance::ReleaseShaped,
        };
        let provenance = || EvidenceProvenance {
            candidate_id: "candidate-v0.18.0".to_string(),
            repository_sha: "0123456789abcdef0123456789abcdef01234567".to_string(),
            artifact_set_id: "candidate-v0.18.0".to_string(),
            topology_digest: format!("sha256:{}", "b".repeat(64)),
        };
        let platform = |status| PlatformEvidence {
            status,
            evidence_refs: vec!["receipt/candidate-v0.18.0".to_string()],
            provenance: provenance(),
            claim_boundary: "bounded platform evidence".to_string(),
        };
        let mechanisms = ["#5900", "#5901", "#5902", "#5903"]
            .into_iter()
            .map(|issue| MechanismDisposition {
                issue: issue.to_string(),
                status: EvidenceStatus::Pass,
                candidate_id: "candidate-v0.18.0".to_string(),
                claim_boundary: "bounded mechanism receipt".to_string(),
            })
            .collect();
        let journey_cells = super::REQUIRED_JOURNEY_CELLS
            .into_iter()
            .map(|id| JourneyCell {
                id: id.to_string(),
                status: EvidenceStatus::Pass,
                evidence_refs: vec![format!("journey/{id}/candidate-v0.18.0")],
                provenance: provenance(),
                limitation: None,
            })
            .collect();
        Packet {
            check: super::CHECK.to_string(),
            schema_version: super::SCHEMA_VERSION.to_string(),
            repository_sha: "0123456789abcdef0123456789abcdef01234567".to_string(),
            candidate_id: "candidate-v0.18.0".to_string(),
            source_version: "0.18.0".to_string(),
            target_release: "0.18.0".to_string(),
            topology_digest: format!("sha256:{}", "b".repeat(64)),
            artifact_set_id: "candidate-v0.18.0".to_string(),
            artifacts: Artifacts {
                perllsp: artifact("perllsp"),
                perl_dap: artifact("perl-dap"),
                vsix: artifact("perl-lsp.vsix"),
            },
            platforms: Platforms {
                linux: platform(EvidenceStatus::Pass),
                macos: platform(EvidenceStatus::Limited),
                windows: platform(EvidenceStatus::Limited),
            },
            journey_cells,
            zero_budget_counts: ZeroBudgetCounts::default(),
            product_blockers: Vec::new(),
            expected_beta_limitations: vec!["DAP remains preview-scoped".to_string()],
            friction_findings: Vec::new(),
            mechanism_dispositions: mechanisms,
            freeze_recommendation: FreezeRecommendation::Ready,
            claim_boundary: "Pre-freeze evidence only; no publication authority".to_string(),
        }
    }

    #[test]
    fn complete_linux_packet_can_recommend_ready() -> Result<()> {
        let packet = ready_packet();
        assert_eq!(validate(&packet)?, FreezeRecommendation::Ready);
        Ok(())
    }

    #[test]
    fn missing_journey_cell_fails_closed() {
        let mut packet = ready_packet();
        packet.journey_cells.pop();
        assert!(validate(&packet).is_err());
    }

    #[test]
    fn platform_provenance_from_another_candidate_fails_closed() {
        let mut packet = ready_packet();
        packet.platforms.linux.provenance.candidate_id = "other-candidate".to_string();
        assert!(validate(&packet).is_err());
    }

    #[test]
    fn journey_provenance_from_another_topology_fails_closed() {
        let mut packet = ready_packet();
        packet.journey_cells[0].provenance.topology_digest = format!("sha256:{}", "c".repeat(64));
        assert!(validate(&packet).is_err());
    }

    #[test]
    fn workspace_artifact_provenance_cannot_pass() {
        let mut packet = ready_packet();
        packet.artifacts.perllsp.provenance = ArtifactProvenance::WorkspaceOutput;
        assert!(validate(&packet).is_err());
    }

    #[test]
    fn artifact_from_another_repository_sha_fails_closed() {
        let mut packet = ready_packet();
        packet.artifacts.perllsp.repository_sha =
            "fedcba9876543210fedcba9876543210fedcba98".to_string();
        assert!(validate(&packet).is_err());
    }

    #[test]
    fn unresolved_product_blocker_cannot_claim_ready() -> Result<()> {
        let mut packet = ready_packet();
        packet.product_blockers = vec!["exact installed binary is missing".to_string()];
        packet.freeze_recommendation = FreezeRecommendation::Blocked;
        assert_eq!(validate(&packet)?, FreezeRecommendation::Blocked);
        Ok(())
    }

    #[test]
    fn nonzero_zero_budget_count_requires_blocked_recommendation() {
        let mut packet = ready_packet();
        packet.zero_budget_counts.false_exact = 1;
        packet.freeze_recommendation = FreezeRecommendation::Blocked;
        assert_eq!(computed_recommendation(&packet), FreezeRecommendation::Blocked);
        assert!(validate(&packet).is_ok());
    }

    #[test]
    fn not_proven_linux_cannot_be_ready() -> Result<()> {
        let mut packet = ready_packet();
        packet.platforms.linux.status = EvidenceStatus::NotProven;
        packet.freeze_recommendation = FreezeRecommendation::NotProven;
        assert_eq!(validate(&packet)?, FreezeRecommendation::NotProven);
        Ok(())
    }

    #[test]
    fn incomplete_mechanism_disposition_fails_closed() {
        let mut packet = ready_packet();
        packet.mechanism_dispositions.pop();
        assert!(validate(&packet).is_err());
    }
}
