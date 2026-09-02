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

use super::file_policy::{self, AllowEntry};
use super::sync_divergence::{Verdict, is_product_or_test_path};

const MANIFEST_SCHEMA_VERSION: &str = "publication_sync_manifest.v1";
const RECEIPT_SCHEMA_VERSION: u32 = 1;

/// The `sync-divergence` receipt version this planner consumes. Pinned so a
/// receipt from another version cannot silently authorize a projection.
const RECONCILIATION_RECEIPT_SCHEMA_VERSION: u32 = 2;

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

    /// Actions whose published bytes do not derive from the prepared swarm
    /// content at that path: the path is dropped, replaced by the release
    /// base's version, or regenerated from elsewhere. These are the operations
    /// that can hide product or test work.
    ///
    /// `translate` is deliberately excluded. A translation still derives from
    /// `S` at that path — #6356 contemplates translating "source comments that
    /// ship through installers, extension or artifacts" — so it is constrained
    /// by class instead of forbidden outright.
    fn displaces_swarm_content(self) -> bool {
        matches!(self, Action::DropSwarmOnly | Action::PreserveRelease | Action::Regenerate)
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
    id: InvariantId,
    sources: Vec<InputId>,
    result: Verdict,
    evidence: Vec<Evidence>,
}

/// The cross-file invariants #6356 requires a projection to settle. The set is
/// closed and exhaustively required: an invented identifier cannot stand in for
/// a required one, and omitting one cannot be hidden by declaring another twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum InvariantId {
    ArtifactReachability,
    EffectiveVersionIdentity,
    PublicClaimStrength,
    GovernanceTimeState,
    ReleaseLineageCompleteness,
}

impl InvariantId {
    fn as_str(self) -> &'static str {
        match self {
            InvariantId::ArtifactReachability => "artifact_reachability",
            InvariantId::EffectiveVersionIdentity => "effective_version_identity",
            InvariantId::PublicClaimStrength => "public_claim_strength",
            InvariantId::GovernanceTimeState => "governance_time_state",
            InvariantId::ReleaseLineageCompleteness => "release_lineage_completeness",
        }
    }
}

/// Every invariant #6356 names. A manifest must settle all of them.
const REQUIRED_INVARIANT_IDS: [InvariantId; 5] = [
    InvariantId::ArtifactReachability,
    InvariantId::EffectiveVersionIdentity,
    InvariantId::PublicClaimStrength,
    InvariantId::GovernanceTimeState,
    InvariantId::ReleaseLineageCompleteness,
];

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
    /// Required for `live_receipt`, which the planner resolves and hashes; the
    /// label alone never proves a live control. `null` for the other roles,
    /// whose references are documents and rulings, not observed artifacts.
    digest: Option<String>,
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

