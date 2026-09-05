//! Validate one packaged first-ten-minutes experience receipt and the
//! checked-in representative fixture set.
//!
//! This instrument validates observation identity, the finite four-stage user
//! journey, explicit trust-breaker counts, friction classification, and the
//! declared pass/blocked/not-proven disposition. It also verifies that the
//! checked-in five-family representative project set is complete and
//! content-current, and that checked-in receipts bind real fixture identity.
//! It does not launch VS Code, observe a user, repair findings, or authorize
//! a release.

#![allow(clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const CHECK: &str = "first-ten-minutes";
const SCHEMA_VERSION: &str = "first_ten_minutes.v1";
const VERIFIED_CHILD_SCHEMA_VERSION: &str = "verified_child_receipt.v1";
const FIXTURE_MANIFEST_SCHEMA_VERSION: &str = "first_ten_minutes_fixtures.v1";
/// The only digest recipe a fixture manifest may declare. Verification
/// executes exactly this algorithm (`fixture_content_digest`), so the
/// manifest's declared recipe is bound to this canonical string instead of
/// accepting any nonblank description.
const FIXTURE_HASH_RECIPE: &str = "sha256 over all regular files of the fixture directory sorted by relative POSIX path; each file contributes its path bytes, LF, its decimal byte length, LF, then its file bytes";
/// Canonical identity of the checked-in representative set: exactly these
/// fixture ids bound to exactly these families and content digests.
/// Verified-child artifact emission requires the verified set to match this
/// identity, so a synthetic self-consistent five-family set — including one
/// that merely swaps family labels — can never emit an indistinguishable
/// trusted artifact. The unit tests pin this table to the checked-in
/// manifest.json, so any fixture refresh must update both together.
const CANONICAL_FIXTURE_SET: [(&str, ProjectFamily, &str); FIXTURE_FAMILY_COUNT] = [
    (
        "conventional-modules-v1",
        ProjectFamily::ConventionalModules,
        "0dc6672278fcac9248381a50ce483203772633abdb2a7d280cb6eac0125a6246",
    ),
    (
        "test-heavy-v1",
        ProjectFamily::TestHeavy,
        "c851ad7fd74133ee0f14e95a17592dcb0dc3e90640eff4d5181242d205268ac8",
    ),
    (
        "framework-shaped-v1",
        ProjectFamily::FrameworkShaped,
        "02264e9564e2ae7830ceabdb7d0337c0e32788334456d42e1443bd98ec460f7f",
    ),
    (
        "environment-sensitive-v1",
        ProjectFamily::EnvironmentSensitive,
        "7684d6cf700fbbef9723c838758d51b32726a048de32ece93f0c647a496c8664",
    ),
    (
        "dynamic-boundary-v1",
        ProjectFamily::DynamicBoundaryControl,
        "d72fc32c859eb6dc11f5ed73cbbdee9c437cc47fc891a60631f703f10a8c5c46",
    ),
];
const OWNER_ISSUE: &str = "#5902";
const FIXTURE_FAMILY_COUNT: usize = 5;
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
    receipt: Option<PathBuf>,

    /// Representative fixture-set root holding `manifest.json` and one
    /// content-addressed directory per experience family.
    #[arg(long)]
    verify_fixture_set: Option<PathBuf>,

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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureManifest {
    schema_version: String,
    owner_issue: String,
    hash_recipe: String,
    fixtures: Vec<FixtureEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureEntry {
    fixture_id: String,
    family: ProjectFamily,
    path: String,
    content_sha256: String,
    exercises: String,
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

fn load_fixture_manifest(root: &Path) -> Result<FixtureManifest> {
    let manifest_path = root.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading fixture manifest {}", manifest_path.display()))?;
    let manifest: FixtureManifest = serde_json::from_str(&raw)
        .with_context(|| format!("parsing fixture manifest {}", manifest_path.display()))?;
    if manifest.schema_version != FIXTURE_MANIFEST_SCHEMA_VERSION {
        bail!("fixture manifest schema_version must be {FIXTURE_MANIFEST_SCHEMA_VERSION}");
    }
    if manifest.owner_issue != OWNER_ISSUE {
        bail!("fixture manifest owner_issue must be {OWNER_ISSUE}");
    }
    if manifest.hash_recipe != FIXTURE_HASH_RECIPE {
        bail!(
            "fixture manifest hash_recipe must be the canonical recipe {FIXTURE_HASH_RECIPE:?}; verification only executes that recipe"
        );
    }
    Ok(manifest)
}

fn all_families() -> BTreeSet<ProjectFamily> {
    BTreeSet::from([
        ProjectFamily::ConventionalModules,
        ProjectFamily::TestHeavy,
        ProjectFamily::FrameworkShaped,
        ProjectFamily::EnvironmentSensitive,
        ProjectFamily::DynamicBoundaryControl,
    ])
}

fn collect_fixture_files(root: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let directory = root.join(relative);
    let entries =
        fs::read_dir(&directory).with_context(|| format!("reading {}", directory.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("enumerating {}", directory.display()))?;
        let child = relative.join(entry.file_name());
        let entry_path = entry.path();
        // The recipe hashes the UTF-8 path bytes, so a non-UTF-8 name would
        // lose information in the preimage; reject it instead of lossily
        // hashing an identity that cannot be reconstructed.
        if entry.file_name().to_str().is_none() {
            bail!(
                "fixture entry {} has a non-UTF-8 name; fixture sets must contain only UTF-8 names",
                entry_path.display()
            );
        }
        // symlink_metadata never follows links, so a symbolic link inside a
        // fixture directory is rejected instead of being traversed or read:
        // the digest must only ever cover bytes inside the declared fixture.
        let metadata = entry_path
            .symlink_metadata()
            .with_context(|| format!("stating {}", entry_path.display()))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            bail!(
                "fixture entry {} is a symbolic link; fixture sets must contain only regular files and directories",
                entry_path.display()
            );
        } else if file_type.is_dir() {
            collect_fixture_files(root, &child, files)?;
        } else if file_type.is_file() {
            files.push(child);
        }
    }
    Ok(())
}

/// Content digest over one fixture directory: SHA-256 across all regular
/// files sorted by relative POSIX path, each contributing its path, LF, its
/// decimal byte length, LF, then its bytes.
fn fixture_content_digest(fixture_dir: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_fixture_files(fixture_dir, Path::new(""), &mut files)?;
    if files.is_empty() {
        bail!("fixture directory {} contains no files", fixture_dir.display());
    }
    files.sort();
    let mut preimage = Vec::new();
    for relative in &files {
        let bytes = fs::read(fixture_dir.join(relative))
            .with_context(|| format!("reading {}", fixture_dir.join(relative).display()))?;
        let rendered = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        preimage.extend_from_slice(rendered.as_bytes());
        preimage.extend_from_slice(b"\n");
        preimage.extend_from_slice(bytes.len().to_string().as_bytes());
        preimage.extend_from_slice(b"\n");
        preimage.extend_from_slice(&bytes);
    }
    Ok(sha256_hex(&preimage))
}

fn safe_fixture_relative_path(path: &str) -> Result<()> {
    let relative = Path::new(path);
    // A fixture path must be exactly one plain directory name: no parent,
    // current, root, or prefix components, no separators, no nesting.
    let single_plain_name =
        matches!(relative.components().collect::<Vec<_>>().as_slice(), [Component::Normal(_)]);
    if path.is_empty() || path.contains('\\') || !single_plain_name {
        bail!("fixtures[].path must be a plain relative directory name, got {path:?}");
    }
    Ok(())
}

fn verify_fixture_set(root: &Path) -> Result<FixtureManifest> {
    let root_metadata = fs::symlink_metadata(root)
        .with_context(|| format!("stating fixture-set root {}", root.display()))?;
    if root_metadata.file_type().is_symlink() {
        bail!(
            "fixture-set root {} is a symbolic link; the representative set must have a real root directory",
            root.display()
        );
    }
    let mut manifest = load_fixture_manifest(root)?;
    let mut families = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut canonical_dirs = BTreeSet::new();
    for entry in &mut manifest.fixtures {
        non_empty(&entry.fixture_id, "fixtures[].fixture_id")?;
        non_empty(&entry.exercises, "fixtures[].exercises")?;
        exact_hex(&entry.content_sha256, 32, "fixtures[].content_sha256")?;
        safe_fixture_relative_path(&entry.path)?;
        if !ids.insert(entry.fixture_id.as_str()) {
            bail!("duplicate fixture id: {}", entry.fixture_id);
        }
        if !families.insert(entry.family) {
            bail!("family {:?} is covered by more than one fixture", entry.family);
        }
        if !paths.insert(entry.path.clone()) {
            bail!(
                "fixtures[].path {} is registered more than once; distinct families must bind distinct directories",
                entry.path
            );
        }
        let fixture_dir = root.join(&entry.path);
        let fixture_link = fs::symlink_metadata(&fixture_dir)
            .with_context(|| format!("stating {}", fixture_dir.display()))?;
        if fixture_link.file_type().is_symlink() {
            bail!(
                "fixture directory {} is a symbolic link; the representative set must contain only real directories",
                fixture_dir.display()
            );
        }
        if !fixture_dir.is_dir() {
            bail!("fixture directory {} is missing", fixture_dir.display());
        }
        // Manifest path strings can still alias on case-insensitive
        // filesystems (proj-a vs PROJ-A), so deduplicate by canonical
        // filesystem identity after the symlink rejection.
        let canonical_dir = fs::canonicalize(&fixture_dir)
            .with_context(|| format!("canonicalizing {}", fixture_dir.display()))?;
        if !canonical_dirs.insert(canonical_dir) {
            bail!(
                "fixtures[].path {} resolves to an already-registered fixture directory; distinct families must bind distinct directories",
                entry.path
            );
        }
        // Digests are lowercase hex; the schema accepts A-F, so normalize the
        // validated manifest value once and compare case-insensitively.
        entry.content_sha256 = entry.content_sha256.to_ascii_lowercase();
        let computed = fixture_content_digest(&fixture_dir)?;
        if computed != entry.content_sha256 {
            bail!(
                "fixture {} content drifted: manifest {}, computed {} (refresh content_sha256 or restore the bytes)",
                entry.fixture_id,
                entry.content_sha256,
                computed
            );
        }
    }
    if manifest.fixtures.len() != FIXTURE_FAMILY_COUNT {
        bail!(
            "the representative set must cover exactly {FIXTURE_FAMILY_COUNT} families, found {}",
            manifest.fixtures.len()
        );
    }
    if families != all_families() {
        bail!("the representative set must cover each experience family exactly once");
    }
    reject_unmanifested_fixture_directories(root, &paths)?;
    Ok(manifest)
}

/// Every directory under the fixture-set root must be a registered fixture
/// path, so the finite representative set cannot hide an unmanifested
/// project directory (files such as `manifest.json` and `README.md` are not
/// project directories).
fn reject_unmanifested_fixture_directories(root: &Path, paths: &BTreeSet<String>) -> Result<()> {
    let entries = fs::read_dir(root).with_context(|| format!("reading {}", root.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("enumerating {}", root.display()))?;
        let entry_path = entry.path();
        let metadata = entry_path
            .symlink_metadata()
            .with_context(|| format!("stating {}", entry_path.display()))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            bail!(
                "fixture-set entry {} is a symbolic link; the representative set must contain only real directories and plain files",
                entry_path.display()
            );
        }
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !paths.contains(name.as_str()) {
            bail!(
                "fixture-set directory {name} is not manifested; every project directory under the set root must be registered in manifest.json exactly once"
            );
        }
    }
    Ok(())
}

