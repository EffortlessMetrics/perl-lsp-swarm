//! Independent expected-outcome oracle for the standalone semantic
//! conformance corpus (#11550, child 1 of #10737).
//!
//! This module is the compact reviewed declarative evaluator: it composes a
//! vector's scripted stage ports under the accepted #10243 transaction rules
//! (immutable subject, validated predecessor chains, fail-closed mandatory
//! stages, mode-authorized `not_applicable`, explicit fallback/retry
//! branches, ordered side-effect ceilings, redacted durable packets) and
//! derives the expected semantic packet.
//!
//! Independence boundary (mutation control 16): this oracle must never call
//! production validation/serialization or execute production adapters. It
//! depends only on `std`, `serde`, and `sha2`; an integration test pins that
//! import surface and forbids subprocess spawning. [`Deviation`] variants
//! are the mutation bank's deliberate wrong behaviors; conformant derivation
//! always uses [`Deviation::None`].

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::schema::{
    ActionClass, Applicability, CeilingLevel, ClaimCeiling, EffectRecord, ExecutableObservation,
    FallbackPolicy, Mode, PACKET_SCHEMA_ID, PathPolicy, PortCall, PortResult, ProductUnit,
    RECEIPT_SCHEMA_ID, ReasonFamily, StageId, StageSpec, TerminalResult, TopologyMaturity, Vector,
};

/// Deliberate wrong coordinator/adapter behavior applied by the mutation
/// bank. Every variant other than [`Deviation::None`] must produce a packet
/// that differs from the checked-in golden (or trip the redaction scanner)
/// for at least its registered target vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deviation {
    /// Conformant composition.
    None,
    /// Mutation 1: re-resolve `latest` after subject creation.
    ReresolveLatest,
    /// Mutation 2: trust wrong subject/predecessor identities.
    TrustWrongIdentity,
    /// Mutation 3: warn-and-continue after mandatory failure.
    WarnAndContinue,
    /// Mutation 4: checksum success implies provenance success.
    ChecksumImpliesProvenance,
    /// Mutation 5: extract (staging) before the required integrity stage.
    ExtractBeforeIntegrity,
    /// Mutation 6: accept `perllsp` without required `perl-dap`.
    AllowMissingDap,
    /// Mutation 7: implicit archive-to-latest-source fallback.
    ImplicitFallback,
    /// Mutation 8: source/local server-only satisfies archive-pair claims.
    SourceAsArchivePair,
    /// Mutation 9: destination/product-unit/PATH policy changes pre-promotion.
    MutateDestinationPolicy,
    /// Mutation 10: relabel a mandatory missing stage `not_applicable`.
    MandatoryAsNotApplicable,
    /// Mutation 11: publication/promotion automatically confirms health.
    PromotionImpliesHealth,
    /// Mutation 12: PATH persistence counts as fresh-process success.
    PathPersistenceAsFreshProcess,
    /// Mutation 13: prior failed attempt erased by successful retry.
    ErasePriorAttempt,
    /// Mutation 14: stale completion advances the newer transaction.
    StaleAdvancesNewer,
    /// Mutation 15: private path/credential leaks into durable packet.
    LeakPrivatePath,
    /// Local development satisfies an installed/public claim.
    LocalDevClaimsInstalled,
    // Mutation 16 is structural (oracle independence from production code)
    // and enforced by an import-boundary test rather than a runtime knob.
}

/// Structural oracle error. Port-level failures are *not* errors: they fold
/// into the derived packet's terminal outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleError {
    StageGraph(String),
    CorpusRule(String),
    Redaction(String),
}

impl fmt::Display for OracleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OracleError::StageGraph(message) => write!(f, "stage graph invalid: {message}"),
            OracleError::CorpusRule(message) => write!(f, "corpus rule violated: {message}"),
            OracleError::Redaction(message) => write!(f, "redaction violated: {message}"),
        }
    }
}

impl std::error::Error for OracleError {}

