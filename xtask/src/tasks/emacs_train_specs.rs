//! Emacs checked leaf-spec disposition compiler (`emacs_train_specs.v1`).
//!
//! Contract notes for #11751 (SPECENG node of `emacs_train.v1`):
//!
//! - This module is the **mechanical Emacs adapter** that plans, compiles,
//!   validates and explains checked per-node spec dispositions over the
//!   stable train graph `.spec/10918-emacs-train-graph/train.manifest.json`
//!   (E01, schema `emacs_train.v1`). It owns **engine mechanics only**:
//!   disposition population stays with the sibling leaves #11752-#11755 and
//!   the fan-in #11717 (`claim ceiling` recorded on the SPECENG node).
//! - It adapts the shared #3983 issue-to-spec method and the repository
//!   `.spec` conventions (SPEC_TEMPLATE.md bundle shape, manifest-as-data,
//!   fail-closed checkers) — it is **not** a second repository-wide spec
//!   engine and encodes no generic spec framework (OD1 of the manifest
//!   routes shared extraction through #10554).
//! - Every stable node gets **exactly one** disposition from the reviewed
//!   vocabulary of #11717: `SPEC_COMPILED`, `EXISTING_CONTRACT_SUFFICIENT`,
//!   `ISSUE_PLAN_SUFFICIENT`, `CONTROLLER_NO_CODING_SPEC`,
//!   `FAN_IN_OR_CERTIFICATION_SPEC`, `EXTERNAL_OR_MANUAL_NO_CODING_SPEC`,
//!   `HISTORICAL_OR_SUPERSEDED`, `RETURN_TO_ISSUE`, `NOT_PROVEN`.
//!   A missing, ambiguous or contradictory disposition fails closed and
//!   blocks bounded-agent eligibility.
//! - The compiled ledger is durable **stable** bytes: it must carry no live
//!   state (SHAs, timestamps, branches, PRs) and must serialize
//!   deterministically — the same tree produces byte-identical output on a
//!   second run.
//!
//! Laws enforced by [`check`] (each fails closed):
//!
//! | Law | Meaning |
//! | --- | --- |
//! | L01 schema | ledger is `emacs_train_specs.v1` version 1 with the fixed programme header |
//! | L02 denominator | record node set equals the manifest node set (missing/unknown fail) |
//! | L03 exactly-one | one record per node id |
//! | L04 vocabulary | disposition parses from the reviewed nine-value enum |
//! | L05 role compatibility | disposition is allowed for `train_role` (controllers never become coding leaves, fan-ins aggregate, external gates stay external); `RETURN_TO_ISSUE`/`NOT_PROVEN` are reviewed exits allowed for any role |
//! | L06 buildability | builder dispositions require `buildable: true` |
//! | L07 binding | `SPEC_COMPILED` records carry an existing three-file checked bundle; other records carry none |
//! | L08 exit reason | `RETURN_TO_ISSUE`/`NOT_PROVEN` records carry a reviewed reason; others carry none |
//! | L09 authority uniqueness | `authority_after` propositions are unique and non-empty |
//! | L10 provenance | `manifest`-provenance records equal the manifest's embedded disposition |
//! | L11 no live state | record values contain no SHA/timestamp/branch/PR tokens |
//! | L12 canonical bytes | ledger bytes equal the canonical serialization of their content |
//! | L13 manifest guard | consumed manifest is still `emacs_train.v1` version 1 |
//! | L14 contract shape | each node carries the required leaf contract: proposition, authority before/after, claim ceiling, dependency classes with resolvable targets and provenance, writer conflict key, allowed components, forbidden adjacent owners, first falsifier, six control classes, proof routing, obligations, exits, rollback quartet and identity fields |
//!
//! Dependency-class vocabulary: `hard`, `evidence`, `optional` (the closed
//! set used by `emacs_train.v1`; #10858 owns the shared semantics).

use clap::ValueEnum;
use color_eyre::eyre::{Context, Result, bail, ensure};
use glob::glob;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::utils::project_root;

/// Schema identifier of the stable train manifest (E01).
pub const MANIFEST_SCHEMA: &str = "emacs_train.v1";
/// Schema identifier of the compiled disposition ledger.
pub const LEDGER_SCHEMA: &str = "emacs_train_specs.v1";
/// Default path of the stable train manifest, relative to the project root.
pub const DEFAULT_MANIFEST_PATH: &str = ".spec/10918-emacs-train-graph/train.manifest.json";
/// Default path of the compiled disposition ledger, relative to the project root.
pub const DEFAULT_LEDGER_PATH: &str = ".spec/11717-emacs-train-specs/specs.ledger.json";

/// The reviewed nine-value disposition vocabulary of #11717.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LeafSpecDisposition {
    /// A checked `.spec` bundle exists and carries the node contract.
    SpecCompiled,
    /// An already-landed contract is consumed unchanged.
    ExistingContractSufficient,
    /// The current reviewed issue contract is already complete.
    IssuePlanSufficient,
    /// Controller node: no coding spec may exist.
    ControllerNoCodingSpec,
    /// Fan-in or certification node: aggregation contract, not a builder packet.
    FanInOrCertificationSpec,
    /// External or manual gate: separately authorized, no coding spec.
    ExternalOrManualNoCodingSpec,
    /// Completed historical or superseded subject.
    HistoricalOrSuperseded,
    /// Reviewed exit: the premise returns to its issue.
    ReturnToIssue,
    /// Reviewed exit: evidence missing, partial, stale or contradictory.
    NotProven,
}

impl LeafSpecDisposition {
    fn as_str(self) -> &'static str {
        match self {
            Self::SpecCompiled => "SPEC_COMPILED",
            Self::ExistingContractSufficient => "EXISTING_CONTRACT_SUFFICIENT",
            Self::IssuePlanSufficient => "ISSUE_PLAN_SUFFICIENT",
            Self::ControllerNoCodingSpec => "CONTROLLER_NO_CODING_SPEC",
            Self::FanInOrCertificationSpec => "FAN_IN_OR_CERTIFICATION_SPEC",
            Self::ExternalOrManualNoCodingSpec => "EXTERNAL_OR_MANUAL_NO_CODING_SPEC",
            Self::HistoricalOrSuperseded => "HISTORICAL_OR_SUPERSEDED",
            Self::ReturnToIssue => "RETURN_TO_ISSUE",
            Self::NotProven => "NOT_PROVEN",
        }
    }

    fn is_builder(self) -> bool {
        matches!(
            self,
            Self::SpecCompiled | Self::ExistingContractSufficient | Self::IssuePlanSufficient
        )
    }

    fn is_reviewed_exit(self) -> bool {
        matches!(self, Self::ReturnToIssue | Self::NotProven)
    }
}

impl std::fmt::Display for LeafSpecDisposition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Train role vocabulary of `emacs_train.v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainRole {
    Controller,
    Specification,
    StableContract,
    SemanticRevision,
    Historical,
    Implementation,
    FanIn,
    PacketAdapter,
    Dogfood,
    EvidencePolicy,
    ExternalGate,
}

impl TrainRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Controller => "controller",
            Self::Specification => "specification",
            Self::StableContract => "stable_contract",
            Self::SemanticRevision => "semantic_revision",
            Self::Historical => "historical",
            Self::Implementation => "implementation",
            Self::FanIn => "fan_in",
            Self::PacketAdapter => "packet_adapter",
            Self::Dogfood => "dogfood",
            Self::EvidencePolicy => "evidence_policy",
            Self::ExternalGate => "external_gate",
        }
    }

    /// Dispositions allowed for the role, excluding the any-role reviewed
    /// exits (`RETURN_TO_ISSUE`, `NOT_PROVEN`).
    fn allowed_dispositions(self) -> &'static [LeafSpecDisposition] {
        use LeafSpecDisposition as D;
        use TrainRole as R;
        match self {
            R::Controller => &[D::ControllerNoCodingSpec],
            R::FanIn => &[D::FanInOrCertificationSpec],
            R::ExternalGate => &[D::ExternalOrManualNoCodingSpec],
            R::Historical => &[D::ExistingContractSufficient, D::HistoricalOrSuperseded],
            R::Specification => &[D::SpecCompiled],
            R::StableContract => &[D::SpecCompiled, D::IssuePlanSufficient],
            R::SemanticRevision => &[D::SpecCompiled, D::IssuePlanSufficient],
            R::Implementation | R::PacketAdapter | R::EvidencePolicy => {
                &[D::SpecCompiled, D::ExistingContractSufficient, D::IssuePlanSufficient]
            }
            // A dogfood aggregator may close over its cohort (DOG, #10936).
            R::Dogfood => &[
                D::SpecCompiled,
                D::ExistingContractSufficient,
                D::IssuePlanSufficient,
                D::FanInOrCertificationSpec,
            ],
        }
    }

    fn allows(self, disposition: LeafSpecDisposition) -> bool {
        disposition.is_reviewed_exit() || self.allowed_dispositions().contains(&disposition)
    }
}

impl std::fmt::Display for TrainRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Closed dependency-class vocabulary of `emacs_train.v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyClass {
    Hard,
    Evidence,
    Optional,
}

impl DependencyClass {
    fn label(self) -> &'static str {
        match self {
            Self::Hard => "hard",
            Self::Evidence => "evidence",
            Self::Optional => "optional",
        }
    }
}