impl EvidenceKind {
    fn as_str(self) -> &'static str {
        match self {
            EvidenceKind::LiveReceipt => "live_receipt",
            EvidenceKind::RepositorySource => "repository_source",
            EvidenceKind::ReviewRuling => "review_ruling",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Blocker {
    code: String,
    message: String,
    owner: String,
}

/// The minimal typed view of a `sync-divergence` reconciliation receipt this
/// planner needs. Unknown fields are tolerated deliberately — `sync_divergence`
/// owns that receipt's shape and may add fields within a version — but
/// `schema_version` is read and pinned, so a receipt from another version (or
/// another producer that happens to carry a `verdict` and `subjects`) cannot
/// authorize a plan.
#[derive(Debug, Clone, Deserialize)]
struct ReconciliationReceipt {
    schema_version: u32,
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

/// The plan receipt. It deliberately carries no local filesystem path: the
/// manifest's identity is its canonical digest, so two runs over the same
/// manifest reached through different absolute or relative paths produce
/// byte-identical receipts.
///
/// The identity fields are optional because a manifest that fails to parse or
/// to validate against the published schema still gets a machine-readable
/// `not_proven` receipt naming why, rather than only a process exit code.
#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: u32,
    manifest_digest: Option<String>,
    manifest_schema_version: Option<String>,
    release: Option<String>,
    track: Option<String>,
    prepared_swarm_sha: Option<String>,
    release_base_sha: Option<String>,
    expected_projected_tree: Option<String>,
    verdict: Verdict,
    inputs: Vec<ReceiptInput>,
    rows: Vec<ReceiptRow>,
    invariants: Vec<ReceiptInvariant>,
    live_controls: Vec<ReceiptLiveControl>,
    findings: Vec<Finding>,
}

impl Receipt {
    /// A receipt for a manifest that never reached evaluation.
    fn unevaluated(manifest_digest: Option<String>, finding: Finding) -> Self {
        Self {
            schema_version: RECEIPT_SCHEMA_VERSION,
            manifest_digest,
            manifest_schema_version: None,
            release: None,
            track: None,
            prepared_swarm_sha: None,
            release_base_sha: None,
            expected_projected_tree: None,
            verdict: Verdict::NotProven,
            inputs: Vec::new(),
            rows: Vec::new(),
            invariants: Vec::new(),
            live_controls: Vec::new(),
            findings: vec![finding],
        }
    }
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
    displaces_swarm_content: bool,
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
// Product surface authority
// ---------------------------------------------------------------------------

/// Which paths carry product or test work, and may therefore never be withheld
/// from publication.
///
/// `sync_divergence::is_product_or_test_path` recognizes source code by segment
/// and extension. That is a floor, not the whole surface: shipped product also
/// includes non-Rust editor clients and packaging metadata such as
/// `clients/sublime/LSP-perllsp/plugin.py` and `clients/lite-xl/compose.lua`,
/// which no source-extension heuristic can see. The repository already
/// classifies those files in `policy/non-rust-allowlist.toml`, so that ledger is
/// consulted as the authority rather than growing a second hand-maintained list
/// here.
struct ProductSurface {
    entries: Vec<AllowEntry>,
    /// `false` when the ledger could not be read, in which case exclusions
    /// cannot be checked and every withholding row is `not_proven`.
    available: bool,
}

impl ProductSurface {
    fn load(repo_root: &Path, state: &mut PlanState) -> Self {
        match file_policy::load_allowlist(repo_root) {
            Ok(allowlist) => Self {
                entries: allowlist
                    .allow
                    .into_iter()
                    .filter(|entry| !entry.retired)
                    .filter(|entry| {
                        entry.classification == "production" || entry.classification == "test"
                    })
                    .collect(),
                available: true,
            },
            Err(error) => {
                state.not_proven(
                    "product_surface_unavailable",
                    format!(
                        "policy/non-rust-allowlist.toml could not be read, so publication exclusions cannot be checked against the product surface: {error}"
                    ),
                    "release/ci",
                );
                Self { entries: Vec::new(), available: false }
            }
        }
    }

    /// Build a surface from explicit `(path, classification)` pairs. Tests use
    /// this so a fixture's exclusion rules do not depend on the live allowlist
    /// evolving underneath them.
    #[cfg(test)]
    fn from_entries_for_test(rows: Vec<(&str, &str)>) -> Self {
        Self {
            entries: rows
                .into_iter()
                .map(|(path, classification)| AllowEntry {
                    id: path.to_string(),
                    glob: None,
                    path: Some(path.to_string()),
                    kind: String::new(),
                    language: String::new(),
                    surface: String::new(),
                    classification: classification.to_string(),
                    owner: String::new(),
                    reason: String::new(),
                    covered_by: Vec::new(),
                    created: String::new(),
                    review_after: String::new(),
                    expires: None,
                    broad_glob_reason: None,
                    retired: false,
                })
                .collect(),
            available: true,
        }
    }

    /// True when the repository treats `path` as product or test work.
    ///
    /// Three sources, because no single one is complete:
    ///
    /// 1. `is_product_or_test_path` recognizes source code by segment and
    ///    extension;
    /// 2. the non-Rust ledger classifies shipped non-source product such as
    ///    editor clients and packaging metadata;
    /// 3. Rust-family build manifests, which neither of the above can see —
    ///    the extension heuristic has no `toml` rule, and the non-Rust ledger
    ///    structurally excludes Rust-family files. `Cargo.toml` and
    ///    `Cargo.lock` define the product's crates and pinned dependencies, so
    ///    displacing them from publication changes what is built.
    fn bears_product_or_test(&self, path: &str) -> bool {
        if is_product_or_test_path(path) || is_rust_build_manifest(path) {
            return true;
        }
        self.entries.iter().any(|entry| file_policy::entry_matches_path(entry, path))
    }
}

/// Rust-family build and toolchain manifests at any depth.
fn is_rust_build_manifest(path: &str) -> bool {
    matches!(
        path.rsplit('/').next().unwrap_or(path),
        "Cargo.toml" | "Cargo.lock" | "rust-toolchain.toml" | "rust-toolchain"
    )
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// The published contract, compiled in so the command enforces exactly the
/// schema this repository ships rather than a Rust approximation of it.
const MANIFEST_SCHEMA: &str =
    include_str!("../../../schemas/publication_sync_manifest.v1.schema.json");

/// Validate a candidate manifest and write the plan receipt. Read-only: the
/// only file written is the receipt.
pub fn plan(config: PlanConfig) -> Result<()> {
    let raw = fs::read(&config.manifest)
        .with_context(|| format!("reading manifest {}", config.manifest.display()))?;

    let receipt = match build_receipt(&raw, &config.repo_root) {
        Ok(receipt) => receipt,
        Err(failure) => Receipt::unevaluated(failure.manifest_digest, failure.finding),
    };
    write_receipt(&config.receipt, &receipt)?;

    match receipt.verdict {
        Verdict::Pass => {
            println!(
                "publication-sync: plan pass for release {} ({}); manifest digest {}",
                receipt.release.as_deref().unwrap_or("<unknown>"),
                receipt.track.as_deref().unwrap_or("<unknown>"),
                receipt.manifest_digest.as_deref().unwrap_or("<unknown>")
            );
            println!("publication-sync: manifest {}", config.manifest.display());
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

/// A manifest that never reached evaluation, with whatever identity could still
/// be established.
struct UnevaluatedManifest {
    manifest_digest: Option<String>,
    finding: Finding,
}

fn finding(code: &str, message: impl Into<String>) -> Finding {
    Finding { code: code.to_string(), message: message.into(), owner: "release/ci".to_string() }
}

/// Parse, schema-validate, then evaluate. The published JSON Schema is the
/// admission boundary: serde alone would accept documents the contract forbids,
/// because an `Option` field tolerates an omitted key and a `Vec` tolerates an
/// empty array where the schema requires the key and a minimum length.
fn build_receipt(raw: &[u8], repo_root: &Path) -> Result<Receipt, UnevaluatedManifest> {
    let document: Value = serde_json::from_slice(raw).map_err(|error| UnevaluatedManifest {
        manifest_digest: None,
        finding: finding("manifest_unparsable", format!("the manifest is not JSON: {error}")),
    })?;

    let manifest_digest = canonical_digest(&document).ok();

    let schema: Value =
        serde_json::from_str(MANIFEST_SCHEMA).map_err(|error| UnevaluatedManifest {
            manifest_digest: manifest_digest.clone(),
            finding: finding(
                "manifest_schema_unreadable",
                format!("the published manifest schema is not JSON: {error}"),
            ),
        })?;
    let validator = jsonschema::validator_for(&schema).map_err(|error| UnevaluatedManifest {
        manifest_digest: manifest_digest.clone(),
        finding: finding(
            "manifest_schema_unreadable",
            format!("the published manifest schema does not compile: {error}"),
        ),
    })?;

    let mut violations: Vec<String> = validator
        .iter_errors(&document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    if !violations.is_empty() {
        violations.sort();
        violations.dedup();
        return Err(UnevaluatedManifest {
            manifest_digest,
            finding: finding(
                "manifest_schema_violation",
                format!(
                    "the manifest violates {MANIFEST_SCHEMA_VERSION}: {}",
                    violations.join("; ")
                ),
            ),
        });
    }

    let manifest: Manifest =
        serde_json::from_value(document).map_err(|error| UnevaluatedManifest {
            manifest_digest: manifest_digest.clone(),
            finding: finding(
                "manifest_model_violation",
                format!("the manifest does not load as {MANIFEST_SCHEMA_VERSION}: {error}"),
            ),
        })?;

    let digest = manifest_digest.clone().unwrap_or_default();
    evaluate(&manifest, &digest, repo_root, load_input).map_err(|error| UnevaluatedManifest {
        manifest_digest,
        finding: finding("plan_failed", format!("planning could not complete: {error}")),
    })
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

/// Why a declared repository artifact could not be read. Distinguishing these
/// matters: "the file is not there" and "the file is there but escapes the
/// checkout" need different remediation, and collapsing both into "missing"
/// hides the second.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LoadFailure {
    Missing,
    Escapes,
    Unreadable(String),
}

/// Reads one declared repository artifact. Injected so the evaluation core
/// stays deterministic and testable without a repository fixture tree.
type InputLoader = fn(&Path, &str) -> Result<Vec<u8>, LoadFailure>;

/// Read a repository-relative artifact, refusing anything that resolves outside
/// the checkout. `valid_repository_path` already rejects lexical traversal, but
/// a symlink inside the tree can still point outside it, so confinement is
/// checked against the resolved location rather than the declared one.
fn load_input(repo_root: &Path, path: &str) -> Result<Vec<u8>, LoadFailure> {
    let candidate = repo_root.join(path);
    let resolved = match candidate.canonicalize() {
        Ok(resolved) => resolved,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(LoadFailure::Missing);
        }
        Err(error) => return Err(LoadFailure::Unreadable(error.to_string())),
    };
    // A repository root that cannot itself be resolved gives no confinement
    // boundary to check against, so refuse rather than read.
    let root =
        repo_root.canonicalize().map_err(|error| LoadFailure::Unreadable(error.to_string()))?;
    if !resolved.starts_with(&root) {
        return Err(LoadFailure::Escapes);
    }
    if !resolved.is_file() {
        return Err(LoadFailure::Unreadable("not a regular file".to_string()));
    }
    fs::read(&resolved).map_err(|error| LoadFailure::Unreadable(error.to_string()))
}

fn evaluate(
    manifest: &Manifest,
    manifest_digest: &str,
    repo_root: &Path,
    loader: InputLoader,
) -> Result<Receipt> {
    let mut probe = PlanState::default();
    let product_surface = ProductSurface::load(repo_root, &mut probe);
    let mut receipt =
        evaluate_with_surface(manifest, manifest_digest, repo_root, loader, &product_surface)?;
    // Surface-loading findings are raised before evaluation, so fold them in.
    let (_, findings) = probe.finish();
    if !findings.is_empty() {
        receipt.verdict = Verdict::NotProven;
        receipt.findings.extend(findings);
        receipt.findings.sort();
        receipt.findings.dedup();
    }
    Ok(receipt)
}

fn evaluate_with_surface(
    manifest: &Manifest,
    manifest_digest: &str,
    repo_root: &Path,
    loader: InputLoader,
    product_surface: &ProductSurface,
) -> Result<Receipt> {
    let mut state = PlanState::default();

    validate_identity(manifest, &mut state);
    let inputs = validate_inputs(manifest, repo_root, loader, &mut state);
    validate_rows(manifest, repo_root, loader, product_surface, &mut state);
    validate_invariants(manifest, repo_root, loader, &mut state);
    validate_live_controls(manifest, repo_root, loader, &mut state);
    validate_declared_blockers(manifest, &mut state);

    let mut rows: Vec<ReceiptRow> = manifest
        .paths
        .iter()
        .map(|row| ReceiptRow {
            path: row.path.clone(),
            action: row.action.as_str().to_string(),
            class: row.class.as_str().to_string(),
            displaces_swarm_content: row.action.displaces_swarm_content(),
        })
        .collect();
    rows.sort_by(|left, right| left.path.cmp(&right.path));

    let mut invariants: Vec<ReceiptInvariant> = manifest
        .invariants
        .iter()
        .map(|invariant| ReceiptInvariant {
            id: invariant.id.as_str().to_string(),
            result: invariant.result,
        })
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
        manifest_digest: Some(manifest_digest.to_string()),
        manifest_schema_version: Some(manifest.schema_version.clone()),
        release: Some(manifest.release.clone()),
        track: Some(manifest.track.as_str().to_string()),
        prepared_swarm_sha: Some(manifest.prepared_swarm_sha.clone()),
        release_base_sha: Some(manifest.release_base_sha.clone()),
        expected_projected_tree: Some(manifest.expected_projected_tree.clone()),
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
            Ok(bytes) => {
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
            Err(failure) => {
                report_load_failure(
                    state,
                    "input",
                    &format!("release input {}", input.id.as_str()),
                    &input.path,
                    &failure,
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
/// Report one unreadable repository artifact, keeping "absent", "escapes the
/// checkout" and "present but unreadable" distinguishable in the receipt.
fn report_load_failure(
    state: &mut PlanState,
    prefix: &str,
    subject: &str,
    path: &str,
    failure: &LoadFailure,
) {
    match failure {
        LoadFailure::Missing => state.not_proven(
            &format!("{prefix}_missing"),
            format!(
                "{subject} is declared at {path} but no such file exists under the repository root"
            ),
            "release/ci",
        ),
        LoadFailure::Escapes => state.not_proven(
            &format!("{prefix}_escapes_repository"),
            format!("{subject} at {path} resolves outside the repository root"),
            "release/ci",
        ),
        LoadFailure::Unreadable(error) => state.not_proven(
            &format!("{prefix}_unreadable"),
            format!("{subject} at {path} could not be read: {error}"),
            "release/ci",
        ),
    }
}

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

    if receipt.schema_version != RECONCILIATION_RECEIPT_SCHEMA_VERSION {
        state.not_proven(
            "reconciliation_schema_version_unknown",
            format!(
                "the reconciliation input declares schema_version {} but this planner consumes sync-divergence receipt v{RECONCILIATION_RECEIPT_SCHEMA_VERSION}",
                receipt.schema_version
            ),
            "release/ci",
        );
        return;
    }

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

fn validate_rows(
    manifest: &Manifest,
    repo_root: &Path,
    loader: InputLoader,
    product_surface: &ProductSurface,
    state: &mut PlanState,
) {
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
        validate_row_authority(row, &declared_inputs, repo_root, loader, state);

        let displaces = row.action.displaces_swarm_content() || row.class == Class::ReleaseLineage;
        let product_bearing = product_surface.bears_product_or_test(&row.path);

        if displaces && product_bearing {
            state.block(
                "row_product_bearing_exclusion",
                format!(
                    "row {} uses {} / {} on a path the repository classifies as product or test work; publication projection may not displace it",
                    row.path,
                    row.action.as_str(),
                    row.class.as_str()
                ),
                "release/ci",
            );
        } else if displaces && !product_surface.available {
            state.not_proven(
                "row_product_bearing_unverifiable",
                format!(
                    "row {} displaces swarm content but the product surface could not be consulted",
                    row.path
                ),
                "release/ci",
            );
        }

        // A translation may touch product code, but only to repair destination
        // context (a repository URL, a branch name, an issue reference) inside
        // it. Any other class on product code is a substantive product change
        // presented as publication-only.
        if row.action == Action::Translate
            && product_bearing
            && !matches!(
                row.class,
                Class::RepositoryContext | Class::BranchContext | Class::IssueReference
            )
        {
            state.block(
                "row_product_translation_class_invalid",
                format!(
                    "row {} translates product or test work under class {}; only destination-context classes may translate product code",
                    row.path,
                    row.class.as_str()
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
fn validate_row_authority(
    row: &PathRow,
    declared_inputs: &BTreeSet<&str>,
    repo_root: &Path,
    loader: InputLoader,
    state: &mut PlanState,
) {
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
        // A document authority is only an authority if a reviewer can open it,
        // so the reference is resolved rather than merely shaped.
        if let Err(failure) = loader(repo_root, reference) {
            report_load_failure(
                state,
                "row_authority",
                &format!("row {}'s authority document", row.path),
                reference,
                &failure,
            );
        }
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

fn validate_invariants(
    manifest: &Manifest,
    repo_root: &Path,
    loader: InputLoader,
    state: &mut PlanState,
) {
    let declared_inputs: BTreeSet<InputId> = manifest.inputs.iter().map(|input| input.id).collect();
    let mut seen: BTreeSet<InvariantId> = BTreeSet::new();

    for invariant in &manifest.invariants {
        if !seen.insert(invariant.id) {
            state.not_proven(
                "invariant_duplicate",
                format!("invariant {} is declared more than once", invariant.id.as_str()),
                "release/ci",
            );
        }

        // An invariant is settled against release inputs. Naming a source the
        // manifest never bound means nothing was actually compared.
        for source in &invariant.sources {
            if !declared_inputs.contains(source) {
                state.not_proven(
                    "invariant_source_undeclared",
                    format!(
                        "invariant {} cites release input {} which the manifest does not declare",
                        invariant.id.as_str(),
                        source.as_str()
                    ),
                    "release/ci",
                );
            }
        }

        match invariant.result {
            Verdict::Pass => {
                if invariant.evidence.is_empty() {
                    state.not_proven(
                        "invariant_unevidenced_pass",
                        format!("invariant {} claims pass with no evidence", invariant.id.as_str()),
                        "release/ci",
                    );
                }
                // A non-empty evidence array is not evidence. Resolve each
                // reference the same way live-control evidence is resolved, so
                // an invented citation cannot carry a required invariant.
                for evidence in &invariant.evidence {
                    validate_evidence_reference(
                        &format!("invariant {}", invariant.id.as_str()),
                        evidence,
                        repo_root,
                        loader,
                        state,
                    );
                }
            }
            Verdict::Blocked => state.block(
                "invariant_blocked",
                format!("required invariant {} is blocked", invariant.id.as_str()),
                "release/ci",
            ),
            Verdict::NotProven => state.not_proven(
                "invariant_not_proven",
                format!("required invariant {} is not proven", invariant.id.as_str()),
                "release/ci",
            ),
        }
    }

    // Exhaustive coverage: a projection that simply omits an invariant has not
    // settled it, and a passing plan must not be reachable by declaring fewer.
    for required in REQUIRED_INVARIANT_IDS {
        if !seen.contains(&required) {
            state.not_proven(
                "invariant_required_missing",
                format!("required invariant {} is not declared", required.as_str()),
                "release/ci",
            );
        }
    }
}

fn validate_live_controls(
    manifest: &Manifest,
    repo_root: &Path,
    loader: InputLoader,
    state: &mut PlanState,
) {
    for (name, control) in [
        ("branch_rules", &manifest.live_controls.branch_rules),
        ("environments", &manifest.live_controls.environments),
        ("quality_exceptions", &manifest.live_controls.quality_exceptions),
    ] {
        match control.result {
            LiveResult::Proven => {
                let live: Vec<&Evidence> = control
                    .evidence
                    .iter()
                    .filter(|evidence| evidence.kind == EvidenceKind::LiveReceipt)
                    .collect();
                if live.is_empty() {
                    state.not_proven(
                        "live_control_source_only",
                        format!(
                            "live control {name} claims proven without a live receipt; checked-in policy is not live enforcement proof"
                        ),
                        "release/ci",
                    );
                }
                // The `live_receipt` label is a claim, not proof. Resolve and
                // hash every referenced receipt so a nonexistent or altered
                // observation cannot prove a live control.
                for evidence in live {
                    validate_live_receipt(name, evidence, repo_root, loader, state);
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

        // Non-live evidence must not carry a digest it does not earn: only
        // resolved observations are hashed, so a digest on a ruling or a
        // document would read as verification that never happened.
        for evidence in &control.evidence {
            if evidence.kind != EvidenceKind::LiveReceipt {
                validate_evidence_reference(
                    &format!("live control {name}"),
                    evidence,
                    repo_root,
                    loader,
                    state,
                );
            }
            if evidence.kind != EvidenceKind::LiveReceipt && evidence.digest.is_some() {
                state.not_proven(
                    "live_control_evidence_digest_unexpected",
                    format!(
                        "live control {name} attaches a digest to {} evidence, which the planner does not resolve",
                        evidence.kind.as_str()
                    ),
                    "release/ci",
                );
            }
        }
    }
}

/// Resolve one evidence reference according to its role.
///
/// `live_receipt` goes through the digest-bound path. `repository_source` must
/// name a document that exists under the repository root. `review_ruling` must
/// be an issue reference. Anything that resolves to nothing is not evidence.
fn validate_evidence_reference(
    subject: &str,
    evidence: &Evidence,
    repo_root: &Path,
    loader: InputLoader,
    state: &mut PlanState,
) {
    match evidence.kind {
        EvidenceKind::LiveReceipt => {
            validate_live_receipt(subject, evidence, repo_root, loader, state);
        }
        EvidenceKind::RepositorySource => {
            if !valid_repository_path(&evidence.reference) {
                state.not_proven(
                    "evidence_path_invalid",
                    format!(
                        "{subject} cites repository source {:?}, which is not a repository-relative POSIX path",
                        evidence.reference
                    ),
                    "release/ci",
                );
                return;
            }
            if let Err(failure) = loader(repo_root, &evidence.reference) {
                report_load_failure(
                    state,
                    "evidence",
                    &format!("{subject}'s repository source"),
                    &evidence.reference,
                    &failure,
                );
            }
        }
        EvidenceKind::ReviewRuling => {
            let reference = evidence.reference.trim();
            let numbered = reference
                .strip_prefix('#')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            if !numbered {
                state.not_proven(
                    "evidence_ruling_unresolved",
                    format!(
                        "{subject} cites review ruling {:?}, which is not an issue reference",
                        evidence.reference
                    ),
                    "release/ci",
                );
            }
        }
    }
}

/// Resolve one `live_receipt` reference and prove it is the exact observation
/// the manifest declares.
fn validate_live_receipt(
    control: &str,
    evidence: &Evidence,
    repo_root: &Path,
    loader: InputLoader,
    state: &mut PlanState,
) {
    let Some(declared) = evidence.digest.as_deref() else {
        state.not_proven(
            "live_receipt_undigested",
            format!(
                "live control {control} cites live receipt {} without a digest, so the observation cannot be bound",
                evidence.reference
            ),
            "release/ci",
        );
        return;
    };
    if !is_sha256_digest(declared) {
        state.not_proven(
            "live_receipt_digest_malformed",
            format!(
                "live control {control} cites live receipt {} with a malformed digest",
                evidence.reference
            ),
            "release/ci",
        );
        return;
    }
    if !valid_repository_path(&evidence.reference) {
        state.not_proven(
            "live_receipt_path_invalid",
            format!(
                "live control {control} cites live receipt {:?}, which is not a repository-relative POSIX path",
                evidence.reference
            ),
            "release/ci",
        );
        return;
    }

    match loader(repo_root, &evidence.reference) {
        Ok(bytes) => {
            let observed = sha256_digest(&bytes);
            if observed != declared {
                state.not_proven(
                    "live_receipt_digest_mismatch",
                    format!(
                        "live control {control} cites live receipt {} which hashes to {observed} but is declared as {declared}",
                        evidence.reference
                    ),
                    "release/ci",
                );
            }
        }
        Err(failure) => report_load_failure(
            state,
            "live_receipt",
            &format!("live control {control}'s receipt"),
            &evidence.reference,
            &failure,
        ),
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
