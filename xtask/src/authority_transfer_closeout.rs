//! Exact-head leaf-closeout verification and canonical handoff emission for
//! the authority-transfer programme (issue #11703).
//!
//! One bounded leaf candidate is verified against its exact builder and
//! reviewer packets and the programme binds recorded in one closeout request.
//! Every bind is conjunctive: subject identity, common ancestry, cumulative
//! diff scope, packet/artifact digests, packet currentness against the
//! repository's current-main subject and the current issue/ruling revision,
//! nonzero selected-and-executed proof, load-bearing negative controls,
//! required durable-artifact changes, forbidden-surface exclusion, a satisfied
//! predecessor or compatibility exit, and exactly one correctly related
//! terminal issue. A violated bind is never green.
//!
//! The emitted handoff is derived from checked state only. It can never close
//! the controller, the parent, or any semantic-completion authority, and it
//! never authorizes a merge, a release, or a GitHub mutation. Absence of an
//! optional live observation leaves offline closeout validity untouched.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Versioned closeout-request schema accepted by this verifier.
pub const CLOSEOUT_REQUEST_SCHEMA_V1: &str = "authority_transfer_leaf_closeout_request.v1";
/// Versioned canonical-handoff schema emitted on a green closeout.
pub const LEAF_HANDOFF_SCHEMA_V1: &str = "authority_transfer_leaf_handoff.v1";
/// Pinned offline regression fixtures directory (repository-relative).
pub const FIXTURE_DIR: &str = "fixtures/authority_transfer_closeout";

/// Process exit code for a green closeout with an emitted handoff.
pub const EXIT_LEAF_READY: i32 = 0;
/// Process exit code for any deterministic non-green closeout result.
pub const EXIT_NON_GREEN: i32 = 2;
/// Process exit code when an evidence producer failed before a domain result
/// could be proved.
pub const EXIT_INSTRUMENT_FAILURE: i32 = 3;

/// Upper bound for one artifact body accepted for hashing.
const MAX_ARTIFACT_BYTES: usize = 8 * 1024 * 1024;

/// Closed result vocabulary of the leaf closeout.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CloseoutResult {
    LeafReady,
    LeafIncomplete,
    StalePacket,
    WrongSubject,
    ScopeBreach,
    ProofNotProven,
    ArtifactMissing,
    PredecessorStillReachable,
    ClaimCeilingExceeded,
    ControllerCloseRequestRejected,
    ContractDrift,
    InstrumentFailure,
}

impl CloseoutResult {
    /// Stable machine spelling used by human and JSON projections.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LeafReady => "LEAF_READY",
            Self::LeafIncomplete => "LEAF_INCOMPLETE",
            Self::StalePacket => "STALE_PACKET",
            Self::WrongSubject => "WRONG_SUBJECT",
            Self::ScopeBreach => "SCOPE_BREACH",
            Self::ProofNotProven => "PROOF_NOT_PROVEN",
            Self::ArtifactMissing => "ARTIFACT_MISSING",
            Self::PredecessorStillReachable => "PREDECESSOR_STILL_REACHABLE",
            Self::ClaimCeilingExceeded => "CLAIM_CEILING_EXCEEDED",
            Self::ControllerCloseRequestRejected => "CONTROLLER_CLOSE_REQUEST_REJECTED",
            Self::ContractDrift => "CONTRACT_DRIFT",
            Self::InstrumentFailure => "INSTRUMENT_FAILURE",
        }
    }

    /// Stable process exit code for shell and workflow consumers.
    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            Self::LeafReady => EXIT_LEAF_READY,
            Self::InstrumentFailure => EXIT_INSTRUMENT_FAILURE,
            _ => EXIT_NON_GREEN,
        }
    }

    /// Deterministic report precedence. A lower number wins when several binds
    /// are violated at once, so the reported cause cannot drift between runs.
    #[must_use]
    fn rank(self) -> u8 {
        match self {
            Self::ContractDrift => 0,
            Self::WrongSubject => 1,
            Self::StalePacket => 2,
            Self::ArtifactMissing => 3,
            Self::ScopeBreach => 4,
            Self::ClaimCeilingExceeded => 5,
            Self::ControllerCloseRequestRejected => 6,
            Self::ProofNotProven => 7,
            Self::PredecessorStillReachable => 8,
            Self::InstrumentFailure => 9,
            Self::LeafIncomplete => 10,
            Self::LeafReady => 11,
        }
    }
}

/// Closed claim-ceiling vocabulary. Only a bounded leaf may be closed out.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimCeiling {
    Leaf,
    Parent,
    Controller,
}

/// Roles of the externally supplied artifacts bound by digest.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    ProgrammeManifest,
    ProbeObservation,
    Frontier,
    BuilderPacket,
    ReviewerPacket,
    ProofProfile,
}

/// Execution state of one selected proof item.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofStatus {
    Passed,
    Failed,
    Skipped,
    InstrumentFailure,
}

/// Satisfied exit shape for a retired or compatibility-projected predecessor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PredecessorExitMode {
    Retired,
    CompatibilityProjection,
}

/// Optional live-candidate observation state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCandidateState {
    NotObserved,
    Observed,
}

/// Repository-local issue reference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IssueRef {
    pub repository: String,
    pub number: u64,
}

/// Identity of the bounded leaf being closed out.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LeafIdentity {
    pub node_id: String,
    pub issue: IssueRef,
    pub controller_issue: IssueRef,
    pub claim_ceiling: ClaimCeiling,
    pub conflict_key: String,
}

/// Exact candidate subject under closeout.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateSubject {
    pub base_sha: String,
    pub head_sha: String,
}

/// Currentness binds carried by the packets.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PacketSubject {
    /// Repository current-main subject the packets were composed against.
    pub current_main_sha: String,
    /// Issue/ruling revision the packets were composed against.
    pub packet_ruling_revision: String,
}