/// The derived expected semantic packet (`standalone_semantic_packet.v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticPacket {
    pub schema: String,
    pub vector_id: String,
    pub contract_generation: u32,
    pub route: super::schema::Route,
    pub mode: Mode,
    pub requested_product_unit: ProductUnit,
    /// Product unit in effect at terminal fold (mutation 9 diverges it).
    pub effective_product_unit: ProductUnit,
    pub effective_path_policy: PathPolicy,
    pub resolved_subject_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_subject_digest: Option<String>,
    /// Attempt history, oldest first; failures are never erased.
    pub attempts: Vec<AttemptRecord>,
    pub branches: Vec<BranchRecord>,
    /// Ordered execution record across all attempts/branches.
    pub executed_stages: Vec<StageExecution>,
    /// Stages skipped with positive mode/corpus authorization.
    pub skipped_stages: Vec<SkipRecord>,
    /// Observations that do not advance transaction state.
    pub observations: Vec<ObservationRecord>,
    pub effects: Vec<EffectRecord>,
    pub side_effect_ceiling: CeilingLevel,
    pub claim_ceiling: ClaimCeiling,
    /// True only when a current archive pair install fully landed.
    pub pair_claims_satisfied: bool,
    pub terminal: TerminalOutcome,
    pub redaction_disposition: String,
    /// Present only under the [`Deviation::LeakPrivatePath`] mutation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leaks: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttemptRecord {
    pub attempt_id: String,
    pub branch_id: String,
    pub outcome: TerminalResult,
    pub reason_family: ReasonFamily,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BranchRecord {
    pub branch_id: String,
    pub subject_digest: String,
    pub terminal_result: TerminalResult,
    pub reason_family: ReasonFamily,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageExecution {
    pub attempt_id: String,
    pub branch_id: String,
    pub stage_id: StageId,
    pub result: TerminalResult,
    pub reason_family: ReasonFamily,
    pub receipt_digest: String,
    pub predecessor_digests: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkipRecord {
    pub attempt_id: String,
    pub branch_id: String,
    pub stage_id: StageId,
    pub authorization: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservationRecord {
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalOutcome {
    pub result: TerminalResult,
    pub stage_id: StageId,
    pub reason_family: ReasonFamily,
    pub action_class: ActionClass,
}

/// Deterministic sha256 digest over canonical JSON, prefixed for display.
/// Serialization of these plain-data fixtures cannot fail; a masked failure
/// would surface as a golden mismatch, never as a silent pass.
fn digest_of<T: Serialize>(value: &T) -> String {
    let canonical = serde_json::to_vec(value).unwrap_or_default();
    prefixed_sha256(&canonical)
}

fn fabricated_digest(label: &str) -> String {
    prefixed_sha256(label.as_bytes())
}

fn prefixed_sha256(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

/// Positively authorizes `not_applicable` for a (mode, stage) pair given the
/// subject's provenance policy. Anything outside this map is mandatory when
/// reached and cannot be skipped or relabeled.
fn mode_authorizes_not_applicable(mode: Mode, stage: StageId, provenance_required: bool) -> bool {
    match mode {
        Mode::ReleaseArchive => match stage {
            StageId::SourceBuild => true,
            StageId::Provenance => !provenance_required,
            _ => false,
        },
        Mode::ExactRegistrySource => matches!(
            stage,
            StageId::Transport
                | StageId::ChecksumIntegrity
                | StageId::Provenance
                | StageId::ArchiveManifestAndStaging
        ),
        Mode::ExplicitLocalDevelopment => matches!(
            stage,
            StageId::Transport
                | StageId::ChecksumIntegrity
                | StageId::Provenance
                | StageId::ArchiveManifestAndStaging
                | StageId::Promotion
                | StageId::PathPersistence
                | StageId::FreshProcessObservation
                | StageId::InstalledTransition
        ),
    }
}

/// Validates vector-level contract rules that bind fixtures to the #10243
/// semantics. Runs before any composition; failures are corpus authoring
/// bugs, never adapter findings.
pub fn validate_vector(vector: &Vector) -> Result<(), OracleError> {
    if vector.vector_id.is_empty() {
        return Err(OracleError::CorpusRule("vector_id must be non-empty".into()));
    }
    if vector.intent.mode != vector.resolved_subject.mode {
        return Err(OracleError::CorpusRule(format!(
            "{}: intent mode {:?} != resolved subject mode {:?}",
            vector.vector_id, vector.intent.mode, vector.resolved_subject.mode
        )));
    }

    // An exact archive selector must equal the frozen release tag: the
    // resolver cannot invent a different one.
    if let (super::schema::Selector::Exact { tag }, Some(release)) =
        (&vector.intent.selector, &vector.resolved_subject.release_ref)
        && tag != &release.tag
    {
        return Err(OracleError::CorpusRule(format!(
            "{}: exact selector tag {tag:?} != resolved release tag {:?}",
            vector.vector_id, release.tag
        )));
    }

    // Mode-specific subject completeness.
    match vector.resolved_subject.mode {
        Mode::ReleaseArchive => {
            if vector.resolved_subject.release_ref.is_none()
                || vector.resolved_subject.topology.is_none()
                || vector.resolved_subject.archive.is_none()
            {
                return Err(OracleError::CorpusRule(format!(
                    "{}: release-archive subject needs release_ref, topology, and archive",
                    vector.vector_id
                )));
            }
            // #12642 review: claim-ceiling derivation reads the resolved
            // subject's topology maturity even when the terminal lands on the
            // fallback branch. Pin fallback-subject maturity to the resolved
            // subject's so a fallback vector cannot mint a claim ceiling its
            // actual source does not support. Modes without a topology carry
            // no maturity claim, so the pin applies only when both have one.
            if let Some(fallback) = &vector.fallback_subject
                && let (Some(resolved_topology), Some(fallback_topology)) =
                    (&vector.resolved_subject.topology, &fallback.topology)
                && resolved_topology.maturity != fallback_topology.maturity
            {
                return Err(OracleError::CorpusRule(format!(
                    "{}: fallback subject maturity {:?} != resolved subject maturity {:?}",
                    vector.vector_id, fallback_topology.maturity, resolved_topology.maturity
                )));
            }
        }
        Mode::ExactRegistrySource => {
            if vector.resolved_subject.registry_subject.is_none() {
                return Err(OracleError::CorpusRule(format!(
                    "{}: registry-source subject needs registry_subject",
                    vector.vector_id
                )));
            }
        }
        Mode::ExplicitLocalDevelopment => {}
    }

    // Pair subjects must positively observe both roles; server-only must not
    // demand the DAP role.
    let requires_dap =
        vector.resolved_subject.required_executables.iter().any(|role| role == "perl-dap");
    match vector.resolved_subject.product_unit {
        ProductUnit::ServerDapPair if !requires_dap => {
            return Err(OracleError::CorpusRule(format!(
                "{}: pair product unit must require perl-dap observation",
                vector.vector_id
            )));
        }
        ProductUnit::ServerOnly if requires_dap => {
            return Err(OracleError::CorpusRule(format!(
                "{}: server-only product unit must not require perl-dap",
                vector.vector_id
            )));
        }
        _ => {}
    }

    validate_stage_graph(vector)?;

    // Fallback rules: only archive intents may allow fallback, and the
    // fallback subject is a NEW registry-source subject fixture.
    match vector.intent.fallback_policy {
        FallbackPolicy::ArchiveToSourceAllowed => {
            if vector.resolved_subject.mode != Mode::ReleaseArchive {
                return Err(OracleError::CorpusRule(format!(
                    "{}: only archive mode may allow archive-to-source fallback",
                    vector.vector_id
                )));
            }
            match &vector.fallback_subject {
                Some(subject) if subject.mode == Mode::ExactRegistrySource => {}
                Some(subject) => {
                    return Err(OracleError::CorpusRule(format!(
                        "{}: fallback subject must be registry-source, got {:?}",
                        vector.vector_id, subject.mode
                    )));
                }
                None => {
                    return Err(OracleError::CorpusRule(format!(
                        "{}: fallback-allowed intent requires a fallback_subject fixture",
                        vector.vector_id
                    )));
                }
            }
        }
        FallbackPolicy::Forbidden => {}
    }

    // Declared applicability rows must match the positive authorization map
    // exactly in the primary mode: unauthorized skips are invalid corpora,
    // and positively-authorizable stages must not be declared mandatory.
    for stage in &vector.stage_graph {
        let authorizable = mode_authorizes_not_applicable(
            vector.resolved_subject.mode,
            stage.stage_id,
            vector.resolved_subject.provenance_required,
        );
        match (stage.applicability, authorizable) {
            (Applicability::NotApplicable, false) => {
                return Err(OracleError::CorpusRule(format!(
                    "{}: {} is not positively authorized as not_applicable in {:?}",
                    vector.vector_id,
                    format_stage(stage.stage_id),
                    vector.resolved_subject.mode
                )));
            }
            (Applicability::Required, true) => {
                return Err(OracleError::CorpusRule(format!(
                    "{}: {} must be declared not_applicable in {:?}",
                    vector.vector_id,
                    format_stage(stage.stage_id),
                    vector.resolved_subject.mode
                )));
            }
            _ => {}
        }
    }

    // Every referenced port script exists. An empty call list is a legal
    // fixture meaning "the port produced nothing" and fails closed at
    // composition time.
    for stage in &vector.stage_graph {
        if !vector.port_scripts.contains_key(&stage.port_script) {
            return Err(OracleError::CorpusRule(format!(
                "{}: stage {} references missing port script {:?}",
                vector.vector_id,
                format_stage(stage.stage_id),
                stage.port_script
            )));
        }
    }

    // Retry plans target declared stages.
    if let Some(retry) = &vector.retry
        && !vector.stage_graph.iter().any(|stage| stage.stage_id == retry.after_stage)
    {
        return Err(OracleError::CorpusRule(format!(
            "{}: retry after_stage {} is not a declared stage",
            vector.vector_id,
            format_stage(retry.after_stage)
        )));
    }

    // Redaction assertions are never vacuous: every forbidden token must
    // actually occur in some scripted payload.
    let mut all_notes = String::new();
    for script in vector.port_scripts.values() {
        for call in &script.calls {
            all_notes.push_str(&call.private_notes.join("\n"));
            all_notes.push('\n');
        }
    }
    for token in &vector.redaction.forbidden_tokens {
        if !all_notes.contains(token) {
            return Err(OracleError::CorpusRule(format!(
                "{}: redaction token {token:?} does not occur in any port payload \
                 (vacuous assertion)",
                vector.vector_id
            )));
        }
    }

    Ok(())
}

fn format_stage(stage: StageId) -> &'static str {
    // serde snake_case spellings double as stable display names.
    serde_name_of_stage(stage)
}

/// Stable display name for a stage (the serde snake_case spelling); shared
/// with the command surface's rendering.
pub fn stage_display_name(stage: StageId) -> &'static str {
    serde_name_of_stage(stage)
}

fn serde_name_of_stage(stage: StageId) -> &'static str {
    match stage {
        StageId::ResolveSubject => "resolve_subject",
        StageId::Transport => "transport",
        StageId::ChecksumIntegrity => "checksum_integrity",
        StageId::Provenance => "provenance",
        StageId::ArchiveManifestAndStaging => "archive_manifest_and_staging",
        StageId::ExecutableObservation => "executable_observation",
        StageId::SourceBuild => "source_build",
        StageId::Promotion => "promotion",
        StageId::PathPersistence => "path_persistence",
        StageId::FreshProcessObservation => "fresh_process_observation",
        StageId::InstalledTransition => "installed_transition",
    }
}

/// Validates DAG structure and computes the deterministic topological order
/// (Kahn's algorithm, lexicographic tie-break on stable stage names).
fn topological_order(stages: &[StageSpec]) -> Result<Vec<StageId>, OracleError> {
    let mut known: BTreeMap<StageId, ()> = BTreeMap::new();
    for stage in stages {
        if known.insert(stage.stage_id, ()).is_some() {
            return Err(OracleError::StageGraph(format!(
                "duplicate stage {}",
                format_stage(stage.stage_id)
            )));
        }
    }
    let mut indegree: BTreeMap<StageId, usize> = stages.iter().map(|s| (s.stage_id, 0)).collect();
    let mut successors: BTreeMap<StageId, Vec<StageId>> = BTreeMap::new();
    for stage in stages {
        for predecessor in &stage.predecessors {
            if !known.contains_key(predecessor) {
                return Err(OracleError::StageGraph(format!(
                    "stage {} cites unknown predecessor {}",
                    format_stage(stage.stage_id),
                    format_stage(*predecessor)
                )));
            }
            *indegree.entry(stage.stage_id).or_default() += 1;
            successors.entry(*predecessor).or_default().push(stage.stage_id);
        }
    }

    let mut ready: Vec<StageId> =
        indegree.iter().filter(|(_, degree)| **degree == 0).map(|(id, _)| *id).collect();
    let mut order = Vec::with_capacity(stages.len());
    while !ready.is_empty() {
        ready.sort_by_key(|id| serde_name_of_stage(*id));
        let next = ready.remove(0);
        order.push(next);
        if let Some(children) = successors.get(&next) {
            for successor in children.clone() {
                if let Some(degree) = indegree.get_mut(&successor) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        ready.push(successor);
                    }
                }
            }
        }
    }
    if order.len() != stages.len() {
        return Err(OracleError::StageGraph("cycle detected in stage graph".into()));
    }
    Ok(order)
}

