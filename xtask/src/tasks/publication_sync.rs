//! `cargo xtask publication-sync plan` — validate a `publication_sync_manifest.v1`
//! against its declared inputs and emit a deterministic `pass|blocked|not_proven`
//! receipt (#7972, controller #6356).
//!
//! Planning is read-only. It resolves nothing from Git, mutates no branch or
//! tree, and writes only the receipt. The projection engine, the join, and the
//! landed-wrapper proofs are owned by #7973 and consume the typed model here
//! rather than re-parsing the manifest.
//!
//! The manifest's default projection basis is the complete prepared swarm tree
//! `S`; every intended difference from `S` requires exactly one declared row.

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::sync_divergence::{Verdict, is_product_or_test_path};

const MANIFEST_SCHEMA_VERSION: &str = "publication_sync_manifest.v1";
const RECEIPT_SCHEMA_VERSION: u32 = 1;

/// Release inputs every manifest must bind. A missing id is not provable and a
/// foreign id fails closed at the schema boundary.
const REQUIRED_INPUT_IDS: [InputId; 6] = [
    InputId::Reconciliation,
    InputId::PreparedTopology,
    InputId::ReleaseNoteCatalog,
    InputId::PublishedApiAudit,
    InputId::PublicClaims,
    InputId::ReleaseIntegrity,
];

/// Arguments for the read-only planning pass.
pub struct PlanConfig {
    /// Candidate `publication_sync_manifest.v1` document.
    pub manifest: PathBuf,
    /// Repository root used to resolve the manifest's repository-relative input paths.
    pub repo_root: PathBuf,
    /// Output plan receipt JSON, written even when the verdict blocks promotion.
    pub receipt: PathBuf,
}

// ---------------------------------------------------------------------------
// Typed manifest model
// ---------------------------------------------------------------------------

/// The typed projection manifest. Unknown fields fail closed so a stale or
/// foreign manifest shape cannot masquerade as `publication_sync_manifest.v1`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    schema_version: String,
    release: String,
    track: Track,
    prepared_swarm_sha: String,
    release_base_sha: String,
    default_action: DefaultAction,
    inputs: Vec<ManifestInput>,
    paths: Vec<PathRow>,
    invariants: Vec<Invariant>,
    live_controls: LiveControls,
    expected_projected_tree: String,
    blockers: Vec<Blocker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Track {
    PublicBeta,
    ReleaseCandidate,
    Stable,
}

impl Track {
    fn as_str(self) -> &'static str {
        match self {
            Track::PublicBeta => "public-beta",
            Track::ReleaseCandidate => "release-candidate",
            Track::Stable => "stable",
        }
    }
}

/// The projection basis is always the complete prepared swarm tree; the token
/// exists so a manifest cannot silently adopt a different basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum DefaultAction {
    TakeSwarm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum InputId {
    Reconciliation,
    PreparedTopology,
    ReleaseNoteCatalog,
    PublishedApiAudit,
    PublicClaims,
    ReleaseIntegrity,
}