/// Declared digest binds of the closeout.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DigestBinds {
    pub programme_manifest_sha256: String,
    pub probe_observation_sha256: String,
    pub frontier_sha256: String,
    pub builder_packet_sha256: String,
    pub reviewer_packet_sha256: String,
    pub proof_profile_sha256: String,
    /// Current issue/ruling revision the closeout is checked against.
    pub issue_ruling_revision: String,
}

/// One externally supplied artifact body bound by a digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactInput {
    pub role: ArtifactRole,
    /// Repository-relative path the artifact was read from.
    pub path: String,
    /// Raw UTF-8 body of the artifact.
    pub contents: String,
}

/// Durable surface changes the leaf is required to contain.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RequiredChanges {
    #[serde(default)]
    pub specs: Vec<String>,
    #[serde(default)]
    pub fixtures: Vec<String>,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub schemas: Vec<String>,
    #[serde(default)]
    pub generated: Vec<String>,
    #[serde(default)]
    pub docs: Vec<String>,
    #[serde(default)]
    pub receipts: Vec<String>,
}

impl RequiredChanges {
    fn iter(&self) -> impl Iterator<Item = &String> {
        [
            &self.specs,
            &self.fixtures,
            &self.tests,
            &self.schemas,
            &self.generated,
            &self.docs,
            &self.receipts,
        ]
        .into_iter()
        .flatten()
    }
}

/// Selected proof work with its execution results and negative controls.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProofProfile {
    pub selected: Vec<String>,
    pub executed: BTreeMap<String, ProofStatus>,
    pub negative_controls: Vec<NegativeControl>,
    /// Identity of the required first falsifier.
    pub first_falsifier_id: String,
    /// Whether required generated outputs are current for this generation.
    pub generated_outputs_current: bool,
    /// Identities proving the generated outputs are current.
    pub generated_identities: Vec<String>,
    /// Whether the leaf claims connected/installed behavior.
    pub claims_installed_behavior: bool,
    /// Observation digest proving installed behavior; required when claimed.
    pub installed_observation_sha256: Option<String>,
}

/// One load-bearing negative control.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NegativeControl {
    pub control_id: String,
    /// The control demonstrably failed before the intended implementation.
    pub red_before_evidence: bool,
    /// Only the intended implementation satisfies the control.
    pub passes_only_intended_implementation: bool,
    /// The control was executed against this candidate's exact head.
    pub subject_matches_candidate: bool,
}

/// Satisfied predecessor or compatibility exit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PredecessorExit {
    pub mode: PredecessorExitMode,
    pub predecessor_ids: Vec<String>,
    /// A predecessor that is still independently reachable hides the transfer.
    pub independently_reachable: bool,
    /// Owner and exit identity of a compatibility projection.
    pub compatibility_exit_identity: Option<String>,
}

/// Optional read-only live observation; absence stays offline-valid.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LiveObservation {
    pub candidate_state: LiveCandidateState,
    pub observation_digest: Option<String>,
}

/// The complete closeout request document.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloseoutRequest {
    pub schema: String,
    pub schema_version: u32,
    pub request_id: String,
    pub repository: String,
    pub identity: LeafIdentity,
    pub candidate: CandidateSubject,
    pub packet_subject: PacketSubject,
    pub digest_binds: DigestBinds,
    pub artifacts: Vec<ArtifactInput>,
    /// Terminal relations the leaf PR claims; must be exactly the leaf issue.
    pub claimed_closes: Vec<IssueRef>,
    /// Additional issues that must never appear in a terminal relation.
    pub forbidden_terminal_issues: Vec<IssueRef>,
    /// Authority transfers claimed by this leaf; the leaf ceiling allows one.
    pub authority_transfers_claimed: Vec<String>,
    /// Surfaces this leaf must never touch.
    pub forbidden_surfaces: Vec<String>,
    pub required_changes: RequiredChanges,
    pub proof_profile: ProofProfile,
    pub predecessor_exit: PredecessorExit,
    /// Explicitly unestablished claims; a canonical handoff always names some.
    pub limitations: Vec<String>,
    pub live_observation: Option<LiveObservation>,
}

/// Git observations backing one closeout evaluation.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct GitFacts {
    pub base_resolved: Option<String>,
    pub head_resolved: Option<String>,
    /// Merge base of base and head, when one exists.
    pub merge_base: Option<String>,
    /// Cumulative diff paths of base..head.
    pub changed_paths: BTreeSet<String>,
    /// Resolved current-main subject.
    pub main_resolved: Option<String>,
    /// Evidence limitations that kept stronger facts unprovable.
    pub limitations: Vec<String>,
}

/// Outcome of one closeout evaluation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CloseoutOutcome {
    pub result: CloseoutResult,
    /// Every violated bind, ordered by deterministic precedence.
    pub reasons: Vec<String>,
    /// Canonical handoff; present only when the result is green.
    pub handoff: Option<LeafHandoff>,
}

/// Canonical handoff derived from checked state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LeafHandoff {
    pub schema: &'static str,
    pub schema_version: u32,
    pub request_id: String,
    pub node_id: String,
    pub issue: IssueRef,
    pub controller_issue: IssueRef,
    pub base_sha: String,
    pub head_sha: String,
    pub bounded_claim: BoundedClaim,
    pub authorities_changed: Vec<String>,
    pub authorities_retired: Vec<String>,
    pub durable_artifacts_changed: Vec<String>,
    pub proof_identities: Vec<String>,
    pub negative_control_identities: Vec<String>,
    pub limitations: Vec<String>,
    pub predecessor_status: String,
    /// The leaf PR may close exactly this issue; nothing else is mutated.
    pub github_relation: GithubRelation,
    /// The completion law reserves controller completion for the controller.
    pub controller_complete_emitted: bool,
    pub next_handoff: String,
    pub warnings: Vec<String>,
    pub cleanup: Vec<String>,
}

/// Bounded claim identity repeated in the handoff.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BoundedClaim {
    pub claim_ceiling: ClaimCeiling,
    pub conflict_key: String,
    pub authority_transfers: Vec<String>,
}

