//! Closed vocabularies and the bounded feature-readiness fixture-node
//! registry for the FR-C05 packet generator (#11286).
//!
//! The registry is deliberately scoped to the representative packet fixtures
//! mandated by #11286. The full stable feature DAG remains owned by #11279;
//! this module never claims programme-graph authority, readiness evaluation,
//! current-tree observation, offline frontier, or live reconciliation.

use std::collections::BTreeSet;

/// True role of a node's packet. Roles are distinct claim ceilings; a role is
/// never widened into product implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    ProductImplementation,
    ProofOnly,
    InstalledClientProof,
    ResearchDecision,
    GovernanceSupport,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Role::ProductImplementation => "product_implementation",
            Role::ProofOnly => "proof_only",
            Role::InstalledClientProof => "installed_client_proof",
            Role::ResearchDecision => "research_decision",
            Role::GovernanceSupport => "governance_support",
        }
    }

    /// Only a product implementation role may encode an implementation step.
    pub const fn allows_product_implementation(self) -> bool {
        matches!(self, Role::ProductImplementation)
    }
}

/// Whether the train currently admits coding work for the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Disposition {
    Actionable,
    Deferred,
    BlockedExternalManual,
}

/// Independent denominator disposition from the controlling feature-readiness
/// issues. This is intentionally separate from `NodeSpec`: the packet
/// registry must be checked against a separately maintained accounting of what
/// is actionable, deferred, or outside this bounded packet surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DenominatorDisposition {
    Actionable,
    Deferred,
    Excluded,
}

/// One entry in the independently derived #11279/#11286 accounting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenominatorEntry {
    pub issue: u32,
    pub packet_node: Option<&'static str>,
    pub disposition: DenominatorDisposition,
    pub reason: &'static str,
}

impl Disposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Disposition::Actionable => "actionable",
            Disposition::Deferred => "deferred",
            Disposition::BlockedExternalManual => "blocked_external_manual",
        }
    }
}

/// Claim profile carried by the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Profile {
    CoreSemantic,
    OptionalFramework,
    InstalledClient,
    ProofOnly,
    Governance,
    Research,
    Deferred,
}

impl Profile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Profile::CoreSemantic => "core_semantic",
            Profile::OptionalFramework => "optional_framework",
            Profile::InstalledClient => "installed_client",
            Profile::ProofOnly => "proof_only",
            Profile::Governance => "governance",
            Profile::Research => "research",
            Profile::Deferred => "deferred",
        }
    }
}

/// Feature domain tags. Explicit per row; never inferred from titles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Domain {
    Imports,
    Navigation,
    SignatureHelp,
    SemanticTokens,
    Critic,
    Formatting,
    ParserResearch,
    VscodeClient,
    Distribution,
    SupportRegistry,
    Dap,
}

impl Domain {
    pub const fn as_str(self) -> &'static str {
        match self {
            Domain::Imports => "imports",
            Domain::Navigation => "navigation",
            Domain::SignatureHelp => "signature_help",
            Domain::SemanticTokens => "semantic_tokens",
            Domain::Critic => "critic",
            Domain::Formatting => "formatting",
            Domain::ParserResearch => "parser_research",
            Domain::VscodeClient => "vscode_client",
            Domain::Distribution => "distribution",
            Domain::SupportRegistry => "support_registry",
            Domain::Dap => "dap",
        }
    }
}

/// Authority-map group for one exact owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuthorityGroup {
    MustAlreadyBeCurrent,
    MustBeConsumedNeverReimplemented,
    OwnedByThisNode,
    CandidateMayBeMined,
    ConsumerFanInAfterPr,
    ExternalManualOwner,
    ExplicitlyNotOwned,
}

