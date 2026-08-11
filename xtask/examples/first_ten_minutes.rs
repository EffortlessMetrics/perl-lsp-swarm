//! Validate one packaged first-ten-minutes experience receipt.
//!
//! This instrument validates observation identity, the finite four-stage user
//! journey, explicit trust-breaker counts, friction classification, and the
//! declared pass/blocked/not-proven disposition. It does not launch VS Code,
//! observe a user, repair findings, or authorize a release.

#![allow(clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const CHECK: &str = "first-ten-minutes";
const SCHEMA_VERSION: &str = "first_ten_minutes.v1";
const VERIFIED_CHILD_SCHEMA_VERSION: &str = "verified_child_receipt.v1";
const OWNER_ISSUE: &str = "#5902";
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

    /// Optional verified-child envelope output consumed by the public-beta fan-in.
    #[arg(long)]
    verified_output: Option<PathBuf>,
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
    candidate_id: String,
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VerifiedChildArtifact<'a> {
    owner_issue: &'static str,
    schema_version: &'static str,
    receipt_schema_version: &'static str,
    candidate_id: &'a str,
    frozen_product_sha: &'a str,
    artifact_set_id: &'a str,
    source_receipt_sha256: &'a str,
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

fn validate_raw_shape(raw: &Value) -> Result<()> {
    let object =
        raw.as_object().ok_or_else(|| color_eyre::eyre::eyre!("receipt must be a JSON object"))?;

    for field in [
        "check",
        "schema_version",
        "status",
        "claim_boundary",
        "study_pass",
        "candidate",
        "project",
        "steps",
        "first_useful_ms",
        "first_correct_ms",
        "counts",
        "findings",
        "expected_beta_boundaries",
        "linked_issues",
        "limitations",
    ] {
        if !object.contains_key(field) {
            bail!("missing required receipt field: {field}");
        }
    }

    for field in ["expected_beta_boundaries", "linked_issues", "limitations"] {
        let values = object
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(|| color_eyre::eyre::eyre!("{field} must be an array"))?;
        let mut unique = BTreeSet::new();
        for value in values {
            if !unique.insert(value.to_string()) {
                bail!("{field} must not contain duplicate items");
            }
        }
    }

    let findings = object
        .get("findings")
        .and_then(Value::as_array)
        .ok_or_else(|| color_eyre::eyre::eyre!("findings must be an array"))?;
    for finding in findings {
        let finding = finding
            .as_object()
            .ok_or_else(|| color_eyre::eyre::eyre!("findings[] must be an object"))?;
        if !finding.contains_key("linked_issue") {
            bail!("missing required finding field: linked_issue");
        }
    }

    let steps = object
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| color_eyre::eyre::eyre!("steps must be an array"))?;
    for step in steps {
        let step =
            step.as_object().ok_or_else(|| color_eyre::eyre::eyre!("steps[] must be an object"))?;
        let limitations = step
            .get("limitations")
            .and_then(Value::as_array)
            .ok_or_else(|| color_eyre::eyre::eyre!("steps[].limitations must be an array"))?;
        let mut unique = BTreeSet::new();
        for limitation in limitations {
            if !unique.insert(limitation.to_string()) {
                bail!("steps[].limitations must not contain duplicate items");
            }
        }
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

fn validate_identity(receipt: &Receipt) -> Result<()> {
    non_empty(&receipt.candidate.candidate_id, "candidate.candidate_id")?;
    exact_hex(&receipt.candidate.repository_sha, 20, "candidate.repository_sha")?;
    exact_hex(&receipt.candidate.vsix_sha256, 32, "candidate.vsix_sha256")?;
    exact_hex(&receipt.candidate.perllsp_sha256, 32, "candidate.perllsp_sha256")?;
    exact_hex(&receipt.candidate.perl_dap_sha256, 32, "candidate.perl_dap_sha256")?;
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
            issue_identity(issue, "findings[].linked_issue")?;
        }
    }
    Ok(())
}

fn validate_limited_steps(receipt: &Receipt) -> Result<()> {
    let has_receipt_limitation = !receipt.limitations.is_empty();
    let has_explanation_finding = receipt.findings.iter().any(|finding| {
        matches!(finding.class, FrictionClass::ExpectedBetaBoundary | FrictionClass::Actionability)
    });

    for step in &receipt.steps {
        if step.status == StepStatus::Limited {
            if step.limitations.is_empty() {
                bail!("a limited journey step must explain its limitation");
            }
            if !has_receipt_limitation && !has_explanation_finding {
                bail!(
                    "a limited journey step must bind to a receipt limitation or an expected-beta/actionability finding"
                );
            }
        }
    }
    Ok(())
}