fn validate_stage_graph(vector: &Vector) -> Result<(), OracleError> {
    topological_order(&vector.stage_graph).map(|_| ())?;

    // Fix 3 (#13295): validate that every mode-required stage is declared in
    // the stage graph. `validate_vector` already verifies that declared rows
    // match the authorization map; this check fills the complementary gap
    // where a stage is omitted entirely. Without it a truncated graph
    // (missing `installed_transition`, say) still derives
    // `InstalledReleaseClaim`, since the composition loop never reaches the
    // absent stage.
    let all_stage_ids: &[StageId] = &[
        StageId::ResolveSubject,
        StageId::Transport,
        StageId::ChecksumIntegrity,
        StageId::Provenance,
        StageId::ArchiveManifestAndStaging,
        StageId::ExecutableObservation,
        StageId::SourceBuild,
        StageId::Promotion,
        StageId::PathPersistence,
        StageId::FreshProcessObservation,
        StageId::InstalledTransition,
    ];
    let declared: std::collections::BTreeSet<StageId> =
        vector.stage_graph.iter().map(|s| s.stage_id).collect();
    for &stage_id in all_stage_ids {
        let authorizable = mode_authorizes_not_applicable(
            vector.resolved_subject.mode,
            stage_id,
            vector.resolved_subject.provenance_required,
        );
        if !authorizable && !declared.contains(&stage_id) {
            return Err(OracleError::StageGraph(format!(
                "mode-required stage {} is absent from the stage graph; \
                 a truncated graph cannot reach the stages that gate the claim ceiling",
                format_stage(stage_id)
            )));
        }
    }

    Ok(())
}

/// Per-attempt execution order. Under [`Deviation::ExtractBeforeIntegrity`]
/// staging is hoisted before integrity ignoring its dependency edges — the
/// exact wrongness the mutation exists to catch.
fn attempt_execution_order(order: &[StageId], deviation: Deviation) -> Vec<StageId> {
    let mut sequence: Vec<StageId> = order.to_vec();
    if deviation == Deviation::ExtractBeforeIntegrity {
        sequence.retain(|stage| *stage != StageId::ArchiveManifestAndStaging);
        match sequence.iter().position(|stage| *stage == StageId::ChecksumIntegrity) {
            Some(position) => sequence.insert(position, StageId::ArchiveManifestAndStaging),
            None => sequence.push(StageId::ArchiveManifestAndStaging),
        }
    }
    sequence
}

struct ReceiptView<'a> {
    schema: &'a str,
    transaction_id: &'a str,
    attempt_id: &'a str,
    branch_id: &'a str,
    subject_digest: &'a str,
    stage_id: StageId,
    result: PortResult,
    reason: ReasonFamily,
    predecessor_digests: &'a [String],
    artifacts: &'a [String],
    evidence: &'a [String],
}

impl Serialize for ReceiptView<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("receipt", 11)?;
        state.serialize_field("schema", self.schema)?;
        state.serialize_field("transaction_id", self.transaction_id)?;
        state.serialize_field("attempt_id", self.attempt_id)?;
        state.serialize_field("branch_id", self.branch_id)?;
        state.serialize_field("subject_digest", self.subject_digest)?;
        state.serialize_field("stage_id", &self.stage_id)?;
        state.serialize_field("result", &self.result)?;
        state.serialize_field("reason", &self.reason)?;
        state.serialize_field("predecessor_digests", self.predecessor_digests)?;
        state.serialize_field("artifacts", self.artifacts)?;
        state.serialize_field("evidence", self.evidence)?;
        state.end()
    }
}

/// Scripted-port call cursors, scoped to one derivation so identical vectors
/// compose identically regardless of prior runs.
#[derive(Default)]
struct CallCursors {
    next_index: BTreeMap<(String, String), usize>,
}

impl CallCursors {
    fn take<'a>(&mut self, vector: &'a Vector, script_id: &str) -> Option<&'a PortCall> {
        let key = (vector.vector_id.clone(), script_id.to_string());
        let index = self.next_index.entry(key).or_insert(0);
        let call = vector.port_scripts.get(script_id)?.calls.get(*index)?;
        *index += 1;
        Some(call)
    }
}

/// Internal per-stage walk result before terminal folding.
enum AttemptStep {
    Complete { stage_id: StageId },
    Stopped { stage_id: StageId, reason: ReasonFamily },
}

/// Composes one vector into its expected semantic packet.
pub fn derive_packet(vector: &Vector, deviation: Deviation) -> Result<SemanticPacket, OracleError> {
    validate_vector(vector)?;

    let order = topological_order(&vector.stage_graph)?;
    let subject_digest = digest_of(&vector.resolved_subject);
    let main_branch = "branch-main";

    let mut packet = SemanticPacket {
        schema: PACKET_SCHEMA_ID.to_string(),
        vector_id: vector.vector_id.clone(),
        contract_generation: vector.contract_generation,
        route: vector.intent.route,
        mode: vector.intent.mode,
        requested_product_unit: vector.intent.requested_product_unit,
        effective_product_unit: vector.intent.requested_product_unit,
        effective_path_policy: vector.intent.path_policy,
        resolved_subject_digest: subject_digest.clone(),
        fallback_subject_digest: None,
        attempts: Vec::new(),
        branches: Vec::new(),
        executed_stages: Vec::new(),
        skipped_stages: Vec::new(),
        observations: Vec::new(),
        effects: Vec::new(),
        side_effect_ceiling: CeilingLevel::None,
        claim_ceiling: ClaimCeiling::None,
        pair_claims_satisfied: false,
        terminal: TerminalOutcome {
            result: TerminalResult::Failed,
            stage_id: StageId::ResolveSubject,
            reason_family: ReasonFamily::None,
            action_class: ActionClass::None,
        },
        redaction_disposition: "roles_only".to_string(),
        leaks: None,
    };

    if deviation == Deviation::LeakPrivatePath {
        let mut leaked = Vec::new();
        for script in vector.port_scripts.values() {
            for call in &script.calls {
                leaked.extend(call.private_notes.iter().cloned());
            }
        }
        packet.leaks = Some(leaked);
    }

    // Branch identity table: main binds the frozen resolved subject; the
    // explicit fallback binds its own NEW subject; the implicit-fallback
    // mutation fabricates a latest-registry subject out of thin air.
    let mut branch_digests: BTreeMap<String, String> = BTreeMap::new();
    branch_digests.insert(main_branch.to_string(), subject_digest.clone());
    if let Some(fallback) = &vector.fallback_subject {
        let digest = digest_of(fallback);
        packet.fallback_subject_digest = Some(digest.clone());
        branch_digests.insert("branch-fallback-1".to_string(), digest);
    }

    // Attempt queue: main attempt first; retries/fallback append as they
    // trigger. History is retained; nothing is erased.
    let mut attempt_queue: Vec<(String, String, String)> = vec![(
        vector.intent.operation_id.clone(),
        vector.intent.attempt_id.clone(),
        main_branch.to_string(),
    )];
    let mut retried_stages: Vec<StageId> = Vec::new();
    let mut fallen_back = false;
    let mut terminal_branch = main_branch.to_string();
    let mut cursors = CallCursors::default();

    while let Some((transaction_id, attempt_id, branch_id)) = attempt_queue.first().cloned() {
        attempt_queue.remove(0);

        let active_digest = branch_digests.get(&branch_id).cloned().ok_or_else(|| {
            OracleError::CorpusRule(format!(
                "{}: branch {branch_id} has no bound subject digest",
                vector.vector_id
            ))
        })?;

        let step = run_attempt(
            vector,
            deviation,
            &order,
            &transaction_id,
            &attempt_id,
            &branch_id,
            &active_digest,
            &mut cursors,
            &mut packet,
        )?;

        let (outcome_for_this_attempt, enqueue_retry, enqueue_fallback) = match &step {
            AttemptStep::Complete { stage_id } => (
                TerminalOutcome {
                    result: TerminalResult::Succeeded,
                    stage_id: *stage_id,
                    reason_family: ReasonFamily::None,
                    action_class: ActionClass::None,
                },
                None,
                None,
            ),
            AttemptStep::Stopped { stage_id, reason } => {
                let terminal = fold_stop(
                    vector,
                    deviation,
                    reason,
                    *stage_id,
                    &mut retried_stages,
                    &mut fallen_back,
                );
                let retry_branch = if matches!(terminal.action_class, ActionClass::RetryNewAttempt)
                {
                    vector.retry.as_ref().map(|plan| plan.attempt_id.clone())
                } else {
                    None
                };
                let fallback_branch =
                    if matches!(terminal.action_class, ActionClass::CreateFallbackBranch) {
                        Some(if deviation == Deviation::ImplicitFallback {
                            (
                                "branch-implicit-latest".to_string(),
                                fabricated_digest("implicit-latest-registry"),
                            )
                        } else {
                            ("branch-fallback-1".to_string(), fabricated_digest("unused-explicit"))
                        })
                    } else {
                        None
                    };
                (terminal, retry_branch, fallback_branch)
            }
        };

        if let Some((branch, digest)) = enqueue_fallback {
            branch_digests.entry(branch.clone()).or_insert(digest);
            attempt_queue.push((
                vector.intent.operation_id.clone(),
                format!("{attempt_id}-fb"),
                branch,
            ));
        }
        if let Some(new_attempt) = enqueue_retry {
            attempt_queue.push((
                vector.intent.operation_id.clone(),
                new_attempt,
                main_branch.to_string(),
            ));
        }

        packet.attempts.push(AttemptRecord {
            attempt_id: attempt_id.clone(),
            branch_id: branch_id.clone(),
            outcome: outcome_for_this_attempt.result,
            reason_family: outcome_for_this_attempt.reason_family,
        });
        upsert_branch(
            &mut packet.branches,
            BranchRecord {
                branch_id: branch_id.clone(),
                subject_digest: active_digest,
                terminal_result: outcome_for_this_attempt.result,
                reason_family: outcome_for_this_attempt.reason_family,
            },
        );
        terminal_branch = branch_id;
        packet.terminal = outcome_for_this_attempt;
    }

    if deviation == Deviation::ErasePriorAttempt
        && packet.attempts.len() > 1
        && let Some(last) = packet.attempts.last().cloned()
    {
        packet.attempts = vec![last];
    }

    packet.side_effect_ceiling =
        packet.effects.iter().map(|effect| effect.level).max().unwrap_or(CeilingLevel::None);
    packet.claim_ceiling =
        derive_claim_ceiling(vector, deviation, &packet.terminal, &packet.effects);
    packet.pair_claims_satisfied =
        derive_pair_claims(vector, deviation, &packet.terminal, terminal_branch == main_branch);
    enforce_redaction(vector, &packet)?;
    Ok(packet)
}