impl AuthorityGroup {
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthorityGroup::MustAlreadyBeCurrent => "must_already_be_current",
            AuthorityGroup::MustBeConsumedNeverReimplemented => {
                "must_be_consumed_never_reimplemented"
            }
            AuthorityGroup::OwnedByThisNode => "owned_by_this_node",
            AuthorityGroup::CandidateMayBeMined => "candidate_may_be_mined",
            AuthorityGroup::ConsumerFanInAfterPr => "consumer_fan_in_after_pr",
            AuthorityGroup::ExternalManualOwner => "external_manual_owner",
            AuthorityGroup::ExplicitlyNotOwned => "explicitly_not_owned",
        }
    }
}

/// Worklist mode of one required domain artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtifactMode {
    Create,
    Update,
    Consume,
    ProveUnchanged,
    /// Closed-vocabulary completeness: emitted when a future artifact row warrants it.
    #[allow(dead_code)]
    NotApplicable,
}

impl ArtifactMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            ArtifactMode::Create => "create",
            ArtifactMode::Update => "update",
            ArtifactMode::Consume => "consume",
            ArtifactMode::ProveUnchanged => "prove_unchanged",
            ArtifactMode::NotApplicable => "not_applicable",
        }
    }
}

/// Durable-spec disposition; exactly one shared value per packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DurableSpecDisposition {
    ExistingNormativeContractSufficient,
    CompileDurableDeltaIntoExistingOwner,
    IssuePlanSufficientForThisLeaf,
    ReturnToIssueForUnsettledDecision,
    /// Closed-vocabulary completeness: emitted only when no owner can be named.
    #[allow(dead_code)]
    NotProven,
}

impl DurableSpecDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            DurableSpecDisposition::ExistingNormativeContractSufficient => {
                "EXISTING_NORMATIVE_CONTRACT_SUFFICIENT"
            }
            DurableSpecDisposition::CompileDurableDeltaIntoExistingOwner => {
                "COMPILE_DURABLE_DELTA_INTO_EXISTING_OWNER"
            }
            DurableSpecDisposition::IssuePlanSufficientForThisLeaf => {
                "ISSUE_PLAN_SUFFICIENT_FOR_THIS_LEAF"
            }
            DurableSpecDisposition::ReturnToIssueForUnsettledDecision => {
                "RETURN_TO_ISSUE_FOR_UNSETTLED_DECISION"
            }
            DurableSpecDisposition::NotProven => "NOT_PROVEN",
        }
    }
}

/// Closed implementation-sequence steps. Non-product roles replace the
/// implementation step with their exact-role step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SequenceStep {
    VerifyPacketAndWriterState,
    ReadNamedAuthorities,
    MaterializeFirstFalsifier,
    ImplementProposition,
    ExecuteProofProtocol,
    ExecuteResearchProtocol,
    ExecuteRegistryMapping,
    RecordDispositionNoExecution,
    RetireOldPaths,
    UpdateRequiredArtifacts,
    RunFocusedProof,
    RunNegativeMutations,
    InspectDiffAgainstSurfaces,
    ProduceReviewForwardHandoff,
    StopAndTransferAdjacentFindings,
}

impl SequenceStep {
    pub const fn as_str(self) -> &'static str {
        match self {
            SequenceStep::VerifyPacketAndWriterState => "verify_packet_and_writer_state",
            SequenceStep::ReadNamedAuthorities => "read_named_authorities",
            SequenceStep::MaterializeFirstFalsifier => "materialize_first_falsifier",
            SequenceStep::ImplementProposition => "implement_proposition",
            SequenceStep::ExecuteProofProtocol => "execute_proof_protocol",
            SequenceStep::ExecuteResearchProtocol => "execute_research_protocol",
            SequenceStep::ExecuteRegistryMapping => "execute_registry_mapping",
            SequenceStep::RecordDispositionNoExecution => "record_disposition_no_execution",
            SequenceStep::RetireOldPaths => "retire_old_paths",
            SequenceStep::UpdateRequiredArtifacts => "update_required_artifacts",
            SequenceStep::RunFocusedProof => "run_focused_proof",
            SequenceStep::RunNegativeMutations => "run_negative_mutations",
            SequenceStep::InspectDiffAgainstSurfaces => "inspect_diff_against_surfaces",
            SequenceStep::ProduceReviewForwardHandoff => "produce_review_forward_handoff",
            SequenceStep::StopAndTransferAdjacentFindings => "stop_and_transfer_adjacent_findings",
        }
    }
}