fn computed_status(receipt: &Receipt) -> ReceiptStatus {
    let blocked_step = receipt.steps.iter().any(|step| step.status == StepStatus::Failed);
    let blocked_finding = receipt.findings.iter().any(|finding| {
        matches!(finding.class, FrictionClass::Broken | FrictionClass::TrustBreaker)
    });
    if receipt.counts.trust_breaker_total() > 0 || blocked_step || blocked_finding {
        return ReceiptStatus::Blocked;
    }

    let unproven_step = receipt.steps.iter().any(|step| step.status == StepStatus::NotProven);
    let unproven_finding =
        receipt.findings.iter().any(|finding| finding.class == FrictionClass::NotProven);
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
    validate_limited_steps(receipt)?;

    if let (Some(first_useful), Some(first_correct)) =
        (receipt.first_useful_ms, receipt.first_correct_ms)
        && first_correct < first_useful
    {
        bail!("first_correct_ms cannot precede first_useful_ms");
    }

    for value in &receipt.expected_beta_boundaries {
        non_empty(value, "expected_beta_boundaries[]")?;
    }
    for issue in &receipt.linked_issues {
        issue_identity(issue, "linked_issues[]")?;
    }
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

fn load(path: &Path) -> Result<(Receipt, String)> {
    let content = fs::read(path)
        .with_context(|| format!("reading first-ten-minutes receipt {}", path.display()))?;
    let source_receipt_sha256 = sha256_hex(&content);
    let content = String::from_utf8(content)
        .with_context(|| format!("receipt {} is not valid UTF-8", path.display()))?;
    let raw: Value = serde_json::from_str(&content)?;
    validate_raw_shape(&raw)?;
    Ok((serde_json::from_value(raw)?, source_receipt_sha256))
}

fn write_verified_child_artifact(
    receipt: &Receipt,
    receipt_sha256: &str,
    status: ReceiptStatus,
    path: &Path,
) -> Result<()> {
    exact_hex(receipt_sha256, 32, "source_receipt_sha256")?;
    let artifact = VerifiedChildArtifact {
        owner_issue: OWNER_ISSUE,
        schema_version: VERIFIED_CHILD_SCHEMA_VERSION,
        receipt_schema_version: SCHEMA_VERSION,
        candidate_id: &receipt.candidate.candidate_id,
        frozen_product_sha: &receipt.candidate.repository_sha,
        artifact_set_id: &receipt.candidate.artifact_set_id,
        source_receipt_sha256: receipt_sha256,
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
    let (receipt, receipt_sha256) = load(&args.receipt)?;
    let status = validate(&receipt)?;
    if let Some(path) = &args.verified_output {
        write_verified_child_artifact(&receipt, &receipt_sha256, status, path)?;
    }
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
    use super::{
        Receipt, ReceiptStatus, StepStatus, VerifiedChildArtifact, load, sha256_hex, validate,
        validate_raw_shape, write_verified_child_artifact,
    };
    use color_eyre::eyre::Result;
    use tempfile::tempdir;

    fn fixture(content: &str) -> Result<Receipt> {
        let raw: serde_json::Value = serde_json::from_str(content)?;
        validate_raw_shape(&raw)?;
        Ok(serde_json::from_value(raw)?)
    }

    #[test]
    fn verified_child_output_carries_validated_identity() -> Result<()> {
        let receipt =
            fixture(include_str!("../../fixtures/experience/first_ten_minutes/valid.json"))?;
        let status = validate(&receipt)?;
        let receipt_sha256 =
            sha256_hex(include_bytes!("../../fixtures/experience/first_ten_minutes/valid.json"));
        let directory = tempdir()?;
        let output = directory.path().join("child.json");
        write_verified_child_artifact(&receipt, &receipt_sha256, status, &output)?;
        let artifact: VerifiedChildArtifact<'_> = serde_json::from_slice(&std::fs::read(output)?)?;
        assert_eq!(artifact.schema_version, "verified_child_receipt.v1");
        assert_eq!(artifact.receipt_schema_version, "first_ten_minutes.v1");
        assert_eq!(artifact.candidate_id, "v0.18.0-pre-freeze");
        assert_eq!(artifact.source_receipt_sha256, receipt_sha256);
        assert_eq!(artifact.status, ReceiptStatus::Pass);
        Ok(())
    }

    #[test]
    fn failed_verified_child_publish_preserves_existing_destination() -> Result<()> {
        let receipt =
            fixture(include_str!("../../fixtures/experience/first_ten_minutes/valid.json"))?;
        let status = validate(&receipt)?;
        let receipt_sha256 =
            sha256_hex(include_bytes!("../../fixtures/experience/first_ten_minutes/valid.json"));
        let directory = tempdir()?;
        let destination = directory.path().join("existing");
        std::fs::create_dir(&destination)?;
        let result = write_verified_child_artifact(&receipt, &receipt_sha256, status, &destination);
        if result.is_ok() {
            return Err(color_eyre::eyre::eyre!(
                "publishing over a directory unexpectedly succeeded"
            ));
        }
        if !destination.is_dir() {
            return Err(color_eyre::eyre::eyre!(
                "failed publication did not preserve the existing destination"
            ));
        }
        Ok(())
    }

    #[test]
    fn load_hashes_the_exact_receipt_bytes() -> Result<()> {
        let directory = tempdir()?;
        let input = directory.path().join("receipt.json");
        let bytes = include_bytes!("../../fixtures/experience/first_ten_minutes/valid.json");
        std::fs::write(&input, bytes)?;
        let (receipt, digest) = load(&input)?;
        assert_eq!(receipt.status, ReceiptStatus::Pass);
        assert_eq!(digest, sha256_hex(bytes));
        Ok(())
    }

    #[test]
    fn valid_fixture_passes() -> Result<()> {
        let receipt =
            fixture(include_str!("../../fixtures/experience/first_ten_minutes/valid.json"))?;
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
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/first_ten_minutes/valid.json"))?;
        let removed = receipt.steps.pop();
        assert!(removed.is_some());
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn a_false_green_status_is_rejected() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/first_ten_minutes/valid.json"))?;
        receipt.counts.stale_exact = 1;
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn malformed_issue_identity_is_rejected() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/first_ten_minutes/valid.json"))?;
        receipt.linked_issues[0] = "#not-a-number".to_string();
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn first_correct_cannot_precede_first_useful() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/first_ten_minutes/valid.json"))?;
        receipt.first_useful_ms = Some(500);
        receipt.first_correct_ms = Some(400);
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn limited_step_requires_an_explanation() -> Result<()> {
        let mut receipt =
            fixture(include_str!("../../fixtures/experience/first_ten_minutes/valid.json"))?;
        receipt.steps[0].status = StepStatus::Limited;
        assert!(validate(&receipt).is_err());
        Ok(())
    }

    #[test]
    fn schema_required_nullable_timings_cannot_be_omitted() -> Result<()> {
        let mut raw: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/experience/first_ten_minutes/valid.json"
        ))?;
        raw.as_object_mut()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture is not an object"))?
            .remove("first_useful_ms");
        assert!(validate_raw_shape(&raw).is_err());
        Ok(())
    }

    #[test]
    fn schema_unique_arrays_cannot_contain_duplicates() -> Result<()> {
        let mut raw: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/experience/first_ten_minutes/valid.json"
        ))?;
        let issues = raw
            .as_object_mut()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture is not an object"))?
            .get_mut("linked_issues")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no linked issues"))?;
        let first = issues
            .first()
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no issue identity"))?;
        issues.push(first);
        assert!(validate_raw_shape(&raw).is_err());
        Ok(())
    }

    #[test]
    fn schema_required_finding_linked_issue_cannot_be_omitted() -> Result<()> {
        let mut raw: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/experience/first_ten_minutes/valid.json"
        ))?;
        raw.get_mut("findings")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|findings| findings.first_mut())
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no finding object"))?
            .remove("linked_issue");
        if validate_raw_shape(&raw).is_ok() {
            return Err(color_eyre::eyre::eyre!(
                "omitted findings[].linked_issue unexpectedly passed raw validation"
            ));
        }
        Ok(())
    }

    #[test]
    fn schema_step_limitations_cannot_contain_duplicates() -> Result<()> {
        let mut raw: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/experience/first_ten_minutes/valid.json"
        ))?;
        let limitations = raw
            .get_mut("steps")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|steps| steps.first_mut())
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|step| step.get_mut("limitations"))
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no step limitations"))?;
        let first = limitations
            .first()
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture has no step limitation"))?;
        limitations.push(first);
        if validate_raw_shape(&raw).is_ok() {
            return Err(color_eyre::eyre::eyre!(
                "duplicate steps[].limitations unexpectedly passed raw validation"
            ));
        }
        Ok(())
    }
}