impl InputId {
    fn as_str(self) -> &'static str {
        match self {
            InputId::Reconciliation => "reconciliation",
            InputId::PreparedTopology => "prepared_topology",
            InputId::ReleaseNoteCatalog => "release_note_catalog",
            InputId::PublishedApiAudit => "published_api_audit",
            InputId::PublicClaims => "public_claims",
            InputId::ReleaseIntegrity => "release_integrity",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestInput {
    id: InputId,
    path: String,
    digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PathRow {
    path: String,
    action: Action,
    class: Class,
    source_digest: Option<String>,
    release_base_digest: Option<String>,
    expected_public_digest: Option<String>,
    reason: String,
    authority_ref: String,
    invalidation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Translate,
    PreserveRelease,
    DropSwarmOnly,
    Regenerate,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Action::Translate => "translate",
            Action::PreserveRelease => "preserve_release",
            Action::DropSwarmOnly => "drop_swarm_only",
            Action::Regenerate => "regenerate",
        }
    }

    /// Actions that withhold prepared swarm content from the publication tree.
    /// These are the operations that can hide product or test work.
    fn withholds_swarm_content(self) -> bool {
        matches!(self, Action::DropSwarmOnly | Action::PreserveRelease)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Class {
    RepositoryContext,
    BranchContext,
    IssueReference,
    PublicClaim,
    ReleaseLineage,
    Governance,
    Generated,
}

impl Class {
    fn as_str(self) -> &'static str {
        match self {
            Class::RepositoryContext => "repository_context",
            Class::BranchContext => "branch_context",
            Class::IssueReference => "issue_reference",
            Class::PublicClaim => "public_claim",
            Class::ReleaseLineage => "release_lineage",
            Class::Governance => "governance",
            Class::Generated => "generated",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Invariant {
    id: String,
    sources: Vec<String>,
    result: Verdict,
    evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveControls {
    branch_rules: LiveControl,
    environments: LiveControl,
    quality_exceptions: LiveControl,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveControl {
    result: LiveResult,
    evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum LiveResult {
    Proven,
    Blocked,
    NotProven,
}

impl LiveResult {
    fn as_str(self) -> &'static str {
        match self {
            LiveResult::Proven => "proven",
            LiveResult::Blocked => "blocked",
            LiveResult::NotProven => "not_proven",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    kind: EvidenceKind,
    reference: String,
}

/// Evidence roles. A checked-in file proves what the repository says, never
/// what the live GitHub control plane currently enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceKind {
    LiveReceipt,
    RepositorySource,
    ReviewRuling,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Blocker {
    code: String,
    message: String,
    owner: String,
}

/// The minimal typed view of a `sync-divergence` reconciliation receipt this
/// planner needs. Unknown fields are tolerated deliberately: `sync_divergence`
/// owns that receipt's shape and may extend it without invalidating currentness
/// here. The fields read below are the ones that carry currentness meaning.
#[derive(Debug, Clone, Deserialize)]
struct ReconciliationReceipt {
    verdict: Verdict,
    subjects: ReconciliationSubjects,
}

#[derive(Debug, Clone, Deserialize)]
struct ReconciliationSubjects {
    source: ReconciliationSubject,
    target: ReconciliationSubject,
}

#[derive(Debug, Clone, Deserialize)]
struct ReconciliationSubject {
    commit: Option<String>,
}

// ---------------------------------------------------------------------------
// Receipt
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: u32,
    manifest: String,
    manifest_digest: String,
    manifest_schema_version: String,
    release: String,
    track: String,
    prepared_swarm_sha: String,
    release_base_sha: String,
    expected_projected_tree: String,
    verdict: Verdict,
    inputs: Vec<ReceiptInput>,
    rows: Vec<ReceiptRow>,
    invariants: Vec<ReceiptInvariant>,
    live_controls: Vec<ReceiptLiveControl>,
    findings: Vec<Finding>,
}

#[derive(Debug, Serialize)]
struct ReceiptInput {
    id: String,
    path: String,
    declared_digest: String,
    observed_digest: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReceiptRow {
    path: String,
    action: String,
    class: String,
    withholds_swarm_content: bool,
}

#[derive(Debug, Serialize)]
struct ReceiptInvariant {
    id: String,
    result: Verdict,
}

#[derive(Debug, Serialize)]
struct ReceiptLiveControl {
    control: String,
    result: String,
}

/// A validation outcome. Findings are accumulated, sorted and deduplicated so a
/// single pass reports every violation deterministically.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct Finding {
    code: String,
    message: String,
    owner: String,
}

/// Accumulates findings and the dominant verdict. `not_proven` dominates
/// `blocked`: an unverifiable manifest cannot be reported as a known hard stop.
#[derive(Debug, Default)]
struct PlanState {
    blocked: bool,
    not_proven: bool,
    findings: Vec<Finding>,
}

impl PlanState {
    fn block(&mut self, code: &str, message: impl Into<String>, owner: &str) {
        self.blocked = true;
        self.push(code, message, owner);
    }

    fn not_proven(&mut self, code: &str, message: impl Into<String>, owner: &str) {
        self.not_proven = true;
        self.push(code, message, owner);
    }

    fn push(&mut self, code: &str, message: impl Into<String>, owner: &str) {
        self.findings.push(Finding {
            code: code.to_string(),
            message: message.into(),
            owner: owner.to_string(),
        });
    }

    fn verdict(&self) -> Verdict {
        if self.not_proven {
            Verdict::NotProven
        } else if self.blocked {
            Verdict::Blocked
        } else {
            Verdict::Pass
        }
    }

    fn finish(mut self) -> (Verdict, Vec<Finding>) {
        let verdict = self.verdict();
        self.findings.sort();
        self.findings.dedup();
        (verdict, self.findings)
    }
}

// ---------------------------------------------------------------------------
// Canonical serialization and digests
// ---------------------------------------------------------------------------

/// Canonical JSON: object keys sorted, no insignificant whitespace, arrays kept
/// in declared order. The digest is therefore insensitive to formatting and key
/// order but sensitive to structure — moving a value between fields or rows
/// changes the digest even when the multiset of values is unchanged.
///
/// This is deliberately not `ci_route_plan::canonical::canonical_json`. That
/// encoder owns the `ci_route_plan.v1` contract, which spells an absent
/// optional as an omitted key and fails closed on any `null`. This manifest
/// spells absence as an explicit `null` — the schema requires every digest key
/// to be present so an author must state "absent from S/R" deliberately rather
/// than leave it unsaid — so the two contracts cannot share one encoder. They
/// agree on the properties they share: sorted keys, declared array order, and
/// no insignificant whitespace. (They are also in different crates: the
/// `ci_route_plan` encoder is `pub(crate)` in the xtask library, and tasks live
/// in the xtask binary.)
fn canonical_json(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            out.push('{');
            for (index, (key, entry)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                let encoded = serde_json::to_string(key).context("encoding canonical JSON key")?;
                out.push_str(&encoded);
                out.push(':');
                canonical_json(entry, out)?;
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical_json(item, out)?;
            }
            out.push(']');
        }
        scalar => {
            let encoded =
                serde_json::to_string(scalar).context("encoding canonical JSON scalar")?;
            out.push_str(&encoded);
        }
    }
    Ok(())
}

/// Deterministic `sha256:` digest over the canonical form of a JSON document.
fn canonical_digest(value: &Value) -> Result<String> {
    let mut canonical = String::new();
    canonical_json(value, &mut canonical)?;
    Ok(sha256_digest(canonical.as_bytes()))
}

/// Lowercase `sha256:<64 hex>` over raw bytes.
fn sha256_digest(raw: &[u8]) -> String {
    let mut rendered = String::from("sha256:");
    for byte in Sha256::digest(raw) {
        // Writing to a String cannot fail; the result is discarded deliberately
        // rather than unwrapped so this stays panic-free.
        let _ = write!(rendered, "{byte:02x}");
    }
    rendered
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Repository-relative POSIX path with no traversal, root anchor, backslash or
/// empty segment. Planning resolves these against the repository root, so a
/// permissive form would let a manifest read outside the checkout.
fn valid_repository_path(path: &str) -> bool {
    if path.is_empty() || path.contains('\\') || path.starts_with('/') || path.ends_with('/') {
        return false;
    }
    path.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// True when `candidate` lies underneath `parent` as a directory prefix.
fn is_path_prefix(parent: &str, candidate: &str) -> bool {
    candidate.len() > parent.len()
        && candidate.starts_with(parent)
        && candidate.as_bytes().get(parent.len()) == Some(&b'/')
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// Validate a candidate manifest and write the plan receipt. Read-only: the
/// only file written is the receipt.
pub fn plan(config: PlanConfig) -> Result<()> {
    let raw = fs::read(&config.manifest)
        .with_context(|| format!("reading manifest {}", config.manifest.display()))?;
    let document: Value = serde_json::from_slice(&raw)
        .with_context(|| format!("parsing manifest {} as JSON", config.manifest.display()))?;
    let manifest: Manifest = serde_json::from_value(document.clone()).with_context(|| {
        format!("parsing manifest {} as {MANIFEST_SCHEMA_VERSION}", config.manifest.display())
    })?;
    let manifest_digest = canonical_digest(&document)?;

    let receipt =
        evaluate(&manifest, &manifest_digest, &config.manifest, &config.repo_root, load_input)?;
    write_receipt(&config.receipt, &receipt)?;

    match receipt.verdict {
        Verdict::Pass => {
            println!(
                "publication-sync: plan pass for release {} ({}); manifest digest {}",
                receipt.release, receipt.track, receipt.manifest_digest
            );
            println!("publication-sync: receipt {}", config.receipt.display());
            Ok(())
        }
        Verdict::Blocked => bail!(
            "publication-sync: plan blocked; see {}",
            display_findings(&receipt, &config.receipt)
        ),
        Verdict::NotProven => bail!(
            "publication-sync: plan not proven; see {}",
            display_findings(&receipt, &config.receipt)
        ),
    }
}

fn display_findings(receipt: &Receipt, path: &Path) -> String {
    let mut rendered = format!("{} ({} finding(s))", path.display(), receipt.findings.len());
    for finding in &receipt.findings {
        let _ = write!(
            rendered,
            "\n  - [{}] {} (owner: {})",
            finding.code, finding.message, finding.owner
        );
    }
    rendered
}

/// Reads one declared input from disk. Injected so the evaluation core stays
/// deterministic and testable without a repository fixture tree.
type InputLoader = fn(&Path, &str) -> Option<Vec<u8>>;

fn load_input(repo_root: &Path, path: &str) -> Option<Vec<u8>> {
    fs::read(repo_root.join(path)).ok()
}

fn evaluate(
    manifest: &Manifest,
    manifest_digest: &str,
    manifest_path: &Path,
    repo_root: &Path,
    loader: InputLoader,
) -> Result<Receipt> {
    let mut state = PlanState::default();

    validate_identity(manifest, &mut state);
    let inputs = validate_inputs(manifest, repo_root, loader, &mut state);
    validate_rows(manifest, &mut state);
    validate_invariants(manifest, &mut state);
    validate_live_controls(manifest, &mut state);
    validate_declared_blockers(manifest, &mut state);

    let mut rows: Vec<ReceiptRow> = manifest
        .paths
        .iter()
        .map(|row| ReceiptRow {
            path: row.path.clone(),
            action: row.action.as_str().to_string(),
            class: row.class.as_str().to_string(),
            withholds_swarm_content: row.action.withholds_swarm_content(),
        })
        .collect();
    rows.sort_by(|left, right| left.path.cmp(&right.path));

    let mut invariants: Vec<ReceiptInvariant> = manifest
        .invariants
        .iter()
        .map(|invariant| ReceiptInvariant { id: invariant.id.clone(), result: invariant.result })
        .collect();
    invariants.sort_by(|left, right| left.id.cmp(&right.id));

    let mut live_controls = vec![
        ReceiptLiveControl {
            control: "branch_rules".to_string(),
            result: manifest.live_controls.branch_rules.result.as_str().to_string(),
        },
        ReceiptLiveControl {
            control: "environments".to_string(),
            result: manifest.live_controls.environments.result.as_str().to_string(),
        },
        ReceiptLiveControl {
            control: "quality_exceptions".to_string(),
            result: manifest.live_controls.quality_exceptions.result.as_str().to_string(),
        },
    ];
    live_controls.sort_by(|left, right| left.control.cmp(&right.control));

    let (verdict, findings) = state.finish();

    Ok(Receipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        manifest: manifest_path.display().to_string(),
        manifest_digest: manifest_digest.to_string(),
        manifest_schema_version: manifest.schema_version.clone(),
        release: manifest.release.clone(),
        track: manifest.track.as_str().to_string(),
        prepared_swarm_sha: manifest.prepared_swarm_sha.clone(),
        release_base_sha: manifest.release_base_sha.clone(),
        expected_projected_tree: manifest.expected_projected_tree.clone(),
        verdict,
        inputs,
        rows,
        invariants,
        live_controls,
        findings,
    })
}

fn validate_identity(manifest: &Manifest, state: &mut PlanState) {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        state.not_proven(
            "manifest_schema_version_unknown",
            format!(
                "manifest declares schema_version {} but this planner owns {MANIFEST_SCHEMA_VERSION}",
                manifest.schema_version
            ),
            "release/ci",
        );
    }
    if manifest.default_action != DefaultAction::TakeSwarm {
        state.not_proven(
            "manifest_default_action_unknown",
            "the projection basis must be the complete prepared swarm tree",
            "release/ci",
        );
    }
    for (field, value) in [
        ("prepared_swarm_sha", &manifest.prepared_swarm_sha),
        ("release_base_sha", &manifest.release_base_sha),
        ("expected_projected_tree", &manifest.expected_projected_tree),
    ] {
        if !is_object_name(value) {
            state.not_proven(
                "manifest_identity_malformed",
                format!("{field} is not a 40-character lowercase hex object name"),
                "release/ci",
            );
        }
    }
    if manifest.prepared_swarm_sha == manifest.release_base_sha {
        state.not_proven(
            "manifest_identity_degenerate",
            "prepared_swarm_sha and release_base_sha name the same commit",
            "release/ci",
        );
    }
    if manifest.release.trim().is_empty() {
        state.not_proven("manifest_release_missing", "release is empty", "release/ci");
    }
}

fn is_object_name(value: &str) -> bool {
    value.len() == 40
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_sha256_digest(value: &str) -> bool {
    match value.strip_prefix("sha256:") {
        Some(hex) => {
            hex.len() == 64
                && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }
        None => false,
    }
}

fn validate_inputs(
    manifest: &Manifest,
    repo_root: &Path,
    loader: InputLoader,
    state: &mut PlanState,
) -> Vec<ReceiptInput> {
    let mut seen: BTreeSet<InputId> = BTreeSet::new();
    let mut receipt_inputs: Vec<ReceiptInput> = Vec::new();
    let mut reconciliation_bytes: Option<Vec<u8>> = None;

    for input in &manifest.inputs {
        if !seen.insert(input.id) {
            state.not_proven(
                "input_duplicate",
                format!("release input {} is declared more than once", input.id.as_str()),
                "release/ci",
            );
        }
        if !valid_repository_path(&input.path) {
            state.not_proven(
                "input_path_invalid",
                format!(
                    "release input {} path {:?} is not a repository-relative POSIX path",
                    input.id.as_str(),
                    input.path
                ),
                "release/ci",
            );
            receipt_inputs.push(ReceiptInput {
                id: input.id.as_str().to_string(),
                path: input.path.clone(),
                declared_digest: input.digest.clone(),
                observed_digest: None,
            });
            continue;
        }
        if !is_sha256_digest(&input.digest) {
            state.not_proven(
                "input_digest_malformed",
                format!("release input {} declares a malformed digest", input.id.as_str()),
                "release/ci",
            );
        }

        let observed = match loader(repo_root, &input.path) {
            Some(bytes) => {
                let digest = sha256_digest(&bytes);
                if digest != input.digest {
                    state.not_proven(
                        "input_digest_mismatch",
                        format!(
                            "release input {} at {} hashes to {digest} but the manifest declares {}",
                            input.id.as_str(),
                            input.path,
                            input.digest
                        ),
                        "release/ci",
                    );
                }
                if input.id == InputId::Reconciliation {
                    reconciliation_bytes = Some(bytes);
                }
                Some(digest)
            }
            None => {
                state.not_proven(
                    "input_missing",
                    format!(
                        "release input {} is declared at {} but no such file exists under the repository root",
                        input.id.as_str(),
                        input.path
                    ),
                    "release/ci",
                );
                None
            }
        };

        receipt_inputs.push(ReceiptInput {
            id: input.id.as_str().to_string(),
            path: input.path.clone(),
            declared_digest: input.digest.clone(),
            observed_digest: observed,
        });
    }

    for required in REQUIRED_INPUT_IDS {
        if !seen.contains(&required) {
            state.not_proven(
                "input_required_missing",
                format!("required release input {} is not declared", required.as_str()),
                "release/ci",
            );
        }
    }

    validate_reconciliation(manifest, reconciliation_bytes.as_deref(), state);

    receipt_inputs.sort_by(|left, right| left.id.cmp(&right.id));
    receipt_inputs
}

/// The reconciliation input is a `sync-divergence` receipt. It is current only
/// when it passed and when it reconciled exactly this manifest's `S` against
/// exactly this manifest's `R`.
fn validate_reconciliation(manifest: &Manifest, raw: Option<&[u8]>, state: &mut PlanState) {
    let Some(raw) = raw else {
        // A missing or unreadable reconciliation input is already reported by
        // `validate_inputs`; do not double-report it as staleness.
        return;
    };
    let receipt: ReconciliationReceipt = match serde_json::from_slice(raw) {
        Ok(receipt) => receipt,
        Err(error) => {
            state.not_proven(
                "reconciliation_unreadable",
                format!("the reconciliation input is not a sync-divergence receipt: {error}"),
                "release/ci",
            );
            return;
        }
    };

    match receipt.verdict {
        Verdict::Pass => {}
        Verdict::Blocked => state.block(
            "reconciliation_not_passing",
            "the declared reconciliation receipt is blocked; projection may not consume it",
            "release/ci",
        ),
        Verdict::NotProven => state.not_proven(
            "reconciliation_not_passing",
            "the declared reconciliation receipt is not proven; projection may not consume it",
            "release/ci",
        ),
    }

    let source = receipt.subjects.source.commit.as_deref();
    let target = receipt.subjects.target.commit.as_deref();
    if source != Some(manifest.prepared_swarm_sha.as_str()) {
        state.block(
            "reconciliation_stale",
            format!(
                "the reconciliation receipt reconciled source {} but this manifest projects prepared_swarm_sha {}",
                source.unwrap_or("<unresolved>"),
                manifest.prepared_swarm_sha
            ),
            "release/ci",
        );
    }
    if target != Some(manifest.release_base_sha.as_str()) {
        state.block(
            "reconciliation_stale",
            format!(
                "the reconciliation receipt reconciled target {} but this manifest projects release_base_sha {}",
                target.unwrap_or("<unresolved>"),
                manifest.release_base_sha
            ),
            "release/ci",
        );
    }
}

fn validate_rows(manifest: &Manifest, state: &mut PlanState) {
    let declared_inputs: BTreeSet<&str> =
        manifest.inputs.iter().map(|input| input.id.as_str()).collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();

    for row in &manifest.paths {
        if !valid_repository_path(&row.path) {
            state.not_proven(
                "row_path_invalid",
                format!("row path {:?} is not a repository-relative POSIX path", row.path),
                "release/ci",
            );
            continue;
        }
        if !seen.insert(row.path.as_str()) {
            state.not_proven(
                "row_duplicate_path",
                format!("path {} carries more than one projection row", row.path),
                "release/ci",
            );
        }

        validate_row_digests(row, state);
        validate_row_authority(row, &declared_inputs, state);

        if row.action.withholds_swarm_content() && is_product_or_test_path(&row.path) {
            state.block(
                "row_product_bearing_exclusion",
                format!(
                    "row {} uses {} on a product- or test-bearing path; publication projection may not withhold product work",
                    row.path,
                    row.action.as_str()
                ),
                "release/ci",
            );
        }

        if row.class == Class::ReleaseLineage && is_product_or_test_path(&row.path) {
            state.block(
                "row_product_bearing_exclusion",
                format!(
                    "row {} is classified release_lineage on a product- or test-bearing path",
                    row.path
                ),
                "release/ci",
            );
        }
    }

    // Parent/child ambiguity: two rows may not claim overlapping authority over
    // the same subtree, because the projection order between them is undefined.
    let mut sorted: Vec<&str> = seen.iter().copied().collect();
    sorted.sort_unstable();
    for (index, parent) in sorted.iter().enumerate() {
        for child in sorted.iter().skip(index + 1) {
            if is_path_prefix(parent, child) {
                state.not_proven(
                    "row_path_ambiguous",
                    format!("row {parent} and row {child} claim overlapping projection authority"),
                    "release/ci",
                );
            }
        }
    }
}

fn validate_row_digests(row: &PathRow, state: &mut PlanState) {
    for (field, digest) in [
        ("source_digest", row.source_digest.as_deref()),
        ("release_base_digest", row.release_base_digest.as_deref()),
        ("expected_public_digest", row.expected_public_digest.as_deref()),
    ] {
        if let Some(value) = digest
            && !is_sha256_digest(value)
        {
            state.not_proven(
                "row_digest_malformed",
                format!("row {} declares a malformed {field}", row.path),
                "release/ci",
            );
        }
    }

    match row.action {
        Action::DropSwarmOnly => {
            if row.expected_public_digest.is_some() {
                state.not_proven(
                    "row_expected_digest_inconsistent",
                    format!(
                        "row {} drops the path from publication but still declares expected_public_digest",
                        row.path
                    ),
                    "release/ci",
                );
            }
            if row.source_digest.is_none() {
                state.not_proven(
                    "row_source_digest_missing",
                    format!("row {} drops a path that it does not prove exists in S", row.path),
                    "release/ci",
                );
            }
        }
        Action::PreserveRelease => {
            if row.expected_public_digest.is_none() {
                state.not_proven(
                    "row_expected_digest_missing",
                    format!("row {} declares no expected_public_digest", row.path),
                    "release/ci",
                );
            }
            if row.release_base_digest.is_none() {
                state.not_proven(
                    "row_release_base_digest_missing",
                    format!(
                        "row {} preserves release content but does not prove it exists in R",
                        row.path
                    ),
                    "release/ci",
                );
            }
            if row.expected_public_digest.is_some()
                && row.expected_public_digest != row.release_base_digest
            {
                state.not_proven(
                    "row_preserve_release_diverges",
                    format!(
                        "row {} preserves release content but projects a digest other than R's",
                        row.path
                    ),
                    "release/ci",
                );
            }
        }
        Action::Translate | Action::Regenerate => {
            if row.expected_public_digest.is_none() {
                state.not_proven(
                    "row_expected_digest_missing",
                    format!("row {} declares no expected_public_digest", row.path),
                    "release/ci",
                );
            }
            if row.source_digest.is_none() {
                state.not_proven(
                    "row_source_digest_missing",
                    format!(
                        "row {} {}s content it does not prove exists in S",
                        row.path,
                        row.action.as_str()
                    ),
                    "release/ci",
                );
            }
            if row.action == Action::Translate
                && row.source_digest.is_some()
                && row.source_digest == row.expected_public_digest
            {
                state.not_proven(
                    "row_translation_is_identity",
                    format!(
                        "row {} declares a translation whose expected output equals its source",
                        row.path
                    ),
                    "release/ci",
                );
            }
        }
    }
}

/// `authority_ref` must resolve to something a reviewer can open: a declared
/// release input, an issue reference, or a repository-relative document.
fn validate_row_authority(row: &PathRow, declared_inputs: &BTreeSet<&str>, state: &mut PlanState) {
    let reference = row.authority_ref.trim();
    if reference.is_empty() {
        state.not_proven(
            "row_authority_missing",
            format!("row {} declares no authority", row.path),
            "release/ci",
        );
        return;
    }
    if declared_inputs.contains(reference) {
        return;
    }
    if let Some(number) = reference.strip_prefix('#')
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return;
    }
    if valid_repository_path(reference) && reference.contains('/') {
        return;
    }
    state.not_proven(
        "row_authority_unresolved",
        format!(
            "row {} names authority {:?}, which is neither a declared release input, an issue reference, nor a repository-relative document",
            row.path, row.authority_ref
        ),
        "release/ci",
    );
}

fn validate_invariants(manifest: &Manifest, state: &mut PlanState) {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for invariant in &manifest.invariants {
        if !seen.insert(invariant.id.as_str()) {
            state.not_proven(
                "invariant_duplicate",
                format!("invariant {} is declared more than once", invariant.id),
                "release/ci",
            );
        }
        match invariant.result {
            Verdict::Pass => {
                if invariant.evidence.is_empty() {
                    state.not_proven(
                        "invariant_unevidenced_pass",
                        format!("invariant {} claims pass with no evidence", invariant.id),
                        "release/ci",
                    );
                }
            }
            Verdict::Blocked => state.block(
                "invariant_blocked",
                format!("required invariant {} is blocked", invariant.id),
                "release/ci",
            ),
            Verdict::NotProven => state.not_proven(
                "invariant_not_proven",
                format!("required invariant {} is not proven", invariant.id),
                "release/ci",
            ),
        }
    }
}

fn validate_live_controls(manifest: &Manifest, state: &mut PlanState) {
    for (name, control) in [
        ("branch_rules", &manifest.live_controls.branch_rules),
        ("environments", &manifest.live_controls.environments),
        ("quality_exceptions", &manifest.live_controls.quality_exceptions),
    ] {
        match control.result {
            LiveResult::Proven => {
                let live = control
                    .evidence
                    .iter()
                    .any(|evidence| evidence.kind == EvidenceKind::LiveReceipt);
                if !live {
                    state.not_proven(
                        "live_control_source_only",
                        format!(
                            "live control {name} claims proven without a live receipt; checked-in policy is not live enforcement proof"
                        ),
                        "release/ci",
                    );
                }
            }
            LiveResult::Blocked => state.block(
                "live_control_blocked",
                format!("live control {name} is blocked"),
                "release/ci",
            ),
            LiveResult::NotProven => state.not_proven(
                "live_control_not_proven",
                format!("live control {name} is not proven"),
                "release/ci",
            ),
        }
    }
}

fn validate_declared_blockers(manifest: &Manifest, state: &mut PlanState) {
    for blocker in &manifest.blockers {
        state.block(
            "manifest_declared_blocker",
            format!("[{}] {}", blocker.code, blocker.message),
            &blocker.owner,
        );
    }
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating receipt directory {}", parent.display()))?;
    }
    let content =
        serde_json::to_string_pretty(receipt).context("serializing publication-sync receipt")?;
    fs::write(path, format!("{content}\n"))
        .with_context(|| format!("writing publication-sync receipt {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests;