fn upsert_branch(branches: &mut Vec<BranchRecord>, record: BranchRecord) {
    match branches.iter_mut().find(|branch| branch.branch_id == record.branch_id) {
        Some(existing) => *existing = record,
        None => branches.push(record),
    }
}

/// Decides what follows a stopped attempt: retry on a fresh identity, an
/// explicitly authorized fallback branch, the implicit-fallback mutation, or
/// a typed terminal failure.
fn fold_stop(
    vector: &Vector,
    deviation: Deviation,
    reason: &ReasonFamily,
    stage_id: StageId,
    retried_stages: &mut Vec<StageId>,
    fallen_back: &mut bool,
) -> TerminalOutcome {
    let terminal_result = if *reason == ReasonFamily::Cancelled {
        TerminalResult::Cancelled
    } else {
        TerminalResult::Failed
    };

    // Retryable classes trigger exactly one fresh attempt per planned stage.
    let retryable = matches!(
        reason,
        ReasonFamily::TransportFailed
            | ReasonFamily::Timeout
            | ReasonFamily::InstrumentFailure
            | ReasonFamily::IntegrityFailed
    );
    if retryable
        && let Some(plan) = &vector.retry
        && plan.after_stage == stage_id
        && !retried_stages.contains(&stage_id)
    {
        retried_stages.push(stage_id);
        return TerminalOutcome {
            result: terminal_result,
            stage_id,
            reason_family: *reason,
            action_class: ActionClass::RetryNewAttempt,
        };
    }

    // Explicitly authorized fallback: a NEW subject/branch; no receipt from
    // the failed archive branch crosses over.
    let archive_pre_promotion_failure = matches!(
        reason,
        ReasonFamily::TransportFailed
            | ReasonFamily::IntegrityFailed
            | ReasonFamily::ProvenanceFailed
            | ReasonFamily::ArchiveInvalid
    );
    if archive_pre_promotion_failure
        && vector.resolved_subject.mode == Mode::ReleaseArchive
        && !*fallen_back
    {
        let explicitly_allowed =
            vector.intent.fallback_policy == FallbackPolicy::ArchiveToSourceAllowed;
        let implicit_mutation = deviation == Deviation::ImplicitFallback;
        if (explicitly_allowed || implicit_mutation) && stage_id != StageId::Promotion {
            *fallen_back = true;
            return TerminalOutcome {
                result: terminal_result,
                stage_id,
                reason_family: *reason,
                action_class: ActionClass::CreateFallbackBranch,
            };
        }
    }

    let action = match reason {
        ReasonFamily::Timeout
        | ReasonFamily::InstrumentFailure
        | ReasonFamily::NotProven
        | ReasonFamily::HealthCheckFailed => ActionClass::VerifyEnvironmentThenRetry,
        _ => ActionClass::AbortInstall,
    };
    TerminalOutcome {
        result: terminal_result,
        stage_id,
        reason_family: *reason,
        action_class: action,
    }
}

fn derive_claim_ceiling(
    vector: &Vector,
    deviation: Deviation,
    terminal: &TerminalOutcome,
    effects: &[EffectRecord],
) -> ClaimCeiling {
    if deviation == Deviation::LocalDevClaimsInstalled
        && vector.resolved_subject.mode == Mode::ExplicitLocalDevelopment
    {
        return ClaimCeiling::InstalledReleaseClaim;
    }
    if vector.resolved_subject.mode == Mode::ExplicitLocalDevelopment {
        return ClaimCeiling::LocalDevelopmentOnly;
    }
    match terminal.result {
        TerminalResult::Succeeded => {
            let historical = vector
                .resolved_subject
                .topology
                .as_ref()
                .is_some_and(|topology| topology.maturity == TopologyMaturity::Historical);
            if historical {
                ClaimCeiling::HistoricalEvidenceOnly
            } else {
                ClaimCeiling::InstalledReleaseClaim
            }
        }
        TerminalResult::Cancelled | TerminalResult::Failed => {
            let promoted =
                effects.iter().any(|effect| effect.level >= CeilingLevel::PromotionReached);
            if promoted { ClaimCeiling::CurrentStatePromotion } else { ClaimCeiling::None }
        }
    }
}

fn derive_pair_claims(
    vector: &Vector,
    deviation: Deviation,
    terminal: &TerminalOutcome,
    terminal_on_main_archive_branch: bool,
) -> bool {
    if deviation == Deviation::SourceAsArchivePair
        && vector.resolved_subject.mode != Mode::ReleaseArchive
    {
        return true;
    }
    terminal.result == TerminalResult::Succeeded
        && terminal_on_main_archive_branch
        && vector.resolved_subject.mode == Mode::ReleaseArchive
        && vector.resolved_subject.product_unit == ProductUnit::ServerDapPair
        && vector
            .resolved_subject
            .topology
            .as_ref()
            .is_some_and(|topology| topology.maturity == TopologyMaturity::Current)
}

