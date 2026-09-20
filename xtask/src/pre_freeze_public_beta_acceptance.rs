//! Consistency checks for explicitly supplied pre-freeze v2 evidence.
//!
//! This module never authenticates receipts or qualifies installed execution.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "pre_freeze_public_beta_acceptance.v2";
pub const ROWS: [&str; 3] =
    ["linux_x64_minimum_supported", "linux_x64_current_stable", "windows_x64_current_stable"];
pub const CELLS: [&str; 24] = [
    "install_upgrade_identity",
    "startup_readiness",
    "workspace_multiroot",
    "diagnostics",
    "completion",
    "hover",
    "definition",
    "references",
    "document_symbols",
    "workspace_symbols",
    "retained_code_action",
    "result_state_distinctions",
    "unicode_crlf_edit_save_requery",
    "file_create_rename_delete",
    "safe_rename_or_refusal",
    "whole_document_formatting",
    "native_only_critic",
    "doctor_optional_tools",
    "diagnosis_and_identity_repair",
    "dap_preview",
    "retained_test_entry",
    "installed_contribution_reachability",
    "restart_recovery_rollback",
    "shutdown_cleanup",
];
const MECHANISMS: [&str; 4] = ["#5900", "#5901", "#5902", "#5903"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Limited,
    Blocked,
    NotProven,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recommendation {
    Ready,
    Blocked,
    NotProven,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstalledQualification {
    NotProven,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Proposition {
    Executed,
    SafeRefusal,
    ClaimWithdrawn,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Perllsp,
    PerlDap,
    Vsix,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    ReleaseShaped,
    WorkspaceOutput,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub candidate_id: String,
    pub repository_sha: String,
    pub topology_digest: String,
    pub artifact_set_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub id: String,
    pub role: Role,
    pub target: String,
    pub path: String,
    pub sha256: String,
    pub subject: Subject,
    pub provenance: Provenance,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub id: String,
    pub locator: String,
    pub sha256: String,
    pub subject: Subject,
    #[serde(deserialize_with = "required_nullable")]
    pub row_id: Option<String>,
    pub artifact_ids: Vec<String>,
}
fn required_nullable<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub id: String,
    pub content_sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSelection {
    pub perllsp: String,
    pub perl_dap: String,
    pub vsix: String,
}
impl ArtifactSelection {
    fn entries(&self) -> [(Role, &str); 3] {
        [(Role::Perllsp, &self.perllsp), (Role::PerlDap, &self.perl_dap), (Role::Vsix, &self.vsix)]
    }
    fn ids(&self) -> Vec<String> {
        self.entries().into_iter().map(|(_, id)| id.to_owned()).collect()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub id: String,
    pub status: Status,
    pub proposition: Proposition,
    pub evidence: Vec<EvidenceRef>,
    #[serde(deserialize_with = "required_nullable")]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JourneyRow {
    pub id: String,
    pub platform: String,
    pub architecture: String,
    pub host_role: String,
    pub vscode_version: String,
    pub host_selection: EvidenceRef,
    pub clean_profile_id: String,
    pub configuration_identity: String,
    pub fixtures: Vec<Fixture>,
    pub artifacts: ArtifactSelection,
    pub subject: Subject,
    pub cells: Vec<Cell>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub status: Status,
    pub observed_rows: Vec<String>,
    pub evidence: Vec<EvidenceRef>,
    #[serde(deserialize_with = "required_nullable")]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationRow {
    pub target: String,
    pub status: Status,
    pub artifact_ids: Vec<String>,
    pub evidence: Vec<EvidenceRef>,
    #[serde(deserialize_with = "required_nullable")]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mechanism {
    pub issue: String,
    pub status: Status,
    pub evidence: Vec<EvidenceRef>,
    #[serde(deserialize_with = "required_nullable")]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZeroBudgetCounts {
    pub wrong_binary_or_artifact: u64,
    pub partial_or_checksum_invalid_install: u64,
    pub false_exact: u64,
    pub stale_exact: u64,
    pub unsafe_edit: u64,
    pub unexplained_successful_empty: u64,
    pub mixed_generation_result: u64,
    pub cross_root_leakage: u64,
    pub orphaned_candidate_process: u64,
    pub silent_product_failure: u64,
    pub false_repair_diagnosis: u64,
    pub optional_tool_false_requirement: u64,
}
impl ZeroBudgetCounts {
    fn any_nonzero(&self) -> bool {
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
            self.false_repair_diagnosis,
            self.optional_tool_false_requirement,
        ]
        .into_iter()
        .any(|n| n != 0)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketV2 {
    pub check: String,
    pub schema_version: String,
    pub phase: String,
    pub subject: Subject,
    pub source_version: String,
    pub target_release: String,
    pub artifacts: Vec<Artifact>,
    pub rows: Vec<JourneyRow>,
    pub first_ten_minutes: Observation,
    pub preparation: Vec<PreparationRow>,
    pub mechanisms: Vec<Mechanism>,
    pub zero_budget_counts: ZeroBudgetCounts,
    pub product_blockers: Vec<String>,
    pub expected_beta_limitations: Vec<String>,
    pub friction_findings: Vec<String>,
    pub freeze_recommendation: Recommendation,
    pub claim_boundary: String,
}
/// Explicit external topology adapter input; deserialization does not authenticate it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopologyRequirements {
    pub subject: Subject,
    pub required_preparation_targets: Vec<String>,
    pub rows: Vec<RowTargets>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowTargets {
    pub row_id: String,
    pub perllsp: Vec<String>,
    pub perl_dap: Vec<String>,
    pub vsix: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterCategory {
    TopologyBinding,
    ArtifactProvenance,
    HostSelection,
    InstalledJourney,
    AcceptedClaim,
    FirstTenMinutes,
    Preparation,
    Mechanism,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceRequirement {
    pub owner_kind: String,
    pub owner_id: String,
    pub subject: Subject,
    pub row_id: Option<String>,
    pub artifact_ids: Vec<String>,
    pub locator: String,
    pub sha256: String,
    pub category: AdapterCategory,
}
#[derive(Debug, Serialize)]
pub struct ValidationReport {
    pub bundle_recommendation: Recommendation,
    pub installed_qualification: InstalledQualification,
    pub evidence_requirements: Vec<EvidenceRequirement>,
}

pub fn parse_v2(bytes: &[u8]) -> Result<PacketV2> {
    let packet: PacketV2 = serde_json::from_slice(bytes).context("decode canonical v2 packet")?;
    ensure!(packet.schema_version == SCHEMA, "v2 schema required; historical v1 cannot qualify");
    Ok(packet)
}
fn nonempty(value: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "empty identity or reason");
    Ok(())
}
fn hex(value: &str, size: usize) -> Result<()> {
    ensure!(
        value.len() == size
            && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "noncanonical hex identity"
    );
    Ok(())
}
fn subject(value: &Subject) -> Result<()> {
    nonempty(&value.candidate_id)?;
    nonempty(&value.artifact_set_id)?;
    hex(&value.repository_sha, 40)?;
    hex(value.topology_digest.strip_prefix("sha256:").context("topology sha256 prefix")?, 64)
}
fn keys<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<BTreeSet<&'a str>> {
    let mut seen = BTreeSet::new();
    for value in values {
        nonempty(value)?;
        ensure!(seen.insert(value), "duplicate key {value}");
    }
    Ok(seen)
}
fn exact<'a>(
    values: impl IntoIterator<Item = &'a str>,
    expected: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    ensure!(keys(values)? == keys(expected)?, "denominator mismatch");
    Ok(())
}
fn status(value: Status, reason: &Option<String>) -> Result<()> {
    if let Some(reason) = reason {
        nonempty(reason)?;
    }
    if value != Status::Pass {
        nonempty(reason.as_deref().context("non-pass reason required")?)?;
    }
    Ok(())
}
struct Validator<'a> {
    packet: &'a PacketV2,
    artifacts: BTreeMap<&'a str, &'a Artifact>,
    obligations: Vec<EvidenceRequirement>,
    statuses: Vec<Status>,
}
impl Validator<'_> {
    fn evidence(
        &mut self,
        refs: &[EvidenceRef],
        row: Option<&str>,
        ids: &[String],
        kind: &str,
        owner: &str,
        category: AdapterCategory,
    ) -> Result<()> {
        ensure!(!refs.is_empty(), "{kind}/{owner}: evidence missing");
        keys(refs.iter().map(|e| e.id.as_str()))?;
        for e in refs {
            nonempty(&e.locator)?;
            hex(&e.sha256, 64)?;
            ensure!(e.subject == self.packet.subject, "{kind}/{owner}: evidence subject mismatch");
            ensure!(e.row_id.as_deref() == row, "{kind}/{owner}: evidence row mismatch");
            exact(e.artifact_ids.iter().map(String::as_str), ids.iter().map(String::as_str))?;
            for id in &e.artifact_ids {
                ensure!(self.artifacts.contains_key(id.as_str()), "unknown evidence artifact {id}");
            }
            self.obligations.push(EvidenceRequirement {
                owner_kind: kind.to_owned(),
                owner_id: owner.to_owned(),
                subject: e.subject.clone(),
                row_id: e.row_id.clone(),
                artifact_ids: e.artifact_ids.clone(),
                locator: e.locator.clone(),
                sha256: e.sha256.clone(),
                category: category.clone(),
            });
        }
        Ok(())
    }
    fn row(&mut self, row: &JourneyRow, targets: &RowTargets) -> Result<()> {
        let (platform, host_role) = match row.id.as_str() {
            "linux_x64_minimum_supported" => ("linux", "minimum_supported"),
            "linux_x64_current_stable" => ("linux", "current_stable"),
            "windows_x64_current_stable" => ("windows", "current_stable"),
            _ => anyhow::bail!("unknown journey row"),
        };
        ensure!(
            row.platform == platform && row.architecture == "x64" && row.host_role == host_role,
            "row tuple mismatch"
        );
        ensure!(row.subject == self.packet.subject, "row subject mismatch");
        ensure!(
            row.vscode_version.split('.').count() == 3
                && row.vscode_version.split('.').all(|part| !part.is_empty()
                    && part.bytes().all(|b| b.is_ascii_digit())
                    && part.parse::<u64>().is_ok()),
            "exact numeric VS Code version required"
        );
        nonempty(&row.clean_profile_id)?;
        nonempty(&row.configuration_identity)?;
        ensure!(!row.fixtures.is_empty(), "row fixtures missing");
        keys(row.fixtures.iter().map(|f| f.id.as_str()))?;
        for f in &row.fixtures {
            hex(&f.content_sha256, 64)?;
        }
        for (role, id) in row.artifacts.entries() {
            let artifact = self.artifacts.get(id).context("unknown row artifact")?;
            let allowed = match role {
                Role::Perllsp => &targets.perllsp,
                Role::PerlDap => &targets.perl_dap,
                Role::Vsix => &targets.vsix,
            };
            ensure!(
                artifact.role == role && allowed.contains(&artifact.target),
                "row artifact role/target mismatch"
            );
        }
        let server =
            self.artifacts.get(row.artifacts.perllsp.as_str()).context("server missing")?;
        let dap = self.artifacts.get(row.artifacts.perl_dap.as_str()).context("DAP missing")?;
        ensure!(server.target == dap.target, "server/DAP target mismatch");
        let ids = row.artifacts.ids();
        self.evidence(
            std::slice::from_ref(&row.host_selection),
            Some(&row.id),
            &ids,
            "host",
            &row.id,
            AdapterCategory::HostSelection,
        )?;
        exact(row.cells.iter().map(|c| c.id.as_str()), CELLS)?;
        for cell in &row.cells {
            status(cell.status, &cell.reason)?;
            self.statuses.push(cell.status);
            match cell.proposition {
                Proposition::Executed => {}
                Proposition::SafeRefusal => {
                    ensure!(
                        [
                            "safe_rename_or_refusal",
                            "whole_document_formatting",
                            "retained_test_entry",
                            "dap_preview"
                        ]
                        .contains(&cell.id.as_str()),
                        "refusal cannot replace this cell"
                    );
                    nonempty(cell.reason.as_deref().context("refusal reason missing")?)?;
                }
                Proposition::ClaimWithdrawn => {
                    ensure!(
                        cell.id == "retained_test_entry",
                        "withdrawal cannot replace this cell"
                    );
                    nonempty(cell.reason.as_deref().context("withdrawal reason missing")?)?;
                }
            }
            self.evidence(
                &cell.evidence,
                Some(&row.id),
                &ids,
                "cell",
                &cell.id,
                AdapterCategory::InstalledJourney,
            )?;
            if cell.proposition != Proposition::Executed {
                self.evidence(
                    &cell.evidence,
                    Some(&row.id),
                    &ids,
                    "cell",
                    &cell.id,
                    AdapterCategory::AcceptedClaim,
                )?;
            }
        }
        Ok(())
    }
}

/// Validate only supplied-bundle consistency. Every external obligation remains explicit.
pub fn validate_v2(
    packet: &PacketV2,
    requirements: &TopologyRequirements,
) -> Result<ValidationReport> {
    ensure!(
        packet.check == "pre-freeze-public-beta-acceptance"
            && packet.schema_version == SCHEMA
            && packet.phase == "pre_freeze_product",
        "packet identity/phase mismatch"
    );
    subject(&packet.subject)?;
    ensure!(requirements.subject == packet.subject, "topology subject mismatch");
    nonempty(&packet.source_version)?;
    nonempty(&packet.claim_boundary)?;
    ensure!(packet.target_release == "0.18.0", "wrong release series");
    for text in packet
        .product_blockers
        .iter()
        .chain(&packet.expected_beta_limitations)
        .chain(&packet.friction_findings)
    {
        nonempty(text)?;
    }
    exact(packet.rows.iter().map(|r| r.id.as_str()), ROWS)?;
    exact(requirements.rows.iter().map(|r| r.row_id.as_str()), ROWS)?;
    for r in &requirements.rows {
        for targets in [&r.perllsp, &r.perl_dap, &r.vsix] {
            ensure!(
                !keys(targets.iter().map(String::as_str))?.is_empty(),
                "empty target admissibility"
            );
        }
    }
    ensure!(
        requirements.required_preparation_targets.windows(2).all(|p| matches!(p, [a, b] if a < b)),
        "preparation targets must be sorted and unique"
    );
    keys(requirements.required_preparation_targets.iter().map(String::as_str))?;
    exact(
        packet.preparation.iter().map(|p| p.target.as_str()),
        requirements.required_preparation_targets.iter().map(String::as_str),
    )?;
    ensure!(!packet.artifacts.is_empty(), "artifact inventory missing");
    keys(packet.artifacts.iter().map(|a| a.id.as_str()))?;
    let mut validator = Validator {
        packet,
        artifacts: BTreeMap::new(),
        obligations: Vec::new(),
        statuses: Vec::new(),
    };
    let all_ids: Vec<String> = packet.artifacts.iter().map(|a| a.id.clone()).collect();
    for artifact in &packet.artifacts {
        nonempty(&artifact.path)?;
        nonempty(&artifact.target)?;
        hex(&artifact.sha256, 64)?;
        ensure!(
            artifact.subject == packet.subject && artifact.provenance == Provenance::ReleaseShaped,
            "artifact subject/provenance mismatch"
        );
        validator.artifacts.insert(&artifact.id, artifact);
        validator.obligations.push(EvidenceRequirement {
            owner_kind: "artifact".into(),
            owner_id: artifact.id.clone(),
            subject: packet.subject.clone(),
            row_id: None,
            artifact_ids: vec![artifact.id.clone()],
            locator: artifact.path.clone(),
            sha256: artifact.sha256.clone(),
            category: AdapterCategory::ArtifactProvenance,
        });
    }
    validator.obligations.push(EvidenceRequirement {
        owner_kind: "topology".into(),
        owner_id: packet.subject.topology_digest.clone(),
        subject: packet.subject.clone(),
        row_id: None,
        artifact_ids: all_ids.clone(),
        locator: packet.subject.topology_digest.clone(),
        sha256: packet
            .subject
            .topology_digest
            .strip_prefix("sha256:")
            .context("topology prefix")?
            .into(),
        category: AdapterCategory::TopologyBinding,
    });
    for row in &packet.rows {
        let targets =
            requirements.rows.iter().find(|r| r.row_id == row.id).context("row targets missing")?;
        validator.row(row, targets).with_context(|| format!("row {}", row.id))?;
    }
    let floor = packet
        .rows
        .iter()
        .find(|r| r.id == "linux_x64_minimum_supported")
        .context("floor missing")?;
    let stable = packet
        .rows
        .iter()
        .find(|r| r.id == "linux_x64_current_stable")
        .context("stable missing")?;
    let windows = packet
        .rows
        .iter()
        .find(|r| r.id == "windows_x64_current_stable")
        .context("Windows missing")?;
    ensure!(
        floor.artifacts == stable.artifacts,
        "Linux host rows must use identical candidate artifacts"
    );
    for (linux_role, linux_id) in
        floor.artifacts.entries().into_iter().filter(|(role, _)| *role != Role::Vsix)
    {
        let linux = validator.artifacts.get(linux_id).context("Linux artifact missing")?;
        for (windows_role, windows_id) in
            windows.artifacts.entries().into_iter().filter(|(role, _)| *role != Role::Vsix)
        {
            let other = validator.artifacts.get(windows_id).context("Windows artifact missing")?;
            ensure!(
                linux_id != windows_id
                    && linux.sha256 != other.sha256
                    && linux.target != other.target,
                "native platform artifact identity reused ({linux_role:?}/{windows_role:?})"
            );
        }
    }
    let observation = &packet.first_ten_minutes;
    status(observation.status, &observation.reason)?;
    validator.statuses.push(observation.status);
    let observed = keys(observation.observed_rows.iter().map(String::as_str))?;
    ensure!(
        !observed.is_empty() && observed.iter().all(|id| ROWS.contains(id)),
        "observation rows invalid"
    );
    keys(observation.evidence.iter().map(|e| e.id.as_str()))?;
    ensure!(!observation.evidence.is_empty(), "observation evidence missing");
    let mut covered = BTreeSet::new();
    for evidence in &observation.evidence {
        let id = evidence.row_id.as_deref().context("observation row missing")?;
        ensure!(observed.contains(id), "observation evidence outside observed rows");
        let row = packet.rows.iter().find(|r| r.id == id).context("observation row missing")?;
        validator.evidence(
            std::slice::from_ref(evidence),
            Some(id),
            &row.artifacts.ids(),
            "observation",
            "first_ten_minutes",
            AdapterCategory::FirstTenMinutes,
        )?;
        covered.insert(id);
    }
    ensure!(covered == observed, "observed row lacks evidence");
    for prep in &packet.preparation {
        status(prep.status, &prep.reason)?;
        validator.statuses.push(prep.status);
        ensure!(!prep.artifact_ids.is_empty(), "preparation artifacts missing");
        let declared = keys(prep.artifact_ids.iter().map(String::as_str))?;
        let applicable = keys(
            packet
                .artifacts
                .iter()
                .filter(|artifact| artifact.target == prep.target)
                .map(|artifact| artifact.id.as_str()),
        )?;
        ensure!(declared == applicable, "preparation artifact denominator mismatch");
        for id in &prep.artifact_ids {
            ensure!(
                validator
                    .artifacts
                    .get(id.as_str())
                    .context("unknown preparation artifact")?
                    .target
                    == prep.target,
                "preparation target mismatch"
            );
        }
        validator.evidence(
            &prep.evidence,
            None,
            &prep.artifact_ids,
            "preparation",
            &prep.target,
            AdapterCategory::Preparation,
        )?;
    }
    exact(packet.mechanisms.iter().map(|m| m.issue.as_str()), MECHANISMS)?;
    for mechanism in &packet.mechanisms {
        status(mechanism.status, &mechanism.reason)?;
        validator.statuses.push(mechanism.status);
        validator.evidence(
            &mechanism.evidence,
            None,
            &all_ids,
            "mechanism",
            &mechanism.issue,
            AdapterCategory::Mechanism,
        )?;
    }
    let recommendation = if !packet.product_blockers.is_empty()
        || packet.zero_budget_counts.any_nonzero()
        || validator.statuses.contains(&Status::Blocked)
    {
        Recommendation::Blocked
    } else if validator.statuses.iter().any(|s| *s != Status::Pass) {
        Recommendation::NotProven
    } else {
        Recommendation::Ready
    };
    ensure!(
        packet.freeze_recommendation == recommendation,
        "declared recommendation disagrees with supplied bundle"
    );
    // Stable report independent of collection input order; retain obligations with different owners/categories.
    for obligation in &mut validator.obligations {
        obligation.artifact_ids.sort();
    }
    validator.obligations.sort_by_cached_key(|o| {
        format!(
            "{}:{}:{:?}:{}:{}:{:?}",
            o.owner_kind, o.owner_id, o.row_id, o.locator, o.sha256, o.category
        )
    });
    validator.obligations.dedup();
    Ok(ValidationReport {
        bundle_recommendation: recommendation,
        installed_qualification: InstalledQualification::NotProven,
        evidence_requirements: validator.obligations,
    })
}
#[cfg(test)]
mod tests;
