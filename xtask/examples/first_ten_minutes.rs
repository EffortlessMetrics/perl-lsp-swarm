//! Validate one packaged first-ten-minutes experience receipt.
//!
//! This instrument validates observation identity, the finite four-stage user
//! journey, explicit trust-breaker counts, friction classification, and the
//! declared pass/blocked/not-proven disposition. It does not launch VS Code,
//! observe a user, repair findings, or authorize a release.

#![allow(clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

const CHECK: &str = "first-ten-minutes";
const SCHEMA_VERSION: &str = "first_ten_minutes.v1";
const REQUIRED_STEPS: [JourneyStepId; 4] = [
    JourneyStepId::InstallStartup,
    JourneyStepId::UnderstandProject,
    JourneyStepId::ChangeProject,
    JourneyStepId::DiagnoseRecover,
];

#[derive(Debug, Parser)]
#[command(name = "first-ten-minutes")]
#[command(about = "Validate a packaged first-ten-minutes experience receipt")]
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
enum StudyPass {
    PreFreezeReleaseShaped,
    ExactCandidateConfirmation,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ProjectFamily {
    ConventionalModules,
    TestHeavy,
    FrameworkShaped,
    EnvironmentSensitive,
    DynamicBoundaryControl,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum JourneyStepId {
    InstallStartup,
    UnderstandProject,
    ChangeProject,
    DiagnoseRecover,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum StepStatus {
    Completed,
    Limited,
    Failed,
    NotProven,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FrictionClass {
    Broken,
    TrustBreaker,
    Actionability,
    Discoverability,
    Noise,
    LatencyOrReadiness,
    Consistency,
    Polish,
    ExpectedBetaBoundary,
    NotProven,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CandidateIdentity {
    repository_sha: String,
    artifact_set_id: String,
    vsix_version: String,
    vsix_sha256: String,
    perllsp_version: String,
    perllsp_sha256: String,
    perl_dap_version: String,
    perl_dap_sha256: String,
    vscode_version: String,
    platform: String,
    clean_profile_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectIdentity {
    fixture_id: String,
    content_sha256: String,
    family: ProjectFamily,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JourneyStep {
    id: JourneyStepId,
    status: StepStatus,
    evidence_ref: String,
    limitations: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ObservationCounts {
    false_exact: u64,
    stale_exact: u64,
    unsafe_edit: u64,
    unexplained_empty: u64,
    silent_startup_failure: u64,
    broken_documented_install: u64,
    wrong_binary_or_version: u64,
    orphaned_server_or_debuggee: u64,
    notifications: u64,
    interventions: u64,
}

impl ObservationCounts {
    fn trust_breaker_total(&self) -> u64 {
        self.false_exact
            + self.stale_exact
            + self.unsafe_edit
            + self.unexplained_empty
            + self.silent_startup_failure
            + self.broken_documented_install
            + self.wrong_binary_or_version
            + self.orphaned_server_or_debuggee
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FrictionFinding {
    id: String,
    class: FrictionClass,
    summary: String,
    evidence_ref: String,
    linked_issue: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    check: String,
    schema_version: String,
    status: ReceiptStatus,
    claim_boundary: String,
    study_pass: StudyPass,
    candidate: CandidateIdentity,
    project: ProjectIdentity,
    steps: Vec<JourneyStep>,
    first_useful_ms: Option<u64>,
    first_correct_ms: Option<u64>,
    counts: ObservationCounts,
    findings: Vec<FrictionFinding>,
    expected_beta_boundaries: Vec<String>,
    linked_issues: Vec<String>,
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

fn validate_identity(receipt: &Receipt) -> Result<()> {
    exact_hex(&receipt.candidate.repository_sha, 20, "candidate.repository_sha")?;
    exact_hex(&receipt.candidate.vsix_sha256, 32, "candidate.vsix_sha256")?;
    exact_hex(
        &receipt.candidate.perllsp_sha256,
        32,
        "candidate.perllsp_sha256",
    )?;
    exact_hex(
        &receipt.candidate.perl_dap_sha256,
        32,
        "candidate.perl_dap_sha256",
    )?;
    exact_hex(&receipt.project.content_sha256, 32, "project.content_sha256")?;

    for (field, value) in [
        ("candidate.artifact_set_id", receipt.candidate.artifact_set_id.as_str()),
        ("candidate.vsix_version", receipt.candidate.vsix_version.as_str()),
        ("candidate.perllsp_version", receipt.candidate.perllsp_version.as_str()),
        ("candidate.perl_dap_version", receipt.candidate.perl_dap_version.as_str()),
        ("candidate.vscode_version", receipt.candidate.vscode_version.as_str()),
        ("candidate.platform", receipt.candidate.platform.as_str()),
        ("candidate.clean_profile_id", receipt.candidate.clean_profile_id.as_str()),
        ("project.fixture_id", receipt.project.fixture_id.as_str()),
    ] {
        non_empty(value, field)?;
    }
    Ok(())
}

fn validate_steps(receipt: &Receipt) -> Result<()> {
    let mut observed = BTreeSet::new();
    for step in &receipt.steps {
        if !observed.insert(step.id) {
            bail!("duplicate journey step: {:?}", step.id);
        }
        non_empty(&step.evidence_ref, "steps[].evidence_ref")?;
        for limitation in &step.limitations {
            non_empty(limitation, "steps[].limitations[]")?;
        }
    }

    let required = BTreeSet::from(REQUIRED_STEPS);
    if observed != required {
        bail!("journey steps must contain each required step exactly once");
    }
    Ok(())
}

fn validate_findings(receipt: &Receipt) -> Result<()> {
    let mut ids = BTreeSet::new();
    for finding in &receipt.findings {
        if !ids.insert(finding.id.as_str()) {
            bail!("duplicate finding id: {}", finding.id);
        }
        non_empty(&finding.id, "findings[].id")?;
        non_empty(&finding.summary, "findings[].summary")?;
        non_empty(&finding.evidence_ref, "findings[].evidence_ref")?;
        if let Some(issue) = &finding.linked_issue {
            if !issue.starts_with('#') || issue.len() < 2 {
                bail!("findings[].linked_issue must use #<number> identity");
            }
        }
    }
    Ok(())
}

fn computed_status(receipt: &Receipt) -> ReceiptStatus {
    let blocked_step = receipt
        .steps
        .iter()
        .any(|step| step.status == StepStatus::Failed);
    let blocked_finding = receipt.findings.iter().any(|finding| {
        matches!(finding.class, FrictionClass::Broken | FrictionClass::TrustBreaker)
    });
    if receipt.counts.trust_breaker_total() > 0 || blocked_step || blocked_finding {
        return ReceiptStatus::Blocked;
    }

    let unproven_step = receipt
        .steps
        .iter()
        .any(|step| step.status == StepStatus::NotProven);
    let unproven_finding = receipt
        .findings
        .iter()
        .any(|finding| finding.class == FrictionClass::NotProven);
    if unproven_step || unproven_finding {
        return ReceiptStatus::NotProven;
    }

    ReceiptStatus::Pass
}

fn validate(receipt: &Receipt) -> Result<ReceiptStatus> {
    if receipt.check != CHECK {
        bail!("check must be {CHECK}");
    }
    if receipt.schema_version != SCHEMA_VERSION {
        bail!("schema_version must be {SCHEMA_VERSION}");
    }
    non_empty(&receipt.claim_boundary, "claim_boundary")?;
    validate_identity(receipt)?;
    validate_steps(receipt)?;
    validate_findings(receipt)?;

    if let (Some(first_useful), Some(first_correct)) =
        (receipt.first_useful_ms, receipt.first_correct_ms)
        && first_correct < first_useful
    {
        bail!("first_correct_ms cannot precede first_useful_ms");
    }

    for value in receipt
        .expected_beta_boundaries
        .iter()
        .chain(receipt.linked_issues.iter())
        .chain(receipt.limitations.iter())
    {
        non_empty(value, "receipt list value")?;
    }

    let computed = computed_status(receipt);
    if receipt.status != computed {
        bail!(
            "declared status {} disagrees with computed status {}",
            receipt.status.as_str(),
            computed.as_str()
        );
    }
    if computed == ReceiptStatus::Pass
        && receipt
            .steps
            .iter()
            .any(|step| step.status != StepStatus::Completed && step.status != StepStatus::Limited)
    {
        bail!("a passing receipt may contain only completed or explicitly limited steps");
    }
    if computed == ReceiptStatus::Pass
        && (receipt.first_useful_ms.is_none() || receipt.first_correct_ms.is_none())
    {
        bail!("a passing receipt must record first useful and first correct timings");
    }

    Ok(computed)
}

fn load(path: &PathBuf) -> Result<Receipt> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let receipt = load(&args.receipt)?;
    let status = validate(&receipt)?;
    println!(
        "first-ten-minutes: status={} pass={:?} project={} trust_breakers={} findings={}",
        status.as_str(),
        receipt.study_pass,
        receipt.project.fixture_id,
        receipt.counts.trust_breaker_total(),
        receipt.findings.len()
    );
    if status != ReceiptStatus::Pass {
        bail!("first-ten-minutes receipt is {}", status.as_str());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Receipt, ReceiptStatus, validate};
    use color_eyre::eyre::Result;

    fn fixture(path: &str) -> Result<Receipt> {
        Ok(serde_json::from_str(path)?)
    }

    #[test]
    fn valid_fixture_passes() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/first_ten_minutes/valid.json"
        ))?;
        assert_eq!(validate(&receipt)?, ReceiptStatus::Pass);
        Ok(())
    }

    #[test]
    fn trust_breaker_fixture_is_blocked() -> Result<()> {
        let receipt = fixture(include_str!(
            "../../fixtures/experience/first_ten_minutes/trust_breaker.json"
        ))?;
        assert_eq!(validate(&receipt)?, ReceiptStatus::Blocked);
        Ok(())
    }

    #[test]
    fn a_missing_required_step_fails_closed() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/first_ten_minutes/valid.json"
        ))?;
        receipt.steps.pop();
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn a_false_green_status_is_rejected() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/first_ten_minutes/valid.json"
        ))?;
        receipt.counts.stale_exact = 1;
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn first_correct_cannot_precede_first_useful() -> Result<()> {
        let mut receipt = fixture(include_str!(
            "../../fixtures/experience/first_ten_minutes/valid.json"
        ))?;
        receipt.first_useful_ms = Some(500);
        receipt.first_correct_ms = Some(400);
        assert!(validate(&receipt).is_err());
        Ok(())
    }
}