/// Executes one attempt across the stage subset in scope for the branch's
/// active subject mode.
///
/// Returns `Err(OracleError::StageGraph)` when a corpus authoring error is
/// detected at composition time (e.g. a declared predecessor has no receipt
/// and was not mode-authorized on this branch). Port-level failures are not
/// errors — they fold into the derived packet's terminal outcome and are
/// returned as `Ok(AttemptStep::Stopped { … })`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn run_attempt(
    vector: &Vector,
    deviation: Deviation,
    order: &[StageId],
    transaction_id: &str,
    attempt_id: &str,
    branch_id: &str,
    subject_digest: &str,
    cursors: &mut CallCursors,
    packet: &mut SemanticPacket,
) -> Result<AttemptStep, OracleError> {
    let sequence = attempt_execution_order(order, deviation);
    let (active_mode, provenance_required, required_executables) =
        active_subject_profile(vector, branch_id);

    // Receipt chain for THIS attempt/branch only; no cross-branch reuse.
    let mut chain: Vec<(StageId, String)> = Vec::new();
    // Stages skipped via positive mode authorization on this branch, used
    // by predecessors_of to distinguish "correctly absent" from "missing".
    let mut skipped_stage_ids: Vec<StageId> = Vec::new();
    let mut promoted_this_attempt = false;

    for stage_id in &sequence {
        let Some(spec) = vector.stage_graph.iter().find(|stage| &stage.stage_id == stage_id) else {
            continue;
        };

        // Positive mode authorization is the ONLY way a stage is skipped.
        if mode_authorizes_not_applicable(active_mode, *stage_id, provenance_required) {
            packet.skipped_stages.push(SkipRecord {
                attempt_id: attempt_id.to_string(),
                branch_id: branch_id.to_string(),
                stage_id: *stage_id,
                authorization: "mode_authorized".to_string(),
            });
            skipped_stage_ids.push(*stage_id);
            continue;
        }

        // Mutation 10 relabels later missing-evidence stops; declared rows
        // remain validated corpus facts, not runtime escape hatches.
        let _ = spec.applicability;

        // Mutation 4: checksum success silently promoted to provenance.
        if deviation == Deviation::ChecksumImpliesProvenance
            && *stage_id == StageId::Provenance
            && chain.iter().any(|(stage, _)| *stage == StageId::ChecksumIntegrity)
        {
            packet.observations.push(ObservationRecord {
                kind: "rule_bypass".to_string(),
                detail: format!("{attempt_id}: provenance implied by checksum success"),
            });
            let predecessors = predecessors_of(&chain, &skipped_stage_ids, spec)?;
            let view = ReceiptView {
                schema: RECEIPT_SCHEMA_ID,
                transaction_id,
                attempt_id,
                branch_id,
                subject_digest,
                stage_id: *stage_id,
                result: PortResult::Success,
                reason: ReasonFamily::None,
                predecessor_digests: &predecessors,
                artifacts: &[],
                evidence: &[],
            };
            let receipt_digest = digest_of(&view);
            chain.push((*stage_id, receipt_digest.clone()));
            packet.executed_stages.push(StageExecution {
                attempt_id: attempt_id.to_string(),
                branch_id: branch_id.to_string(),
                stage_id: *stage_id,
                result: TerminalResult::Succeeded,
                reason_family: ReasonFamily::None,
                receipt_digest,
                predecessor_digests: predecessors,
                artifacts: Vec::new(),
                evidence: Vec::new(),
            });
            continue;
        }

        // Pull scripted calls, filtering stale completions from older
        // attempts (they never advance the newer transaction).
        let call = loop {
            match cursors.take(vector, &spec.port_script) {
                None => {
                    // Missing-evidence path: fail closed unless a mutation
                    // relabels or warns.
                    if deviation == Deviation::MandatoryAsNotApplicable {
                        packet.skipped_stages.push(SkipRecord {
                            attempt_id: attempt_id.to_string(),
                            branch_id: branch_id.to_string(),
                            stage_id: *stage_id,
                            authorization: "mutation_relabelled_not_applicable".to_string(),
                        });
                        break None;
                    }
                    if deviation == Deviation::WarnAndContinue {
                        packet.observations.push(ObservationRecord {
                            kind: "warned_and_continued".to_string(),
                            detail: format!(
                                "{attempt_id}: {} produced no evidence",
                                format_stage(*stage_id)
                            ),
                        });
                        break None;
                    }
                    return Ok(fail_step(
                        packet,
                        attempt_id,
                        branch_id,
                        *stage_id,
                        ReasonFamily::MissingEvidence,
                    ));
                }
                Some(call) => match &call.stale_from_attempt {
                    Some(stale_from) if stale_from != attempt_id => {
                        if deviation == Deviation::StaleAdvancesNewer {
                            packet.observations.push(ObservationRecord {
                                kind: "stale_completion_applied".to_string(),
                                detail: format!(
                                    "{attempt_id}: stale completion from {stale_from} advanced this attempt"
                                ),
                            });
                            break Some(call);
                        }
                        packet.observations.push(ObservationRecord {
                            kind: "stale_completion_ignored".to_string(),
                            detail: format!(
                                "{attempt_id}: ignored stale completion from {stale_from} at {}",
                                format_stage(*stage_id)
                            ),
                        });
                        continue;
                    }
                    _ => break Some(call),
                },
            }
        };

        let Some(call) = call else { continue };

        // Unknown receipt schema fails closed.
        if call.claimed_receipt_schema() != RECEIPT_SCHEMA_ID {
            return Ok(fail_step(
                packet,
                attempt_id,
                branch_id,
                *stage_id,
                ReasonFamily::UnknownSchema,
            ));
        }

        // Identity corruption fails closed; mutation 2 trusts it instead.
        if deviation != Deviation::TrustWrongIdentity {
            if call.corrupt_subject_digest {
                return Ok(fail_step(
                    packet,
                    attempt_id,
                    branch_id,
                    *stage_id,
                    ReasonFamily::SubjectMismatch,
                ));
            }
            if call.corrupt_predecessor_digests {
                return Ok(fail_step(
                    packet,
                    attempt_id,
                    branch_id,
                    *stage_id,
                    ReasonFamily::PredecessorMismatch,
                ));
            }
        }

        // Latest drift after resolution must not re-resolve the subject.
        if call.newer_latest_tag.is_some() && deviation != Deviation::ReresolveLatest {
            return Ok(fail_step(
                packet,
                attempt_id,
                branch_id,
                *stage_id,
                ReasonFamily::SubjectMismatch,
            ));
        }

        // Observed side effects are recorded regardless of how the call
        // terminates: ports observe reality, then the transaction folds it.
        for effect in &call.effects {
            packet.effects.push(effect.clone());
        }

        // Non-success results fold to typed failures unless a deviation
        // substitutes the specific wrong behavior first.
        if call.result != PortResult::Success {
            if deviation == Deviation::PromotionImpliesHealth
                && *stage_id == StageId::FreshProcessObservation
            {
                packet.observations.push(ObservationRecord {
                    kind: "rule_bypass".to_string(),
                    detail: format!("{attempt_id}: promotion treated as health confirmation"),
                });
            } else if deviation == Deviation::PathPersistenceAsFreshProcess
                && *stage_id == StageId::FreshProcessObservation
                && packet.effects.iter().any(|effect| effect.kind == "path_persisted")
            {
                packet.observations.push(ObservationRecord {
                    kind: "rule_bypass".to_string(),
                    detail: format!(
                        "{attempt_id}: PATH persistence counted as fresh-process success"
                    ),
                });
            } else if deviation == Deviation::WarnAndContinue {
                packet.observations.push(ObservationRecord {
                    kind: "warned_and_continued".to_string(),
                    detail: format!(
                        "{attempt_id}: {} returned {:?}",
                        format_stage(*stage_id),
                        call.result
                    ),
                });
            } else if deviation == Deviation::MandatoryAsNotApplicable {
                packet.skipped_stages.push(SkipRecord {
                    attempt_id: attempt_id.to_string(),
                    branch_id: branch_id.to_string(),
                    stage_id: *stage_id,
                    authorization: "mutation_relabelled_not_applicable".to_string(),
                });
                continue;
            } else {
                let reason = failure_reason(*stage_id, call.result);
                if reason == ReasonFamily::HealthCheckFailed && promoted_this_attempt {
                    packet.effects.push(EffectRecord {
                        level: CeilingLevel::PromotionReached,
                        kind: "rollback".to_string(),
                    });
                }
                return Ok(fail_step(packet, attempt_id, branch_id, *stage_id, reason));
            }
        }

        // Pair completeness gate: every required role must be positively
        // observed; DAP preview maturity never weakens the pair.
        //
        // Fix 2 (#13295): an absent `executables` map is treated as all
        // required roles unsatisfied. The prior `if let Some` skipped the
        // check entirely when the map was omitted, allowing a successful
        // observation to reach promotion with `pair_claims_satisfied` despite
        // having positively observed neither `perllsp` nor `perl-dap`.
        if *stage_id == StageId::ExecutableObservation {
            let executables = call.executables.as_ref();
            let unsatisfied: Vec<String> = required_executables
                .iter()
                .filter(|role| {
                    executables.and_then(|m| m.get(*role)) != Some(&ExecutableObservation::Ok)
                })
                .cloned()
                .collect();
            if !unsatisfied.is_empty() {
                if deviation != Deviation::AllowMissingDap {
                    return Ok(fail_step(
                        packet,
                        attempt_id,
                        branch_id,
                        *stage_id,
                        ReasonFamily::PairIncomplete,
                    ));
                }
                packet.observations.push(ObservationRecord {
                    kind: "rule_bypass".to_string(),
                    detail: format!(
                        "{attempt_id}: promotion allowed despite unobserved {unsatisfied:?}"
                    ),
                });
            }
        }

        // Mutation 1 adopts a (possibly fabricated) newer latest at the
        // transport boundary instead of retaining the resolved subject.
        if deviation == Deviation::ReresolveLatest
            && *stage_id == StageId::Transport
            && call.result == PortResult::Success
        {
            let adopted =
                call.newer_latest_tag.clone().unwrap_or_else(|| "fabricated-latest".into());
            packet.observations.push(ObservationRecord {
                kind: "reresolved_latest".to_string(),
                detail: format!("{attempt_id}: adopted newer latest {adopted:?}"),
            });
        }

        // Record the successful receipt bound to THIS attempt/branch chain.
        let received_predecessors: Vec<String> = if call.corrupt_predecessor_digests {
            vec![fabricated_digest("corrupted-predecessor")]
        } else {
            predecessors_of(&chain, &skipped_stage_ids, spec)?
        };
        let received_subject = if call.corrupt_subject_digest {
            fabricated_digest("corrupted-subject")
        } else if deviation == Deviation::ReresolveLatest && *stage_id == StageId::Transport {
            let tag = call.newer_latest_tag.as_deref().unwrap_or("fabricated-latest").to_string();
            fabricated_digest(&format!("latest:{tag}"))
        } else {
            subject_digest.to_string()
        };

        let view = ReceiptView {
            schema: call.claimed_receipt_schema(),
            transaction_id,
            attempt_id,
            branch_id,
            subject_digest: &received_subject,
            stage_id: *stage_id,
            result: call.result,
            reason: ReasonFamily::None,
            predecessor_digests: &received_predecessors,
            artifacts: &call.artifacts,
            evidence: &call.evidence,
        };
        let receipt_digest = digest_of(&view);
        chain.push((*stage_id, receipt_digest.clone()));

        packet.executed_stages.push(StageExecution {
            attempt_id: attempt_id.to_string(),
            branch_id: branch_id.to_string(),
            stage_id: *stage_id,
            result: TerminalResult::Succeeded,
            reason_family: ReasonFamily::None,
            receipt_digest,
            predecessor_digests: received_predecessors,
            artifacts: call.artifacts.clone(),
            evidence: call.evidence.clone(),
        });

        // Mutation 9 changes destination/product-unit/PATH policy right
        // before promotion locks them.
        if *stage_id == StageId::Promotion {
            promoted_this_attempt = true;
        }
        if deviation == Deviation::MutateDestinationPolicy && *stage_id == StageId::Promotion {
            packet.effective_product_unit = ProductUnit::ServerDapPair;
            packet.effective_path_policy = PathPolicy::Persist;
        }

        // Installed transition requires a healthy fresh process on the SAME
        // attempt/branch; otherwise rollback fires and the claim stays below
        // installed.
        if *stage_id == StageId::InstalledTransition {
            let healthy_same_run = packet.executed_stages.iter().any(|execution| {
                execution.attempt_id == attempt_id
                    && execution.branch_id == branch_id
                    && execution.stage_id == StageId::FreshProcessObservation
                    && execution.result == TerminalResult::Succeeded
            }) || deviation == Deviation::PromotionImpliesHealth
                || deviation == Deviation::PathPersistenceAsFreshProcess;
            if !healthy_same_run {
                packet.effects.push(EffectRecord {
                    level: CeilingLevel::PromotionReached,
                    kind: "rollback".to_string(),
                });
                return Ok(fail_step(
                    packet,
                    attempt_id,
                    branch_id,
                    StageId::FreshProcessObservation,
                    ReasonFamily::HealthCheckFailed,
                ));
            }
        }
    }

    // Success terminal = last executed stage of THIS attempt/branch.
    match packet
        .executed_stages
        .iter()
        .rev()
        .find(|execution| {
            execution.attempt_id == attempt_id
                && execution.branch_id == branch_id
                && execution.result == TerminalResult::Succeeded
        })
        .map(|execution| execution.stage_id)
    {
        Some(stage_id) => Ok(AttemptStep::Complete { stage_id }),
        None => Ok(AttemptStep::Stopped {
            stage_id: StageId::ResolveSubject,
            reason: ReasonFamily::MissingEvidence,
        }),
    }
}