fn assert_receipt_binds_fixture_set(
    receipt: &Receipt,
    receipt_path: &Path,
    manifest: &FixtureManifest,
) -> Result<()> {
    let entry = manifest
        .fixtures
        .iter()
        .find(|entry| entry.fixture_id == receipt.project.fixture_id)
        .ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "receipt {} binds fixture id {} which is not in the checked-in representative set",
                receipt_path.display(),
                receipt.project.fixture_id
            )
        })?;
    if entry.family != receipt.project.family {
        bail!(
            "receipt {} declares family {:?} but fixture {} is registered as {:?}",
            receipt_path.display(),
            receipt.project.family,
            entry.fixture_id,
            entry.family
        );
    }
    if !entry.content_sha256.eq_ignore_ascii_case(&receipt.project.content_sha256) {
        bail!(
            "receipt {} binds stale content for fixture {}: receipt {}, set {}",
            receipt_path.display(),
            entry.fixture_id,
            receipt.project.content_sha256,
            entry.content_sha256
        );
    }
    Ok(())
}

/// Verified-child artifact emission is trusted, so the verified set must be
/// the canonical checked-in representative set itself — not merely a
/// self-consistent synthetic set. Standalone verification of synthetic sets
/// stays available; only artifact emission is gated here.
fn assert_canonical_fixture_set(manifest: &FixtureManifest) -> Result<()> {
    for (fixture_id, family, content_sha256) in &CANONICAL_FIXTURE_SET {
        let entry = manifest.fixtures.iter().find(|entry| entry.fixture_id == *fixture_id);
        let Some(entry) = entry else {
            bail!(
                "fixture {fixture_id} from the canonical checked-in set is missing; verified child artifacts may only be emitted from the checked-in representative set"
            );
        };
        if entry.family != *family {
            bail!(
                "fixture {fixture_id} is registered as family {:?} but the canonical checked-in set binds it to {:?}; verified child artifacts may only be emitted from the checked-in representative set",
                entry.family,
                family
            );
        }
        if !entry.content_sha256.eq_ignore_ascii_case(content_sha256) {
            bail!(
                "fixture {fixture_id} content digest {} does not match the canonical checked-in set digest {content_sha256}; verified child artifacts may only be emitted from the checked-in representative set",
                entry.content_sha256
            );
        }
    }
    Ok(())
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

/// Argument-combination contract, enforced before any validation runs:
/// - some work must be requested;
/// - `--verified-output` requests a trusted verified-child artifact, so it
///   requires `--receipt` (something to bind) and `--verify-fixture-set`
///   (the checked-in representative set to bind it against). Receipt
///   binding therefore can never be skipped on the artifact path.
fn require_artifact_preconditions(
    receipt: Option<&Path>,
    verify_fixture_set: Option<&Path>,
    verified_output: Option<&Path>,
) -> Result<()> {
    if receipt.is_none() && verify_fixture_set.is_none() {
        bail!("provide --receipt, --verify-fixture-set, or both");
    }
    if verified_output.is_some() && receipt.is_none() {
        bail!("--verified-output requires --receipt");
    }
    if verified_output.is_some() && verify_fixture_set.is_none() {
        bail!(
            "--verified-output requires --verify-fixture-set so the receipt binds the checked-in representative set before any verified child artifact is written"
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    require_artifact_preconditions(
        args.receipt.as_deref(),
        args.verify_fixture_set.as_deref(),
        args.verified_output.as_deref(),
    )?;
    let mut fixture_manifest = None;
    if let Some(root) = &args.verify_fixture_set {
        let manifest = verify_fixture_set(root)?;
        if args.verified_output.is_some() {
            assert_canonical_fixture_set(&manifest).with_context(|| {
                format!(
                    "binding fixture set {} to the canonical checked-in representative identity",
                    root.display()
                )
            })?;
        }
        println!(
            "first-ten-minutes: fixture set {} verified ({} families, content current)",
            root.display(),
            manifest.fixtures.len()
        );
        fixture_manifest = Some(manifest);
    }
    if let Some(receipt_path) = &args.receipt {
        let (receipt, receipt_sha256) = load(receipt_path)?;
        let status = validate(&receipt)?;
        if let (Some(manifest), Some(root)) = (&fixture_manifest, &args.verify_fixture_set) {
            assert_receipt_binds_fixture_set(&receipt, receipt_path, manifest)
                .with_context(|| format!("binding receipt to fixture set {}", root.display()))?;
        }
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CANONICAL_FIXTURE_SET, FIXTURE_HASH_RECIPE, FixtureEntry, FixtureManifest, OWNER_ISSUE,
        ProjectFamily, Receipt, ReceiptStatus, StepStatus, assert_canonical_fixture_set,
        assert_receipt_binds_fixture_set, fixture_content_digest, load, load_fixture_manifest,
        reject_unmanifested_fixture_directories, require_artifact_preconditions, sha256_hex,
        validate, validate_raw_shape, verify_fixture_set, write_verified_child_artifact,
    };
    use color_eyre::eyre::Result;
    use serde::Deserialize;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn default_fixture_set_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/ux/first_ten_minutes")
    }

    /// Owned strict mirror of the written verified-child envelope. Decoding
    /// through this type enforces every required field, its type, and
    /// `deny_unknown_fields` without the borrowed `&'static`/`&'a` mixing
    /// that makes the write-side struct's derive unsatisfiable from input
    /// data (the trap behind #12650).
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct VerifiedChildArtifactExpectation {
        owner_issue: String,
        schema_version: String,
        receipt_schema_version: String,
        candidate_id: String,
        frozen_product_sha: String,
        artifact_set_id: String,
        source_receipt_sha256: String,
        status: ReceiptStatus,
        claim_boundary: String,
        limitation: Option<String>,
    }

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
        let artifact: VerifiedChildArtifactExpectation =
            serde_json::from_slice(&std::fs::read(output)?)?;
        assert_eq!(artifact.owner_issue, OWNER_ISSUE);
        assert_eq!(artifact.schema_version, "verified_child_receipt.v1");
        assert_eq!(artifact.receipt_schema_version, "first_ten_minutes.v1");
        assert_eq!(artifact.candidate_id, "v0.18.0-pre-freeze");
        assert_eq!(artifact.frozen_product_sha, receipt.candidate.repository_sha);
        assert_eq!(artifact.artifact_set_id, receipt.candidate.artifact_set_id);
        assert_eq!(artifact.source_receipt_sha256, receipt_sha256);
        assert_eq!(artifact.status, ReceiptStatus::Pass);
        assert_eq!(artifact.claim_boundary, receipt.claim_boundary);
        assert_eq!(artifact.limitation.as_deref(), receipt.limitations.first().map(String::as_str));
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
        let seeded = "synthetic step limitation for duplicate rejection".to_string();
        limitations.push(serde_json::Value::String(seeded.clone()));
        limitations.push(serde_json::Value::String(seeded));
        if validate_raw_shape(&raw).is_ok() {
            return Err(color_eyre::eyre::eyre!(
                "duplicate steps[].limitations unexpectedly passed raw validation"
            ));
        }
        Ok(())
    }

    fn checked_in_manifest() -> Result<FixtureManifest> {
        load_fixture_manifest(&default_fixture_set_path())
    }

    /// The checked-in representative set must exist, cover each experience
    /// family exactly once, and be content-current: any byte drift in a
    /// fixture directory without a manifest refresh fails here.
    #[test]
    fn representative_fixture_set_is_complete_and_current() -> Result<()> {
        let manifest = verify_fixture_set(&default_fixture_set_path())?;
        assert_eq!(manifest.fixtures.len(), 5);
        let ids: Vec<_> = manifest.fixtures.iter().map(|entry| entry.fixture_id.as_str()).collect();
        assert_eq!(ids.len(), 5);
        Ok(())
    }

    /// Checked-in sample receipts must bind real fixture identity from the
    /// representative set, not placeholder hashes.
    #[test]
    fn checked_in_receipts_bind_checked_in_fixtures() -> Result<()> {
        let manifest = checked_in_manifest()?;
        for name in ["valid.json", "trust_breaker.json"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../fixtures/experience/first_ten_minutes")
                .join(name);
            let (receipt, _) = load(&path)?;
            assert_receipt_binds_fixture_set(&receipt, &path, &manifest)?;
        }
        Ok(())
    }

    /// Negative control, red then green inside the test: content drift in a
    /// fixture directory must be rejected, and a manifest refresh for the
    /// drifted bytes must accept the same tree again.
    #[test]
    fn tampered_fixture_content_is_rejected() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path();
        let fixture_dir = root.join("proj-a");
        std::fs::create_dir(&fixture_dir)?;
        std::fs::write(fixture_dir.join("main.pl"), "print 1;\n")?;
        let manifest_path = root.join("manifest.json");
        let write_manifest = |content_sha256: &str| -> Result<()> {
            std::fs::write(
                &manifest_path,
                serde_json::to_vec_pretty(&serde_json::json!({
                    "schema_version": "first_ten_minutes_fixtures.v1",
                    "owner_issue": OWNER_ISSUE,
                    "hash_recipe": FIXTURE_HASH_RECIPE,
                    "fixtures": [{
                        "fixture_id": "proj-a",
                        "family": "conventional_modules",
                        "path": "proj-a",
                        "content_sha256": content_sha256,
                        "exercises": "synthetic coverage"
                    }]
                }))?,
            )?;
            Ok(())
        };

        // The synthetic set is deliberately incomplete: expect the count gate.
        let error = |root: &std::path::Path| -> String {
            match verify_fixture_set(root) {
                Ok(_) => "unexpectedly passed".to_string(),
                Err(error) => format!("{error}"),
            }
        };
        write_manifest(&fixture_content_digest(&fixture_dir)?)?;
        let outcome = error(root);
        if !outcome.contains("must cover exactly 5 families") {
            return Err(color_eyre::eyre::eyre!(
                "complete-but-small synthetic set failed for the wrong reason: {outcome}"
            ));
        }

        // Red: drifted bytes are reported as drift, not as set incompleteness.
        std::fs::write(fixture_dir.join("main.pl"), "print 2;\n")?;
        let outcome = error(root);
        if !outcome.contains("content drifted") {
            return Err(color_eyre::eyre::eyre!(
                "drifted fixture content was not discriminated as drift: {outcome}"
            ));
        }

        // Green: refreshing the manifest hash resolves the drift report. The
        // refreshed content passes the digest gate, so the only remaining
        // failure must be the deliberately incomplete count gate.
        write_manifest(&fixture_content_digest(&fixture_dir)?)?;
        let outcome = error(root);
        if !outcome.contains("must cover exactly 5 families") {
            return Err(color_eyre::eyre::eyre!(
                "refreshed manifest did not proceed past the digest gate to the set gate: {outcome}"
            ));
        }
        Ok(())
    }

    /// Negative control: a set that drops a family must fail closed even if
    /// every remaining entry is content-current.
    #[test]
    fn fixture_set_missing_a_family_fails_closed() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path();
        let fixture_dir = root.join("proj-a");
        std::fs::create_dir(&fixture_dir)?;
        std::fs::write(fixture_dir.join("main.pl"), "print 1;\n")?;
        let computed = fixture_content_digest(&fixture_dir)?;
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": "first_ten_minutes_fixtures.v1",
                "owner_issue": OWNER_ISSUE,
                "hash_recipe": FIXTURE_HASH_RECIPE,
                "fixtures": [{
                    "fixture_id": "proj-a",
                    "family": "conventional_modules",
                    "path": "proj-a",
                    "content_sha256": computed,
                    "exercises": "synthetic coverage"
                }]
            }))?,
        )?;
        if verify_fixture_set(root).is_ok() {
            return Err(color_eyre::eyre::eyre!(
                "an incomplete representative set unexpectedly passed verification"
            ));
        }
        Ok(())
    }

    fn write_synth_manifest(root: &Path, fixtures: serde_json::Value) -> Result<()> {
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": "first_ten_minutes_fixtures.v1",
                "owner_issue": OWNER_ISSUE,
                "hash_recipe": FIXTURE_HASH_RECIPE,
                "fixtures": fixtures,
            }))?,
        )?;
        Ok(())
    }

    fn verification_error(root: &Path) -> String {
        match verify_fixture_set(root) {
            Ok(_) => "unexpectedly passed".to_string(),
            Err(error) => format!("{error}"),
        }
    }

    fn expect_error_contains(outcome: &str, needle: &str, context: &str) -> Result<()> {
        if !outcome.contains(needle) {
            return Err(color_eyre::eyre::eyre!("{context}: {outcome}"));
        }
        Ok(())
    }

    /// Negative control for the plain-name path rule: traversal, current-dir,
    /// nested, backslash, absolute, and parent paths must all be rejected as
    /// "not a plain relative directory name" before any directory is touched.
    #[test]
    fn unsafe_fixture_paths_are_rejected() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path();
        for bad in ["../outside", "./dot", "a/b", "back\\slash", "/absolute", ".."] {
            write_synth_manifest(
                root,
                serde_json::json!([{
                    "fixture_id": "proj-a",
                    "family": "conventional_modules",
                    "path": bad,
                    "content_sha256": "0".repeat(64),
                    "exercises": "synthetic coverage"
                }]),
            )?;
            let outcome = verification_error(root);
            expect_error_contains(
                &outcome,
                "must be a plain relative directory name",
                &format!("unsafe fixture path {bad:?} was not rejected"),
            )?;
        }
        Ok(())
    }

    /// Negative control: two families aliasing one directory must be rejected
    /// even when ids, families, and the shared digest all look valid, so one
    /// real project cannot masquerade as the five-family representative set.
    #[test]
    fn duplicate_fixture_paths_are_rejected() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path();
        let fixture_dir = root.join("proj-a");
        std::fs::create_dir(&fixture_dir)?;
        std::fs::write(fixture_dir.join("main.pl"), "print 1;\n")?;
        let digest = fixture_content_digest(&fixture_dir)?;
        write_synth_manifest(
            root,
            serde_json::json!([
                {
                    "fixture_id": "proj-a",
                    "family": "conventional_modules",
                    "path": "proj-a",
                    "content_sha256": digest,
                    "exercises": "synthetic coverage"
                },
                {
                    "fixture_id": "proj-b",
                    "family": "test_heavy",
                    "path": "proj-a",
                    "content_sha256": digest,
                    "exercises": "aliased directory"
                }
            ]),
        )?;
        let outcome = verification_error(root);
        expect_error_contains(
            &outcome,
            "registered more than once",
            "aliased fixture directories were not rejected",
        )
    }

    /// The set root may not carry project directories that manifest.json
    /// never registers; plain files (manifest, README) stay exempt.
    #[test]
    fn unmanifested_fixture_directory_is_rejected() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path();
        std::fs::create_dir(root.join("proj-a"))?;
        std::fs::write(root.join("manifest.json"), "{}")?;
        std::fs::write(root.join("README.md"), "notes")?;
        std::fs::create_dir(root.join("sneaky-extra"))?;
        let mut registered = std::collections::BTreeSet::from(["proj-a".to_string()]);
        match reject_unmanifested_fixture_directories(root, &registered) {
            Ok(()) => Err(color_eyre::eyre::eyre!(
                "an unmanifested project directory unexpectedly passed"
            )),
            Err(error) => expect_error_contains(
                &format!("{error}"),
                "is not manifested",
                "unmanifested directory failed for the wrong reason",
            ),
        }
    }

    /// The declared recipe must be the canonical recipe the verifier actually
    /// executes; a manifest claiming any other recipe fails closed.
    #[test]
    fn non_canonical_hash_recipe_is_rejected() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path();
        let fixture_dir = root.join("proj-a");
        std::fs::create_dir(&fixture_dir)?;
        std::fs::write(fixture_dir.join("main.pl"), "print 1;\n")?;
        let mut manifest = serde_json::json!({
            "schema_version": "first_ten_minutes_fixtures.v1",
            "owner_issue": OWNER_ISSUE,
            "hash_recipe": FIXTURE_HASH_RECIPE,
            "fixtures": [{
                "fixture_id": "proj-a",
                "family": "conventional_modules",
                "path": "proj-a",
                "content_sha256": fixture_content_digest(&fixture_dir)?,
                "exercises": "synthetic coverage"
            }]
        });
        manifest["hash_recipe"] = serde_json::json!("sha256(file bytes only)");
        std::fs::write(root.join("manifest.json"), serde_json::to_vec_pretty(&manifest)?)?;
        let outcome = verification_error(root);
        expect_error_contains(
            &outcome,
            "canonical recipe",
            "a mutated hash_recipe was not rejected",
        )
    }

    /// The schema accepts uppercase A-F, so an uppercase but current manifest
    /// digest must pass the digest gate; the only remaining failure for this
    /// deliberately small synthetic set is the five-family count gate.
    #[test]
    fn uppercase_manifest_digest_passes_the_digest_gate() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path();
        let fixture_dir = root.join("proj-a");
        std::fs::create_dir(&fixture_dir)?;
        std::fs::write(fixture_dir.join("main.pl"), "print 1;\n")?;
        let digest = fixture_content_digest(&fixture_dir)?.to_uppercase();
        write_synth_manifest(
            root,
            serde_json::json!([{
                "fixture_id": "proj-a",
                "family": "conventional_modules",
                "path": "proj-a",
                "content_sha256": digest,
                "exercises": "synthetic coverage"
            }]),
        )?;
        let outcome = verification_error(root);
        expect_error_contains(
            &outcome,
            "must cover exactly 5 families",
            "uppercase digest did not pass the digest gate",
        )
    }

    /// Receipt digests are compared case-insensitively: uppercasing the bound
    /// content identity must not reject an otherwise-current receipt.
    #[test]
    fn receipt_binding_compares_digests_case_insensitively() -> Result<()> {
        let manifest = checked_in_manifest()?;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/experience/first_ten_minutes/valid.json");
        let (mut receipt, _) = load(&path)?;
        receipt.project.content_sha256 = receipt.project.content_sha256.to_uppercase();
        assert_receipt_binds_fixture_set(&receipt, &path, &manifest)
    }

    /// Negative control for artifact trust: a receipt whose content identity
    /// no longer matches the set must fail binding, so it can never reach
    /// verified-child artifact emission.
    #[test]
    fn stale_receipt_binding_is_rejected() -> Result<()> {
        let manifest = checked_in_manifest()?;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/experience/first_ten_minutes/valid.json");
        let (mut receipt, _) = load(&path)?;
        receipt.project.content_sha256 = "0".repeat(64);
        match assert_receipt_binds_fixture_set(&receipt, &path, &manifest) {
            Ok(()) => Err(color_eyre::eyre::eyre!(
                "a receipt binding all-zero fixture content unexpectedly passed"
            )),
            Err(error) => expect_error_contains(
                &format!("{error}"),
                "binds stale content",
                "stale receipt failed for the wrong reason",
            ),
        }
    }

    /// A trusted verified-child artifact requires both a receipt and the
    /// checked-in fixture set, so receipt binding can never be skipped on the
    /// artifact path; a fixture-set-only or receipt-only run without output
    /// remains valid.
    #[test]
    fn verified_output_requires_receipt_and_fixture_set() -> Result<()> {
        let output = Path::new("child.json");
        let receipt = Path::new("receipt.json");
        let fixture_set = Path::new("testdata/ux/first_ten_minutes");
        if require_artifact_preconditions(None, None, None).is_ok() {
            return Err(color_eyre::eyre::eyre!(
                "an invocation with no work requested unexpectedly passed"
            ));
        }
        let outcome = match require_artifact_preconditions(None, Some(fixture_set), Some(output)) {
            Ok(()) => "unexpectedly passed".to_string(),
            Err(error) => format!("{error}"),
        };
        expect_error_contains(
            &outcome,
            "--verified-output requires --receipt",
            "verified output without a receipt was not rejected",
        )?;
        let outcome = match require_artifact_preconditions(Some(receipt), None, Some(output)) {
            Ok(()) => "unexpectedly passed".to_string(),
            Err(error) => format!("{error}"),
        };
        expect_error_contains(
            &outcome,
            "--verified-output requires --verify-fixture-set",
            "verified output without a fixture set was not rejected",
        )?;
        require_artifact_preconditions(Some(receipt), Some(fixture_set), Some(output))?;
        require_artifact_preconditions(Some(receipt), Some(fixture_set), None)?;
        require_artifact_preconditions(None, Some(fixture_set), None)?;
        Ok(())
    }

    /// The canonical identity table and the checked-in manifest must stay in
    /// lockstep: any fixture refresh updates both, or this proof fails.
    #[test]
    fn checked_in_manifest_matches_canonical_identity() -> Result<()> {
        let manifest = checked_in_manifest()?;
        assert_canonical_fixture_set(&manifest)?;
        assert_eq!(CANONICAL_FIXTURE_SET.len(), manifest.fixtures.len());
        Ok(())
    }

    /// Checked-in sample receipts must not claim evidence their bound
    /// fixture cannot produce: conventional-modules-v1 has no dynamic
    /// dispatch boundary, so a receipt bound to it must not claim
    /// dynamic-method observations.
    #[test]
    fn checked_in_passing_receipt_evidence_matches_its_fixture() -> Result<()> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/experience/first_ten_minutes/valid.json");
        let (receipt, _) = load(&path)?;
        if receipt.project.fixture_id != "conventional-modules-v1" {
            return Ok(());
        }
        for finding in &receipt.findings {
            let text = format!("{} {}", finding.id, finding.summary).to_lowercase();
            if text.contains("dynamic") {
                return Err(color_eyre::eyre::eyre!(
                    "receipt bound to conventional-modules-v1 claims dynamic-dispatch evidence: {}",
                    finding.id
                ));
            }
        }
        for step in &receipt.steps {
            if step.evidence_ref.to_lowercase().contains("dynamic-method-control") {
                return Err(color_eyre::eyre::eyre!(
                    "receipt bound to conventional-modules-v1 claims dynamic-method evidence: {}",
                    step.evidence_ref
                ));
            }
        }
        Ok(())
    }

    /// Negative control: a manifest that keeps every canonical id and digest
    /// but swaps two family labels must fail artifact eligibility, so family
    /// assignments cannot be rearranged behind a trusted artifact.
    #[test]
    fn swapped_canonical_families_fail_artifact_eligibility() -> Result<()> {
        let manifest = checked_in_manifest()?;
        let mut entries = manifest.fixtures.clone();
        entries[0].family = ProjectFamily::TestHeavy;
        entries[1].family = ProjectFamily::ConventionalModules;
        let swapped = FixtureManifest {
            schema_version: manifest.schema_version.clone(),
            owner_issue: manifest.owner_issue.clone(),
            hash_recipe: manifest.hash_recipe.clone(),
            fixtures: entries,
        };
        let outcome = match assert_canonical_fixture_set(&swapped) {
            Ok(()) => "unexpectedly passed".to_string(),
            Err(error) => format!("{error}"),
        };
        expect_error_contains(
            &outcome,
            "is registered as family",
            "a family-swapped canonical set claimed artifact eligibility",
        )
    }

    /// A synthetic self-consistent set stays verifiable standalone but can
    /// never claim the canonical identity required for artifact emission.
    #[test]
    fn synthetic_set_cannot_claim_canonical_identity() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path();
        let fixture_dir = root.join("proj-a");
        std::fs::create_dir(&fixture_dir)?;
        std::fs::write(fixture_dir.join("main.pl"), "print 1;\n")?;
        let mut manifest = serde_json::json!({
            "schema_version": "first_ten_minutes_fixtures.v1",
            "owner_issue": OWNER_ISSUE,
            "hash_recipe": FIXTURE_HASH_RECIPE,
            "fixtures": [{
                "fixture_id": "proj-a",
                "family": "conventional_modules",
                "path": "proj-a",
                "content_sha256": fixture_content_digest(&fixture_dir)?,
                "exercises": "synthetic coverage"
            }]
        });
        let parsed: FixtureManifest = serde_json::from_value(manifest)?;
        let outcome = match assert_canonical_fixture_set(&parsed) {
            Ok(()) => "unexpectedly passed".to_string(),
            Err(error) => format!("{error}"),
        };
        expect_error_contains(
            &outcome,
            "from the canonical checked-in set is missing",
            "a synthetic set claimed canonical identity",
        )
    }

    /// A symbolic-link fixture-set root must be rejected before the manifest
    /// is even read, so the whole verification flow cannot be redirected.
    #[cfg(unix)]
    #[test]
    fn symlinked_fixture_set_root_is_rejected() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path().join("real-set");
        std::fs::create_dir(&root)?;
        std::fs::write(root.join("manifest.json"), "{}")?;
        let link = directory.path().join("link-to-set");
        std::os::unix::fs::symlink(&root, &link)?;
        let outcome = match verify_fixture_set(&link) {
            Ok(_) => "unexpectedly passed".to_string(),
            Err(error) => format!("{error}"),
        };
        expect_error_contains(
            &outcome,
            "is a symbolic link",
            "a symlinked fixture-set root was not rejected",
        )
    }

    /// The recipe hashes UTF-8 path bytes, so a non-UTF-8 fixture name must
    /// be rejected instead of being lossily collapsed into the preimage.
    #[cfg(unix)]
    #[test]
    fn non_utf8_fixture_names_are_rejected() -> Result<()> {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let directory = tempdir()?;
        let fixture_dir = directory.path().join("proj-a");
        std::fs::create_dir(&fixture_dir)?;
        let bad_name = OsStr::from_bytes(b"bad\xff.pl");
        std::fs::write(fixture_dir.join(bad_name), "print 1;\n")?;
        let outcome = match fixture_content_digest(&fixture_dir) {
            Ok(_) => "unexpectedly passed".to_string(),
            Err(error) => format!("{error}"),
        };
        expect_error_contains(
            &outcome,
            "non-UTF-8 name",
            "a non-UTF-8 fixture name was not rejected",
        )
    }

    /// On case-insensitive filesystems, manifest path strings can alias one
    /// directory (proj-a vs PROJ-A); canonical-identity dedup must reject it.
    #[cfg(windows)]
    #[test]
    fn case_insensitive_path_aliasing_is_rejected() -> Result<()> {
        let directory = tempdir()?;
        let root = directory.path();
        let fixture_dir = root.join("proj-a");
        std::fs::create_dir(&fixture_dir)?;
        std::fs::write(fixture_dir.join("main.pl"), "print 1;\n")?;
        let digest = fixture_content_digest(&fixture_dir)?;
        write_synth_manifest(
            root,
            serde_json::json!([
                {
                    "fixture_id": "proj-a",
                    "family": "conventional_modules",
                    "path": "proj-a",
                    "content_sha256": digest,
                    "exercises": "synthetic coverage"
                },
                {
                    "fixture_id": "proj-b",
                    "family": "test_heavy",
                    "path": "PROJ-A",
                    "content_sha256": digest,
                    "exercises": "aliased directory"
                }
            ]),
        )?;
        let outcome = verification_error(root);
        expect_error_contains(
            &outcome,
            "resolves to an already-registered fixture directory",
            "case-insensitive aliasing was not rejected",
        )
    }
}