/// Typed GitHub relation asserted by the handoff.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GithubRelation {
    pub closes: Vec<IssueRef>,
    pub controller_remains_open: bool,
    pub merge_authorized: bool,
}

struct Violation {
    result: CloseoutResult,
    reason: String,
}

impl Violation {
    fn new(result: CloseoutResult, reason: impl Into<String>) -> Self {
        Self { result, reason: reason.into() }
    }
}

fn is_sha_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Git object names are SHA-1 (40 hex) by default and SHA-256 (64 hex) in
/// object-format-256 repositories; both spellings are exact identities.
fn is_git_object_name(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn digest_binds(request: &CloseoutRequest) -> [(ArtifactRole, &str); 6] {
    [
        (ArtifactRole::ProgrammeManifest, request.digest_binds.programme_manifest_sha256.as_str()),
        (ArtifactRole::ProbeObservation, request.digest_binds.probe_observation_sha256.as_str()),
        (ArtifactRole::Frontier, request.digest_binds.frontier_sha256.as_str()),
        (ArtifactRole::BuilderPacket, request.digest_binds.builder_packet_sha256.as_str()),
        (ArtifactRole::ReviewerPacket, request.digest_binds.reviewer_packet_sha256.as_str()),
        (ArtifactRole::ProofProfile, request.digest_binds.proof_profile_sha256.as_str()),
    ]
}

fn validate_request(request: &CloseoutRequest) -> Vec<Violation> {
    let mut violations = Vec::new();
    if request.schema != CLOSEOUT_REQUEST_SCHEMA_V1 {
        violations.push(Violation::new(
            CloseoutResult::ContractDrift,
            format!("request schema `{}` is not `{CLOSEOUT_REQUEST_SCHEMA_V1}`", request.schema),
        ));
    }
    if request.schema_version != 1 {
        violations.push(Violation::new(
            CloseoutResult::ContractDrift,
            format!("unsupported request schema_version {}", request.schema_version),
        ));
    }
    if request.request_id.trim().is_empty() {
        violations
            .push(Violation::new(CloseoutResult::ContractDrift, "request_id must not be empty"));
    }
    if request.repository.trim().is_empty() || !request.repository.contains('/') {
        violations.push(Violation::new(
            CloseoutResult::ContractDrift,
            "repository must be spelled `owner/name`",
        ));
    }
    for (label, sha) in [
        ("candidate.base_sha", &request.candidate.base_sha),
        ("candidate.head_sha", &request.candidate.head_sha),
        ("packet_subject.current_main_sha", &request.packet_subject.current_main_sha),
    ] {
        if !is_git_object_name(sha) {
            violations.push(Violation::new(
                CloseoutResult::ContractDrift,
                format!("{label} `{sha}` is not a full git object name"),
            ));
        }
    }
    let mut seen_roles = BTreeSet::new();
    for artifact in &request.artifacts {
        if !seen_roles.insert(artifact.role) {
            violations.push(Violation::new(
                CloseoutResult::ContractDrift,
                format!("artifact role {:?} is supplied more than once", artifact.role),
            ));
        }
    }
    for (role, digest) in digest_binds(request) {
        if !is_sha_hex(digest) {
            violations.push(Violation::new(
                CloseoutResult::ContractDrift,
                format!("digest bind for {role:?} is not a SHA-256 digest"),
            ));
        }
    }
    if request.identity.node_id.trim().is_empty() || request.identity.conflict_key.trim().is_empty()
    {
        violations.push(Violation::new(
            CloseoutResult::ContractDrift,
            "identity node_id and conflict_key must not be empty",
        ));
    }
    let mut seen_controls = BTreeSet::new();
    for control in &request.proof_profile.negative_controls {
        if !seen_controls.insert(control.control_id.as_str()) {
            violations.push(Violation::new(
                CloseoutResult::ContractDrift,
                format!("negative control `{}` is declared twice", control.control_id),
            ));
        }
    }
    violations
}

fn artifact_digests(
    request: &CloseoutRequest,
) -> Result<BTreeMap<ArtifactRole, String>, Violation> {
    let mut digests = BTreeMap::new();
    for artifact in &request.artifacts {
        if artifact.contents.len() > MAX_ARTIFACT_BYTES {
            return Err(Violation::new(
                CloseoutResult::InstrumentFailure,
                format!("artifact {} exceeds the accepted input size", artifact.path),
            ));
        }
        let mut hasher = Sha256::new();
        hasher.update(artifact.contents.as_bytes());
        let digest: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        digests.insert(artifact.role, digest);
    }
    Ok(digests)
}

fn bind_artifacts(request: &CloseoutRequest) -> Vec<Violation> {
    let digests = match artifact_digests(request) {
        Ok(digests) => digests,
        Err(violation) => return vec![violation],
    };
    let mut violations = Vec::new();
    for (role, expected) in digest_binds(request) {
        match digests.get(&role) {
            None => violations.push(Violation::new(
                CloseoutResult::ArtifactMissing,
                format!("artifact body for {role:?} was not supplied"),
            )),
            Some(actual) if actual != expected => violations.push(Violation::new(
                CloseoutResult::StalePacket,
                format!(
                    "artifact {role:?} digests to {actual}, not the bound {expected}; the packet is stale or tampered"
                ),
            )),
            Some(_) => {}
        }
    }
    violations
}

fn bind_subject(request: &CloseoutRequest, facts: &GitFacts) -> Vec<Violation> {
    let mut violations = Vec::new();
    for limitation in &facts.limitations {
        violations.push(Violation::new(CloseoutResult::InstrumentFailure, limitation.clone()));
    }
    match (&facts.base_resolved, &facts.head_resolved) {
        (Some(base), Some(head)) if base == head => violations.push(Violation::new(
            CloseoutResult::LeafIncomplete,
            "cumulative diff of base..head is empty; no leaf candidate exists",
        )),
        (Some(base), Some(head)) => match &facts.merge_base {
            Some(merge_base) if merge_base == base => {}
            Some(_) => violations.push(Violation::new(
                CloseoutResult::WrongSubject,
                format!(
                    "requested base {} is not an ancestor of requested head {}; common ancestry fails",
                    base, head
                ),
            )),
            None => violations.push(Violation::new(
                CloseoutResult::WrongSubject,
                "base and head share no common ancestor; the exact-head bind cannot hold",
            )),
        },
        _ => violations.push(Violation::new(
            CloseoutResult::WrongSubject,
            "the requested base or head does not resolve locally; the exact-head bind is unestablished",
        )),
    }
    match &facts.main_resolved {
        Some(main) if *main != request.packet_subject.current_main_sha => {
            violations.push(Violation::new(
                CloseoutResult::StalePacket,
                format!(
                    "packets were composed against main {}, but the repository currently resolves {}",
                    request.packet_subject.current_main_sha, main
                ),
            ));
        }
        Some(_) => {}
        None => violations.push(Violation::new(
            CloseoutResult::InstrumentFailure,
            "the repository current-main subject could not be resolved",
        )),
    }
    if request.packet_subject.packet_ruling_revision != request.digest_binds.issue_ruling_revision {
        violations.push(Violation::new(
            CloseoutResult::StalePacket,
            format!(
                "packets predate the current ruling revision {}: they were composed against {}",
                request.digest_binds.issue_ruling_revision,
                request.packet_subject.packet_ruling_revision
            ),
        ));
    }
    violations
}

fn bind_scope(request: &CloseoutRequest, facts: &GitFacts) -> Vec<Violation> {
    let mut violations = Vec::new();
    for surface in &request.forbidden_surfaces {
        let prefix = normalize_path(surface);
        let touched = facts.changed_paths.iter().filter(|path| path.starts_with(&prefix)).count();
        if touched > 0 {
            violations.push(Violation::new(
                CloseoutResult::ScopeBreach,
                format!("forbidden surface `{surface}` is touched by {touched} changed path(s)"),
            ));
        }
    }
    for required in request.required_changes.iter() {
        if !facts.changed_paths.contains(&normalize_path(required)) {
            violations.push(Violation::new(
                CloseoutResult::ArtifactMissing,
                format!("required change `{required}` is absent from the cumulative diff"),
            ));
        }
    }
    violations
}

fn same_issue(left: &IssueRef, right: &IssueRef) -> bool {
    left.repository == right.repository && left.number == right.number
}

fn bind_ceiling_and_relations(request: &CloseoutRequest) -> Vec<Violation> {
    let mut violations = Vec::new();
    if request.identity.claim_ceiling != ClaimCeiling::Leaf {
        violations.push(Violation::new(
            CloseoutResult::ClaimCeilingExceeded,
            "only a leaf claim ceiling may be closed out by this verifier",
        ));
    }
    if request.authority_transfers_claimed.len() != 1 {
        violations.push(Violation::new(
            CloseoutResult::ClaimCeilingExceeded,
            format!(
                "exactly one authority transfer must be claimed, found {}",
                request.authority_transfers_claimed.len()
            ),
        ));
    }
    if request.claimed_closes.len() != 1 {
        violations.push(Violation::new(
            CloseoutResult::ControllerCloseRequestRejected,
            format!(
                "the leaf PR must claim exactly one terminal relation, found {}",
                request.claimed_closes.len()
            ),
        ));
    } else {
        let claimed = &request.claimed_closes[0];
        let targets_controller_or_parent = same_issue(claimed, &request.identity.controller_issue)
            || request
                .forbidden_terminal_issues
                .iter()
                .any(|forbidden| same_issue(forbidden, claimed));
        if targets_controller_or_parent {
            violations.push(Violation::new(
                CloseoutResult::ControllerCloseRequestRejected,
                format!(
                    "terminal relation targets controller or parent authority {}/{}; a leaf closeout may never close it",
                    claimed.repository, claimed.number
                ),
            ));
        } else if !same_issue(claimed, &request.identity.issue) {
            violations.push(Violation::new(
                CloseoutResult::WrongSubject,
                format!(
                    "terminal relation targets {}/{}, not the leaf issue {}/{}",
                    claimed.repository,
                    claimed.number,
                    request.identity.issue.repository,
                    request.identity.issue.number
                ),
            ));
        }
    }
    violations
}

fn bind_proof(request: &CloseoutRequest) -> Vec<Violation> {
    let mut violations = Vec::new();
    let profile = &request.proof_profile;
    if profile.selected.is_empty() {
        violations.push(Violation::new(
            CloseoutResult::ProofNotProven,
            "zero proof work items are selected",
        ));
    }
    if profile.executed.is_empty() {
        violations.push(Violation::new(
            CloseoutResult::ProofNotProven,
            "zero proof work items were executed",
        ));
    }
    for id in &profile.selected {
        match profile.executed.get(id) {
            None => violations.push(Violation::new(
                CloseoutResult::ProofNotProven,
                format!("selected proof `{id}` was never executed"),
            )),
            Some(ProofStatus::Failed) => violations.push(Violation::new(
                CloseoutResult::ProofNotProven,
                format!("selected proof `{id}` failed"),
            )),
            Some(ProofStatus::Skipped) => violations.push(Violation::new(
                CloseoutResult::ProofNotProven,
                format!("selected proof `{id}` was skipped"),
            )),
            Some(ProofStatus::InstrumentFailure) => violations.push(Violation::new(
                CloseoutResult::InstrumentFailure,
                format!("selected proof `{id}` ended in instrument failure"),
            )),
            Some(ProofStatus::Passed) => {}
        }
    }
    for id in profile.executed.keys() {
        if !profile.selected.contains(id) {
            violations.push(Violation::new(
                CloseoutResult::ProofNotProven,
                format!("executed proof `{id}` is not part of the selected profile"),
            ));
        }
    }
    if profile.negative_controls.is_empty() {
        violations.push(Violation::new(
            CloseoutResult::ProofNotProven,
            "no negative controls are declared",
        ));
    }
    if !profile
        .negative_controls
        .iter()
        .any(|control| control.control_id == profile.first_falsifier_id)
    {
        violations.push(Violation::new(
            CloseoutResult::ProofNotProven,
            format!(
                "required first falsifier `{}` is absent from the negative controls",
                profile.first_falsifier_id
            ),
        ));
    }
    for control in &profile.negative_controls {
        if !control.red_before_evidence || !control.passes_only_intended_implementation {
            violations.push(Violation::new(
                CloseoutResult::ProofNotProven,
                format!(
                    "negative control `{}` is not load-bearing (red-before evidence and single-implementation discrimination are required)",
                    control.control_id
                ),
            ));
        }
        if !control.subject_matches_candidate {
            violations.push(Violation::new(
                CloseoutResult::WrongSubject,
                format!(
                    "negative control `{}` was executed against another head or denominator",
                    control.control_id
                ),
            ));
        }
    }
    if profile.generated_outputs_current && profile.generated_identities.is_empty() {
        violations.push(Violation::new(
            CloseoutResult::ArtifactMissing,
            "generated outputs claim currency without any generation identity",
        ));
    }
    if !profile.generated_outputs_current {
        violations.push(Violation::new(
            CloseoutResult::StalePacket,
            "required generated outputs are stale for this generation",
        ));
    }
    if profile.claims_installed_behavior && profile.installed_observation_sha256.is_none() {
        violations.push(Violation::new(
            CloseoutResult::ProofNotProven,
            "installed behavior is claimed from mechanism evidence alone; an observation digest is required",
        ));
    }
    violations
}

fn bind_predecessor(request: &CloseoutRequest) -> Vec<Violation> {
    let mut violations = Vec::new();
    let exit = &request.predecessor_exit;
    if exit.predecessor_ids.is_empty() {
        violations.push(Violation::new(
            CloseoutResult::PredecessorStillReachable,
            "no predecessor identity is declared, so no exit can be checked",
        ));
    }
    if exit.independently_reachable {
        violations.push(Violation::new(
            CloseoutResult::PredecessorStillReachable,
            "a predecessor remains independently reachable and hides the transfer",
        ));
    }
    if exit.mode == PredecessorExitMode::CompatibilityProjection {
        match &exit.compatibility_exit_identity {
            Some(identity) if !identity.trim().is_empty() => {}
            _ => violations.push(Violation::new(
                CloseoutResult::PredecessorStillReachable,
                "compatibility projection requires an owner and exit identity",
            )),
        }
    }
    violations
}

fn bind_handoff_inputs(request: &CloseoutRequest) -> Vec<Violation> {
    let mut violations = Vec::new();
    if request.limitations.is_empty() {
        violations.push(Violation::new(
            CloseoutResult::LeafIncomplete,
            "a canonical handoff must name explicit limitations; none are declared",
        ));
    }
    violations
}

/// Evaluate the closeout conjunctively over the supplied git observations.
#[must_use]
pub fn evaluate(request: &CloseoutRequest, facts: &GitFacts) -> CloseoutOutcome {
    let mut violations = validate_request(request);
    violations.extend(bind_artifacts(request));
    violations.extend(bind_subject(request, facts));
    violations.extend(bind_scope(request, facts));
    violations.extend(bind_ceiling_and_relations(request));
    violations.extend(bind_proof(request));
    violations.extend(bind_predecessor(request));
    violations.extend(bind_handoff_inputs(request));

    violations.sort_by(|left, right| {
        left.result.rank().cmp(&right.result.rank()).then_with(|| left.reason.cmp(&right.reason))
    });

    if let Some(worst) = violations.first() {
        return CloseoutOutcome {
            result: worst.result,
            reasons: violations.iter().map(|violation| violation.reason.clone()).collect(),
            handoff: None,
        };
    }

    let durable_artifacts: BTreeSet<String> =
        request.required_changes.iter().map(|required| normalize_path(required)).collect();
    let handoff = LeafHandoff {
        schema: LEAF_HANDOFF_SCHEMA_V1,
        schema_version: 1,
        request_id: request.request_id.clone(),
        node_id: request.identity.node_id.clone(),
        issue: request.identity.issue.clone(),
        controller_issue: request.identity.controller_issue.clone(),
        base_sha: request.candidate.base_sha.clone(),
        head_sha: request.candidate.head_sha.clone(),
        bounded_claim: BoundedClaim {
            claim_ceiling: request.identity.claim_ceiling,
            conflict_key: request.identity.conflict_key.clone(),
            authority_transfers: request.authority_transfers_claimed.clone(),
        },
        authorities_changed: request.authority_transfers_claimed.clone(),
        authorities_retired: request.predecessor_exit.predecessor_ids.clone(),
        durable_artifacts_changed: durable_artifacts.into_iter().collect(),
        proof_identities: request.proof_profile.selected.clone(),
        negative_control_identities: request
            .proof_profile
            .negative_controls
            .iter()
            .map(|control| control.control_id.clone())
            .collect(),
        limitations: request.limitations.clone(),
        predecessor_status: predecessor_status_text(request),
        github_relation: GithubRelation {
            closes: vec![request.identity.issue.clone()],
            controller_remains_open: true,
            merge_authorized: false,
        },
        controller_complete_emitted: false,
        next_handoff: "review_then_controller_fan_in".to_string(),
        warnings: Vec::new(),
        cleanup: Vec::new(),
    };
    CloseoutOutcome {
        result: CloseoutResult::LeafReady,
        reasons: Vec::new(),
        handoff: Some(handoff),
    }
}

fn predecessor_status_text(request: &CloseoutRequest) -> String {
    match request.predecessor_exit.mode {
        PredecessorExitMode::Retired => "retired_not_independently_reachable".to_string(),
        PredecessorExitMode::CompatibilityProjection => format!(
            "compatibility_projection_behind {}",
            request.predecessor_exit.compatibility_exit_identity.as_deref().unwrap_or("unknown")
        ),
    }
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Collect the git observations for one closeout from a real repository.
///
/// Read-only: no fetch, no ref/index/worktree mutation.
pub fn collect_git_facts(
    repository: &Path,
    base: &str,
    head: &str,
    main_ref: &str,
) -> Result<GitFacts, String> {
    let mut facts = GitFacts {
        base_resolved: resolve_commit(repository, base)?,
        head_resolved: resolve_commit(repository, head)?,
        main_resolved: resolve_commit(repository, main_ref)?,
        ..GitFacts::default()
    };
    if let (Some(base), Some(head)) = (&facts.base_resolved, &facts.head_resolved) {
        facts.merge_base = run_git(repository, &["merge-base", base, head])?
            .map(|output| output.trim().to_string());
        if let Some(diff) =
            run_git(repository, &["diff", "--name-only", "--no-renames", "-z", base, head])?
        {
            facts.changed_paths =
                diff.split('\0').filter(|entry| !entry.is_empty()).map(normalize_path).collect();
        }
    }
    Ok(facts)
}

fn resolve_commit(repository: &Path, revision: &str) -> Result<Option<String>, String> {
    if revision.trim().is_empty() || revision.starts_with('-') {
        return Err(format!("revision `{revision}` is empty or option-like"));
    }
    let specification = format!("{revision}^{{commit}}");
    run_git(repository, &["rev-parse", "--verify", "--end-of-options", &specification])
        .map(|output| output.map(|text| text.trim().to_string()).filter(|sha| !sha.is_empty()))
}

fn run_git(repository: &Path, arguments: &[&str]) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .map_err(|error| format!("failed to execute git {}: {error}", arguments.join(" ")))?;
    if output.status.success() {
        return Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()));
    }
    // Exit 1 with a silent stderr is git's proven-negative answer (no merge
    // base); anything else is an instrument failure, not domain evidence.
    if output.status.code() == Some(1) && String::from_utf8_lossy(&output.stderr).trim().is_empty()
    {
        return Ok(None);
    }
    Err(format!(
        "git {} failed with status {}: {}",
        arguments.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

/// Parse and evaluate one request document against one real repository.
pub fn evaluate_request_file(
    repository: &Path,
    request_path: &Path,
    main_ref: &str,
) -> Result<CloseoutOutcome, String> {
    let raw = fs::read_to_string(request_path)
        .map_err(|error| format!("could not read request {}: {error}", request_path.display()))?;
    let request: CloseoutRequest = serde_json::from_str(&raw)
        .map_err(|error| format!("closeout request is not a valid document: {error}"))?;
    let facts = collect_git_facts(
        repository,
        &request.candidate.base_sha,
        &request.candidate.head_sha,
        main_ref,
    )?;
    Ok(evaluate(&request, &facts))
}

/// Canonical serialization of one handoff for golden comparison and output.
pub fn render_handoff_json(handoff: &LeafHandoff) -> Result<String, serde_json::Error> {
    serde_json::to_string(handoff)
}

/// Render the stable human projection of one outcome.
#[must_use]
pub fn render_outcome_human(outcome: &CloseoutOutcome) -> String {
    let mut lines = vec![
        format!("authority-transfer-closeout: {}", outcome.result.as_str()),
        format!("exit: {}", outcome.result.exit_code()),
    ];
    for reason in &outcome.reasons {
        lines.push(format!("reason: {reason}"));
    }
    if outcome.handoff.is_some() {
        lines.push("handoff: emitted".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};

    const VALID_FIXTURES: &[&str] =
        &["valid/leaf_ready_offline_live_unavailable.v1.json", "valid/leaf_ready_offline.v1.json"];

    fn fixture_path(relative: &str) -> Result<std::path::PathBuf> {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir.parent().context("xtask Cargo manifest has a repository parent")?;
        Ok(root.join(FIXTURE_DIR).join(relative))
    }

    fn load_document(relative: &str) -> Result<(CloseoutRequest, serde_json::Value)> {
        let raw = fs::read_to_string(fixture_path(relative)?)
            .with_context(|| format!("fixture {relative} is readable"))?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).context("fixture parses as JSON")?;
        let request: CloseoutRequest = serde_json::from_value(value["request"].clone())
            .context("fixture parses as a request")?;
        Ok((request, value))
    }

    fn evaluate_fixture(relative: &str) -> Result<CloseoutOutcome> {
        let (request, value) = load_document(relative)?;
        let facts: GitFacts = serde_json::from_value(value["git_facts"].clone())
            .context("fixture git facts deserialize")?;
        Ok(evaluate(&request, &facts))
    }

    fn expected_result(value: &serde_json::Value) -> Result<CloseoutResult> {
        serde_json::from_value(value["expected_result"].clone())
            .context("fixture expectation deserializes into the closed vocabulary")
    }

    #[test]
    fn valid_fixtures_are_green_with_canonical_handoffs() -> Result<()> {
        for relative in VALID_FIXTURES {
            let (_, value) = load_document(relative)?;
            assert_eq!(expected_result(&value)?, CloseoutResult::LeafReady);
            let outcome = evaluate_fixture(relative)?;
            assert_eq!(outcome.result, CloseoutResult::LeafReady, "fixture {relative}");
            let handoff = outcome
                .handoff
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("green closeout {relative} must emit a handoff"))?;
            assert_eq!(handoff.schema, LEAF_HANDOFF_SCHEMA_V1);
            assert!(!handoff.controller_complete_emitted);
            assert!(!handoff.github_relation.merge_authorized);
            assert!(handoff.github_relation.controller_remains_open);
            assert_eq!(handoff.github_relation.closes.len(), 1);
            assert!(!handoff.limitations.is_empty());
        }
        Ok(())
    }

    #[test]
    fn golden_handoff_is_stable() -> Result<()> {
        let outcome = evaluate_fixture(VALID_FIXTURES[1])?;
        let handoff =
            outcome.handoff.ok_or_else(|| anyhow::anyhow!("golden fixture must stay green"))?;
        let rendered = render_handoff_json(&handoff).context("handoff serializes")?;
        let golden_path = fixture_path("valid/golden/leaf_ready_handoff.v1.json")?;
        if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
            fs::write(&golden_path, rendered).context("golden updates")?;
            return Ok(());
        }
        let golden = fs::read_to_string(&golden_path).context("golden exists")?;
        assert_eq!(rendered, golden, "canonical handoff drifted from the golden");
        Ok(())
    }

    #[test]
    fn offline_validity_survives_an_absent_live_observer() -> Result<()> {
        for relative in VALID_FIXTURES {
            let outcome = evaluate_fixture(relative)?;
            assert_eq!(
                outcome.result,
                CloseoutResult::LeafReady,
                "offline closeout must stay valid for {relative}"
            );
        }
        Ok(())
    }

    #[test]
    fn injected_drift_stays_non_green_with_typed_results() -> Result<()> {
        let negatives = [
            (
                "invalid/controller_close_request_rejected.v1.json",
                CloseoutResult::ControllerCloseRequestRejected,
            ),
            ("invalid/cross_subject_control.v1.json", CloseoutResult::WrongSubject),
            ("invalid/wrong_ancestry.v1.json", CloseoutResult::WrongSubject),
            ("invalid/stale_packet_current_main.v1.json", CloseoutResult::StalePacket),
            ("invalid/stale_packet_digest_mismatch.v1.json", CloseoutResult::StalePacket),
            ("invalid/stale_packet_ruling_change.v1.json", CloseoutResult::StalePacket),
            ("invalid/generated_artifact_missing.v1.json", CloseoutResult::ArtifactMissing),
            ("invalid/missing_required_change.v1.json", CloseoutResult::ArtifactMissing),
            ("invalid/scope_breach_forbidden_surface.v1.json", CloseoutResult::ScopeBreach),
            ("invalid/claim_ceiling_exceeded.v1.json", CloseoutResult::ClaimCeilingExceeded),
            ("invalid/proof_zero_work.v1.json", CloseoutResult::ProofNotProven),
            ("invalid/proof_failed_status.v1.json", CloseoutResult::ProofNotProven),
            ("invalid/proof_skipped_status.v1.json", CloseoutResult::ProofNotProven),
            ("invalid/proof_missing_falsifier.v1.json", CloseoutResult::ProofNotProven),
            ("invalid/non_load_bearing_control.v1.json", CloseoutResult::ProofNotProven),
            ("invalid/mechanism_evidence_installed_claim.v1.json", CloseoutResult::ProofNotProven),
            ("invalid/instrument_failure_proof.v1.json", CloseoutResult::InstrumentFailure),
            (
                "invalid/predecessor_still_reachable.v1.json",
                CloseoutResult::PredecessorStillReachable,
            ),
            (
                "invalid/compatibility_projection_without_exit.v1.json",
                CloseoutResult::PredecessorStillReachable,
            ),
            ("invalid/contract_drift_schema.v1.json", CloseoutResult::ContractDrift),
            ("invalid/leaf_incomplete_empty_diff.v1.json", CloseoutResult::LeafIncomplete),
            ("invalid/leaf_incomplete_no_limitations.v1.json", CloseoutResult::LeafIncomplete),
        ];
        for (relative, expected) in negatives {
            let (_, value) = load_document(relative)?;
            assert_eq!(expected_result(&value)?, expected, "{relative} pins its result");
            let outcome = evaluate_fixture(relative)?;
            assert_eq!(outcome.result, expected, "fixture {relative}");
            assert!(outcome.handoff.is_none(), "{relative} must not emit a handoff");
            let expected_exit = if expected == CloseoutResult::InstrumentFailure {
                EXIT_INSTRUMENT_FAILURE
            } else {
                EXIT_NON_GREEN
            };
            assert_eq!(outcome.result.exit_code(), expected_exit, "{relative}");
            assert!(!outcome.reasons.is_empty(), "{relative} must explain the failure");
        }
        Ok(())
    }

    #[test]
    fn instrument_failures_exit_distinctly() -> Result<()> {
        let outcome = evaluate_fixture("invalid/instrument_failure_proof.v1.json")?;
        assert_eq!(outcome.result, CloseoutResult::InstrumentFailure);
        assert_eq!(outcome.result.exit_code(), EXIT_INSTRUMENT_FAILURE);
        Ok(())
    }

    #[test]
    fn reporting_precedence_is_deterministic_under_compound_drift() -> Result<()> {
        let (mut request, value) = load_document(VALID_FIXTURES[0])?;
        let mut facts: GitFacts = serde_json::from_value(value["git_facts"].clone())?;
        request.proof_profile.selected.clear();
        request.forbidden_surfaces.push("specs/".to_string());
        facts.changed_paths.insert("specs/forbidden.md".to_string());
        let outcome = evaluate(&request, &facts);
        assert_eq!(outcome.result, CloseoutResult::ScopeBreach);
        assert!(outcome.reasons.iter().any(|reason| reason.contains("zero proof work")));
        let reordered = evaluate(&request, &facts);
        assert_eq!(outcome, reordered);
        Ok(())
    }

    #[test]
    fn unknown_request_fields_are_contract_drift() {
        let raw = r#"{"schema":"authority_transfer_leaf_closeout_request.v1","schema_version":1,"surprise":true}"#;
        let parsed: Result<CloseoutRequest, _> = serde_json::from_str(raw);
        assert!(parsed.is_err(), "unknown fields must fail closed");
    }

    #[test]
    fn real_repository_end_to_end_round_trip() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        let repository = workspace.path().join("repository");
        let repository_argument = repository.to_string_lossy().into_owned();
        git_at(workspace.path(), &["init", "--initial-branch", "main", &repository_argument])?;
        git_at(&repository, &["config", "user.name", "test"])?;
        git_at(&repository, &["config", "user.email", "test@example.com"])?;
        write_commit(&repository, "seed.txt", "seed\n", "seed")?;
        let base = git_at(&repository, &["rev-parse", "HEAD"])?;

        let manifest_body = "{\"graph\":\"v1\"}\n";
        write_commit(&repository, ".ci/graph.v1.json", manifest_body, "manifest")?;
        write_commit(&repository, "docs/spec.md", "spec\n", "spec")?;
        write_commit(&repository, "tests/leaf.rs", "test\n", "test")?;
        let head = git_at(&repository, &["rev-parse", "HEAD"])?;

        let request = round_trip_request(&base, &head, manifest_body);
        let facts =
            collect_git_facts(&repository, &base, &head, "main").map_err(anyhow::Error::msg)?;
        let outcome = evaluate(&request, &facts);
        assert_eq!(outcome.result, CloseoutResult::LeafReady, "{:?}", outcome.reasons);

        let mut stale_facts = facts;
        stale_facts.main_resolved = Some("0".repeat(64));
        let stale = evaluate(&request, &stale_facts);
        assert_eq!(stale.result, CloseoutResult::StalePacket);
        assert!(stale.handoff.is_none());
        Ok(())
    }

    fn round_trip_request(base: &str, head: &str, manifest_body: &str) -> CloseoutRequest {
        CloseoutRequest {
            schema: CLOSEOUT_REQUEST_SCHEMA_V1.to_string(),
            schema_version: 1,
            request_id: "round-trip".to_string(),
            repository: "owner/name".to_string(),
            identity: LeafIdentity {
                node_id: "node.round.trip".to_string(),
                issue: IssueRef { repository: "owner/name".to_string(), number: 11703 },
                controller_issue: IssueRef { repository: "owner/name".to_string(), number: 11696 },
                claim_ceiling: ClaimCeiling::Leaf,
                conflict_key: "authority_transfer.closeout".to_string(),
            },
            candidate: CandidateSubject { base_sha: base.to_string(), head_sha: head.to_string() },
            packet_subject: PacketSubject {
                current_main_sha: head.to_string(),
                packet_ruling_revision: "ruling.r1".to_string(),
            },
            digest_binds: DigestBinds {
                programme_manifest_sha256: sha256_hex(manifest_body),
                probe_observation_sha256: sha256_hex("probe\n"),
                frontier_sha256: sha256_hex("frontier\n"),
                builder_packet_sha256: sha256_hex("builder\n"),
                reviewer_packet_sha256: sha256_hex("reviewer\n"),
                proof_profile_sha256: sha256_hex("profile\n"),
                issue_ruling_revision: "ruling.r1".to_string(),
            },
            artifacts: vec![
                artifact(ArtifactRole::ProgrammeManifest, "artifacts/graph.json", manifest_body),
                artifact(ArtifactRole::ProbeObservation, "artifacts/probe.json", "probe\n"),
                artifact(ArtifactRole::Frontier, "artifacts/frontier.json", "frontier\n"),
                artifact(ArtifactRole::BuilderPacket, "artifacts/builder.json", "builder\n"),
                artifact(ArtifactRole::ReviewerPacket, "artifacts/reviewer.json", "reviewer\n"),
                artifact(ArtifactRole::ProofProfile, "artifacts/profile.json", "profile\n"),
            ],
            claimed_closes: vec![IssueRef { repository: "owner/name".to_string(), number: 11703 }],
            forbidden_terminal_issues: vec![IssueRef {
                repository: "owner/name".to_string(),
                number: 11696,
            }],
            authority_transfers_claimed: vec!["authority_transfer.closeout".to_string()],
            forbidden_surfaces: vec![".ci/semantic-close-containment".to_string()],
            required_changes: RequiredChanges {
                specs: vec!["docs/spec.md".to_string()],
                tests: vec!["tests/leaf.rs".to_string()],
                ..RequiredChanges::default()
            },
            proof_profile: ProofProfile {
                selected: vec!["cargo.test.leaf".to_string()],
                executed: BTreeMap::from([("cargo.test.leaf".to_string(), ProofStatus::Passed)]),
                negative_controls: vec![NegativeControl {
                    control_id: "falsifier.first".to_string(),
                    red_before_evidence: true,
                    passes_only_intended_implementation: true,
                    subject_matches_candidate: true,
                }],
                first_falsifier_id: "falsifier.first".to_string(),
                generated_outputs_current: true,
                generated_identities: vec!["generation.g1".to_string()],
                claims_installed_behavior: false,
                installed_observation_sha256: None,
            },
            predecessor_exit: PredecessorExit {
                mode: PredecessorExitMode::Retired,
                predecessor_ids: vec!["pred.p1".to_string()],
                independently_reachable: false,
                compatibility_exit_identity: None,
            },
            limitations: vec!["live observation unavailable; offline closeout only".to_string()],
            live_observation: None,
        }
    }

    fn artifact(role: ArtifactRole, path: &str, contents: &str) -> ArtifactInput {
        ArtifactInput { role, path: path.to_string(), contents: contents.to_string() }
    }

    fn sha256_hex(body: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(body.as_bytes());
        hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn write_commit(repository: &Path, path: &str, contents: &str, message: &str) -> Result<()> {
        let full = repository.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("parent directory creates for {path}"))?;
        }
        fs::write(&full, contents).with_context(|| format!("file writes for {path}"))?;
        git_at(repository, &["add", "--", path])?;
        git_at(repository, &["commit", "-m", message])?;
        Ok(())
    }

    fn git_at(directory: &Path, arguments: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(directory)
            .output()
            .with_context(|| format!("git {} executes", arguments.join(" ")))?;
        if !output.status.success() {
            anyhow::bail!(
                "git {} failed: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