type SubjectProfile = (Mode, bool, Vec<String>);

/// Mode, provenance policy, and required executable roles for the subject
/// active on this branch. The implicit-fallback mutation keeps the main
/// subject profile (it fabricates only the branch digest), so a missing
/// fallback fixture degrades to the resolved subject rather than panicking.
fn active_subject_profile(vector: &Vector, branch_id: &str) -> SubjectProfile {
    const MAIN_BRANCH: &str = "branch-main";
    let active = if branch_id == MAIN_BRANCH {
        &vector.resolved_subject
    } else {
        vector.fallback_subject.as_ref().unwrap_or(&vector.resolved_subject)
    };
    (active.mode, active.provenance_required, active.required_executables.clone())
}

/// Resolves predecessor receipt digests for a stage, in the declaration order
/// from the stage spec. A declared predecessor is satisfied by one of:
///
/// 1. A receipt already in the attempt's chain (stage executed successfully).
/// 2. Being in `skipped_stage_ids` (positively mode-authorized as
///    not_applicable for the active branch subject).
///
/// Fix 1 (#13295): if a declared predecessor is absent from both the chain
/// and the skipped set the corpus is inconsistent — the stage graph declares
/// a dependency that can never be resolved for this branch. Return
/// `OracleError::StageGraph` so `derive_packet` surfaces the authoring bug
/// rather than silently producing an empty predecessor list that would pass
/// corpus checks while certifying a broken dependency chain.
fn predecessors_of(
    chain: &[(StageId, String)],
    skipped_stage_ids: &[StageId],
    spec: &StageSpec,
) -> Result<Vec<String>, OracleError> {
    let mut result = Vec::new();
    for predecessor in &spec.predecessors {
        if let Some((_, digest)) = chain.iter().find(|(stage, _)| stage == predecessor) {
            result.push(digest.clone());
        } else if skipped_stage_ids.contains(predecessor) {
            // Skipped (mode-authorized) predecessors contributed no receipt;
            // the dependency is considered satisfied by the authorization map.
        } else {
            return Err(OracleError::StageGraph(format!(
                "stage {} has declared predecessor {} that has no receipt and was not \
                 mode-authorized on this branch; the stage graph is inconsistent for \
                 this subject mode",
                format_stage(spec.stage_id),
                format_stage(*predecessor),
            )));
        }
    }
    Ok(result)
}

fn failure_reason(stage: StageId, result: PortResult) -> ReasonFamily {
    match result {
        PortResult::NotApplicable => ReasonFamily::UnauthorizedNotApplicable,
        PortResult::Cancelled => ReasonFamily::Cancelled,
        PortResult::Timeout => ReasonFamily::Timeout,
        PortResult::NotProven => ReasonFamily::NotProven,
        PortResult::InstrumentUnavailable | PortResult::InstrumentDegraded => {
            ReasonFamily::InstrumentFailure
        }
        PortResult::Success => ReasonFamily::None,
        PortResult::Failure => match stage {
            StageId::Transport => ReasonFamily::TransportFailed,
            StageId::ChecksumIntegrity => ReasonFamily::IntegrityFailed,
            StageId::Provenance => ReasonFamily::ProvenanceFailed,
            StageId::ArchiveManifestAndStaging => ReasonFamily::ArchiveInvalid,
            StageId::FreshProcessObservation => ReasonFamily::HealthCheckFailed,
            _ => ReasonFamily::ObservationFailed,
        },
    }
}

fn fail_step(
    packet: &mut SemanticPacket,
    attempt_id: &str,
    branch_id: &str,
    stage_id: StageId,
    reason: ReasonFamily,
) -> AttemptStep {
    packet.executed_stages.push(StageExecution {
        attempt_id: attempt_id.to_string(),
        branch_id: branch_id.to_string(),
        stage_id,
        result: TerminalResult::Failed,
        reason_family: reason,
        receipt_digest: fabricated_digest(&format!("failed:{}:{reason:?}", format_stage(stage_id))),
        // Intentionally empty: a failed stage never completed, so it consumes
        // no predecessor receipts. The fabricated receipt digest records the
        // failure; the empty predecessor list must not be read as a broken
        // dependency chain.
        predecessor_digests: Vec::new(),
        artifacts: Vec::new(),
        evidence: Vec::new(),
    });
    AttemptStep::Stopped { stage_id, reason }
}