impl std::fmt::Display for DependencyClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Typed dependency edge of a manifest node.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeDependency {
    pub target: String,
    pub class: DependencyClass,
    pub provenance: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeWriter {
    conflict_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeSpec {
    disposition: LeafSpecDisposition,
    owner: String,
    #[serde(default)]
    #[allow(dead_code, reason = "stale policy is informative, not engine-checked")]
    stale_policy: Option<String>,
    #[serde(default)]
    #[allow(dead_code, reason = "spec authority is uniform (#11717), not engine-checked")]
    spec_authority: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeControls {
    positive: String,
    opposite: String,
    stale: String,
    wrong_subject: String,
    fault: String,
    mutation: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeProof {
    focused: String,
    routed: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeObligations {
    schema: String,
    generated: String,
    docs: String,
    changelog: String,
    receipt: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeExits {
    old_path: String,
    compatibility: String,
    supersession: String,
    transfer: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeRollback {
    rollback: String,
    return_to_issue: String,
    not_proven: String,
    stop: String,
}

/// The subset of an `emacs_train.v1` node the engine consumes. Unknown keys
/// are tolerated within schema version 1 (the E01 bundle's own checker owns
/// exact graph-byte laws); every field below is required and validated.
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestNode {
    pub node_id: String,
    pub issue: u64,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub train_role: TrainRole,
    #[serde(default)]
    pub lane: String,
    pub buildable: bool,
    pub one_pr_outcome: String,
    pub authority_before: String,
    pub authority_after: String,
    #[serde(default)]
    pub dependencies: Vec<NodeDependency>,
    pub claim_ceiling: String,
    pub writer: NodeWriter,
    #[serde(default)]
    pub consumed_authorities: Vec<String>,
    #[serde(default)]
    pub allowed_components: Vec<String>,
    #[serde(default)]
    pub forbidden_adjacent_owners: Vec<String>,
    pub spec: NodeSpec,
    pub first_falsifier: String,
    pub controls: NodeControls,
    pub proof: NodeProof,
    pub obligations: NodeObligations,
    pub exits: NodeExits,
    pub rollback: NodeRollback,
    #[serde(default)]
    pub successors: Vec<String>,
    #[serde(default)]
    pub identity_fields: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalAuthority {
    id: String,
    #[serde(default)]
    #[allow(dead_code, reason = "subject is informative; only id resolution is checked")]
    subject: Option<String>,
}

/// Parsed stable train manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct TrainManifest {
    pub schema: String,
    pub schema_version: u64,
    #[serde(default)]
    pub nodes: Vec<ManifestNode>,
    #[serde(default)]
    external_authorities: Vec<ExternalAuthority>,
}

impl TrainManifest {
    /// Parse and guard the manifest schema identifier and version.
    pub fn parse(bytes: &str) -> Result<Self> {
        let manifest: TrainManifest = serde_json::from_str(bytes)
            .with_context(|| format!("parsing {MANIFEST_SCHEMA} manifest"))?;
        ensure!(
            manifest.schema == MANIFEST_SCHEMA,
            "manifest schema {:?} is not {MANIFEST_SCHEMA}",
            manifest.schema
        );
        ensure!(
            manifest.schema_version == 1,
            "manifest schema_version {} is not 1",
            manifest.schema_version
        );
        let mut node_ids = BTreeSet::new();
        let mut issues = BTreeSet::new();
        for node in &manifest.nodes {
            ensure!(
                node_ids.insert(node.node_id.as_str()),
                "duplicate node id {:?} in manifest",
                node.node_id
            );
            ensure!(
                issues.insert(node.issue),
                "duplicate issue {} in manifest (node {:?})",
                node.issue,
                node.node_id
            );
        }
        Ok(manifest)
    }

    fn resolve(&self, subject: &str) -> Result<&ManifestNode> {
        let trimmed = subject.trim();
        let bare = trimmed.trim_start_matches('#');
        let by_id = self
            .nodes
            .iter()
            .filter(|n| n.node_id.eq_ignore_ascii_case(trimmed))
            .collect::<Vec<_>>();
        let by_alias = self
            .nodes
            .iter()
            .filter(|n| n.aliases.iter().any(|a| a.eq_ignore_ascii_case(trimmed)))
            .collect::<Vec<_>>();
        let by_issue = bare
            .parse::<u64>()
            .map(|issue| self.nodes.iter().filter(|n| n.issue == issue).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut candidates: Vec<&ManifestNode> = by_id;
        candidates.extend(by_alias);
        candidates.extend(by_issue);
        candidates.sort_by_key(|n| n.node_id.clone());
        candidates.dedup_by_key(|n| n.node_id.clone());
        match candidates.as_slice() {
            [node] => Ok(node),
            [] => bail!(
                "subject {subject:?} resolves to no node in {MANIFEST_SCHEMA}; \
                 pass a node id, alias or issue number"
            ),
            many => bail!(
                "subject {subject:?} is ambiguous between {}",
                many.iter()
                    .map(|n| format!("{} (#{} node)", n.node_id, n.issue))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// Whether a record's disposition came from the manifest's embedded value or
/// from an explicit reviewed adjudication (population-leaf override).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispositionProvenance {
    Manifest,
    Adjudicated,
}

impl DispositionProvenance {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manifest => "manifest",
            Self::Adjudicated => "adjudicated",
        }
    }
}

impl std::fmt::Display for DispositionProvenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Programme header of the compiled ledger; fixed identity of the plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerProgramme {
    pub parent_programme_issue: u64,
    pub controller_issue: u64,
    pub durable_architecture_issue: u64,
    pub stable_train_issue: u64,
    pub governing_issue: u64,
    pub engine_issue: u64,
    pub method_authority: String,
    pub consumed_manifest: String,
}

impl LedgerProgramme {
    fn new(consumed_manifest: &str) -> Self {
        Self {
            parent_programme_issue: 7979,
            controller_issue: 8706,
            durable_architecture_issue: 11716,
            stable_train_issue: 10918,
            governing_issue: 11717,
            engine_issue: 11751,
            method_authority: "#3983".to_string(),
            consumed_manifest: consumed_manifest.to_string(),
        }
    }
}

/// One compiled disposition record. Field order is the canonical byte order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispositionRecord {
    pub node_id: String,
    pub issue: u64,
    pub train_role: TrainRole,
    pub disposition: LeafSpecDisposition,
    pub disposition_provenance: DispositionProvenance,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub compiled_spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reviewed_reason: Option<String>,
    pub authority_after: String,
    pub spec_owner: String,
}

/// Compiled disposition ledger (`emacs_train_specs.v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecsLedger {
    pub schema: String,
    pub schema_version: u64,
    pub programme: LedgerProgramme,
    pub records: Vec<DispositionRecord>,
}

impl SpecsLedger {
    fn new(consumed_manifest: &str) -> Self {
        Self {
            schema: LEDGER_SCHEMA.to_string(),
            schema_version: 1,
            programme: LedgerProgramme::new(consumed_manifest),
            records: Vec::new(),
        }
    }

    fn parse(bytes: &str) -> Result<Self> {
        let ledger: SpecsLedger = serde_json::from_str(bytes)
            .with_context(|| format!("parsing {LEDGER_SCHEMA} ledger"))?;
        Ok(ledger)
    }

    /// Canonical deterministic serialization: fixed field order, manifest
    /// record order, no timestamps, exactly one trailing newline.
    fn to_canonical_bytes(&self) -> Result<String> {
        let mut out = serde_json::to_string_pretty(self)
            .with_context(|| format!("serializing {LEDGER_SCHEMA} ledger"))?;
        out.push('\n');
        Ok(out)
    }
}

/// Output format for report operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SpecsOutputFormat {
    Human,
    Json,
}

/// A failed law, reported by [`check`] and [`compile`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LawViolation {
    pub law: String,
    pub subject: String,
    pub detail: String,
}

impl std::fmt::Display for LawViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]: {}", self.law, self.subject, self.detail)
    }
}

fn law(law_id: &str, subject: &str, detail: impl Into<String>) -> LawViolation {
    LawViolation { law: law_id.to_string(), subject: subject.to_string(), detail: detail.into() }
}

#[allow(clippy::expect_used, reason = "static LazyLock regexes with known-good constant patterns")]
fn live_state_patterns() -> Vec<(&'static str, Regex)> {
    vec![
        ("sha-like hex run", Regex::new(r"[0-9a-f]{8,}")),
        ("iso timestamp", Regex::new(r"\d{4}-\d{2}-\d{2}[T ]")),
        ("branch ref", Regex::new(r"refs/heads/")),
        ("remote ref", Regex::new(r"origin/[A-Za-z0-9._-]+")),
        ("pull path", Regex::new(r"/pull/")),
        ("commit path", Regex::new(r"github\.com/[^/]+/[^/]+/(blob|tree|commit)/")),
    ]
    .into_iter()
    .map(|(name, re)| (name, re.expect("static LazyLock regex with known-good pattern")))
    .collect()
}

static LIVE_STATE_PATTERNS: LazyLock<Vec<(&'static str, Regex)>> =
    LazyLock::new(live_state_patterns);

fn live_state_tokens(value: &str) -> Vec<&'static str> {
    LIVE_STATE_PATTERNS.iter().filter(|(_, re)| re.is_match(value)).map(|(name, _)| *name).collect()
}

/// Validate the full leaf-contract shape of one node (L14) plus the
/// manifest-level role laws (L05, L06).
fn node_violations(node: &ManifestNode, manifest: &TrainManifest) -> Vec<LawViolation> {
    let id = node.node_id.as_str();
    let mut violations = Vec::new();

    if !node.train_role.allows(node.spec.disposition) {
        let mut allowed: Vec<&str> =
            node.train_role.allowed_dispositions().iter().map(|d| d.as_str()).collect();
        allowed.extend(["RETURN_TO_ISSUE", "NOT_PROVEN"]);
        violations.push(law(
            "L05-role-compatibility",
            id,
            format!(
                "{} role {} may not carry {} (allowed: {})",
                MANIFEST_SCHEMA,
                node.train_role,
                node.spec.disposition,
                allowed.join(", ")
            ),
        ));
    }
    if node.spec.disposition.is_builder() && !node.buildable {
        violations.push(law(
            "L06-buildability",
            id,
            format!(
                "builder disposition {} requires buildable=true (manifest says {})",
                node.spec.disposition, node.buildable
            ),
        ));
    }

    for (field, value) in [
        ("one_pr_outcome", node.one_pr_outcome.as_str()),
        ("authority_before", node.authority_before.as_str()),
        ("authority_after", node.authority_after.as_str()),
        ("claim_ceiling", node.claim_ceiling.as_str()),
        ("first_falsifier", node.first_falsifier.as_str()),
        ("writer.conflict_key", node.writer.conflict_key.as_str()),
    ] {
        if value.trim().is_empty() {
            violations.push(law(
                "L14-contract-shape",
                id,
                format!("required leaf-contract field {field} is empty"),
            ));
        }
    }
    for (field, value) in [
        ("positive", node.controls.positive.as_str()),
        ("opposite", node.controls.opposite.as_str()),
        ("stale", node.controls.stale.as_str()),
        ("wrong_subject", node.controls.wrong_subject.as_str()),
        ("fault", node.controls.fault.as_str()),
        ("mutation", node.controls.mutation.as_str()),
    ] {
        if value.trim().is_empty() {
            violations.push(law(
                "L14-contract-shape",
                id,
                format!("control class {field} is empty"),
            ));
        }
    }
    for (field, value) in
        [("focused", node.proof.focused.as_str()), ("routed", node.proof.routed.as_str())]
    {
        if value.trim().is_empty() {
            violations.push(law(
                "L14-contract-shape",
                id,
                format!("proof reference {field} is empty"),
            ));
        }
    }
    for (field, value) in [
        ("schema", node.obligations.schema.as_str()),
        ("generated", node.obligations.generated.as_str()),
        ("docs", node.obligations.docs.as_str()),
        ("changelog", node.obligations.changelog.as_str()),
        ("receipt", node.obligations.receipt.as_str()),
    ] {
        if value.trim().is_empty() {
            violations.push(law("L14-contract-shape", id, format!("obligation {field} is empty")));
        }
    }
    for (field, value) in [
        ("old_path", node.exits.old_path.as_str()),
        ("compatibility", node.exits.compatibility.as_str()),
        ("supersession", node.exits.supersession.as_str()),
        ("transfer", node.exits.transfer.as_str()),
    ] {
        if value.trim().is_empty() {
            violations.push(law("L14-contract-shape", id, format!("exit {field} is empty")));
        }
    }
    for (field, value) in [
        ("rollback", node.rollback.rollback.as_str()),
        ("return_to_issue", node.rollback.return_to_issue.as_str()),
        ("not_proven", node.rollback.not_proven.as_str()),
        ("stop", node.rollback.stop.as_str()),
    ] {
        if value.trim().is_empty() {
            violations.push(law(
                "L14-contract-shape",
                id,
                format!("rollback boundary {field} is empty"),
            ));
        }
    }
    if node.allowed_components.is_empty() {
        violations.push(law("L14-contract-shape", id, "allowed_components is empty"));
    }
    if node.forbidden_adjacent_owners.is_empty() {
        violations.push(law(
            "L14-contract-shape",
            id,
            "forbidden_adjacent_owners is empty (adjacent-owner boundary missing)",
        ));
    }
    if node.identity_fields.is_empty() {
        violations.push(law("L14-contract-shape", id, "identity_fields is empty"));
    }

    let node_ids: BTreeSet<&str> = manifest.nodes.iter().map(|n| n.node_id.as_str()).collect();
    let authority_ids: BTreeSet<&str> =
        manifest.external_authorities.iter().map(|a| a.id.as_str()).collect();
    let mut dep_targets = BTreeSet::new();
    for dep in &node.dependencies {
        if !dep_targets.insert(dep.target.as_str()) {
            violations.push(law(
                "L14-contract-shape",
                id,
                format!("duplicate dependency target {}", dep.target),
            ));
        }
        if dep.provenance.trim().is_empty() {
            violations.push(law(
                "L14-contract-shape",
                id,
                format!("dependency {} carries no provenance", dep.target),
            ));
        }
        if !node_ids.contains(dep.target.as_str()) && !authority_ids.contains(dep.target.as_str()) {
            violations.push(law(
                "L14-contract-shape",
                id,
                format!(
                    "dependency target {} resolves to neither a manifest node nor a declared external authority",
                    dep.target
                ),
            ));
        }
    }
    let mut successors = BTreeSet::new();
    for successor in &node.successors {
        if !successors.insert(successor.as_str()) {
            violations.push(law(
                "L14-contract-shape",
                id,
                format!("duplicate successor {successor}"),
            ));
        }
        if !node_ids.contains(successor.as_str()) {
            violations.push(law(
                "L14-contract-shape",
                id,
                format!("successor {successor} resolves to no manifest node"),
            ));
        }
    }

    violations
}

/// Record-level laws that need both the record and its node.
fn record_violations(
    record: &DispositionRecord,
    node: &ManifestNode,
    repo_root: &Path,
) -> Vec<LawViolation> {
    let id = record.node_id.as_str();
    let mut violations = Vec::new();

    if !node.train_role.allows(record.disposition) {
        violations.push(law(
            "L05-role-compatibility",
            id,
            format!(
                "{LEDGER_SCHEMA} role {} may not carry {}",
                node.train_role, record.disposition
            ),
        ));
    }
    if record.disposition.is_builder() && !node.buildable {
        violations.push(law(
            "L06-buildability",
            id,
            format!("builder disposition {} requires buildable=true", record.disposition),
        ));
    }
    match (&record.compiled_spec, record.disposition) {
        (Some(path), LeafSpecDisposition::SpecCompiled) => {
            let binding = Path::new(path);
            if binding.is_absolute()
                || binding
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
                || !path.starts_with(".spec/")
            {
                violations.push(law(
                    "L07-binding",
                    id,
                    format!(
                        "compiled_spec {path:?} escapes the repository .spec tree; \
                         durable bindings must be repository-relative .spec/ paths"
                    ),
                ));
            } else {
                let bundle = repo_root.join(binding);
                if !bundle.is_dir() {
                    violations.push(law(
                        "L07-binding",
                        id,
                        format!("compiled spec {path:?} does not exist"),
                    ));
                } else {
                    for member in ["context.md", "acceptance.md", "checklist.md"] {
                        if !bundle.join(member).is_file() {
                            violations.push(law(
                                "L07-binding",
                                id,
                                format!("compiled spec {path:?} is missing {member}"),
                            ));
                        }
                    }
                }
            }
        }
        (Some(_), _) => violations.push(law(
            "L07-binding",
            id,
            format!("compiled_spec is only valid for SPEC_COMPILED, not {}", record.disposition),
        )),
        (None, LeafSpecDisposition::SpecCompiled) => violations.push(law(
            "L07-binding",
            id,
            "SPEC_COMPILED record carries no compiled_spec bundle",
        )),
        (None, _) => {}
    }
    match (&record.reviewed_reason, record.disposition.is_reviewed_exit()) {
        (Some(reason), true) => {
            if reason.trim().is_empty() {
                violations.push(law(
                    "L08-exit-reason",
                    id,
                    "reviewed exit disposition carries an empty reviewed_reason",
                ));
            }
        }
        (Some(_), false) => violations.push(law(
            "L08-exit-reason",
            id,
            format!(
                "reviewed_reason is only valid for RETURN_TO_ISSUE/NOT_PROVEN, not {}",
                record.disposition
            ),
        )),
        (None, true) => violations.push(law(
            "L08-exit-reason",
            id,
            format!("{} requires a non-empty reviewed_reason", record.disposition),
        )),
        (None, false) => {}
    }
    if record.disposition_provenance == DispositionProvenance::Manifest
        && record.disposition != node.spec.disposition
    {
        violations.push(law(
            "L10-provenance",
            id,
            format!(
                "manifest-provenance record says {} but {MANIFEST_SCHEMA} embeds {}; \
                 re-adjudicate explicitly",
                record.disposition, node.spec.disposition
            ),
        ));
    }
    // Copied authority and owner fields must stay current with the manifest:
    // after a train revision an old ledger certifies stale authority until
    // the affected nodes are recompiled.
    if record.authority_after != node.authority_after || record.spec_owner != node.spec.owner {
        violations.push(law(
            "L10-provenance",
            id,
            "record is stale: authority_after/spec_owner differ from the manifest node; \
             recompile the node after the train revision",
        ));
    }
    if record.authority_after.trim().is_empty() {
        violations.push(law("L09-authority-uniqueness", id, "authority_after is empty"));
    }
    if record.spec_owner.trim().is_empty() {
        violations.push(law("L01-schema", id, "spec_owner is empty"));
    }
    for value in [
        record.node_id.as_str(),
        record.authority_after.as_str(),
        record.spec_owner.as_str(),
        record.compiled_spec.as_deref().unwrap_or(""),
        record.reviewed_reason.as_deref().unwrap_or(""),
    ] {
        for token in live_state_tokens(value) {
            violations.push(law(
                "L11-no-live-state",
                id,
                format!("record value contains live-state token ({token}): {value:?}"),
            ));
        }
    }
    violations
}

/// The root of the spec tree that owns the manifest: bindings resolve inside
/// the repository that carries the manifest. The manifest lives somewhere
/// under `<root>/.spec/` (`<root>/.spec/<bundle>/train.manifest.json` or
/// `<root>/.spec/train.manifest.json`), so the root is the parent of the
/// first ancestor directory named `.spec`.
fn spec_tree_root(manifest_path: &Path) -> Result<PathBuf> {
    let mut current = manifest_path.parent();
    while let Some(dir) = current {
        if dir.file_name().is_some_and(|name| name == ".spec") {
            if let Some(root) = dir.parent() {
                return Ok(root.to_path_buf());
            }
            break;
        }
        current = dir.parent();
    }
    bail!(
        "manifest path {manifest_path:?} is not inside a .spec spec tree; \
         bindings cannot be resolved"
    )
}

fn read_manifest(path: &Path) -> Result<TrainManifest> {
    let bytes =
        fs::read_to_string(path).with_context(|| format!("reading manifest {}", path.display()))?;
    TrainManifest::parse(&bytes)
}

fn load_ledger(path: &Path, consumed_manifest: &str) -> Result<SpecsLedger> {
    match fs::read_to_string(path) {
        Ok(bytes) => SpecsLedger::parse(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(SpecsLedger::new(consumed_manifest))
        }
        Err(error) => Err(color_eyre::eyre::eyre!("reading ledger {}: {error}", path.display())),
    }
}

fn normalize_repo_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Discover the checked bundle for a `SPEC_COMPILED` node by the `.spec`
/// convention `.spec/<issue>-<slug>/`. Zero or multiple matches fail closed.
fn discover_compiled_spec(node: &ManifestNode, manifest_path: &Path) -> Result<String> {
    let root = spec_tree_root(manifest_path)?;
    let pattern =
        root.join(".spec").join(format!("{}-*", node.issue)).to_string_lossy().replace('\\', "/");
    let mut matches: Vec<PathBuf> = Vec::new();
    let entries = glob(&pattern)
        .with_context(|| format!("scanning for compiled spec with pattern {pattern}"))?;
    for entry in entries {
        let entry = entry.with_context(|| "reading compiled spec candidate")?;
        if entry.is_dir() {
            matches.push(entry);
        }
    }
    matches.sort();
    match matches.as_slice() {
        [single] => {
            let relative = single
                .strip_prefix(&root)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| single.clone());
            Ok(normalize_repo_path(&relative))
        }
        [] => bail!(
            "SPEC_COMPILED node {} (#{}) has no compiled spec: no .spec/{}-*/ bundle found; \
             pass --compiled-spec or leave the node to its population leaf",
            node.node_id,
            node.issue,
            node.issue
        ),
        many => bail!(
            "SPEC_COMPILED node {} (#{}) is ambiguous between {}",
            node.node_id,
            node.issue,
            many.iter().map(|p| normalize_repo_path(p)).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// Resolve `--compiled-spec`/discovery into the record binding for one node.
fn resolve_binding(
    node: &ManifestNode,
    disposition: LeafSpecDisposition,
    explicit: Option<&Path>,
    manifest_path: &Path,
) -> Result<Option<String>> {
    if disposition != LeafSpecDisposition::SpecCompiled {
        if let Some(explicit) = explicit {
            bail!(
                "--compiled-spec {} is only valid for SPEC_COMPILED nodes; {} carries {}",
                explicit.display(),
                node.node_id,
                disposition
            );
        }
        return Ok(None);
    }
    let binding = match explicit {
        Some(path) => {
            // Durable bindings must stay inside the repository's `.spec`
            // tree: an absolute or `..`-traversing path would let a ledger
            // pass `check` against mutable machine-local files outside the
            // checked tree.
            ensure!(
                !path.is_absolute(),
                "--compiled-spec must be repository-relative, got absolute path {}",
                path.display()
            );
            ensure!(
                !path
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir)),
                "--compiled-spec must not traverse outside the repository (..): {}",
                path.display()
            );
            let normalized = normalize_repo_path(path);
            ensure!(
                normalized.starts_with(".spec/"),
                "--compiled-spec must name a bundle under the repository's .spec tree: {}",
                normalized
            );
            normalized
        }
        None => discover_compiled_spec(node, manifest_path)?,
    };
    Ok(Some(binding))
}

fn build_record(
    node: &ManifestNode,
    disposition: LeafSpecDisposition,
    provenance: DispositionProvenance,
    binding: Option<String>,
    reviewed_reason: Option<String>,
) -> DispositionRecord {
    DispositionRecord {
        node_id: node.node_id.clone(),
        issue: node.issue,
        train_role: node.train_role,
        disposition,
        disposition_provenance: provenance,
        compiled_spec: binding,
        reviewed_reason,
        authority_after: node.authority_after.clone(),
        spec_owner: node.spec.owner.clone(),
    }
}

fn consumed_manifest_ref(manifest_path: &Path, root: &Path) -> String {
    if let Ok(relative) = manifest_path.strip_prefix(root) {
        return normalize_repo_path(relative);
    }
    // Trees outside the project root (fixtures, external checkouts) record
    // their spec-tree-relative reference so identical trees stay identical.
    match spec_tree_root(manifest_path) {
        Ok(tree_root) => {
            let relative = manifest_path.strip_prefix(&tree_root).unwrap_or(manifest_path);
            normalize_repo_path(relative)
        }
        Err(_) => normalize_repo_path(manifest_path),
    }
}

/// `plan` — deterministic disposition plan over the stable denominator.
///
/// Report-only: prints every node's embedded disposition, allowed set and
/// ledger state; a partial ledger is the normal pre-population state and is
/// not an error.
pub fn plan(
    manifest_path: Option<PathBuf>,
    ledger_path: Option<PathBuf>,
    format: SpecsOutputFormat,
) -> Result<()> {
    let root = project_root()?;
    let manifest_path = manifest_path.unwrap_or_else(|| root.join(DEFAULT_MANIFEST_PATH));
    let ledger_path = ledger_path.unwrap_or_else(|| root.join(DEFAULT_LEDGER_PATH));
    let manifest = read_manifest(&manifest_path)?;
    let consumed = consumed_manifest_ref(&manifest_path, &root);
    let ledger = load_ledger(&ledger_path, &consumed)?;
    let records: std::collections::BTreeMap<&str, &DispositionRecord> =
        ledger.records.iter().map(|r| (r.node_id.as_str(), r)).collect();

    let mut rows = Vec::new();
    for node in &manifest.nodes {
        let record = records.get(node.node_id.as_str()).copied();
        let embedded = node.spec.disposition;
        let state = match record {
            None => "uncompiled".to_string(),
            Some(record) => {
                let base = format!("compiled:{}", record.disposition);
                if record.disposition != embedded {
                    format!("{base} (differs from embedded {embedded})")
                } else {
                    base
                }
            }
        };
        let action: &'static str = match record {
            None => {
                if embedded == LeafSpecDisposition::SpecCompiled {
                    "compile (bind checked bundle)"
                } else {
                    "compile"
                }
            }
            Some(record) if record.disposition != embedded => "review mismatch",
            Some(_) => "none",
        };
        let mut allowed: Vec<&str> =
            node.train_role.allowed_dispositions().iter().map(|d| d.as_str()).collect();
        allowed.extend(["RETURN_TO_ISSUE", "NOT_PROVEN"]);
        rows.push(PlanRow {
            node_id: node.node_id.clone(),
            issue: node.issue,
            train_role: node.train_role.to_string(),
            buildable: node.buildable,
            embedded_disposition: embedded.to_string(),
            allowed,
            ledger_state: state,
            action,
        });
    }

    let uncompiled = rows.iter().filter(|r| r.ledger_state == "uncompiled").count();
    let report = PlanReport {
        schema: LEDGER_SCHEMA.to_string(),
        manifest: consumed,
        node_count: manifest.nodes.len(),
        compiled_count: records.len(),
        uncompiled_count: uncompiled,
        rows,
    };
    match format {
        SpecsOutputFormat::Human => print!("{}", render_plan(&report)),
        SpecsOutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct PlanRow {
    node_id: String,
    issue: u64,
    train_role: String,
    buildable: bool,
    embedded_disposition: String,
    allowed: Vec<&'static str>,
    ledger_state: String,
    action: &'static str,
}

#[derive(Debug, Serialize)]
struct PlanReport {
    schema: String,
    manifest: String,
    node_count: usize,
    compiled_count: usize,
    uncompiled_count: usize,
    rows: Vec<PlanRow>,
}

fn render_plan(report: &PlanReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "emacs train specs plan ({LEDGER_SCHEMA} over {}): {} nodes, {} compiled",
        report.manifest, report.node_count, report.compiled_count
    );
    let _ = writeln!(
        out,
        "{:<14} {:>6}  {:<16} {:<8} {:<30} {:<34} ACTION",
        "NODE", "ISSUE", "ROLE", "BUILD", "EMBEDDED DISPOSITION", "LEDGER STATE"
    );
    for row in &report.rows {
        let _ = writeln!(
            out,
            "{:<14} {:>6}  {:<16} {:<8} {:<30} {:<34} {}",
            row.node_id,
            row.issue,
            row.train_role,
            row.buildable,
            row.embedded_disposition,
            row.ledger_state,
            row.action
        );
    }
    let _ = writeln!(
        out,
        "summary: {}/{} nodes carry a compiled disposition; {} uncompiled",
        report.compiled_count, report.node_count, report.uncompiled_count
    );
    out
}

/// Configuration for one `compile` invocation.
#[derive(Debug)]
pub struct CompileConfig {
    /// Node id, alias or issue number; `None` with `all` compiles the whole
    /// denominator from manifest-embedded dispositions.
    pub subject: Option<String>,
    pub all: bool,
    pub manifest_path: Option<PathBuf>,
    pub ledger_path: Option<PathBuf>,
    pub disposition: Option<LeafSpecDisposition>,
    pub compiled_spec: Option<PathBuf>,
    pub reviewed_reason: Option<String>,
    pub readjudicate: bool,
}

/// `compile` — mechanically compile checked disposition record(s).
///
/// Fail-closed: one law violation aborts the write; a disposition change
/// against an existing record requires explicit `--readjudicate`. `--all` is
/// all-or-nothing and never touches already-compiled nodes.
pub fn compile(config: CompileConfig) -> Result<()> {
    ensure!(config.subject.is_some() != config.all, "pass exactly one compile subject or --all");
    if config.all {
        ensure!(
            config.disposition.is_none()
                && config.compiled_spec.is_none()
                && config.reviewed_reason.is_none(),
            "--all compiles manifest-embedded dispositions; per-node overrides need a subject"
        );
    }
    let root = project_root()?;
    let manifest_path =
        config.manifest_path.clone().unwrap_or_else(|| root.join(DEFAULT_MANIFEST_PATH));
    let ledger_path = config.ledger_path.clone().unwrap_or_else(|| root.join(DEFAULT_LEDGER_PATH));
    let manifest = read_manifest(&manifest_path)?;
    let consumed = consumed_manifest_ref(&manifest_path, &root);
    let mut ledger = load_ledger(&ledger_path, &consumed)?;
    let repo_root = spec_tree_root(&manifest_path)?;

    let targets: Vec<&ManifestNode> = if config.all {
        manifest.nodes.iter().collect()
    } else {
        let subject = config
            .subject
            .as_deref()
            .ok_or_else(|| color_eyre::eyre::eyre!("compile subject missing"))?;
        vec![manifest.resolve(subject)?]
    };

    let existing: std::collections::BTreeMap<String, &DispositionRecord> =
        ledger.records.iter().map(|r| (r.node_id.clone(), r)).collect();

    let mut compiled = Vec::new();
    let mut skipped = 0usize;
    for node in targets {
        if config.all && existing.contains_key(&node.node_id) {
            skipped += 1;
            continue;
        }
        let disposition = config.disposition.unwrap_or(node.spec.disposition);
        let provenance = if config.disposition.is_some() {
            DispositionProvenance::Adjudicated
        } else {
            DispositionProvenance::Manifest
        };
        // The re-adjudication gate fires before binding resolution so a
        // silent disposition change is never masked by a later binding error.
        if let Some(prior) = existing.get(&node.node_id) {
            let changes =
                (prior.disposition, prior.disposition_provenance, prior.reviewed_reason.as_deref())
                    != (disposition, provenance, config.reviewed_reason.as_deref());
            if changes {
                ensure!(
                    config.readjudicate,
                    "node {} already carries disposition {} ({}); changing it requires --readjudicate",
                    node.node_id,
                    prior.disposition,
                    prior.disposition_provenance
                );
            }
        }
        let mut violations = node_violations(node, &manifest);
        let probe = build_record(
            node,
            disposition,
            provenance,
            resolve_binding(node, disposition, config.compiled_spec.as_deref(), &manifest_path)
                .map_err(|error| color_eyre::eyre::eyre!("{}/{}", node.node_id, error))?,
            config.reviewed_reason.clone(),
        );
        violations.extend(record_violations(&probe, node, &repo_root));
        ensure!(
            violations.is_empty(),
            "compile rejected for {} ({}):\n{}",
            node.node_id,
            MANIFEST_SCHEMA,
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n")
        );
        if let Some(prior) = existing.get(&node.node_id) {
            ensure!(
                **prior == probe || config.readjudicate,
                "recompiling {} would change committed ledger bytes; requires --readjudicate",
                node.node_id
            );
        }
        compiled.push(probe);
    }

    for probe in &compiled {
        match ledger.records.iter().position(|r| r.node_id == probe.node_id) {
            Some(index) => ledger.records[index] = probe.clone(),
            None => ledger.records.push(probe.clone()),
        }
    }
    // Canonical record order follows manifest node order.
    let order: std::collections::BTreeMap<&str, usize> =
        manifest.nodes.iter().enumerate().map(|(i, n)| (n.node_id.as_str(), i)).collect();
    ledger.records.sort_by_key(|r| order.get(r.node_id.as_str()).copied().unwrap_or(usize::MAX));

    // `--all` claims the complete denominator, so the final ledger must
    // satisfy every check law — including records carried over from a
    // pre-existing partial ledger — before a single byte is written.
    if config.all {
        let final_bytes = ledger.to_canonical_bytes()?;
        let final_violations =
            check_ledger(&manifest, &ledger, &final_bytes, &repo_root, &consumed);
        ensure!(
            final_violations.is_empty(),
            "compile --all produced a ledger that fails check:\n{}",
            final_violations.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n")
        );
    }

    if let Some(parent) = ledger_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating ledger directory {}", parent.display()))?;
    }
    fs::write(&ledger_path, ledger.to_canonical_bytes()?)
        .with_context(|| format!("writing ledger {}", ledger_path.display()))?;

    for probe in &compiled {
        println!(
            "compiled {} (#{}): {} [{}]{}",
            probe.node_id,
            probe.issue,
            probe.disposition,
            probe.disposition_provenance,
            probe.compiled_spec.as_deref().map(|p| format!(" -> {p}")).unwrap_or_default()
        );
    }
    if config.all {
        println!(
            "compile --all: {} record(s) written, {skipped} already-compiled node(s) skipped",
            compiled.len()
        );
    }
    Ok(())
}

/// `check` — fail-closed validation of the whole denominator.
pub fn check(
    manifest_path: Option<PathBuf>,
    ledger_path: Option<PathBuf>,
    format: SpecsOutputFormat,
) -> Result<()> {
    let root = project_root()?;
    let manifest_path = manifest_path.unwrap_or_else(|| root.join(DEFAULT_MANIFEST_PATH));
    let ledger_path = ledger_path.unwrap_or_else(|| root.join(DEFAULT_LEDGER_PATH));
    let manifest = read_manifest(&manifest_path)?;
    let ledger_bytes = fs::read_to_string(&ledger_path).with_context(|| {
        format!(
            "reading ledger {} (a missing ledger means the denominator is not compiled: \
             run compile first)",
            ledger_path.display()
        )
    })?;
    let ledger = SpecsLedger::parse(&ledger_bytes)?;
    let repo_root = spec_tree_root(&manifest_path)?;
    let consumed = consumed_manifest_ref(&manifest_path, &root);
    let violations = check_ledger(&manifest, &ledger, &ledger_bytes, &repo_root, &consumed);

    let digest = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(ledger_bytes.as_bytes());
        hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let report = CheckReport {
        schema: LEDGER_SCHEMA.to_string(),
        nodes: manifest.nodes.len(),
        records: ledger.records.len(),
        digest,
        ok: violations.is_empty(),
        violations,
    };
    match format {
        SpecsOutputFormat::Human => print!("{}", render_check(&report)),
        SpecsOutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    if report.ok {
        Ok(())
    } else {
        bail!("{} law violation(s) in {LEDGER_SCHEMA}", report.violations.len());
    }
}

#[derive(Debug, Serialize)]
struct CheckReport {
    schema: String,
    nodes: usize,
    records: usize,
    digest: String,
    ok: bool,
    violations: Vec<LawViolation>,
}

fn render_check(report: &CheckReport) -> String {
    let mut out = String::new();
    let verdict = if report.ok { "PASS" } else { "FAIL" };
    let _ = writeln!(
        out,
        "emacs train specs check ({LEDGER_SCHEMA}): {verdict} — {} node(s), {} record(s), ledger digest sha256:{}",
        report.nodes,
        report.records,
        &report.digest[..16.min(report.digest.len())]
    );
    for violation in &report.violations {
        let _ = writeln!(out, "  {violation}");
    }
    out
}

fn check_ledger(
    manifest: &TrainManifest,
    ledger: &SpecsLedger,
    ledger_bytes: &str,
    repo_root: &Path,
    expected_consumed: &str,
) -> Vec<LawViolation> {
    let mut violations = Vec::new();

    if ledger.schema != LEDGER_SCHEMA || ledger.schema_version != 1 {
        violations.push(law(
            "L01-schema",
            LEDGER_SCHEMA,
            format!(
                "ledger identifies as {} v{}, expected {LEDGER_SCHEMA} v1",
                ledger.schema, ledger.schema_version
            ),
        ));
    }
    // The consumed-manifest reference is anchored to the manifest this check
    // actually parsed; the header never certifies its own source.
    if ledger.programme.consumed_manifest != expected_consumed {
        violations.push(law(
            "L01-schema",
            LEDGER_SCHEMA,
            format!(
                "ledger claims consumed manifest {:?} but this check ran against {expected_consumed:?}",
                ledger.programme.consumed_manifest
            ),
        ));
    }
    let expected_programme = LedgerProgramme::new(expected_consumed);
    if ledger.programme != expected_programme {
        violations.push(law(
            "L01-schema",
            LEDGER_SCHEMA,
            "programme header differs from the fixed emacs-train plane identity",
        ));
    }
    if manifest.schema != MANIFEST_SCHEMA || manifest.schema_version != 1 {
        violations.push(law(
            "L13-manifest-guard",
            MANIFEST_SCHEMA,
            format!(
                "consumed manifest identifies as {} v{}, expected {MANIFEST_SCHEMA} v1",
                manifest.schema, manifest.schema_version
            ),
        ));
    }

    let manifest_ids: BTreeSet<&str> = manifest.nodes.iter().map(|n| n.node_id.as_str()).collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut authorities: BTreeSet<&str> = BTreeSet::new();
    for record in &ledger.records {
        if !seen.insert(record.node_id.as_str()) {
            violations.push(law(
                "L03-exactly-one",
                &record.node_id,
                "duplicate record for one node",
            ));
        }
        let Some(node) = manifest.nodes.iter().find(|n| n.node_id == record.node_id) else {
            violations.push(law(
                "L02-denominator",
                &record.node_id,
                "record names a node absent from the manifest denominator",
            ));
            continue;
        };
        if record.issue != node.issue || record.train_role != node.train_role {
            violations.push(law(
                "L02-denominator",
                &record.node_id,
                format!(
                    "record issue/role ({}/#{}) differ from the manifest ({}/#{})",
                    record.issue, record.train_role, node.issue, node.train_role
                ),
            ));
        }
        if !authorities.insert(record.authority_after.as_str()) {
            violations.push(law(
                "L09-authority-uniqueness",
                &record.node_id,
                format!(
                    "authority_after proposition already claimed: {:?}",
                    record.authority_after
                ),
            ));
        }
        violations.extend(record_violations(record, node, repo_root));
        violations.extend(node_violations(node, manifest));
    }
    for node_id in manifest_ids.difference(&seen) {
        violations.push(law(
            "L02-denominator",
            node_id,
            "stable node carries no compiled disposition (bounded-agent eligibility blocked)",
        ));
    }

    match ledger.to_canonical_bytes() {
        Ok(canonical) if ledger_bytes != canonical => violations.push(law(
            "L12-canonical-bytes",
            LEDGER_SCHEMA,
            "ledger bytes are not the canonical serialization of their content",
        )),
        Err(_) => violations.push(law(
            "L12-canonical-bytes",
            LEDGER_SCHEMA,
            "ledger content cannot be canonically serialized",
        )),
        Ok(_) => {}
    }
    // Canonical bytes are order-bound: the record sequence must appear in
    // manifest node order (a subsequence for partial ledgers; combined with
    // the denominator law this is full equality for complete ledgers), so a
    // reordered ledger has no second valid byte form even when re-serialized
    // canonically.
    let manifest_positions: std::collections::BTreeMap<&str, usize> =
        manifest.nodes.iter().enumerate().map(|(i, n)| (n.node_id.as_str(), i)).collect();
    let record_positions: Vec<usize> = ledger
        .records
        .iter()
        .map(|r| manifest_positions.get(r.node_id.as_str()).copied().unwrap_or(usize::MAX))
        .collect();
    let sorted = record_positions.windows(2).all(|pair| pair[0] < pair[1]);
    if !sorted {
        violations.push(law(
            "L12-canonical-bytes",
            LEDGER_SCHEMA,
            "ledger records are not in manifest node order; canonical bytes are order-bound",
        ));
    }

    violations
}

/// `explain` — deterministic disposition and contract trace for one subject.
pub fn explain(
    subject: &str,
    manifest_path: Option<PathBuf>,
    ledger_path: Option<PathBuf>,
) -> Result<()> {
    let root = project_root()?;
    let manifest_path = manifest_path.unwrap_or_else(|| root.join(DEFAULT_MANIFEST_PATH));
    let ledger_path = ledger_path.unwrap_or_else(|| root.join(DEFAULT_LEDGER_PATH));
    let manifest = read_manifest(&manifest_path)?;
    let node = manifest.resolve(subject)?;
    let consumed = consumed_manifest_ref(&manifest_path, &root);
    let ledger = load_ledger(&ledger_path, &consumed)?;
    let record = ledger.records.iter().find(|r| r.node_id == node.node_id).cloned();

    print!("{}", render_explain(node, record.as_ref()));
    Ok(())
}

fn render_explain(node: &ManifestNode, record: Option<&DispositionRecord>) -> String {
    let mut out = String::new();
    let disposition = record.map(|r| r.disposition).unwrap_or(node.spec.disposition);
    let mut allowed: Vec<&str> =
        node.train_role.allowed_dispositions().iter().map(|d| d.as_str()).collect();
    allowed.extend(["RETURN_TO_ISSUE", "NOT_PROVEN"]);

    let _ = writeln!(
        out,
        "{} (#{}), role {}, lane {}, buildable={}",
        node.node_id, node.issue, node.train_role, node.lane, node.buildable
    );
    match record {
        Some(record) => {
            let _ = writeln!(
                out,
                "disposition: {} [{}]{}",
                record.disposition,
                record.disposition_provenance,
                record
                    .compiled_spec
                    .as_deref()
                    .map(|p| format!(" compiled_spec={p}"))
                    .unwrap_or_default()
            );
            if let Some(reason) = record.reviewed_reason.as_deref() {
                let _ = writeln!(out, "reviewed_reason: {reason}");
            }
        }
        None => {
            let _ = writeln!(
                out,
                "disposition: {} [embedded in {MANIFEST_SCHEMA}; not compiled into the ledger]",
                node.spec.disposition
            );
        }
    }
    let _ = writeln!(out, "allowed for role: {}", allowed.join(", "));
    if disposition.is_builder() {
        let _ = writeln!(
            out,
            "law basis: builder disposition requires buildable=true (node: {})",
            node.buildable
        );
    } else {
        let _ = writeln!(out, "law basis: non-builder disposition; buildability not required");
    }
    if disposition == LeafSpecDisposition::SpecCompiled {
        let _ = writeln!(
            out,
            "law basis: SPEC_COMPILED requires an existing three-file checked bundle"
        );
    }
    if disposition.is_reviewed_exit() {
        let _ = writeln!(out, "law basis: reviewed exit requires a non-empty reviewed_reason");
    }
    let _ = writeln!(out, "proposition: {}", node.one_pr_outcome);
    let _ = writeln!(out, "authority before: {}", node.authority_before);
    let _ = writeln!(out, "authority after: {}", node.authority_after);
    let _ = writeln!(out, "claim ceiling: {}", node.claim_ceiling);
    let _ = writeln!(out, "writer conflict key: {}", node.writer.conflict_key);
    if !node.dependencies.is_empty() {
        let _ = writeln!(out, "dependencies:");
        for dep in &node.dependencies {
            let _ = writeln!(out, "  {} [{}] ({})", dep.target, dep.class, dep.provenance);
        }
    }
    if !node.consumed_authorities.is_empty() {
        let _ = writeln!(out, "consumed authorities: {}", node.consumed_authorities.join(", "));
    }
    let _ = writeln!(out, "allowed components: {}", node.allowed_components.join("; "));
    let _ =
        writeln!(out, "forbidden adjacent owners: {}", node.forbidden_adjacent_owners.join("; "));
    let _ = writeln!(out, "first falsifier: {}", node.first_falsifier);
    let _ = writeln!(
        out,
        "controls: positive={} opposite={} stale={} wrong_subject={} fault={} mutation={}",
        node.controls.positive,
        node.controls.opposite,
        node.controls.stale,
        node.controls.wrong_subject,
        node.controls.fault,
        node.controls.mutation
    );
    let _ = writeln!(out, "proof: focused={} routed={}", node.proof.focused, node.proof.routed);
    let _ = writeln!(
        out,
        "obligations: schema={} generated={} docs={} changelog={} receipt={}",
        node.obligations.schema,
        node.obligations.generated,
        node.obligations.docs,
        node.obligations.changelog,
        node.obligations.receipt
    );
    let _ = writeln!(
        out,
        "exits: old_path={} compatibility={} supersession={} transfer={}",
        node.exits.old_path, node.exits.compatibility, node.exits.supersession, node.exits.transfer
    );
    let _ = writeln!(out, "rollback: {}", node.rollback.rollback);
    let _ = writeln!(out, "return to issue: {}", node.rollback.return_to_issue);
    let _ = writeln!(out, "not proven: {}", node.rollback.not_proven);
    let _ = writeln!(out, "stop: {}", node.rollback.stop);
    if !node.successors.is_empty() {
        let _ = writeln!(out, "successors: {}", node.successors.join(", "));
    }
    let _ = writeln!(out, "identity fields: {}", node.identity_fields.join("; "));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_manifest() -> String {
        let node = |node_id: &str,
                    issue: u64,
                    role: &str,
                    buildable: bool,
                    disposition: &str,
                    authority_after: &str,
                    deps: &str,
                    aliases: &str| {
            format!(
                r##"{{
      "node_id": "{node_id}",
      "issue": {issue},
      "title": "fixture {node_id}",
      "aliases": [{aliases}],
      "train_role": "{role}",
      "lane": "fixture",
      "chain": {{"home": "fixture", "controller": "N_CTRL"}},
      "one_pr_outcome": "fixture proposition for {node_id}",
      "authority_before": "no authority before {node_id}",
      "authority_after": "{authority_after}",
      "buildable": {buildable},
      "dependencies": [{deps}],
      "claim_ceiling": "fixture ceiling for {node_id}",
      "writer": {{"conflict_key": "fixture.{node_id}", "parallel_group": "none", "stack_relation": "none"}},
      "consumed_authorities": ["#3983"],
      "allowed_components": ["fixture component"],
      "forbidden_adjacent_owners": ["adjacent owner"],
      "spec": {{"disposition": "{disposition}", "owner": "{node_id}", "stale_policy": "E01R", "spec_authority": "#11717"}},
      "first_falsifier": "fixture falsifier for {node_id}",
      "controls": {{"positive": "p", "opposite": "o", "stale": "s", "wrong_subject": "w", "fault": "f", "mutation": "m"}},
      "proof": {{"focused": "focused proof", "routed": "routed proof"}},
      "obligations": {{"schema": "none", "generated": "none", "docs": "contract notes", "changelog": "none", "receipt": "none"}},
      "exits": {{"old_path": "none", "compatibility": "none", "supersession": "E01R", "transfer": "E01R"}},
      "rollback": {{"rollback": "revert", "return_to_issue": "return", "not_proven": "stays not_proven", "stop": "stop"}},
      "successors": [],
      "identity_fields": ["fixture identity"],
      "limitations": ["fixture"]
    }}"##
            )
        };

        let ctrl_dep = r#"{"target": "N_IMPL", "class": "hard", "provenance": "fixture"}"#;
        let ext_dep = r##"{"target": "#3983", "class": "evidence", "provenance": "fixture"}"##;
        let nodes = [
            node(
                "N_CTRL",
                9001,
                "controller",
                false,
                "CONTROLLER_NO_CODING_SPEC",
                "controller authority",
                ctrl_dep,
                "",
            ),
            node(
                "N_FAN",
                9002,
                "fan_in",
                false,
                "FAN_IN_OR_CERTIFICATION_SPEC",
                "fan-in authority",
                "",
                "",
            ),
            node(
                "N_EXT",
                9003,
                "external_gate",
                false,
                "EXTERNAL_OR_MANUAL_NO_CODING_SPEC",
                "external authority",
                "",
                "",
            ),
            node(
                "N_HIST",
                9004,
                "historical",
                true,
                "EXISTING_CONTRACT_SUFFICIENT",
                "historical authority",
                "",
                "",
            ),
            node(
                "N_SPEC",
                9005,
                "specification",
                true,
                "SPEC_COMPILED",
                "specification authority",
                ext_dep,
                "",
            ),
            node(
                "N_IMPL",
                9006,
                "implementation",
                true,
                "ISSUE_PLAN_SUFFICIENT",
                "implementation authority",
                ctrl_dep,
                r#""ALIAS_IMPL""#,
            ),
            node(
                "N_RET",
                9007,
                "implementation",
                true,
                "ISSUE_PLAN_SUFFICIENT",
                "return authority",
                "",
                "",
            ),
            node(
                "N_NP",
                9008,
                "implementation",
                true,
                "ISSUE_PLAN_SUFFICIENT",
                "not-proven authority",
                "",
                "",
            ),
            node(
                "N_SUP",
                9009,
                "historical",
                true,
                "HISTORICAL_OR_SUPERSEDED",
                "superseded authority",
                "",
                "",
            ),
        ];
        format!(
            r##"{{"schema": "{MANIFEST_SCHEMA}", "schema_version": 1,
  "programme": {{"parent_programme_issue": 7979, "controller_issue": 8706}},
  "external_authorities": [{{"id": "#3983", "subject": "spec method"}}],
  "nodes": [
    {}
  ]}}"##,
            nodes.join(",\n")
        )
    }

    /// Fixtures mirror the real layout: the manifest lives inside a `.spec`
    /// directory so [`spec_tree_root`] resolves to the fixture root and
    /// bundle discovery finds `<root>/.spec/<issue>-*`.
    fn write_fixture(dir: &Path) -> Result<PathBuf> {
        let spec_dir = dir.join(".spec");
        fs::create_dir_all(&spec_dir)
            .with_context(|| format!("creating fixture spec dir {}", spec_dir.display()))?;
        let manifest_path = spec_dir.join("train.manifest.json");
        fs::write(&manifest_path, fixture_manifest())
            .with_context(|| format!("writing fixture manifest {}", manifest_path.display()))?;
        Ok(manifest_path)
    }

    fn fixture_bundle(dir: &Path, issue: u64) -> Result<PathBuf> {
        let bundle = dir.join(".spec").join(format!("{issue}-fixture"));
        fs::create_dir_all(&bundle)
            .with_context(|| format!("creating fixture bundle {}", bundle.display()))?;
        for member in ["context.md", "acceptance.md", "checklist.md"] {
            fs::write(bundle.join(member), "# fixture\n")
                .with_context(|| format!("writing fixture member {member}"))?;
        }
        Ok(bundle)
    }

    fn compile_one(manifest_path: &Path, ledger_path: &Path, subject: &str) -> Result<()> {
        compile(CompileConfig {
            subject: Some(subject.to_string()),
            all: false,
            manifest_path: Some(manifest_path.to_path_buf()),
            ledger_path: Some(ledger_path.to_path_buf()),
            disposition: None,
            compiled_spec: None,
            reviewed_reason: None,
            readjudicate: false,
        })
    }

    fn tempdir(tag: &str) -> Result<tempfile::TempDir> {
        tempfile::Builder::new()
            .prefix(&format!("emacs-specs-{tag}-"))
            .tempdir()
            .with_context(|| "creating temp dir")
    }

    fn run_check(manifest_path: &Path, ledger_path: &Path) -> Result<Vec<LawViolation>> {
        let manifest = read_manifest(manifest_path)?;
        let bytes = fs::read_to_string(ledger_path).with_context(|| "reading check ledger")?;
        let ledger = SpecsLedger::parse(&bytes)?;
        let repo_root = spec_tree_root(manifest_path)?;
        let consumed = consumed_manifest_ref(manifest_path, &project_root()?);
        Ok(check_ledger(&manifest, &ledger, &bytes, &repo_root, &consumed))
    }

    #[test]
    fn fixture_manifest_parses_with_nine_nodes() -> Result<()> {
        let manifest = TrainManifest::parse(&fixture_manifest())?;
        assert_eq!(manifest.nodes.len(), 9);
        assert_eq!(manifest.nodes[0].node_id, "N_CTRL");
        Ok(())
    }

    #[test]
    fn every_disposition_path_round_trips() -> Result<()> {
        let dir = tempdir("roundtrip")?;
        let manifest_path = write_fixture(dir.path())?;
        fixture_bundle(dir.path(), 9005)?;
        let ledger_path = dir.path().join("specs.ledger.json");
        for subject in
            ["N_CTRL", "N_FAN", "N_EXT", "N_HIST", "N_SPEC", "N_IMPL", "N_RET", "N_NP", "N_SUP"]
        {
            compile_one(&manifest_path, &ledger_path, subject)
                .with_context(|| format!("compiling {subject}"))?;
        }
        let bytes = fs::read_to_string(&ledger_path)?;
        let ledger = SpecsLedger::parse(&bytes)?;
        assert_eq!(ledger.records.len(), 9);
        let dispositions: Vec<&str> =
            ledger.records.iter().map(|r| r.disposition.as_str()).collect();
        for expected in [
            "SPEC_COMPILED",
            "EXISTING_CONTRACT_SUFFICIENT",
            "ISSUE_PLAN_SUFFICIENT",
            "CONTROLLER_NO_CODING_SPEC",
            "FAN_IN_OR_CERTIFICATION_SPEC",
            "EXTERNAL_OR_MANUAL_NO_CODING_SPEC",
            "HISTORICAL_OR_SUPERSEDED",
        ] {
            assert!(
                dispositions.contains(&expected),
                "missing disposition {expected} in {dispositions:?}"
            );
        }
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert!(violations.is_empty(), "full fixture denominator failed check: {violations:?}");
        Ok(())
    }

    #[test]
    fn reviewed_exit_dispositions_require_reason() -> Result<()> {
        let dir = tempdir("exit-reason")?;
        let manifest_path = write_fixture(dir.path())?;
        let ledger_path = dir.path().join("specs.ledger.json");
        let result = compile(CompileConfig {
            subject: Some("N_RET".to_string()),
            all: false,
            manifest_path: Some(manifest_path.clone()),
            ledger_path: Some(ledger_path.clone()),
            disposition: Some(LeafSpecDisposition::ReturnToIssue),
            compiled_spec: None,
            reviewed_reason: None,
            readjudicate: false,
        });
        match result {
            Ok(()) => bail!("RETURN_TO_ISSUE without a reason must fail closed"),
            Err(error) => assert!(
                error.to_string().contains("L08-exit-reason"),
                "expected L08 violation, got: {error}"
            ),
        }
        compile(CompileConfig {
            subject: Some("N_RET".to_string()),
            all: false,
            manifest_path: Some(manifest_path.clone()),
            ledger_path: Some(ledger_path.clone()),
            disposition: Some(LeafSpecDisposition::ReturnToIssue),
            compiled_spec: None,
            reviewed_reason: Some("premise collapsed back to the owning issue".to_string()),
            readjudicate: false,
        })?;
        compile(CompileConfig {
            subject: Some("N_NP".to_string()),
            all: false,
            manifest_path: Some(manifest_path.clone()),
            ledger_path: Some(ledger_path.clone()),
            disposition: Some(LeafSpecDisposition::NotProven),
            compiled_spec: None,
            reviewed_reason: Some("evidence still missing for the fixture".to_string()),
            readjudicate: false,
        })?;
        let violations = run_check(&manifest_path, &ledger_path)?;
        let non_denominator: Vec<&LawViolation> =
            violations.iter().filter(|v| v.law != "L02-denominator").collect();
        assert!(
            non_denominator.is_empty(),
            "reviewed exits with reasons must carry no violation beyond the partial denominator: {non_denominator:?}"
        );
        Ok(())
    }

    #[test]
    fn controller_cannot_become_ordinary_implementation() -> Result<()> {
        let dir = tempdir("controller-role")?;
        let manifest_path = write_fixture(dir.path())?;
        let ledger_path = dir.path().join("specs.ledger.json");
        let result = compile(CompileConfig {
            subject: Some("N_CTRL".to_string()),
            all: false,
            manifest_path: Some(manifest_path.clone()),
            ledger_path: Some(ledger_path.clone()),
            disposition: Some(LeafSpecDisposition::IssuePlanSufficient),
            compiled_spec: None,
            reviewed_reason: None,
            readjudicate: false,
        });
        match result {
            Ok(()) => bail!("controller with a builder disposition must fail closed"),
            Err(error) => assert!(
                error.to_string().contains("L05-role-compatibility"),
                "expected L05 violation, got: {error}"
            ),
        }
        assert!(!ledger_path.exists(), "a rejected compile must not write ledger bytes");
        Ok(())
    }

    #[test]
    fn non_buildable_node_rejects_builder_disposition_by_law() -> Result<()> {
        // A fan_in node carrying a builder disposition is vocabulary-legal but
        // must be rejected by the role and buildability laws, not by parsing.
        let mut value: serde_json::Value = serde_json::from_str(&fixture_manifest())?;
        let nodes = value
            .get_mut("nodes")
            .and_then(|n| n.as_array_mut())
            .ok_or_else(|| color_eyre::eyre::eyre!("fixture nodes missing"))?;
        for node in nodes.iter_mut() {
            if node.get("node_id").and_then(|v| v.as_str()) == Some("N_FAN") {
                node["spec"]["disposition"] = serde_json::json!("ISSUE_PLAN_SUFFICIENT");
            }
        }
        let manifest = TrainManifest::parse(&serde_json::to_string(&value)?)?;
        let fan = manifest
            .nodes
            .iter()
            .find(|n| n.node_id == "N_FAN")
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("N_FAN missing from mutated fixture"))?;
        let violations = node_violations(&fan, &manifest);
        assert!(
            violations.iter().any(|v| v.law == "L05-role-compatibility"),
            "expected L05 for fan_in with builder disposition: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn unknown_disposition_string_fails_closed_at_parse() -> Result<()> {
        let mutated = fixture_manifest().replace(
            "\"disposition\": \"ISSUE_PLAN_SUFFICIENT\"",
            "\"disposition\": \"MAYBE_SOMEDAY\"",
        );
        match TrainManifest::parse(&mutated) {
            Ok(_) => bail!("unknown disposition vocabulary must fail closed"),
            Err(error) => assert!(
                error.to_string().contains("emacs_train.v1"),
                "parse error should name the manifest schema: {error}"
            ),
        }
        Ok(())
    }

    #[test]
    fn missing_disposition_field_fails_closed_at_parse() -> Result<()> {
        let mutated = fixture_manifest().replace(
            "\"disposition\": \"ISSUE_PLAN_SUFFICIENT\"",
            "\"disposition_x\": \"ISSUE_PLAN_SUFFICIENT\"",
        );
        match TrainManifest::parse(&mutated) {
            Ok(_) => bail!("missing disposition must fail closed"),
            Err(_) => Ok(()),
        }
    }

    #[test]
    fn spec_compiled_binding_is_discovered_and_validated() -> Result<()> {
        let dir = tempdir("binding")?;
        let manifest_path = write_fixture(dir.path())?;
        let ledger_path = dir.path().join("specs.ledger.json");

        // No bundle yet: compile of the SPEC_COMPILED node must fail closed.
        match compile_one(&manifest_path, &ledger_path, "N_SPEC") {
            Ok(()) => bail!("SPEC_COMPILED without a bundle must fail closed"),
            Err(error) => assert!(
                error.to_string().contains("no compiled spec"),
                "expected missing-bundle failure, got: {error}"
            ),
        }

        // With the convention bundle present, compile discovers and binds it.
        fixture_bundle(dir.path(), 9005)?;
        compile_one(&manifest_path, &ledger_path, "N_SPEC")?;
        let bytes = fs::read_to_string(&ledger_path)?;
        let ledger = SpecsLedger::parse(&bytes)?;
        let record = ledger
            .records
            .iter()
            .find(|r| r.node_id == "N_SPEC")
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("N_SPEC record missing"))?;
        assert_eq!(record.compiled_spec.as_deref(), Some(".spec/9005-fixture"));

        // A bundle missing a required member must fail the check law.
        fs::remove_file(dir.path().join(".spec").join("9005-fixture").join("checklist.md"))?;
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert!(
            violations.iter().any(|v| v.law == "L07-binding" && v.detail.contains("checklist.md")),
            "expected missing-member L07 violation: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn compiled_spec_flag_only_applies_to_spec_compiled() -> Result<()> {
        let dir = tempdir("flag-scope")?;
        let manifest_path = write_fixture(dir.path())?;
        fixture_bundle(dir.path(), 9005)?;
        let ledger_path = dir.path().join("specs.ledger.json");
        let result = compile(CompileConfig {
            subject: Some("N_IMPL".to_string()),
            all: false,
            manifest_path: Some(manifest_path),
            ledger_path: Some(ledger_path),
            disposition: None,
            compiled_spec: Some(PathBuf::from(".spec/9005-fixture")),
            reviewed_reason: None,
            readjudicate: false,
        });
        match result {
            Ok(()) => bail!("--compiled-spec on a non-SPEC_COMPILED node must fail"),
            Err(error) => assert!(
                error.to_string().contains("only valid for SPEC_COMPILED"),
                "unexpected error: {error}"
            ),
        }
        Ok(())
    }

    #[test]
    fn disposition_change_requires_explicit_readjudication() -> Result<()> {
        let dir = tempdir("readjudicate")?;
        let manifest_path = write_fixture(dir.path())?;
        let ledger_path = dir.path().join("specs.ledger.json");
        compile_one(&manifest_path, &ledger_path, "N_IMPL")?;
        let first = fs::read_to_string(&ledger_path)?;

        // Idempotent recompile is byte-clean.
        compile_one(&manifest_path, &ledger_path, "N_IMPL")?;
        assert_eq!(first, fs::read_to_string(&ledger_path)?);

        // Silent change is rejected.
        let result = compile(CompileConfig {
            subject: Some("N_IMPL".to_string()),
            all: false,
            manifest_path: Some(manifest_path.clone()),
            ledger_path: Some(ledger_path.clone()),
            disposition: Some(LeafSpecDisposition::SpecCompiled),
            compiled_spec: None,
            reviewed_reason: None,
            readjudicate: false,
        });
        match result {
            Ok(()) => bail!("silent disposition change must fail closed"),
            Err(error) => assert!(
                error.to_string().contains("--readjudicate"),
                "expected readjudication requirement, got: {error}"
            ),
        }
        Ok(())
    }

    #[test]
    fn ambiguous_and_unknown_subjects_fail_closed() -> Result<()> {
        let dir = tempdir("subjects")?;
        let manifest_path = write_fixture(dir.path())?;
        let manifest = read_manifest(&manifest_path)?;
        match manifest.resolve("N_MISSING") {
            Ok(_) => bail!("unknown subject must fail closed"),
            Err(error) => assert!(
                error.to_string().contains("resolves to no node"),
                "unexpected error: {error}"
            ),
        }
        match manifest.resolve("9001") {
            Ok(_) => {}
            Err(error) => bail!("issue-number resolution must work: {error}"),
        }
        match manifest.resolve("#9006") {
            Ok(node) => assert_eq!(node.node_id, "N_IMPL"),
            Err(error) => bail!("hashed issue resolution must work: {error}"),
        }
        match manifest.resolve("ALIAS_IMPL") {
            Ok(node) => assert_eq!(node.node_id, "N_IMPL"),
            Err(error) => bail!("alias resolution must work: {error}"),
        }

        // Two nodes sharing one issue number make that issue ambiguous and
        // the manifest itself unparseable.
        let mutated = fixture_manifest().replace(r#""issue": 9007"#, r#""issue": 9006"#);
        let mutated_path = dir.path().join(".spec").join("train.manifest.json");
        fs::write(&mutated_path, mutated)?;
        match read_manifest(&mutated_path) {
            Ok(_) => bail!("duplicate issue numbers must fail closed at parse"),
            Err(error) => assert!(
                error.to_string().contains("duplicate issue"),
                "expected duplicate-issue failure, got: {error}"
            ),
        }
        Ok(())
    }

    #[test]
    fn duplicate_authority_after_is_rejected() -> Result<()> {
        let dir = tempdir("authority")?;
        let mutated =
            fixture_manifest().replace("implementation authority", "controller authority");
        let manifest_path = write_fixture_mutated(dir.path(), mutated)?;
        let ledger_path = dir.path().join("specs.ledger.json");
        compile_one(&manifest_path, &ledger_path, "N_CTRL")?;
        compile_one(&manifest_path, &ledger_path, "N_IMPL")?;
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert!(
            violations.iter().any(|v| v.law == "L09-authority-uniqueness"),
            "expected L09 violation: {violations:?}"
        );
        Ok(())
    }

    fn write_fixture_mutated(dir: &Path, content: String) -> Result<PathBuf> {
        let spec_dir = dir.join(".spec");
        fs::create_dir_all(&spec_dir)
            .with_context(|| format!("creating fixture spec dir {}", spec_dir.display()))?;
        let manifest_path = spec_dir.join("train.manifest.json");
        fs::write(&manifest_path, content)
            .with_context(|| format!("writing fixture manifest {}", manifest_path.display()))?;
        Ok(manifest_path)
    }

    #[test]
    fn live_state_tokens_are_rejected() -> Result<()> {
        let clean = tempdir("live-clean")?;
        let clean_path = write_fixture(clean.path())?;
        let clean_ledger = clean.path().join("specs.ledger.json");
        compile_one(&clean_path, &clean_ledger, "N_IMPL")?;
        let violations = run_check(&clean_path, &clean_ledger)?;
        let non_denominator: Vec<&LawViolation> =
            violations.iter().filter(|v| v.law != "L02-denominator").collect();
        assert!(
            non_denominator.is_empty(),
            "fixture without live state must carry no violation beyond the partial denominator: {non_denominator:?}"
        );

        let sha = tempdir("live-sha")?;
        let sha_manifest = write_fixture_mutated(
            sha.path(),
            fixture_manifest().replace(
                "implementation authority",
                "implementation authority as of deadbeefdeadbeef",
            ),
        )?;
        let manifest = read_manifest(&sha_manifest)?;
        let node = manifest
            .nodes
            .iter()
            .find(|n| n.node_id == "N_IMPL")
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("N_IMPL missing"))?;
        // Compile itself must reject the live-state bytes fail-closed.
        let result = compile_one(&sha_manifest, &sha.path().join("l.json"), "N_IMPL");
        match result {
            Ok(()) => bail!("SHA-like tokens in durable bytes must fail closed at compile"),
            Err(error) => assert!(
                error.to_string().contains("L11-no-live-state"),
                "expected L11 at compile, got: {error}"
            ),
        }
        // And a hand-written ledger carrying them fails the check law too.
        let mut ledger = SpecsLedger::new(".spec/train.manifest.json");
        ledger.records.push(build_record(
            &node,
            node.spec.disposition,
            DispositionProvenance::Manifest,
            None,
            None,
        ));
        let bytes = ledger.to_canonical_bytes()?;
        let ledger_path = sha.path().join("specs.ledger.json");
        fs::write(&ledger_path, &bytes)?;
        let violations = run_check(&sha_manifest, &ledger_path)?;
        assert!(
            violations.iter().any(|v| v.law == "L11-no-live-state"),
            "expected L11 violation: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn non_canonical_ledger_bytes_fail_closed() -> Result<()> {
        let dir = tempdir("canonical")?;
        let manifest_path = write_fixture(dir.path())?;
        let ledger_path = dir.path().join("specs.ledger.json");
        compile_one(&manifest_path, &ledger_path, "N_IMPL")?;
        let canonical = fs::read_to_string(&ledger_path)?;
        // Hand-edited formatting drift.
        let drifted = canonical.replace("  ", "    ");
        fs::write(&ledger_path, drifted)?;
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert!(
            violations.iter().any(|v| v.law == "L12-canonical-bytes"),
            "expected L12 violation: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn ledger_header_is_bound_to_the_checked_manifest() -> Result<()> {
        // The consumed-manifest reference must be anchored to the manifest
        // the check actually parsed; a ledger claiming another source fails
        // closed even when every other header field is canonical.
        let dir = tempdir("header-bind")?;
        let manifest_path = write_fixture(dir.path())?;
        let ledger_path = dir.path().join("specs.ledger.json");
        compile_one(&manifest_path, &ledger_path, "N_IMPL")?;
        let bytes = fs::read_to_string(&ledger_path)?;
        let forged = bytes.replace(
            "\"consumed_manifest\": \".spec/train.manifest.json\"",
            "\"consumed_manifest\": \".spec/other-train/train.manifest.json\"",
        );
        ensure!(forged != bytes, "fixture ledger must carry the expected consumed reference");
        fs::write(&ledger_path, forged)?;
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert!(
            violations.iter().any(|v| v.law == "L01-schema" && v.detail.contains("other-train")),
            "expected the header to be bound to the checked manifest: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn stale_authority_and_owner_fields_are_rejected() -> Result<()> {
        // A train revision that changes a node's authority_after must
        // invalidate the old record: check certifies current authority only.
        let dir = tempdir("stale-authority")?;
        let manifest_path = write_fixture(dir.path())?;
        let ledger_path = dir.path().join("specs.ledger.json");
        compile_one(&manifest_path, &ledger_path, "N_IMPL")?;

        let revised = fixture_manifest().replace(
            "implementation authority",
            "revised implementation authority after a train revision",
        );
        ensure!(revised != fixture_manifest(), "mutation must change the manifest");
        let revised_path = write_fixture_mutated(dir.path(), revised)?;
        let violations = run_check(&revised_path, &ledger_path)?;
        assert!(
            violations.iter().any(|v| v.law == "L10-provenance" && v.detail.contains("stale")),
            "expected a stale-record violation: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn compiled_spec_escape_paths_are_rejected() -> Result<()> {
        let dir = tempdir("escape")?;
        let manifest_path = write_fixture(dir.path())?;
        let ledger_path = dir.path().join("specs.ledger.json");
        for escape in ["../outside-bundle", "target/escape-bundle", "C:/elsewhere/bundle"] {
            let result = compile(CompileConfig {
                subject: Some("N_SPEC".to_string()),
                all: false,
                manifest_path: Some(manifest_path.clone()),
                ledger_path: Some(ledger_path.clone()),
                disposition: None,
                compiled_spec: Some(PathBuf::from(escape)),
                reviewed_reason: None,
                readjudicate: false,
            });
            match result {
                Ok(()) => bail!("escaping --compiled-spec {escape:?} must fail closed"),
                Err(error) => assert!(
                    error.to_string().contains("--compiled-spec"),
                    "expected a binding rejection for {escape:?}, got: {error}"
                ),
            }
        }
        assert!(!ledger_path.exists(), "rejected bindings must not write ledger bytes");

        // A hand-written record carrying an escaping binding fails check.
        let manifest = read_manifest(&manifest_path)?;
        let node = manifest
            .nodes
            .iter()
            .find(|n| n.node_id == "N_SPEC")
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("N_SPEC missing"))?;
        let mut ledger = SpecsLedger::new(".spec/train.manifest.json");
        ledger.records.push(build_record(
            &node,
            LeafSpecDisposition::SpecCompiled,
            DispositionProvenance::Manifest,
            Some("../outside-bundle".to_string()),
            None,
        ));
        let bytes = ledger.to_canonical_bytes()?;
        fs::write(&ledger_path, &bytes)?;
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert!(
            violations.iter().any(|v| v.law == "L07-binding" && v.detail.contains(".spec tree")),
            "expected an escaping-binding violation: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn compile_all_validates_carried_over_records() -> Result<()> {
        // compile --all must refuse to absorb a malformed pre-existing
        // record: the complete final ledger is law-checked before the write.
        let dir = tempdir("all-validate")?;
        let manifest_path = write_fixture(dir.path())?;
        fixture_bundle(dir.path(), 9005)?;
        let ledger_path = dir.path().join("specs.ledger.json");

        // Seed a partial ledger whose N_RET record is a reviewed exit
        // without the required reason (hand-written canonical bytes).
        let manifest = read_manifest(&manifest_path)?;
        let ret = manifest
            .nodes
            .iter()
            .find(|n| n.node_id == "N_RET")
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("N_RET missing"))?;
        let mut seed = SpecsLedger::new(".spec/train.manifest.json");
        seed.records.push(build_record(
            &ret,
            LeafSpecDisposition::ReturnToIssue,
            DispositionProvenance::Adjudicated,
            None,
            None,
        ));
        fs::write(&ledger_path, seed.to_canonical_bytes()?)?;
        let seeded = fs::read_to_string(&ledger_path)?;

        let result = compile(CompileConfig {
            subject: None,
            all: true,
            manifest_path: Some(manifest_path),
            ledger_path: Some(ledger_path.clone()),
            disposition: None,
            compiled_spec: None,
            reviewed_reason: None,
            readjudicate: false,
        });
        match result {
            Ok(()) => bail!("compile --all must fail closed over a malformed carried-over record"),
            Err(error) => assert!(
                error.to_string().contains("L08-exit-reason"),
                "expected the carried-over L08 violation to surface: {error}"
            ),
        }
        assert_eq!(
            seeded,
            fs::read_to_string(&ledger_path)?,
            "a failed compile --all must leave the existing ledger bytes untouched"
        );
        Ok(())
    }

    #[test]
    fn reordered_canonical_ledger_bytes_are_rejected() -> Result<()> {
        // Hand-reordering records and re-serializing canonically must not
        // produce a second valid byte form for the same denominator.
        let dir = tempdir("reorder")?;
        let manifest_path = write_fixture(dir.path())?;
        fixture_bundle(dir.path(), 9005)?;
        let ledger_path = dir.path().join("specs.ledger.json");
        for subject in ["N_CTRL", "N_FAN", "N_IMPL"] {
            compile_one(&manifest_path, &ledger_path, subject)?;
        }
        let bytes = fs::read_to_string(&ledger_path)?;
        let mut ledger = SpecsLedger::parse(&bytes)?;
        ledger.records.reverse();
        let reordered = ledger.to_canonical_bytes()?;
        ensure!(reordered != bytes, "reversal must change the ledger bytes");
        fs::write(&ledger_path, reordered)?;
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert!(
            violations
                .iter()
                .any(|v| v.law == "L12-canonical-bytes" && v.detail.contains("manifest node order")),
            "expected an order violation: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn missing_and_unknown_records_fail_the_denominator() -> Result<()> {
        let dir = tempdir("denominator")?;
        let manifest_path = write_fixture(dir.path())?;
        let ledger_path = dir.path().join("specs.ledger.json");
        compile_one(&manifest_path, &ledger_path, "N_CTRL")?;
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert_eq!(
            violations.iter().filter(|v| v.law == "L02-denominator").count(),
            8,
            "expected one missing-node violation per uncompiled node: {violations:?}"
        );

        // A record naming an unknown node fails closed.
        let manifest = read_manifest(&manifest_path)?;
        let mut ledger = SpecsLedger::new(".spec/train.manifest.json");
        ledger.records.push(DispositionRecord {
            node_id: "N_GHOST".to_string(),
            issue: 9999,
            train_role: TrainRole::Implementation,
            disposition: LeafSpecDisposition::IssuePlanSufficient,
            disposition_provenance: DispositionProvenance::Manifest,
            compiled_spec: None,
            reviewed_reason: None,
            authority_after: "ghost authority".to_string(),
            spec_owner: "N_GHOST".to_string(),
        });
        let bytes = ledger.to_canonical_bytes()?;
        let violations = check_ledger(
            &manifest,
            &ledger,
            &bytes,
            &spec_tree_root(&manifest_path)?,
            ".spec/train.manifest.json",
        );
        assert!(
            violations.iter().any(|v| v.law == "L02-denominator" && v.subject == "N_GHOST"),
            "expected unknown-node violation: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn contract_shape_violations_are_rejected() -> Result<()> {
        let dir = tempdir("shape")?;
        // Empty first falsifier and a duplicate dependency target.
        let mutated = fixture_manifest()
            .replace(
                "\"first_falsifier\": \"fixture falsifier for N_IMPL\"",
                "\"first_falsifier\": \"\"",
            )
            .replace(
                "\"dependencies\": [{\"target\": \"N_IMPL\", \"class\": \"hard\", \"provenance\": \"fixture\"}],\n      \"claim_ceiling\": \"fixture ceiling for N_IMPL\"",
                "\"dependencies\": [{\"target\": \"N_IMPL\", \"class\": \"hard\", \"provenance\": \"fixture\"}, {\"target\": \"N_IMPL\", \"class\": \"optional\", \"provenance\": \"fixture\"}],\n      \"claim_ceiling\": \"fixture ceiling for N_IMPL\"",
            );
        let manifest_path = write_fixture_mutated(dir.path(), mutated)?;
        let manifest = read_manifest(&manifest_path)?;
        let node = manifest
            .nodes
            .iter()
            .find(|n| n.node_id == "N_IMPL")
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("N_IMPL missing"))?;
        let violations = node_violations(&node, &manifest);
        assert!(
            violations
                .iter()
                .any(|v| v.law == "L14-contract-shape" && v.detail.contains("first_falsifier")),
            "expected empty-falsifier violation: {violations:?}"
        );
        assert!(
            violations
                .iter()
                .any(|v| v.law == "L14-contract-shape" && v.detail.contains("duplicate dependency")),
            "expected duplicate-dependency violation: {violations:?}"
        );

        let result = compile_one(&manifest_path, &dir.path().join("l.json"), "N_IMPL");
        match result {
            Ok(()) => bail!("compile must reject contract-shape violations"),
            Err(error) => assert!(
                error.to_string().contains("L14-contract-shape"),
                "expected L14 in compile rejection: {error}"
            ),
        }
        Ok(())
    }

    #[test]
    fn unresolvable_dependency_target_is_rejected() -> Result<()> {
        let dir = tempdir("dep-target")?;
        let mutated = fixture_manifest().replace(
            "{\"target\": \"N_IMPL\", \"class\": \"hard\", \"provenance\": \"fixture\"}",
            "{\"target\": \"N_NOWHERE\", \"class\": \"hard\", \"provenance\": \"fixture\"}",
        );
        let manifest_path = write_fixture_mutated(dir.path(), mutated)?;
        let manifest = read_manifest(&manifest_path)?;
        let node = manifest
            .nodes
            .iter()
            .find(|n| n.node_id == "N_CTRL")
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("N_CTRL missing"))?;
        let violations = node_violations(&node, &manifest);
        assert!(
            violations
                .iter()
                .any(|v| v.law == "L14-contract-shape" && v.detail.contains("N_NOWHERE")),
            "expected unresolvable-target violation: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn compile_all_is_atomic_and_skips_existing_records() -> Result<()> {
        let dir = tempdir("all")?;
        let manifest_path = write_fixture(dir.path())?;
        fixture_bundle(dir.path(), 9005)?;
        let ledger_path = dir.path().join("specs.ledger.json");
        compile(CompileConfig {
            subject: None,
            all: true,
            manifest_path: Some(manifest_path.clone()),
            ledger_path: Some(ledger_path.clone()),
            disposition: None,
            compiled_spec: None,
            reviewed_reason: None,
            readjudicate: false,
        })?;
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert!(
            violations.is_empty(),
            "compile --all must produce a check-clean ledger: {violations:?}"
        );

        // A second --all run skips everything and stays byte-clean.
        let first = fs::read_to_string(&ledger_path)?;
        compile(CompileConfig {
            subject: None,
            all: true,
            manifest_path: Some(manifest_path.clone()),
            ledger_path: Some(ledger_path.clone()),
            disposition: None,
            compiled_spec: None,
            reviewed_reason: None,
            readjudicate: false,
        })?;
        assert_eq!(first, fs::read_to_string(&ledger_path)?);

        // Atomicity: one unbindable SPEC_COMPILED node aborts the whole write.
        let dir2 = tempdir("all-atomic")?;
        let manifest_path2 = write_fixture(dir2.path())?;
        let ledger_path2 = dir2.path().join("specs.ledger.json");
        let result = compile(CompileConfig {
            subject: None,
            all: true,
            manifest_path: Some(manifest_path2.clone()),
            ledger_path: Some(ledger_path2.clone()),
            disposition: None,
            compiled_spec: None,
            reviewed_reason: None,
            readjudicate: false,
        });
        match result {
            Ok(()) => bail!("compile --all with an unbound SPEC_COMPILED node must fail"),
            Err(error) => assert!(
                error.to_string().contains("N_SPEC"),
                "failure must name the unbound node: {error}"
            ),
        }
        assert!(
            !ledger_path2.exists(),
            "a failed compile --all must not write partial ledger bytes"
        );
        Ok(())
    }

    #[test]
    fn compile_is_deterministic_across_identical_trees() -> Result<()> {
        let dir_a = tempdir("det-a")?;
        let dir_b = tempdir("det-b")?;
        let mut ledger_bytes = Vec::new();
        for dir in [dir_a.path(), dir_b.path()] {
            let manifest_path = write_fixture(dir)?;
            fixture_bundle(dir, 9005)?;
            let ledger_path = dir.join("specs.ledger.json");
            compile_one(&manifest_path, &ledger_path, "N_CTRL")?;
            compile_one(&manifest_path, &ledger_path, "N_IMPL")?;
            ledger_bytes.push(fs::read_to_string(&ledger_path)?);
        }
        assert_eq!(
            ledger_bytes[0], ledger_bytes[1],
            "identical trees must produce byte-identical ledger bytes"
        );
        Ok(())
    }

    // ---- real-train tests over the committed E01 manifest ----

    fn real_manifest_path() -> Result<PathBuf> {
        Ok(project_root()?.join(DEFAULT_MANIFEST_PATH))
    }

    #[test]
    fn real_train_manifest_parses_and_embeds_reviewed_dispositions() -> Result<()> {
        let path = real_manifest_path()?;
        let bytes = fs::read_to_string(&path)
            .with_context(|| format!("reading real manifest {}", path.display()))?;
        let manifest = TrainManifest::parse(&bytes)?;
        assert_eq!(manifest.nodes.len(), 55);
        for node in &manifest.nodes {
            assert!(
                node.train_role.allows(node.spec.disposition),
                "real node {} ({}) embedded disposition {} violates the role table",
                node.node_id,
                node.train_role,
                node.spec.disposition
            );
            let violations = node_violations(node, &manifest);
            assert!(
                violations.is_empty(),
                "real node {} violates contract shape: {violations:?}",
                node.node_id
            );
        }
        Ok(())
    }

    #[test]
    fn real_train_nodes_compile_and_fail_closed_on_partial_denominator() -> Result<()> {
        let dir = tempdir("real")?;
        let manifest_path = real_manifest_path()?;
        let ledger_path = dir.path().join("specs.ledger.json");
        // Representative implementation, controller and fan-in round-trip.
        for subject in ["CTRL", "SUBJ_FAN", "RUNCONF", "E01R", "COHORT", "E02"] {
            compile_one(&manifest_path, &ledger_path, subject)
                .with_context(|| format!("compiling real node {subject}"))?;
        }
        let bytes = fs::read_to_string(&ledger_path)?;
        let ledger = SpecsLedger::parse(&bytes)?;
        let ctrl = ledger
            .records
            .iter()
            .find(|r| r.node_id == "CTRL")
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("CTRL record missing"))?;
        assert_eq!(ctrl.disposition, LeafSpecDisposition::ControllerNoCodingSpec);
        // Partial denominators fail closed (missing-node law) with exactly the
        // unpopulated remainder named.
        let violations = run_check(&manifest_path, &ledger_path)?;
        assert!(
            violations.iter().all(|v| v.law == "L02-denominator"),
            "only missing-node violations are expected: {violations:?}"
        );
        assert_eq!(
            violations.len(),
            49,
            "expected 49 missing-node violations for the unpopulated remainder: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn real_train_spec_compiled_nodes_bind_to_landed_bundles() -> Result<()> {
        let dir = tempdir("real-bind")?;
        let manifest_path = real_manifest_path()?;
        let ledger_path = dir.path().join("specs.ledger.json");
        compile_one(&manifest_path, &ledger_path, "E00").with_context(|| "compiling E00")?;
        let bytes = fs::read_to_string(&ledger_path)?;
        let ledger = SpecsLedger::parse(&bytes)?;
        let record = ledger
            .records
            .iter()
            .find(|r| r.node_id == "E00")
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("E00 record missing"))?;
        assert_eq!(
            record.compiled_spec.as_deref(),
            Some(".spec/11716-emacs-support-architecture"),
            "E00 must auto-bind its landed bundle"
        );
        Ok(())
    }

    #[test]
    fn real_train_compile_all_fails_closed_on_unbound_spec_compiled() -> Result<()> {
        let dir = tempdir("real-all")?;
        let manifest_path = real_manifest_path()?;
        let ledger_path = dir.path().join("specs.ledger.json");
        let result = compile(CompileConfig {
            subject: None,
            all: true,
            manifest_path: Some(manifest_path),
            ledger_path: Some(ledger_path.clone()),
            disposition: None,
            compiled_spec: None,
            reviewed_reason: None,
            readjudicate: false,
        });
        match result {
            Ok(()) => bail!(
                "compile --all over the real train must fail closed while RELY (#11766) has no landed bundle"
            ),
            Err(error) => {
                assert!(error.to_string().contains("RELY"), "failure must name RELY: {error}")
            }
        }
        assert!(
            !ledger_path.exists(),
            "failed real-train compile --all must not write partial bytes"
        );
        Ok(())
    }

    // Simplify-review DRIFT_RISK repair: `integration emacs train specs check`
    // had no automated run against the real tree — every other specs test
    // compiles synthetic ledgers first. The shipped ledger must pass the full
    // L01-L14 law table (mirrors `train_edge_contract`'s real-tree run).
    #[test]
    fn check_passes_on_current_tree() -> Result<()> {
        check(None, None, SpecsOutputFormat::Json)
    }

    // Simplify-review DRIFT_RISK repair: the ledger plane (this module,
    // `emacs_train_specs.v1`) and the mappings plane
    // (`emacs_train_context::resolve`, `emacs_train_context_mappings.v1`)
    // never read each other; their consistency was asserted in PR prose
    // only. Both shipped documents are loaded through their existing typed
    // models and joined per node. Agreement predicate:
    //
    // 1. join: every mappings entry names exactly one ledger record (the
    //    mappings plane may cover a subset of the ledger denominator —
    //    absent nodes carry the resolver's implicit engine blocker — but it
    //    may never name an unadjudicated node);
    // 2. mapped ⇒ artifact-backed: a node whose exact-tree population is
    //    mapped must carry a ledger disposition that claims existing
    //    artifacts (`SPEC_COMPILED` / `EXISTING_CONTRACT_SUFFICIENT`). A
    //    deferred (`ISSUE_PLAN_SUFFICIENT`), aggregated
    //    (`FAN_IN_OR_CERTIFICATION_SPEC`), no-coding
    //    (`CONTROLLER_NO_CODING_SPEC`/`EXTERNAL_OR_MANUAL_NO_CODING_SPEC`),
    //    historical or reviewed-exit record would deny the surfaces the
    //    mapping anchors — that is the drift this check exists to catch.
    //    The converse is intentionally NOT asserted: a `SPEC_COMPILED` node
    //    with a still-unmapped population is an agreed state (a spec bundle
    //    can exist before the implementation lands);
    // 3. unmapped ⇒ same governing issue: an unmapped entry's blocker owner
    //    must equal the issue the ledger record carries — with the closed
    //    `return_to_issue` action, the missing population returns to the
    //    node's own governing issue, so a blocker naming any other issue is
    //    a stale-owner disagreement between the planes.
    #[test]
    fn mappings_and_ledger_planes_agree_on_shipped_documents() -> Result<()> {
        use crate::tasks::emacs_train_context::model::MappingDocument;
        use crate::tasks::emacs_train_context::resolve::MAPPING_RELATIVE_PATH;
        use std::collections::BTreeMap;

        let root = project_root()?;
        let ledger = SpecsLedger::parse(&fs::read_to_string(root.join(DEFAULT_LEDGER_PATH))?)?;
        let mapping_bytes = fs::read_to_string(root.join(MAPPING_RELATIVE_PATH))?;
        let mappings: MappingDocument = serde_json::from_str(&mapping_bytes)?;
        assert!(!mappings.nodes.is_empty(), "shipped mappings document has no nodes");

        let by_node: BTreeMap<&str, &DispositionRecord> =
            ledger.records.iter().map(|record| (record.node_id.as_str(), record)).collect();
        for entry in &mappings.nodes {
            let record = by_node.get(entry.node_id.as_str()).ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "mappings node {} has no ledger record: the planes disagree on the denominator",
                    entry.node_id
                )
            })?;
            match entry.status.as_str() {
                "mapped" => {
                    assert!(
                        matches!(
                            record.disposition,
                            LeafSpecDisposition::SpecCompiled
                                | LeafSpecDisposition::ExistingContractSufficient
                        ),
                        "node {} is mapped in the context plane but its ledger disposition {} \
                         does not claim existing artifacts",
                        entry.node_id,
                        record.disposition
                    );
                    assert!(
                        entry.blocker.is_none(),
                        "node {} is mapped but still carries a blocker",
                        entry.node_id
                    );
                }
                "unmapped" => {
                    let blocker = entry.blocker.as_ref().ok_or_else(|| {
                        color_eyre::eyre::eyre!(
                            "unmapped node {} carries no blocker (mappings L08)",
                            entry.node_id
                        )
                    })?;
                    assert_eq!(
                        blocker.owner_issue, record.issue,
                        "unmapped node {} names blocker owner issue {} while both planes govern \
                         issue {}: stale owner disagreement",
                        entry.node_id, blocker.owner_issue, record.issue
                    );
                }
                other => bail!("unknown mapping status {other:?} for node {}", entry.node_id),
            }
        }
        Ok(())
    }
}