/// Required action implied by an observed live snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiveAction {
    None,
    Resume,
    Repair,
    Restack,
    Review,
}

impl LiveAction {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(LiveAction::None),
            "resume" => Some(LiveAction::Resume),
            "repair" => Some(LiveAction::Repair),
            "restack" => Some(LiveAction::Restack),
            "review" => Some(LiveAction::Review),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            LiveAction::None => "none",
            LiveAction::Resume => "resume",
            LiveAction::Repair => "repair",
            LiveAction::Restack => "restack",
            LiveAction::Review => "review",
        }
    }
}

/// One exact owner grouped into the authority map.
#[derive(Clone, Debug)]
pub struct Authority {
    pub reference: &'static str,
    pub subject: &'static str,
    pub group: AuthorityGroup,
}

/// One typed feature/operation row compiled from explicit domain semantics.
#[derive(Clone, Debug)]
pub struct OperationRow {
    pub feature: &'static str,
    pub provider_or_client: &'static str,
    pub source_subject: &'static str,
    /// semantic / currentness / fallback / refusal / legitimate-empty policy.
    pub policy_semantic: &'static str,
    pub policy_currentness: &'static str,
    pub policy_fallback: &'static str,
    pub policy_refusal: &'static str,
    pub policy_legitimate_empty: &'static str,
    pub canonical_owner: &'static str,
    pub old_path: &'static str,
    pub proof_owner: &'static str,
}

/// One required domain artifact on the stable worklist.
#[derive(Clone, Debug)]
pub struct ArtifactRow {
    pub id: &'static str,
    pub kind: &'static str,
    pub owner: &'static str,
    pub mode: ArtifactMode,
    pub current_disposition: &'static str,
    pub required_change_or_proof: &'static str,
    pub check_command: &'static str,
    pub review_lens: &'static str,
    pub claim_impact: &'static str,
}

/// One negative/near-miss/mutation control class with its exact subject.
#[derive(Clone, Debug)]
pub struct ControlRow {
    pub class: &'static str,
    pub subject: &'static str,
}

/// One review lens with applicability, reason, and typed questions.
#[derive(Clone, Debug)]
pub struct LensSpec {
    pub name: &'static str,
    pub applicable: bool,
    pub reason: &'static str,
    pub questions: &'static [&'static str],
}

/// One stage-specific falsification example emitted into reviewer packets.
#[derive(Clone, Copy, Debug)]
pub struct StageExample {
    pub stage: &'static str,
    pub question: &'static str,
}

/// One old-path seam with its terminal disposition.
#[derive(Clone, Copy, Debug)]
pub struct OldPathRow {
    pub seam: &'static str,
    pub terminal_disposition: &'static str,
}