/// The durable packet may carry roles, digests, and bounded ids only. Any
/// scripted private token reaching serialization is a hard redaction error.
fn enforce_redaction(vector: &Vector, packet: &SemanticPacket) -> Result<(), OracleError> {
    let serialized = serde_json::to_string(packet).unwrap_or_default();
    for token in &vector.redaction.forbidden_tokens {
        if serialized.contains(token) {
            return Err(OracleError::Redaction(format!(
                "{}: forbidden token {token:?} serialized into the durable packet",
                vector.vector_id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::standalone_vectors::load_corpus;

    /// Panic/unwrap are denied workspace-wide; tests fail through a distinct
    /// nonzero exit instead.
    fn fail(message: &str) -> ! {
        eprintln!("test failure: {message}");
        std::process::exit(101)
    }

    fn corpus() -> Vec<Vector> {
        match load_corpus() {
            Ok(vectors) => vectors,
            Err(error) => fail(&format!("corpus must load: {error}")),
        }
    }

    fn vector_by_id<'a>(vectors: &'a [Vector], id: &str) -> &'a Vector {
        match vectors.iter().find(|vector| vector.vector_id == id) {
            Some(vector) => vector,
            None => fail(&format!("{id} missing from corpus")),
        }
    }

    fn derive(vector: &Vector, deviation: Deviation) -> SemanticPacket {
        match derive_packet(vector, deviation) {
            Ok(packet) => packet,
            Err(error) => fail(&format!("{}: derive failed: {error}", vector.vector_id)),
        }
    }

    fn serialized(packet: &SemanticPacket) -> String {
        serde_json::to_string_pretty(packet).unwrap_or_default()
    }

    #[test]
    fn derivation_is_byte_deterministic_for_every_vector() {
        for vector in corpus() {
            let first = serialized(&derive(&vector, Deviation::None));
            let second = serialized(&derive(&vector, Deviation::None));
            assert_eq!(first, second, "{}: second generation drifted", vector.vector_id);
        }
    }

    #[test]
    fn corpus_covers_the_full_platform_neutral_denominator() {
        let vectors = corpus();
        assert_eq!(vectors.len(), 22, "corpus denominator");
        for vector in &vectors {
            assert_eq!(vector.contract_generation, 1, "{}", vector.vector_id);
        }
    }

    #[test]
    fn healthy_pair_reaches_installed_claim_with_pair_satisfied() {
        let vectors = corpus();
        let packet = derive(vector_by_id(&vectors, "v001-archive-pair-success"), Deviation::None);
        assert_eq!(packet.terminal.result, TerminalResult::Succeeded);
        assert_eq!(packet.claim_ceiling, ClaimCeiling::InstalledReleaseClaim);
        assert!(packet.pair_claims_satisfied);
        assert_eq!(packet.side_effect_ceiling, CeilingLevel::InstalledClaim);
    }

    #[test]
    fn historical_topology_stays_a_bounded_historical_unit() {
        let vectors = corpus();
        let packet =
            derive(vector_by_id(&vectors, "v002-archive-historical-server-only"), Deviation::None);
        assert_eq!(packet.claim_ceiling, ClaimCeiling::HistoricalEvidenceOnly);
        assert!(!packet.pair_claims_satisfied);
    }

    #[test]
    fn local_development_never_authorizes_install_claims() {
        let vectors = corpus();
        let packet = derive(
            vector_by_id(&vectors, "v004-local-development-non-authoritative"),
            Deviation::None,
        );
        assert_eq!(packet.claim_ceiling, ClaimCeiling::LocalDevelopmentOnly);
        assert!(!packet.pair_claims_satisfied);
    }

    #[test]
    fn fallback_branch_binds_a_new_subject_and_isolates_receipts() {
        let vectors = corpus();
        let packet =
            derive(vector_by_id(&vectors, "v007-fallback-allowed-new-branch"), Deviation::None);
        assert_eq!(packet.branches.len(), 2);
        assert_eq!(packet.attempts.len(), 2);
        assert_ne!(
            packet.branches[0].subject_digest, packet.branches[1].subject_digest,
            "fallback must resolve a NEW subject"
        );
        // No receipt crosses the branch boundary: fallback promotion may not
        // cite any failed-archive-branch receipt digest.
        let main_staging = packet
            .executed_stages
            .iter()
            .find(|e| {
                e.branch_id == "branch-main" && e.stage_id == StageId::ArchiveManifestAndStaging
            })
            .map(|e| e.receipt_digest.clone());
        if let Some(main_digest) = main_staging {
            for execution in &packet.executed_stages {
                if execution.branch_id == "branch-fallback-1" {
                    assert!(
                        !execution.predecessor_digests.contains(&main_digest),
                        "fallback reused a failed-archive-branch receipt"
                    );
                }
            }
        }
    }

    #[test]
    fn implicit_fallback_is_rejected_when_policy_forbids() {
        let vectors = corpus();
        let packet = derive(
            vector_by_id(&vectors, "v008-fallback-forbidden-no-registry-action"),
            Deviation::None,
        );
        assert_eq!(packet.branches.len(), 1, "no registry action may appear");
        assert_eq!(packet.attempts.len(), 1);
        assert_eq!(packet.terminal.reason_family, ReasonFamily::IntegrityFailed);
    }

    #[test]
    fn wrong_subject_receipt_fails_closed_even_with_valid_bytes() {
        let vectors = corpus();
        let vector = vector_by_id(&vectors, "v009-transport-checksum-subject-mix");
        let packet = derive(vector, Deviation::None);
        assert_eq!(packet.terminal.reason_family, ReasonFamily::SubjectMismatch);
        let trusted = derive(vector, Deviation::TrustWrongIdentity);
        assert_ne!(serialized(&packet), serialized(&trusted));
    }

    #[test]
    fn stale_completion_never_advances_the_newer_attempt() {
        let vectors = corpus();
        let vector = vector_by_id(&vectors, "v016-retry-stale-completion");
        let packet = derive(vector, Deviation::None);
        assert!(packet.observations.iter().any(|o| o.kind == "stale_completion_ignored"));
        assert_eq!(packet.attempts.len(), 2, "retry history retained");
        assert_eq!(packet.terminal.result, TerminalResult::Failed);

        let stale_applied = derive(vector, Deviation::StaleAdvancesNewer);
        assert_ne!(serialized(&packet), serialized(&stale_applied));
        let erased = derive(vector, Deviation::ErasePriorAttempt);
        assert_eq!(erased.attempts.len(), 1, "mutation 13 drops history");
    }

    /// v022 is the success-path twin of v016: attempt a1 integrity-fails,
    /// attempt a2 succeeds to installed_transition. The conformant
    /// derivation must retain BOTH attempts in the durable history —
    /// asserted here independently of the mutation bank and the
    /// golden-comparison path.
    #[test]
    fn retry_success_keeps_the_prior_failed_attempt_in_history() {
        let vectors = corpus();
        let vector = vector_by_id(&vectors, "v022-retry-succeeds-erase-prior-attempt");
        let packet = derive(vector, Deviation::None);
        assert_eq!(packet.terminal.result, TerminalResult::Succeeded);
        assert_eq!(packet.terminal.stage_id, StageId::InstalledTransition);
        assert_eq!(packet.attempts.len(), 2, "retry history retained on success");
        assert_eq!(packet.attempts[0].outcome, TerminalResult::Failed);
        assert_eq!(packet.attempts[0].reason_family, ReasonFamily::IntegrityFailed);
        assert_eq!(packet.attempts[1].outcome, TerminalResult::Succeeded);
        assert_eq!(packet.claim_ceiling, ClaimCeiling::InstalledReleaseClaim);

        let erased = derive(vector, Deviation::ErasePriorAttempt);
        assert_eq!(erased.attempts.len(), 1, "mutation 13 drops history on success too");
    }

    #[test]
    fn pair_gate_blocks_promotion_without_dap() {
        let vectors = corpus();
        let packet = derive(vector_by_id(&vectors, "v013-pair-missing-dap"), Deviation::None);
        assert_eq!(packet.terminal.reason_family, ReasonFamily::PairIncomplete);
        assert_eq!(packet.side_effect_ceiling, CeilingLevel::Staged);
        assert!(!packet.effects.iter().any(|effect| effect.kind == "promoted"));
    }

    #[test]
    fn health_failure_rolls_back_and_caps_the_claim_below_installed() {
        let vectors = corpus();
        let packet =
            derive(vector_by_id(&vectors, "v014-health-failure-rollback"), Deviation::None);
        assert!(packet.effects.iter().any(|effect| effect.kind == "rollback"));
        assert_eq!(packet.claim_ceiling, ClaimCeiling::CurrentStatePromotion);
        assert_eq!(packet.terminal.reason_family, ReasonFamily::HealthCheckFailed);
    }

    #[test]
    fn redaction_scanner_catches_the_leak_mutation_only() {
        let vectors = corpus();
        let vector = vector_by_id(&vectors, "v019-instrument-failure-redaction");
        let rendered = serialized(&derive(vector, Deviation::None));
        for token in &vector.redaction.forbidden_tokens {
            assert!(!rendered.contains(token), "conformant packet leaked {token:?}");
        }
        match derive_packet(vector, Deviation::LeakPrivatePath) {
            Err(OracleError::Redaction(_)) => {}
            other => fail(&format!("leak mutation must trip redaction, got {other:?}")),
        }
    }

    #[test]
    fn applicability_rows_are_two_way_validated_against_the_authorization_map() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/standalone_install_vectors/vectors/v001-archive-pair-success.json");
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text.replace("\r\n", "\n"),
            Err(error) => fail(&format!("fixture read failed: {error}")),
        };

        // Unauthorized skip: archive-mode checksum declared not_applicable.
        let bad_row = text.replace(
            "\"stage_id\": \"checksum_integrity\", \"applicability\": \"required\"",
            "\"stage_id\": \"checksum_integrity\", \"applicability\": \"not_applicable\"",
        );
        assert_ne!(text, bad_row, "fixture edit must apply");
        let vector: Vector = match serde_json::from_str(&bad_row) {
            Ok(vector) => vector,
            Err(error) => fail(&format!("parse failed: {error}")),
        };
        match validate_vector(&vector) {
            Err(OracleError::CorpusRule(message)) => {
                assert!(message.contains("not positively authorized"), "{message}");
            }
            other => fail(&format!("unauthorized n/a must be rejected, got {other:?}")),
        }

        // Mandatory row where policy authorizes a skip: with provenance not
        // required, a Required provenance row is invalid authoring.
        let relaxed =
            text.replace("\"provenance_required\": true", "\"provenance_required\": false");
        let vector: Vector = match serde_json::from_str(&relaxed) {
            Ok(vector) => vector,
            Err(error) => fail(&format!("parse failed: {error}")),
        };
        match validate_vector(&vector) {
            Err(OracleError::CorpusRule(message)) => {
                assert!(message.contains("must be declared not_applicable"), "{message}");
            }
            other => fail(&format!("undeclared skip must be rejected, got {other:?}")),
        }
    }

    #[test]
    fn exact_selector_must_match_the_resolved_release_tag() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/standalone_install_vectors/vectors/v001-archive-pair-success.json");
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => fail(&format!("fixture read failed: {error}")),
        };
        let drifted = text.replace(
            "{ \"exact\": { \"tag\": \"synthetic-v1.0.0\" } }",
            "{ \"exact\": { \"tag\": \"synthetic-v0.0.1\" } }",
        );
        assert_ne!(text, drifted, "fixture edit must apply");
        let vector: Vector = match serde_json::from_str(&drifted) {
            Ok(vector) => vector,
            Err(error) => fail(&format!("parse failed: {error}")),
        };
        match validate_vector(&vector) {
            Err(OracleError::CorpusRule(message)) => {
                assert!(message.contains("exact selector tag"), "{message}");
            }
            other => fail(&format!("drifted selector must be rejected, got {other:?}")),
        }
    }

    #[test]
    fn unknown_fields_fail_closed_at_the_schema_boundary() {
        let text = r#"{"vector_id":"x","contract_generation":1,"family":"f","platform_classification":"platform_neutral","intent":{"operation_id":"o","attempt_id":"a","route":"first_party_posix","mode":"release_archive","selector":{"latest_requested":null},"target":{"platform":"p","arch":"a","libc":"l"},"requested_product_unit":"server_only","fallback_policy":"forbidden","path_policy":"persist","config_digest":"c"},"resolved_subject":{"subject_id":"s","mode":"release_archive","product_unit":"server_only","required_executables":["perllsp"],"destination_role":"install_root","provenance_required":true},"stage_graph":[],"port_scripts":{},"expected":{"terminal_result":"succeeded","terminal_stage":"resolve_subject","reason_family":"none","action_class":"none","side_effect_ceiling":"none","claim_ceiling":"none","pair_claims_satisfied":false,"branch_count":0,"attempt_count":0},"redaction":{"forbidden_tokens":[]},"sneaky_field":1}"#;
        let parse: std::result::Result<Vector, _> = serde_json::from_str(text);
        assert!(parse.is_err(), "deny_unknown_fields must reject sneaky_field");
    }

    // Negative controls for the three #13295 fail-closed closures (#13300
    // review F2). Each control asserts the typed refusal its closure
    // introduced on a minimally edited fixture, so reverting the closure
    // flips exactly its control red.

    /// Fix 3 (#13295): a truncated stage graph — a mode-required stage
    /// omitted entirely — must be rejected. The declared-row two-way check
    /// above only inspects rows that exist, so the complement check in
    /// `validate_stage_graph` is the only surface that sees the absent
    /// stage.
    #[test]
    fn truncated_graph_rejected_when_a_mode_required_stage_is_absent() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/standalone_install_vectors/vectors/v001-archive-pair-success.json");
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text.replace("\r\n", "\n"),
            Err(error) => fail(&format!("fixture read failed: {error}")),
        };
        let truncated = text.replace(
            ",\n    { \"stage_id\": \"installed_transition\", \"applicability\": \"required\", \
             \"predecessors\": [\"fresh_process_observation\"], \"port_script\": \"install\" }",
            "",
        );
        assert_ne!(text, truncated, "fixture edit must apply");
        let vector: Vector = match serde_json::from_str(&truncated) {
            Ok(vector) => vector,
            Err(error) => fail(&format!("parse failed: {error}")),
        };
        match validate_vector(&vector) {
            Err(OracleError::StageGraph(message)) => {
                assert!(
                    message.contains("mode-required stage")
                        && message.contains("absent from the stage graph"),
                    "{message}"
                );
            }
            other => fail(&format!("truncated graph must be rejected, got {other:?}")),
        }
    }

    /// Fix 2 (#13295): a successful executable observation whose call omits
    /// the `executables` map positively observes no required role. The pair
    /// gate must fold the attempt to `PairIncomplete` instead of letting the
    /// success reach promotion with `pair_claims_satisfied`.
    #[test]
    fn absent_executables_map_fails_the_pair_gate_at_observation() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/standalone_install_vectors/vectors/v001-archive-pair-success.json");
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text.replace("\r\n", "\n"),
            Err(error) => fail(&format!("fixture read failed: {error}")),
        };
        let no_map =
            text.replace(", \"executables\": { \"perllsp\": \"ok\", \"perl-dap\": \"ok\" }", "");
        assert_ne!(text, no_map, "fixture edit must apply");
        let vector: Vector = match serde_json::from_str(&no_map) {
            Ok(vector) => vector,
            Err(error) => fail(&format!("parse failed: {error}")),
        };
        if let Err(error) = validate_vector(&vector) {
            fail(&format!("edited vector must stay corpus-valid: {error}"));
        }
        let packet = match derive_packet(&vector, Deviation::None) {
            Ok(packet) => packet,
            Err(error) => {
                fail(&format!("absent map must fold to a typed failure, not error: {error}"))
            }
        };
        assert_eq!(packet.terminal.result, TerminalResult::Failed);
        assert_eq!(packet.terminal.reason_family, ReasonFamily::PairIncomplete);
        assert_eq!(packet.terminal.stage_id, StageId::ExecutableObservation);
        assert_eq!(packet.side_effect_ceiling, CeilingLevel::Staged);
        assert!(!packet.pair_claims_satisfied);
        assert!(!packet.effects.iter().any(|effect| effect.kind == "promoted"));
    }

    /// Fix 1 (#13295): a declared predecessor with neither a receipt in this
    /// attempt's chain nor positive mode authorization is a corpus authoring
    /// bug; composition must fail closed instead of minting an empty
    /// predecessor list. v011's `checksum_integrity` script produces no call,
    /// so under WarnAndContinue the mandatory failure is bypassed without a
    /// receipt and without mode authorization, and the declared successor's
    /// predecessor chain cannot be resolved.
    #[test]
    fn unresolvable_declared_predecessor_fails_closed_at_composition() {
        let vectors = corpus();
        let vector = vector_by_id(&vectors, "v011-missing-mandatory-stage");

        // Conformant anchor: without a deviation the missing mandatory stage
        // stops the attempt before any successor runs.
        let packet = derive(vector, Deviation::None);
        assert_eq!(packet.terminal.reason_family, ReasonFamily::MissingEvidence);

        match derive_packet(vector, Deviation::WarnAndContinue) {
            Err(OracleError::StageGraph(message)) => {
                assert!(
                    message.contains("checksum_integrity")
                        && message.contains("no receipt and was not"),
                    "{message}"
                );
            }
            other => {
                fail(&format!("unresolvable declared predecessor must fail closed, got {other:?}"))
            }
        }
    }
}