/// One bounded representative fixture node at its true role.
///
/// Semantics are anchored to the node's owning issue; the registry carries no
/// readiness evaluation and no observational state.
#[derive(Clone, Debug)]
pub struct NodeSpec {
    pub node_id: &'static str,
    pub issues: Vec<u32>,
    pub controller_issue: u32,
    pub domain: Domain,
    pub role: Role,
    pub disposition: Disposition,
    pub profile: Profile,
    pub objective_sentence: &'static str,
    pub establishes: Vec<&'static str>,
    pub cannot_establish: Vec<&'static str>,
    pub prerequisite_disposition: &'static str,
    pub successors: Vec<&'static str>,
    pub remaining_not_proven: Vec<&'static str>,
    pub rollback_meaning: &'static str,
    pub authorities: Vec<Authority>,
    pub operations: Vec<OperationRow>,
    pub allowed_surfaces: Vec<&'static str>,
    pub forbidden_surfaces: Vec<&'static str>,
    pub artifacts: Vec<ArtifactRow>,
    pub durable_spec: (DurableSpecDisposition, &'static str, &'static str),
    pub first_falsifier: (&'static str, &'static str, &'static str),
    pub positive_discriminator: &'static str,
    pub controls: Vec<ControlRow>,
    pub commands: Vec<(&'static str, &'static str, &'static str)>,
    pub lenses: Vec<LensSpec>,
    /// Indices into nodes::STAGE_EXAMPLES; keeps node data 'static and small.
    pub stage_examples: Vec<usize>,
    pub old_paths: Vec<OldPathRow>,
    /// Extra stop conditions beyond the shared role-derived ones.
    pub extra_stop_conditions: Vec<&'static str>,
}

fn base_sequence(role: Role) -> Vec<SequenceStep> {
    let core_step = match role {
        Role::ProductImplementation => SequenceStep::ImplementProposition,
        Role::ProofOnly | Role::InstalledClientProof => SequenceStep::ExecuteProofProtocol,
        Role::ResearchDecision => SequenceStep::ExecuteResearchProtocol,
        Role::GovernanceSupport => SequenceStep::ExecuteRegistryMapping,
    };
    let disposition_tail = match role {
        Role::GovernanceSupport => vec![SequenceStep::RecordDispositionNoExecution],
        _ => vec![],
    };
    vec![
        SequenceStep::VerifyPacketAndWriterState,
        SequenceStep::ReadNamedAuthorities,
        SequenceStep::MaterializeFirstFalsifier,
        core_step,
        SequenceStep::RetireOldPaths,
        SequenceStep::UpdateRequiredArtifacts,
        SequenceStep::RunFocusedProof,
        SequenceStep::RunNegativeMutations,
        SequenceStep::InspectDiffAgainstSurfaces,
        SequenceStep::ProduceReviewForwardHandoff,
    ]
    .into_iter()
    .chain(disposition_tail)
    .chain([SequenceStep::StopAndTransferAdjacentFindings])
    .collect()
}

/// Deterministic registry digest over every node's stable identity fields.
/// Inputs are sorted by node id, so iteration order never changes bytes.
pub fn registry_digest(nodes: &[NodeSpec]) -> String {
    let inputs: Vec<(String, String)> = nodes
        .iter()
        .map(|node| {
            let mut text = String::new();
            text.push_str(node.node_id);
            for issue in &node.issues {
                text.push_str(&format!("|issue:{issue}"));
            }
            text.push_str("|domain:");
            text.push_str(node.domain.as_str());
            text.push_str("|role:");
            text.push_str(node.role.as_str());
            text.push_str("|disposition:");
            text.push_str(node.disposition.as_str());
            text.push_str("|profile:");
            text.push_str(node.profile.as_str());
            text.push_str("|objective:");
            text.push_str(node.objective_sentence);
            (node.node_id.to_owned(), text)
        })
        .collect();
    crate::tasks::emacs_train_context::digest::composite_digest(&inputs)
}
pub(crate) fn sequence_strings(role: Role) -> Vec<String> {
    base_sequence(role).iter().map(|step| step.as_str().to_owned()).collect()
}

/// Look up one node by node id, issue number (`11286`/`#11286`), or unique
/// prefix. Ambiguity and absence fail closed with the candidate set named.
pub fn find_node<'a>(nodes: &'a [NodeSpec], query: &str) -> color_eyre::eyre::Result<&'a NodeSpec> {
    use color_eyre::eyre::bail;
    let trimmed = query.trim_start_matches('#');
    let issue = trimmed.parse::<u32>().ok();
    let matches: Vec<&NodeSpec> = nodes
        .iter()
        .filter(|node| {
            node.node_id == query
                || issue.is_some_and(|value| node.issues.contains(&value))
                || (trimmed.len() >= 3 && node.node_id.starts_with(trimmed))
        })
        .collect();
    match matches.len() {
        1 => Ok(matches[0]),
        0 => {
            let known: BTreeSet<&str> = nodes.iter().map(|node| node.node_id).collect();
            bail!("no fixture node matches {query:?}; known ids: {known:?}")
        }
        n => bail!("{query:?} matches {n} fixture nodes; name exactly one node id"),
    }
}
