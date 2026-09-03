//! Accepted-ticket effect-sink contract and checked effect inventory (#11672).
//!
//! This module defines, without changing any sink behavior:
//!
//! 1. One closed common outcome vocabulary
//!    [`ParseEffectCommitOutcomeV1`] that every governed parse-derived sink
//!    mutation will return once its focused consumer child (#11675 readiness,
//!    #11674 document symbols, #11673 diagnostics) cuts over to sink-local
//!    compare-and-mutate commits.
//! 2. One checked static inventory [`parse_effect_sinks_v1`] mapping each
//!    parse-derived effect to its actual sink owner, accepted-ticket inputs,
//!    irreversible mutation boundary, currentness-comparison location,
//!    terminal/clear policy, proof owner, compatibility exit, and disposition.
//!
//! # Claim ceiling
//!
//! Contract and inventory only. No runtime dispatcher, no generic callback
//! engine, no effect scheduling, no sink mutation change, no provider or
//! publication behavior change, no mutable lifecycle/status database. The
//! legacy helper [`crate::runtime::text_sync`]::`commit_parse_effect_if_current`
//! remains exactly what it is today: a reported compatibility adapter with a
//! named exit owner, never final mutation authority.
//!
//! # Ownership rulings (from the controlling issue)
//!
//! - #11665 owns accepted parser tickets (not yet landed; see the
//!   `NotProven` snapshot-publication row).
//! - Each sink owns its compare-and-mutate operation.
//! - #8619/#8642 remain the workspace-index atomic publication/read
//!   authority. #7309 remains semantic/project publication authority.
//!   #7286/#7288 own diagnostic computation/result reuse, not outbound
//!   publication currentness. #6729 owns per-kind result-ID caches.
//! - The common outcome vocabulary normalizes evidence; it does not
//!   centralize mutation.

use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// Outcome vocabulary
// ---------------------------------------------------------------------------

/// Closed common outcome vocabulary for one governed parse-derived sink
/// commit attempt (version V1).
///
/// A stale/superseded ticket produces a typed rejection or typed
/// non-application variant -- never silent success. Evidence problems are
/// split by cause, never collapsed into a commit/no-op/clear:
///
/// - *absent* evidence (currentness could not be observed at all) maps to
///   [`ParseEffectCommitOutcomeV1::NotProven`] -- nothing may be claimed;
/// - *unreliable* evidence (an instrument/schema participating in the commit
///   failed mid-commit) maps to the distinct
///   [`ParseEffectCommitOutcomeV1::InstrumentOrSchemaFailure`] so downstream
///   policy can distinguish "could not look" from "looked, reading untrustworthy".
///
/// Neither variant is a commit, a non-application, or a clear.
///
/// Consumers land with the focused children (#11675, #11674, #11673); until
/// those cuts land this type is intentionally unreferenced by production
/// mutation paths, which is why dead-code allowance follows the repo's
/// declared-dormant-contract precedent.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseEffectCommitOutcomeV1 {
    /// The exact accepted ticket was validated at the sink-owned boundary
    /// and the irreversible mutation happened inside it.
    CommittedCurrent,
    /// Ticket generation was superseded before/at the sink boundary.
    RejectedStaleTicket,
    /// Ticket document-instance identity does not match the sink subject.
    RejectedWrongDocumentInstance,
    /// Source projection/configuration excludes the ticket from effects.
    RejectedSourceProjectionOrConfiguration,
    /// The sink's own local generation advanced past the ticket.
    RejectedSinkGenerationAdvanced,
    /// Lifecycle/shutdown state forbids the mutation.
    RejectedLifecycleState,
    /// Another accepted state superseded this effect before mutation;
    /// typed non-application, never success.
    SupersededBeforeMutation,
    /// The accepted ticket provably requires no sink effect.
    NoEffectRequired,
    /// A current empty/partial/failure terminal result safely cleared or
    /// replaced stale prior contribution.
    SafeClearCommitted,
    /// The sink store is unavailable (poisoned lock, missing store).
    SinkUnavailable,
    /// The mutation failed after admission for a product reason; the
    /// predecessor state is preserved and reported.
    ProductFailure,
    /// An instrument/schema needed by the commit failed; evidence is
    /// unreliable rather than absent.
    InstrumentOrSchemaFailure,
    /// Currentness/mutation could not be observed; nothing may be claimed.
    NotProven,
}

impl ParseEffectCommitOutcomeV1 {
    /// Whether the outcome proves an irreversible current mutation happened.
    #[allow(dead_code)]
    pub(crate) fn is_committed(self) -> bool {
        matches!(
            self,
            ParseEffectCommitOutcomeV1::CommittedCurrent
                | ParseEffectCommitOutcomeV1::SafeClearCommitted
        )
    }

    /// Whether the outcome is a typed non-application (stale, superseded,
    /// excluded, or provably-empty attempt). Non-application outcomes are
    /// honest results; they are not commits and must not trigger downstream
    /// effects without explicit policy.
    #[allow(dead_code)]
    pub(crate) fn is_non_application(self) -> bool {
        matches!(
            self,
            ParseEffectCommitOutcomeV1::RejectedStaleTicket
                | ParseEffectCommitOutcomeV1::RejectedWrongDocumentInstance
                | ParseEffectCommitOutcomeV1::RejectedSourceProjectionOrConfiguration
                | ParseEffectCommitOutcomeV1::RejectedSinkGenerationAdvanced
                | ParseEffectCommitOutcomeV1::RejectedLifecycleState
                | ParseEffectCommitOutcomeV1::SupersededBeforeMutation
                | ParseEffectCommitOutcomeV1::NoEffectRequired
        )
    }
}

/// Terminal parser-result classes a current ticket can carry. Every sink row
/// declares an explicit action for every class so a current empty/partial/
/// failure result can never silently keep stale prior success.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum TerminalParseClassV1 {
    Clean,
    RecoveredPartial,
    BudgetExhausted,
    Cancelled,
    CatastrophicMinimal,
    GuardedNoParserState,
    Desynchronized,
    InstrumentFailure,
}

/// All terminal classes, in canonical order.
#[allow(dead_code)]
pub(crate) const TERMINAL_PARSE_CLASSES_V1: [TerminalParseClassV1; 8] = [
    TerminalParseClassV1::Clean,
    TerminalParseClassV1::RecoveredPartial,
    TerminalParseClassV1::BudgetExhausted,
    TerminalParseClassV1::Cancelled,
    TerminalParseClassV1::CatastrophicMinimal,
    TerminalParseClassV1::GuardedNoParserState,
    TerminalParseClassV1::Desynchronized,
    TerminalParseClassV1::InstrumentFailure,
];

/// What a sink does when a CURRENT ticket of some terminal class arrives.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SinkCurrentActionV1 {
    /// Replace prior contribution with the new candidate.
    Replace,
    /// Remove this document's prior contribution.
    Clear,
    /// Provably identical contribution; keep existing state with identity
    /// evidence (no churn of result IDs/readiness).
    IdentityNoOp,
    /// Emit the notification/publication for this class.
    Publish,
    /// Compatibility gate only: forwards to whichever row owns the actual
    /// mutation; carries no mutation authority of its own.
    DelegateToOwningSink,
    /// Advisory bookkeeping/observation only (lifecycle counters, evidence
    /// spans); carries no content state, so nothing can go stale here.
    Observe,
    /// No correctness-bearing mutation exists for this class in this sink;
    /// requires the row's claim ceiling to say why.
    OutOfScope,
}

/// Explicit per-terminal-class policy for one sink row.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClearReplacePolicyV1 {
    clean: SinkCurrentActionV1,
    recovered_partial: SinkCurrentActionV1,
    budget_exhausted: SinkCurrentActionV1,
    cancelled: SinkCurrentActionV1,
    catastrophic_minimal: SinkCurrentActionV1,
    guarded_no_parser_state: SinkCurrentActionV1,
    desynchronized: SinkCurrentActionV1,
    instrument_failure: SinkCurrentActionV1,
}

#[allow(dead_code)]
impl ClearReplacePolicyV1 {
    /// Build a policy from the canonical-class-ordered action list.
    const fn new(actions: [SinkCurrentActionV1; 8]) -> Self {
        Self {
            clean: actions[0],
            recovered_partial: actions[1],
            budget_exhausted: actions[2],
            cancelled: actions[3],
            catastrophic_minimal: actions[4],
            guarded_no_parser_state: actions[5],
            desynchronized: actions[6],
            instrument_failure: actions[7],
        }
    }

    const fn action(self, class: TerminalParseClassV1) -> SinkCurrentActionV1 {
        match class {
            TerminalParseClassV1::Clean => self.clean,
            TerminalParseClassV1::RecoveredPartial => self.recovered_partial,
            TerminalParseClassV1::BudgetExhausted => self.budget_exhausted,
            TerminalParseClassV1::Cancelled => self.cancelled,
            TerminalParseClassV1::CatastrophicMinimal => self.catastrophic_minimal,
            TerminalParseClassV1::GuardedNoParserState => self.guarded_no_parser_state,
            TerminalParseClassV1::Desynchronized => self.desynchronized,
            TerminalParseClassV1::InstrumentFailure => self.instrument_failure,
        }
    }

    /// Policy for sinks whose current candidates always replace prior
    /// contributions (symbols, workspace facts, published snapshots):
    /// success replaces; every failure/cancelled/guarded terminal clears or
    /// replaces stale prior success; desynchronized/instrument-failure
    /// classes cannot act (NotProven domain).
    const fn replace_on_success_clear_on_failure() -> Self {
        Self::new([
            SinkCurrentActionV1::Replace,
            SinkCurrentActionV1::Replace,
            SinkCurrentActionV1::Replace,
            SinkCurrentActionV1::Clear,
            SinkCurrentActionV1::Replace,
            SinkCurrentActionV1::Clear,
            SinkCurrentActionV1::OutOfScope,
            SinkCurrentActionV1::OutOfScope,
        ])
    }

    /// Policy for outbound publication sinks: publish whatever the current
    /// ticket carries (including empty/binary/failure sets -- LSP
    /// publishDiagnostics is replace-mode); desynchronized and
    /// instrument-failure classes stay out of scope.
    const fn publish_current_terminal() -> Self {
        Self::new([
            SinkCurrentActionV1::Publish,
            SinkCurrentActionV1::Publish,
            SinkCurrentActionV1::Publish,
            SinkCurrentActionV1::Publish,
            SinkCurrentActionV1::Publish,
            SinkCurrentActionV1::Publish,
            SinkCurrentActionV1::OutOfScope,
            SinkCurrentActionV1::OutOfScope,
        ])
    }

    /// Policy for advisory sinks (parse-lifecycle counters, evidence
    /// observations): observe every terminal class, mutate no content state.
    const fn observe_only() -> Self {
        Self::new([
            SinkCurrentActionV1::Observe,
            SinkCurrentActionV1::Observe,
            SinkCurrentActionV1::Observe,
            SinkCurrentActionV1::Observe,
            SinkCurrentActionV1::Observe,
            SinkCurrentActionV1::Observe,
            SinkCurrentActionV1::Observe,
            SinkCurrentActionV1::Observe,
        ])
    }

    /// Policy for the legacy compatibility gate: it never acts on terminal
    /// classes itself; every class delegates to the owning sink row.
    const fn delegate_to_owning_sink() -> Self {
        Self::new([
            SinkCurrentActionV1::DelegateToOwningSink,
            SinkCurrentActionV1::DelegateToOwningSink,
            SinkCurrentActionV1::DelegateToOwningSink,
            SinkCurrentActionV1::DelegateToOwningSink,
            SinkCurrentActionV1::DelegateToOwningSink,
            SinkCurrentActionV1::DelegateToOwningSink,
            SinkCurrentActionV1::DelegateToOwningSink,
            SinkCurrentActionV1::DelegateToOwningSink,
        ])
    }

    /// Policy for reader/projection rows and externally owned authorities
    /// this contract references but does not govern.
    const fn out_of_scope() -> Self {
        Self::new([
            SinkCurrentActionV1::OutOfScope,
            SinkCurrentActionV1::OutOfScope,
            SinkCurrentActionV1::OutOfScope,
            SinkCurrentActionV1::OutOfScope,
            SinkCurrentActionV1::OutOfScope,
            SinkCurrentActionV1::OutOfScope,
            SinkCurrentActionV1::OutOfScope,
            SinkCurrentActionV1::OutOfScope,
        ])
    }

    fn rows(self) -> [(TerminalParseClassV1, SinkCurrentActionV1); 8] {
        [
            (TerminalParseClassV1::Clean, self.clean),
            (TerminalParseClassV1::RecoveredPartial, self.recovered_partial),
            (TerminalParseClassV1::BudgetExhausted, self.budget_exhausted),
            (TerminalParseClassV1::Cancelled, self.cancelled),
            (TerminalParseClassV1::CatastrophicMinimal, self.catastrophic_minimal),
            (TerminalParseClassV1::GuardedNoParserState, self.guarded_no_parser_state),
            (TerminalParseClassV1::Desynchronized, self.desynchronized),
            (TerminalParseClassV1::InstrumentFailure, self.instrument_failure),
        ]
    }
}

// ---------------------------------------------------------------------------
// Inventory schema
// ---------------------------------------------------------------------------

/// Canonical logical sink stores. The call-site ledger enforces needle-level
/// uniqueness -- one registered (file, needle) ratchet maps to exactly one
/// row -- but cross-row site disjointness within a shared store is a review
/// obligation for this contract, not a mechanically enforced partition.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SinkStoreV1(&'static str);

#[allow(dead_code)]
impl SinkStoreV1 {
    pub(crate) const SYMBOL_INDEX: SinkStoreV1 = SinkStoreV1("symbol_index");
    pub(crate) const OUTBOUND_PUBLISH_DIAGNOSTICS: SinkStoreV1 =
        SinkStoreV1("outbound_publishDiagnostics");
    pub(crate) const DIAGNOSTIC_DEBOUNCER_QUEUE: SinkStoreV1 =
        SinkStoreV1("diagnostic_debouncer_queue");
    pub(crate) const DOCUMENTS_MAP: SinkStoreV1 = SinkStoreV1("documents_map");
    pub(crate) const WORKSPACE_INDEX: SinkStoreV1 = SinkStoreV1("workspace_index");
    pub(crate) const COORDINATOR_PARSE_LIFECYCLE: SinkStoreV1 =
        SinkStoreV1("coordinator_parse_lifecycle");
    pub(crate) const WORKSPACE_READINESS_PUBLICATION: SinkStoreV1 =
        SinkStoreV1("workspace_readiness_publication");
    pub(crate) const SEMANTIC_TOKENS_CACHE: SinkStoreV1 = SinkStoreV1("semantic_tokens_cache");

    const fn name(self) -> &'static str {
        self.0
    }
}

/// Where the currentness comparison happens for this row today.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurrentnessComparisonV1 {
    /// Legacy helper precheck: `(document_instance, generation)` compared
    /// under `documents.lock()`, released, THEN arbitrary callback runs.
    /// Admitted residual TOCTOU window; migration target; never atomic.
    HelperPrecheckThenCallback,
    /// Sink-local compare-and-mutate: the target law. Not yet implemented
    /// by any production sink; consumers cut over in focused children.
    SinkLocalCompareAndMutateTarget,
    /// Same-thread admission immediately after inserting the document
    /// instance; no deferred staleness window exists yet none can be ruled
    /// out once acceptance goes async (#11668).
    SameThreadAdmissionPreAcceptance,
    /// Currentness is owned by another landed authority; this contract
    /// references it and must not reimplement it.
    ExternalOwnedCurrentness,
    /// No currentness comparison applies (evidence-only or projection row).
    NotApplicable,
}

/// Accepted-ticket fields a row consumes (or explicitly does not).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TicketFieldRequirementV1 {
    DocumentInstanceIdentity,
    GenerationNumber,
    ClientUriShape,
    NormalizedUriKey,
    PublishedSnapshot,
    CapturedSourceText,
    SettleOwnershipFlag,
    /// Row consumes no accepted-ticket fields; reason required.
    NotRequired(&'static str),
}

/// Compatibility adapter with its named exit owner. An adapter can never
/// satisfy a row merely because its callback was invoked.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompatibilityAdapterExitV1 {
    pub adapter: &'static str,
    pub exit_owner_issue: &'static str,
}

/// Exactly-one disposition per row.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SinkDispositionV1 {
    NewFocusedChild(&'static str),
    ExistingExactOwner(&'static str),
    CompatibilityProjectionWithExit,
    NotParseDerived(&'static str),
    NotApplicable(&'static str),
    RetireUnreachable(&'static str),
    NotProven(&'static str),
}

impl SinkDispositionV1 {
    fn render(self) -> String {
        match self {
            SinkDispositionV1::NewFocusedChild(issue) => format!("new focused child ({issue})"),
            SinkDispositionV1::ExistingExactOwner(issue) => {
                format!("existing exact owner ({issue})")
            }
            SinkDispositionV1::CompatibilityProjectionWithExit => {
                "compatibility projection with exit".to_string()
            }
            SinkDispositionV1::NotParseDerived(reason) => {
                format!("not parse-derived ({reason})")
            }
            SinkDispositionV1::NotApplicable(reason) => format!("not applicable ({reason})"),
            SinkDispositionV1::RetireUnreachable(reason) => {
                format!("retire/unreachable ({reason})")
            }
            SinkDispositionV1::NotProven(reason) => format!("not proven ({reason})"),
        }
    }
}

/// One governed parse-derived effect: the complete row schema.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ParseEffectSinkRowV1 {
    /// Stable unique sink/effect ID (`<domain>.<effect>`).
    pub effect_id: &'static str,
    pub title: &'static str,
    /// Exactly one owning issue/component for the mutation decision.
    pub owner_issue: &'static str,
    pub ticket_inputs: &'static [TicketFieldRequirementV1],
    /// Additional sink-local subject/generation identity requirements.
    pub sink_local_subject: &'static str,
    pub store: SinkStoreV1,
    /// True only for the row that owns this store's registered mutation
    /// sites; reader/projection/external rows are false.
    pub owns_mutation_sites: bool,
    /// Exact irreversible mutation boundary (functions/stores).
    pub mutation_boundary: &'static str,
    pub currentness_comparison: CurrentnessComparisonV1,
    pub terminal_policy: ClearReplacePolicyV1,
    /// Focused proof owner (test filter this crate answers to).
    pub focused_proof_filter: &'static str,
    /// Composed proof owner (issue owning cross-sink proof).
    pub composed_proof_owner: &'static str,
    pub compatibility_adapter: Option<CompatibilityAdapterExitV1>,
    pub disposition: SinkDispositionV1,
    pub claim_ceiling: &'static str,
}

// ---------------------------------------------------------------------------
// Checked static inventory: parse_effect_sinks.v1
// ---------------------------------------------------------------------------

/// Ticket inputs consumed by every deferred post-parse effect routed through
/// the legacy helper today.
const DEFERRED_TICKET_INPUTS: &[TicketFieldRequirementV1] = &[
    TicketFieldRequirementV1::DocumentInstanceIdentity,
    TicketFieldRequirementV1::GenerationNumber,
    TicketFieldRequirementV1::ClientUriShape,
    TicketFieldRequirementV1::NormalizedUriKey,
    TicketFieldRequirementV1::PublishedSnapshot,
    TicketFieldRequirementV1::CapturedSourceText,
    TicketFieldRequirementV1::SettleOwnershipFlag,
];

/// The checked static inventory (`parse_effect_sinks.v1`).
///
/// Architecture/routing metadata only -- never mutable runtime state, and
/// never carrying runtime status or GitHub issue state. Every row names
/// exactly one owner and exactly one disposition; the deterministic checks
/// below fail on unknown/duplicated/unowned rows.
#[allow(dead_code)]
pub(crate) fn parse_effect_sinks_v1() -> &'static [ParseEffectSinkRowV1] {
    PARSE_EFFECT_SINKS_V1
}

#[allow(dead_code)]
static PARSE_EFFECT_SINKS_V1: &[ParseEffectSinkRowV1] = &[
    ParseEffectSinkRowV1 {
        effect_id: "diagnostics.parser-outbound-publication",
        title: "Parser diagnostics outbound publication (fast + debounced + syntax-only routes)",
        owner_issue: "#11673",
        ticket_inputs: DEFERRED_TICKET_INPUTS,
        sink_local_subject: "publishDiagnostics stream keyed by client URI; replace-mode per LSP",
        store: SinkStoreV1::OUTBOUND_PUBLISH_DIAGNOSTICS,
        owns_mutation_sites: true,
        mutation_boundary: "Outbound::notify(\"textDocument/publishDiagnostics\") admission \
            via publish_parse_errors_fast / publish_diagnostics (debounced target) / \
            syntax-only publisher",
        currentness_comparison: CurrentnessComparisonV1::HelperPrecheckThenCallback,
        terminal_policy: ClearReplacePolicyV1::publish_current_terminal(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#11676",
        compatibility_adapter: Some(CompatibilityAdapterExitV1 {
            adapter: "commit_parse_effect_if_current",
            exit_owner_issue: "#7379",
        }),
        disposition: SinkDispositionV1::NewFocusedChild("#11673"),
        claim_ceiling: "Route inventory + outcome vocabulary only; no publication behavior change here.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "diagnostics.didopen-guard-admission-publication",
        title: "Open/edit guard paths' empty/binary diagnostics publication (template/oversize/\
            binary, didOpen + didChange)",
        owner_issue: "#11673",
        ticket_inputs: &[TicketFieldRequirementV1::NotRequired(
            "pre-parse admission; no accepted ticket exists by construction until #11668 mints \
                one for guarded opens",
        )],
        sink_local_subject: "publishDiagnostics stream keyed by client URI (empty or binary set)",
        store: SinkStoreV1::OUTBOUND_PUBLISH_DIAGNOSTICS,
        owns_mutation_sites: false,
        mutation_boundary: "shares the #12031 sink-local diagnostics_sink::commit_push_diagnostics \
            boundary with diagnostics.parser-outbound-publication; its pre-#12031 direct \
            Outbound::notify guard branches retired into that single enqueue",
        currentness_comparison: CurrentnessComparisonV1::SameThreadAdmissionPreAcceptance,
        terminal_policy: ClearReplacePolicyV1::publish_current_terminal(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#11676",
        compatibility_adapter: None,
        disposition: SinkDispositionV1::NewFocusedChild("#11673"),
        claim_ceiling: "Inventory + ledger registration only; admission route unchanged in this PR. \
            Mutation-site ownership is reported through the shared diagnostics_sink registration \
            until #11673 gives this row its own focused commit law.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "document-symbols.replace-or-clear",
        title: "Document-symbol replacement/clear (reindex after accepted parse, clear on \
            failure/close/guard)",
        owner_issue: "#11674",
        ticket_inputs: DEFERRED_TICKET_INPUTS,
        sink_local_subject: "per-URI symbol document inside symbol_index",
        store: SinkStoreV1::SYMBOL_INDEX,
        owns_mutation_sites: true,
        mutation_boundary: "document_symbols_sink replace_document_symbols(uri, symbols) / \
            remove_document(uri) under one lock acquisition (#12035 accepted-symbols boundary; \
            the pre-#12035 text_sync call sites retired with it)",
        currentness_comparison: CurrentnessComparisonV1::HelperPrecheckThenCallback,
        terminal_policy: ClearReplacePolicyV1::replace_on_success_clear_on_failure(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#11676",
        compatibility_adapter: Some(CompatibilityAdapterExitV1 {
            adapter: "commit_parse_effect_if_current",
            exit_owner_issue: "#7379",
        }),
        disposition: SinkDispositionV1::NewFocusedChild("#11674"),
        claim_ceiling: "Route inventory only; symbol store untouched in this PR.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "workspace-index.live-contribution-replacement",
        title: "Workspace-index contribution replacement from live open buffers",
        owner_issue: "#8619/#8642",
        ticket_inputs: &[
            TicketFieldRequirementV1::DocumentInstanceIdentity,
            TicketFieldRequirementV1::GenerationNumber,
            TicketFieldRequirementV1::ClientUriShape,
            TicketFieldRequirementV1::NormalizedUriKey,
            TicketFieldRequirementV1::CapturedSourceText,
        ],
        sink_local_subject: "WorkspaceIndex file/fact entries crossed with typed non-zero SourceCommit",
        store: SinkStoreV1::WORKSPACE_INDEX,
        owns_mutation_sites: true,
        mutation_boundary: "external: WorkspaceIndex::index_live_file returning SourceCommitOutcome \
            {Accepted,NoOp,RejectedStale,Failed} -- atomic publication authority stays \
            #8619/#8642; this contract references it and does not reimplement it",
        currentness_comparison: CurrentnessComparisonV1::ExternalOwnedCurrentness,
        terminal_policy: ClearReplacePolicyV1::replace_on_success_clear_on_failure(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#11676",
        compatibility_adapter: Some(CompatibilityAdapterExitV1 {
            adapter: "commit_parse_effect_if_current",
            exit_owner_issue: "#7379",
        }),
        disposition: SinkDispositionV1::ExistingExactOwner("#8619/#8642"),
        claim_ceiling: "Reference existing SourceCommit/SourceCommitOutcome authority; exact \
            accepted-ticket integration lands with the parser-state train.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "workspace-index.reader-capture-projection",
        title: "Workspace-index reader capture/projection (query-time reads of indexed facts)",
        owner_issue: "#8619/#8642",
        ticket_inputs: &[TicketFieldRequirementV1::NotRequired(
            "read-side projection consumes committed index state, not tickets",
        )],
        sink_local_subject: "read locks over WorkspaceIndex maps",
        store: SinkStoreV1::WORKSPACE_INDEX,
        owns_mutation_sites: false,
        mutation_boundary: "none (read-only projection over externally owned index)",
        currentness_comparison: CurrentnessComparisonV1::ExternalOwnedCurrentness,
        terminal_policy: ClearReplacePolicyV1::out_of_scope(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#8619",
        compatibility_adapter: None,
        disposition: SinkDispositionV1::ExistingExactOwner("#8619/#8642"),
        claim_ceiling: "Read-authority reference only; no read path is touched here.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "semantic-project.contribution-publication",
        title: "Semantic/project contribution publication (fact shards, cross-file indexes)",
        owner_issue: "#7309",
        ticket_inputs: &[TicketFieldRequirementV1::NotRequired(
            "publication authority derives its own candidate identity from committed facts",
        )],
        sink_local_subject: "semantic fact shards and cross-file semantic indexes",
        store: SinkStoreV1::WORKSPACE_INDEX,
        owns_mutation_sites: false,
        mutation_boundary: "external: semantic fact-shard write-through owned by the #7309 publication seam; \
            no local reimplementation permitted",
        currentness_comparison: CurrentnessComparisonV1::ExternalOwnedCurrentness,
        terminal_policy: ClearReplacePolicyV1::out_of_scope(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#7309",
        compatibility_adapter: None,
        disposition: SinkDispositionV1::ExistingExactOwner("#7309"),
        claim_ceiling: "Authority reference only.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "result-id.local-state",
        title: "Local document-symbol/semantic-token result-ID cache state (per-kind result IDs)",
        owner_issue: "#6729",
        ticket_inputs: &[
            TicketFieldRequirementV1::ClientUriShape,
            TicketFieldRequirementV1::NormalizedUriKey,
        ],
        sink_local_subject: "semantic_tokens_cache entry keyed by normalized URI",
        store: SinkStoreV1::SEMANTIC_TOKENS_CACHE,
        owns_mutation_sites: true,
        mutation_boundary: "semantic_tokens_cache.lock() insert on provider compute + remove on open-document \
            session eviction (#6729 owns per-kind result-ID identity; parser acceptance does \
            not write this cache today)",
        currentness_comparison: CurrentnessComparisonV1::NotApplicable,
        terminal_policy: ClearReplacePolicyV1::replace_on_success_clear_on_failure(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#6729",
        compatibility_adapter: None,
        disposition: SinkDispositionV1::ExistingExactOwner("#6729"),
        claim_ceiling: "Cache-authority reference + eviction-site ledger only; cache policy unchanged.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "semantic-tokens.current-result-publication",
        title: "Semantic-token current-result publication (pull-mode provider responses)",
        owner_issue: "#9162/#9165/#9167",
        ticket_inputs: &[
            TicketFieldRequirementV1::PublishedSnapshot,
            TicketFieldRequirementV1::ClientUriShape,
        ],
        sink_local_subject: "provider response derived from the published snapshot",
        store: SinkStoreV1::SEMANTIC_TOKENS_CACHE,
        owns_mutation_sites: false,
        mutation_boundary: "none locally: pull publication currentness/equivalence is owned by the #9162 train; \
            this row prevents collapsing it with diagnostic computation or result-ID caches",
        currentness_comparison: CurrentnessComparisonV1::ExternalOwnedCurrentness,
        terminal_policy: ClearReplacePolicyV1::out_of_scope(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#9162",
        compatibility_adapter: None,
        disposition: SinkDispositionV1::ExistingExactOwner("#9162/#9165/#9167"),
        claim_ceiling: "Authority reference only.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "parser-state.accepted-snapshot-publication",
        title: "Accepted parsed-snapshot publication into the open document (lazy source-region/\
            type/semantic results derive from it)",
        owner_issue: "#11665/#11668/#11670",
        ticket_inputs: &[
            TicketFieldRequirementV1::DocumentInstanceIdentity,
            TicketFieldRequirementV1::GenerationNumber,
            TicketFieldRequirementV1::PublishedSnapshot,
            TicketFieldRequirementV1::CapturedSourceText,
        ],
        sink_local_subject: "DocumentState snapshot slot; instance-minting insert at didOpen; generation Arc \
            identity closes reopen ABA",
        store: SinkStoreV1::DOCUMENTS_MAP,
        owns_mutation_sites: true,
        mutation_boundary: "DocumentState::from_parts + publish_parsed_if_current (didOpen and synchronous \
            fallback routes) + the fresh-instance documents.lock().insert at didOpen",
        currentness_comparison: CurrentnessComparisonV1::SameThreadAdmissionPreAcceptance,
        terminal_policy: ClearReplacePolicyV1::replace_on_success_clear_on_failure(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#11670",
        compatibility_adapter: None,
        disposition: SinkDispositionV1::NotProven(
            "accepted-ticket minting/atomic acceptance has not landed yet (#11665/#11668/#11670 \
            open); publish_parsed_if_current exists but the immutable AcceptedParseGeneration \
            contract does not, so this row cannot claim proven governance",
        ),
        claim_ceiling: "Dependency pinning only; this contract neither changes nor gates the publication.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "didopen-guard.minimal-document-admission",
        title: "Pre-acceptance document-map admissions (guard minimal states on \
            template/oversize/binary opens+edits; didChange text-state advance and reinstall)",
        owner_issue: "#11665/#11668",
        ticket_inputs: &[TicketFieldRequirementV1::NotRequired(
            "these admissions happen before (or beside) any accepted parse; no ticket exists",
        )],
        sink_local_subject: "documents_map entry installed as minimal state, text-state-replaced ahead of a \
            deferred parse, or reinstated after the synchronous fallback publish",
        store: SinkStoreV1::DOCUMENTS_MAP,
        owns_mutation_sites: true,
        mutation_boundary: "minimal_state/minimal_state_from_rope guard inserts, replace_text_state advances, \
            and the scoped documents.insert(doc_state) sites in didChange/didOpen lifecycle code",
        currentness_comparison: CurrentnessComparisonV1::SameThreadAdmissionPreAcceptance,
        terminal_policy: ClearReplacePolicyV1::replace_on_success_clear_on_failure(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#11668",
        compatibility_adapter: None,
        disposition: SinkDispositionV1::NotProven(
            "pre-acceptance synchronous admission predates the accepted-state train; governed \
            once #11668 mints tickets for guarded opens and deferred parses",
        ),
        claim_ceiling: "Ledger registration only; admission behavior unchanged.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "readiness.active-document-parse-lifecycle",
        title: "Active-document parser readiness/progress lifecycle counters",
        owner_issue: "#11675",
        ticket_inputs: &[
            TicketFieldRequirementV1::ClientUriShape,
            TicketFieldRequirementV1::SettleOwnershipFlag,
        ],
        sink_local_subject: "Coordinator pending-parse lifecycle per URI (notify_change increments; \
            notify_parse_complete decrements exactly once per lifecycle, settle-hook owned when \
            the async worker already credited it)",
        store: SinkStoreV1::COORDINATOR_PARSE_LIFECYCLE,
        owns_mutation_sites: true,
        mutation_boundary: "Coordinator::notify_change / Coordinator::notify_parse_complete under coordinator \
            state lock (#3660 settle ownership)",
        currentness_comparison: CurrentnessComparisonV1::NotApplicable,
        terminal_policy: ClearReplacePolicyV1::observe_only(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#11675",
        compatibility_adapter: Some(CompatibilityAdapterExitV1 {
            adapter: "commit_parse_effect_if_current",
            exit_owner_issue: "#7379",
        }),
        disposition: SinkDispositionV1::NewFocusedChild("#11675"),
        claim_ceiling: "Lifecycle-route inventory only; counter semantics deliberately untouched here. \
            Currentness is not applicable: counters are idempotent bookkeeping keyed by \
            settle-ownership (#3660) and intentionally fire even when a stale effect's content \
            mutation was rejected, so coordinator state stays consistent.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "readiness.open-ready-publication",
        title: "Open-buffer active-document-ready notification and first-file readiness \
            transition",
        owner_issue: "#11675",
        ticket_inputs: &[
            TicketFieldRequirementV1::DocumentInstanceIdentity,
            TicketFieldRequirementV1::GenerationNumber,
            TicketFieldRequirementV1::ClientUriShape,
        ],
        sink_local_subject: "$/perlLsp/activeDocumentReady envelope generation-tagged with the first accepted \
            generation; IndexState Idle->Ready transition",
        store: SinkStoreV1::WORKSPACE_READINESS_PUBLICATION,
        owns_mutation_sites: true,
        mutation_boundary: "workspace_progress::send_active_document_ready_notification + \
            Coordinator::transition_to_ready inside the didOpen background task's Accepted arm \
            and the workspace-scan completion path",
        currentness_comparison: CurrentnessComparisonV1::HelperPrecheckThenCallback,
        terminal_policy: ClearReplacePolicyV1::publish_current_terminal(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#11675",
        compatibility_adapter: Some(CompatibilityAdapterExitV1 {
            adapter: "commit_parse_effect_if_current",
            exit_owner_issue: "#7379",
        }),
        disposition: SinkDispositionV1::NewFocusedChild("#11675"),
        claim_ceiling: "Publication-route inventory only.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "evidence.parse-effect-observations",
        title: "Parse/effect timing and evidence observations (spans, worker metrics)",
        owner_issue: "#9444",
        ticket_inputs: &[TicketFieldRequirementV1::NotRequired(
            "observations are advisory; they never gate correctness-bearing commits",
        )],
        sink_local_subject: "PERL_LSP_TIMING spans, ParseWorkerMetrics counters",
        store: SinkStoreV1::COORDINATOR_PARSE_LIFECYCLE,
        owns_mutation_sites: false,
        mutation_boundary: "none correctness-bearing (advisory observation only)",
        currentness_comparison: CurrentnessComparisonV1::NotApplicable,
        terminal_policy: ClearReplacePolicyV1::observe_only(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#9444",
        compatibility_adapter: None,
        disposition: SinkDispositionV1::NotApplicable(
            "evidence-only sink; excluded from the outcome contract because it can never be a \
            stale-correctness boundary",
        ),
        claim_ceiling: "Classification only.",
    },
    ParseEffectSinkRowV1 {
        effect_id: "compat.legacy-generic-callback-helper",
        title: "Legacy generic callback helper commit_parse_effect_if_current (+ free-function \
            core)",
        owner_issue: "#7379",
        ticket_inputs: &[
            TicketFieldRequirementV1::DocumentInstanceIdentity,
            TicketFieldRequirementV1::GenerationNumber,
            TicketFieldRequirementV1::NormalizedUriKey,
        ],
        sink_local_subject: "documents_map read-only check; NOT an atomic sink boundary",
        store: SinkStoreV1::DOCUMENTS_MAP,
        owns_mutation_sites: true,
        mutation_boundary: "documents.lock() precheck released BEFORE arbitrary closure runs; admitted residual \
            TOCTOU window; invoking this helper never satisfies any row's commit law",
        currentness_comparison: CurrentnessComparisonV1::HelperPrecheckThenCallback,
        terminal_policy: ClearReplacePolicyV1::delegate_to_owning_sink(),
        focused_proof_filter: "parse_effect_sink",
        composed_proof_owner: "#7379",
        compatibility_adapter: Some(CompatibilityAdapterExitV1 {
            adapter: "commit_parse_effect_if_current",
            exit_owner_issue: "#7379",
        }),
        disposition: SinkDispositionV1::CompatibilityProjectionWithExit,
        claim_ceiling: "Reported compatibility adapter with explicit consumers and removal owner (#7379 \
            fan-in); retires as focused children cut each call site over to sink-local \
            compare-and-mutate commits returning ParseEffectCommitOutcomeV1.",
    },
];

// ---------------------------------------------------------------------------
// Call-site ledger: structural falsifier for unregistered effects
// ---------------------------------------------------------------------------

/// One registered production mutation call site. `needle` counts byte
/// occurrences in `file` (repo-relative); `expected_count` is the ratchet --
/// adding/removing a registered mutation site requires updating its row here.
///
/// # Claim boundary (deliberately narrowed)
///
/// The per-file counts prove drift *at registered sites*; they cannot, by
/// themselves, discover a brand-new mutation API with a novel name. The
/// companion sweep test
/// [`parse_effect_sink_call_site_ledger_covers_registered_needles`] closes the
/// complementary direction for every needle registered here: any non-test
/// runtime source file containing that needle must either carry its own ledger
/// entry or appear in the explicit, individually-reasoned exemption table.
/// Discovery of genuinely novel sink APIs therefore fails closed (unregistered
/// occurrence of a known needle) or stays review-owned (new name), and is never
/// silently accepted.
#[cfg(test)]
struct CallSiteLedgerEntry {
    file: &'static str,
    needle: &'static str,
    expected_count: usize,
    effect_id: &'static str,
}

#[cfg(test)]
#[rustfmt::skip]
const CALL_SITE_LEDGER: &[CallSiteLedgerEntry] = &[
    // Legacy helper: wrapper + free-fn definitions, all invocation sites.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: "pub(crate) fn commit_parse_effect_if_current",
        expected_count: 2,
        effect_id: "compat.legacy-generic-callback-helper",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: "self.commit_parse_effect_if_current(&ticket",
        expected_count: 5,
        effect_id: "compat.legacy-generic-callback-helper",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: "= commit_parse_effect_if_current(",
        expected_count: 1,
        effect_id: "compat.legacy-generic-callback-helper",
    },
    // #13183 moved the open-path free-function invocation inside a scoped
    // `indexing_transition_lock` block, so its binding no longer sits on the
    // call expression. The mutation site is unchanged and stays ratcheted on
    // its own argument shape.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: "commit_parse_effect_if_current(\n                                &documents_for_task,",
        expected_count: 1,
        effect_id: "compat.legacy-generic-callback-helper",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: "commit_parse_effect_if_current(\n            &self.documents,",
        expected_count: 1,
        effect_id: "compat.legacy-generic-callback-helper",
    },
    // Document symbols (#12035 relocated the mutation boundary into the
    // accepted-symbols sink).
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/document_symbols_sink.rs",
        needle: "replace_document_symbols(",
        expected_count: 1,
        effect_id: "document-symbols.replace-or-clear",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/document_symbols_sink.rs",
        needle: "remove_document(",
        expected_count: 1,
        effect_id: "document-symbols.replace-or-clear",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync/symbols.rs",
        needle: "remove_document(",
        expected_count: 1,
        effect_id: "document-symbols.replace-or-clear",
    },
    // Definition surface of the clear helper plus its single eviction call.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync/symbols.rs",
        needle: "clear_document_symbols(",
        expected_count: 1,
        effect_id: "document-symbols.replace-or-clear",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/mod.rs",
        needle: "self.clear_document_symbols(key)",
        expected_count: 1,
        effect_id: "document-symbols.replace-or-clear",
    },
    // Parser diagnostics outbound publication (#12031 moved the didChange/
    // didOpen stream boundary into the sink-local commit_push_diagnostics).
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/diagnostics.rs",
        needle: "textDocument/publishDiagnostics",
        expected_count: 3,
        effect_id: "diagnostics.parser-outbound-publication",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/diagnostics_sink.rs",
        needle: "textDocument/publishDiagnostics",
        expected_count: 1,
        effect_id: "diagnostics.parser-outbound-publication",
    },
    // didOpen guard admissions. Their direct publishDiagnostics sites moved
    // into diagnostics_sink with the #12031 stream boundary; the entry above
    // now carries that surface for both outbound rows.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: "minimal_state(text, version)",
        expected_count: 3,
        effect_id: "didopen-guard.minimal-document-admission",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: "minimal_state_from_rope(",
        expected_count: 3,
        effect_id: "didopen-guard.minimal-document-admission",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: ".replace_text_state(",
        expected_count: 2,
        effect_id: "didopen-guard.minimal-document-admission",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: "documents.insert(normalized_uri.clone(), doc_state)",
        expected_count: 5,
        effect_id: "didopen-guard.minimal-document-admission",
    },
    // Accepted-snapshot publication (normal didOpen route).
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: ".publish_parsed_if_current(",
        expected_count: 2,
        effect_id: "parser-state.accepted-snapshot-publication",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: "self.documents.lock().insert(normalized_uri.clone(), doc_state)",
        expected_count: 1,
        effect_id: "parser-state.accepted-snapshot-publication",
    },
    // Workspace-index live contribution replacement. Needles are split
    // through concat! so these ratchet patterns are not mistaken by the
    // #11301 text-level caller scan for actual index_live_file call sites.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: concat!(".index_live", "_file("),
        expected_count: 2,
        effect_id: "workspace-index.live-contribution-replacement",
    },
    // Save-reconciliation live commit route. The needle is split through
    // concat! so this ratchet pattern is not mistaken by the #11301
    // text-level caller scan for an actual index_live_file call site.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync/lifecycle.rs",
        needle: concat!(".index_live", "_file("),
        expected_count: 1,
        effect_id: "workspace-index.live-contribution-replacement",
    },
    // Readiness lifecycle + open-ready publication.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: ".notify_parse_complete(",
        expected_count: 6,
        effect_id: "readiness.active-document-parse-lifecycle",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: ".notify_change(",
        expected_count: 2,
        effect_id: "readiness.active-document-parse-lifecycle",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/readiness.rs",
        needle: "::send_active_document_ready_notification(",
        expected_count: 1,
        effect_id: "readiness.open-ready-publication",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync.rs",
        needle: ".transition_to_ready(",
        expected_count: 1,
        effect_id: "readiness.open-ready-publication",
    },
    // Result-ID cache mutation sites (#6729).
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/language/semantic_tokens.rs",
        needle: "let mut cache = self.semantic_tokens_cache.lock();",
        expected_count: 1,
        effect_id: "result-id.local-state",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/mod.rs",
        needle: "let mut cache = self.semantic_tokens_cache.lock();",
        expected_count: 1,
        effect_id: "result-id.local-state",
    },
    // Async parse-worker accepted-snapshot publication route.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/parse_worker.rs",
        needle: ".publish_parsed_if_current(",
        expected_count: 1,
        effect_id: "parser-state.accepted-snapshot-publication",
    },
    // Workspace-task Coordinator lifecycle routes (async didOpen/scan paths).
    // #13183 deleted the bespoke `handle_did_create_files` indexing arm and
    // routed explicit creates through `process_file_watcher_uri_immediate`,
    // which carries its own notify_change/notify_parse_complete pair. That
    // removed exactly one pair from this file (7 -> 6 and 6 -> 5); the
    // lifecycle invariant is unchanged, since the surviving shared path still
    // decrements exactly once on every exit.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/workspace.rs",
        needle: ".notify_parse_complete(",
        expected_count: 6,
        effect_id: "readiness.active-document-parse-lifecycle",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/workspace.rs",
        needle: ".notify_change(",
        expected_count: 5,
        effect_id: "readiness.active-document-parse-lifecycle",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/workspace.rs",
        needle: ".transition_to_ready(",
        expected_count: 1,
        effect_id: "readiness.open-ready-publication",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/workspace.rs",
        needle: "textDocument/publishDiagnostics",
        expected_count: 1,
        effect_id: "diagnostics.parser-outbound-publication",
    },
    // Coordinator module-level lifecycle routes. Comment-stripped counting:
    // only production statements are load-bearing here.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/mod.rs",
        needle: ".notify_parse_complete(",
        expected_count: 1,
        effect_id: "readiness.active-document-parse-lifecycle",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/mod.rs",
        needle: ".notify_change(",
        expected_count: 1,
        effect_id: "readiness.active-document-parse-lifecycle",
    },
    // didClose diagnostics-clear publication route.
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync/lifecycle.rs",
        needle: ".notify_change(",
        expected_count: 1,
        effect_id: "readiness.active-document-parse-lifecycle",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync/lifecycle.rs",
        needle: ".notify_parse_complete(",
        expected_count: 1,
        effect_id: "readiness.active-document-parse-lifecycle",
    },
    CallSiteLedgerEntry {
        file: "crates/perl-lsp-rs/src/runtime/text_sync/lifecycle.rs",
        needle: "textDocument/publishDiagnostics",
        expected_count: 1,
        effect_id: "diagnostics.parser-outbound-publication",
    },
];

#[cfg(test)]
fn repo_source_path(file: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join(file)
}

#[cfg(test)]
fn count_occurrences(file: &str, needle: &str) -> Option<usize> {
    let content = std::fs::read_to_string(repo_source_path(file)).ok()?;
    // Line comments are documentation, not mutation sites; counting them
    // would make prose edits trip ratchets and let doc-only mentions pose as
    // coverage. Block comments are not stripped: no ledger needle appears
    // inside one today.
    let mut code = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }
        code.push_str(line);
        code.push('\n');
    }
    Some(code.matches(needle).count())
}

// ---------------------------------------------------------------------------
// Deterministic human projection
// ---------------------------------------------------------------------------

/// Render the deterministic human-readable projection of the inventory.
///
/// Pure function of the static inventory: identical input always produces
/// byte-identical output (second-run clean).
#[allow(dead_code)]
pub(crate) fn render_inventory_projection() -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# parse_effect_sinks.v1");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Checked static inventory generated from \
         `crates/perl-lsp-rs/src/runtime/parse_effect_contract.rs` (#11672)."
    );
    let _ = writeln!(out);
    for row in parse_effect_sinks_v1() {
        let _ = writeln!(out, "## `{}`", row.effect_id);
        let _ = writeln!(out);
        let _ = writeln!(out, "- title: {}", row.title);
        let _ = writeln!(out, "- owner: {}", row.owner_issue);
        let _ = writeln!(
            out,
            "- ticket inputs: {}",
            row.ticket_inputs
                .iter()
                .map(|field| match field {
                    TicketFieldRequirementV1::DocumentInstanceIdentity => {
                        "document_instance".to_string()
                    }
                    TicketFieldRequirementV1::GenerationNumber => "generation".to_string(),
                    TicketFieldRequirementV1::ClientUriShape => "client_uri".to_string(),
                    TicketFieldRequirementV1::NormalizedUriKey => "normalized_uri".to_string(),
                    TicketFieldRequirementV1::PublishedSnapshot => "snapshot".to_string(),
                    TicketFieldRequirementV1::CapturedSourceText => "captured_text".to_string(),
                    TicketFieldRequirementV1::SettleOwnershipFlag => {
                        "settle_ownership".to_string()
                    }
                    TicketFieldRequirementV1::NotRequired(reason) => {
                        format!("none ({reason})")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(out, "- sink-local subject: {}", row.sink_local_subject);
        let _ = writeln!(out, "- store: {}", row.store.name());
        let _ = writeln!(
            out,
            "- owns mutation sites: {}",
            if row.owns_mutation_sites { "yes" } else { "no" }
        );
        let _ = writeln!(out, "- mutation boundary: {}", row.mutation_boundary);
        let _ = writeln!(
            out,
            "- currentness comparison: {}",
            match row.currentness_comparison {
                CurrentnessComparisonV1::HelperPrecheckThenCallback =>
                    "helper precheck then callback (residual window admitted)",
                CurrentnessComparisonV1::SinkLocalCompareAndMutateTarget =>
                    "sink-local compare-and-mutate (target law)",
                CurrentnessComparisonV1::SameThreadAdmissionPreAcceptance =>
                    "same-thread admission before acceptance exists",
                CurrentnessComparisonV1::ExternalOwnedCurrentness => {
                    "external owner"
                }
                CurrentnessComparisonV1::NotApplicable => "not applicable",
            }
        );
        let _ = writeln!(out, "- terminal/clear policy:");
        for (class, action) in row.terminal_policy.rows() {
            let class_name = match class {
                TerminalParseClassV1::Clean => "clean",
                TerminalParseClassV1::RecoveredPartial => "recovered_partial",
                TerminalParseClassV1::BudgetExhausted => "budget_exhausted",
                TerminalParseClassV1::Cancelled => "cancelled",
                TerminalParseClassV1::CatastrophicMinimal => "catastrophic_minimal",
                TerminalParseClassV1::GuardedNoParserState => "guarded_no_parser_state",
                TerminalParseClassV1::Desynchronized => "desynchronized",
                TerminalParseClassV1::InstrumentFailure => "instrument_failure",
            };
            let action_name = match action {
                SinkCurrentActionV1::Replace => "replace",
                SinkCurrentActionV1::Clear => "clear",
                SinkCurrentActionV1::IdentityNoOp => "identity_noop",
                SinkCurrentActionV1::Publish => "publish",
                SinkCurrentActionV1::DelegateToOwningSink => "delegate_to_owning_sink",
                SinkCurrentActionV1::Observe => "observe",
                SinkCurrentActionV1::OutOfScope => "out_of_scope",
            };
            let _ = writeln!(out, "  - {class_name}: {action_name}");
        }
        let _ = writeln!(out, "- focused proof filter: {}", row.focused_proof_filter);
        let _ = writeln!(out, "- composed proof owner: {}", row.composed_proof_owner);
        let _ = writeln!(
            out,
            "- compatibility adapter: {}",
            row.compatibility_adapter
                .map(|exit| format!("{} (exit: {})", exit.adapter, exit.exit_owner_issue))
                .unwrap_or_else(|| "none".to_string())
        );
        let _ = writeln!(out, "- disposition: {}", row.disposition.render());
        let _ = writeln!(out, "- claim ceiling: {}", row.claim_ceiling);
        let _ = writeln!(out);
    }
    // Single trailing newline: a blank line at EOF trips `git diff --check`
    // in the Repository Contract advisory.
    while out.ends_with('\n') {
        out.pop();
    }
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Deterministic checks (focused proof: filter `parse_effect_sink`)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod parse_effect_sink_contract_tests {

    use super::*;

    fn inventory() -> &'static [ParseEffectSinkRowV1] {
        parse_effect_sinks_v1()
    }

    /// Falsifier 9/10: stable unique sink/effect IDs; no two rows claim the
    /// same irreversible mutation authority. The exact expected ID set pins
    /// the full V1 denominator: removing or renaming a row fails here, not
    /// just shrinking the count.
    #[test]
    fn parse_effect_sink_ids_unique_and_stable_format() {
        let rows = inventory();
        let expected_ids: std::collections::BTreeSet<&str> = [
            "diagnostics.parser-outbound-publication",
            "diagnostics.didopen-guard-admission-publication",
            "document-symbols.replace-or-clear",
            "workspace-index.live-contribution-replacement",
            "workspace-index.reader-capture-projection",
            "semantic-project.contribution-publication",
            "result-id.local-state",
            "semantic-tokens.current-result-publication",
            "parser-state.accepted-snapshot-publication",
            "didopen-guard.minimal-document-admission",
            "readiness.active-document-parse-lifecycle",
            "readiness.open-ready-publication",
            "evidence.parse-effect-observations",
            "compat.legacy-generic-callback-helper",
        ]
        .into_iter()
        .collect();
        let actual_ids: std::collections::BTreeSet<&str> =
            rows.iter().map(|row| row.effect_id).collect();
        assert_eq!(
            actual_ids, expected_ids,
            "inventory must exactly cover the required V1 effect denominator"
        );
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            assert!(!row.effect_id.is_empty(), "empty effect id");
            assert!(
                row.effect_id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '-' || c.is_ascii_digit()),
                "effect id `{}` must be lowercase dotted-stable",
                row.effect_id
            );
            assert!(seen.insert(row.effect_id), "duplicate effect id `{}`", row.effect_id);
        }
    }

    /// Every required row names one exact owner and exactly one disposition.
    #[test]
    fn parse_effect_sink_rows_have_exactly_one_owner_and_disposition() {
        for row in inventory() {
            assert!(
                row.owner_issue.starts_with('#'),
                "row `{}` owner `{}` must name an issue/component",
                row.effect_id,
                row.owner_issue
            );
            // Disposition is a closed enum by construction; render must be
            // non-empty and single-classified.
            let rendered = row.disposition.render();
            assert!(!rendered.is_empty(), "row `{}` has empty disposition", row.effect_id);
            let classifications = [
                rendered.contains("new focused child"),
                rendered.contains("existing exact owner"),
                rendered.contains("compatibility projection with exit"),
                rendered.contains("not parse-derived"),
                rendered.contains("not applicable"),
                rendered.contains("retire/unreachable"),
                rendered.contains("not proven"),
            ];
            assert_eq!(
                classifications.iter().filter(|flag| **flag).count(),
                1,
                "row `{}` disposition `{rendered}` must classify exactly once",
                row.effect_id
            );
            assert!(!row.claim_ceiling.is_empty(), "row `{}` missing claim ceiling", row.effect_id);
        }
    }

    /// Falsifier 10: no duplicate mutation authority -- each store's
    /// registered mutation sites are owned by exactly one row.
    #[test]
    fn parse_effect_sink_no_duplicate_mutation_authority() {
        let mut site_owners: Vec<(&'static str, &str)> = Vec::new();
        for entry in CALL_SITE_LEDGER {
            assert!(
                inventory()
                    .iter()
                    .find(|row| row.effect_id == entry.effect_id)
                    .is_some_and(|row| row.owns_mutation_sites),
                "ledger entry {}:{} maps to `{}` which is not a mutating row",
                entry.file,
                entry.needle,
                entry.effect_id
            );
            site_owners.push((entry.file, entry.effect_id));
        }
        // Two different mutating rows may share a file but never the same
        // registered needle; needle-level authority is unique by map key.
        for (idx, entry) in CALL_SITE_LEDGER.iter().enumerate() {
            for other in CALL_SITE_LEDGER.iter().skip(idx + 1) {
                assert!(
                    entry.file != other.file || entry.needle != other.needle,
                    "duplicate ledger needle {}:{}",
                    entry.file,
                    entry.needle
                );
            }
        }
        assert!(!site_owners.is_empty(), "ledger must not be empty");
    }

    /// Every accepted-ticket-consuming row declares its ticket fields; every
    /// deferred post-parse row requires instance identity + generation.
    #[test]
    fn parse_effect_sink_ticket_fields_declared_for_governed_rows() {
        for row in inventory() {
            assert!(
                !row.ticket_inputs.is_empty(),
                "row `{}` must declare ticket inputs (or explicit NotRequired)",
                row.effect_id
            );
            let helper_routed = matches!(
                row.currentness_comparison,
                CurrentnessComparisonV1::HelperPrecheckThenCallback
            );
            if helper_routed {
                assert!(
                    row.ticket_inputs.contains(&TicketFieldRequirementV1::DocumentInstanceIdentity)
                        && row.ticket_inputs.contains(&TicketFieldRequirementV1::GenerationNumber),
                    "helper-routed row `{}` must declare instance+generation inputs",
                    row.effect_id
                );
            }
        }
    }

    /// Falsifier 6/7: every terminal class has an explicit sink policy and a
    /// current success/failure terminal can never silently keep stale state.
    #[test]
    fn parse_effect_sink_terminal_policy_total_per_class() {
        for row in inventory() {
            let mut covered = std::collections::BTreeSet::new();
            for (class, _action) in row.terminal_policy.rows() {
                assert!(
                    covered.insert(class),
                    "row `{}` covers a terminal class twice",
                    row.effect_id
                );
            }
            for class in TERMINAL_PARSE_CLASSES_V1 {
                let _action = row.terminal_policy.action(class); // total by construction
                assert!(covered.contains(&class), "row `{}` missing {class:?}", row.effect_id);
            }
            if row.owns_mutation_sites {
                let clean = row.terminal_policy.action(TerminalParseClassV1::Clean);
                assert_ne!(
                    clean,
                    SinkCurrentActionV1::OutOfScope,
                    "mutating row `{}` must act on current clean results",
                    row.effect_id
                );
                // Falsifier 7: content sinks can never keep stale exact
                // state on a current result.
                if matches!(
                    row.store,
                    SinkStoreV1::SYMBOL_INDEX | SinkStoreV1::OUTBOUND_PUBLISH_DIAGNOSTICS
                ) {
                    assert!(
                        matches!(
                            clean,
                            SinkCurrentActionV1::Replace
                                | SinkCurrentActionV1::Clear
                                | SinkCurrentActionV1::Publish
                        ),
                        "content row `{}` must replace/clear/publish on current clean results",
                        row.effect_id
                    );
                }
            }
        }
    }

    /// Every compatibility adapter names an exit owner and exists in source,
    /// and exactly the V1 rows that still route through the legacy helper
    /// declare one.
    #[test]
    fn parse_effect_sink_compat_adapters_have_exit_owner_in_source() {
        let expected_adapter_rows: std::collections::BTreeSet<&str> = [
            "diagnostics.parser-outbound-publication",
            "document-symbols.replace-or-clear",
            "workspace-index.live-contribution-replacement",
            "readiness.active-document-parse-lifecycle",
            "readiness.open-ready-publication",
            "compat.legacy-generic-callback-helper",
        ]
        .into_iter()
        .collect();
        let mut adapter_rows: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for row in inventory() {
            if let Some(exit) = row.compatibility_adapter {
                assert!(
                    adapter_rows.insert(row.effect_id),
                    "duplicate adapter on `{}`",
                    row.effect_id
                );
                assert!(
                    exit.exit_owner_issue.starts_with('#'),
                    "row `{}` adapter exit owner must be an issue",
                    row.effect_id
                );
                let occurrences =
                    count_occurrences("crates/perl-lsp-rs/src/runtime/text_sync.rs", exit.adapter);
                assert!(
                    occurrences.is_some_and(|count| count > 0),
                    "adapter `{}` of row `{}` not found in source",
                    exit.adapter,
                    row.effect_id
                );
            }
        }
        assert_eq!(
            adapter_rows, expected_adapter_rows,
            "compatibility-adapter denominator drifted from the V1 legacy-helper routes"
        );
    }

    /// Falsifier 3/4/5: externally owned authorities are referenced, never
    /// reimplemented locally -- external rows may register the local
    /// invocation sites of that authority, but their boundary must be marked
    /// external (the mutation law itself lives with the cited owner).
    #[test]
    fn parse_effect_sink_external_owners_not_reimplemented_locally() {
        for row in inventory() {
            let external = matches!(
                row.currentness_comparison,
                CurrentnessComparisonV1::ExternalOwnedCurrentness
            );
            if external {
                assert!(
                    row.mutation_boundary.starts_with("external:")
                        || row.mutation_boundary.starts_with("none"),
                    "row `{}` external boundary must be marked external:/none",
                    row.effect_id
                );
                if row.owns_mutation_sites {
                    assert!(
                        row.mutation_boundary.contains("authority"),
                        "row `{}` owning external-authority call sites must cite the authority",
                        row.effect_id
                    );
                }
            }
        }
    }

    /// Falsifier 9 (structural): every registered production mutation call
    /// site still exists with the registered count, maps to an inventory row,
    /// and every mutating row owns at least one registered site. Adding an
    /// unregistered effect call site changes a count and fails here.
    #[test]
    fn parse_effect_sink_call_site_ledger_matches_source() {
        for entry in CALL_SITE_LEDGER {
            assert!(
                inventory().iter().any(|row| row.effect_id == entry.effect_id),
                "ledger entry {}:{} references unknown row `{}`",
                entry.file,
                entry.needle,
                entry.effect_id
            );
            let actual = count_occurrences(entry.file, entry.needle);
            assert!(actual.is_some(), "ledger could not read {}", entry.file);
            assert_eq!(
                actual,
                Some(entry.expected_count),
                "call-site ratchet drifted for {}:{:?} -- re-register the site against its \
                 parse_effect_sinks_v1 row or fix the regression",
                entry.file,
                entry.needle
            );
        }
        for row in inventory().iter().filter(|row| row.owns_mutation_sites) {
            assert!(
                CALL_SITE_LEDGER.iter().any(|entry| entry.effect_id == row.effect_id),
                "mutating row `{}` owns no registered production call site",
                row.effect_id
            );
        }
    }

    /// Files whose occurrences of a registered needle are not governed
    /// mutation sites. Each pair was individually verified against source when
    /// this ledger was completed, and each carries the exact expected
    /// occurrence count (post comment-stripping) so an exempted file cannot
    /// silently gain a new production call site: any count drift fails the
    /// sweep and forces reclassification (#12085 review round).
    #[cfg(test)]
    const NEEDLE_SWEEP_EXEMPTIONS: &[(&str, &str, usize)] = &[
        // Doc-comment mentions of the method name (module documentation).
        ("crates/perl-lsp-rs/src/runtime/lifecycle/mod.rs", "textDocument/publishDiagnostics", 0),
        // Doc-comment mention describing non-gated per-file publication.
        ("crates/perl-lsp-rs/src/runtime/mod.rs", "textDocument/publishDiagnostics", 0),
        // Occurrences inside the #[cfg(test)] module (RecordingSink assertions).
        ("crates/perl-lsp-rs/src/runtime/outbound.rs", "textDocument/publishDiagnostics", 2),
        // String literal asserted on by file_preflight's own self-scan test.
        (
            "crates/perl-lsp-rs/src/runtime/dispatch/workspace/file_preflight.rs",
            ".notify_parse_complete(",
            2,
        ),
        // pub(super) definition site; invocation sites are registered instead.
        (
            "crates/perl-lsp-rs/src/runtime/text_sync/document_state.rs",
            "minimal_state_from_rope(",
            1,
        ),
        // Test-module helper invocations of the readiness transition.
        ("crates/perl-lsp-rs/src/runtime/routing.rs", ".transition_to_ready(", 2),
        ("crates/perl-lsp-rs/src/runtime/readiness.rs", ".transition_to_ready(", 3),
        ("crates/perl-lsp-rs/src/runtime/language/completion.rs", ".transition_to_ready(", 5),
        ("crates/perl-lsp-rs/src/runtime/language/rename.rs", ".transition_to_ready(", 4),
        ("crates/perl-lsp-rs/src/runtime/language/misc.rs", ".transition_to_ready(", 1),
        (
            "crates/perl-lsp-rs/src/runtime/language/hover/signature_help.rs",
            ".transition_to_ready(",
            3,
        ),
        // Test-module Coordinator notification helper.
        ("crates/perl-lsp-rs/src/runtime/routing.rs", ".notify_change(", 1),
    ];

    #[cfg(test)]
    fn is_test_source_file(path: &std::path::Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        name == "test_api.rs" || name == "tests.rs" || name.ends_with("_tests.rs")
    }

    #[cfg(test)]
    fn collect_runtime_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        let mut sorted: Vec<_> = entries.flatten().collect();
        sorted.sort_by_key(std::fs::DirEntry::path);
        for entry in sorted {
            let path = entry.path();
            if path.is_dir() {
                collect_runtime_rs_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// Completeness half of the ledger claim: every non-test runtime source
    /// file that contains a registered needle must either register its own
    /// (file, needle) ratchet or appear verbatim in the reasoned exemption
    /// table. Together with `parse_effect_sink_call_site_ledger_matches_source`
    /// this makes "a registered needle exists in production source but is not
    /// inventoried" fail deterministically.
    #[test]
    fn parse_effect_sink_call_site_ledger_covers_registered_needles() {
        let runtime_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("runtime");
        let mut files = Vec::new();
        collect_runtime_rs_files(&runtime_root, &mut files);
        assert!(
            files.len() > 20,
            "runtime source walk found only {} files; walk is broken",
            files.len()
        );
        let contract_file = "crates/perl-lsp-rs/src/runtime/parse_effect_contract.rs";
        let mut needles: Vec<&str> = CALL_SITE_LEDGER.iter().map(|entry| entry.needle).collect();
        needles.sort_unstable();
        needles.dedup();
        for file in &files {
            let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
            let repo_rel = match file.strip_prefix(&repo_root) {
                Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if repo_rel == contract_file || is_test_source_file(file) {
                continue;
            }
            let content = crate::must_with(
                std::fs::read_to_string(file),
                "ledger sweep must be able to read every walked runtime source file",
            );
            // Same comment-stripping contract as count_occurrences: prose
            // mentions are not call sites.
            let mut code = String::with_capacity(content.len());
            for line in content.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                code.push_str(line);
                code.push('\n');
            }
            for needle in &needles {
                let occurrences = code.matches(needle).count();
                if occurrences == 0 {
                    continue;
                }
                let registered = CALL_SITE_LEDGER
                    .iter()
                    .any(|entry| entry.file == repo_rel && entry.needle == *needle);
                let exempt_entry =
                    NEEDLE_SWEEP_EXEMPTIONS.iter().find(|(exempt_file, exempt_needle, _)| {
                        exempt_file == &repo_rel && exempt_needle == needle
                    });
                if let Some((_, _, expected)) = exempt_entry {
                    // Counted exemption: the file may keep its documented
                    // non-mutation occurrences, but gaining or losing one
                    // must fail here so the pair is reclassified.
                    assert_eq!(
                        occurrences, *expected,
                        "exempted occurrence count drifted for {needle:?} in {repo_rel} -- \
                         reclassify the file (register production sites or update the \
                         reasoned exemption)"
                    );
                    continue;
                }
                assert!(
                    registered,
                    "unregistered production occurrence of needle {:?} in {} -- \
                     register it against its parse_effect_sinks_v1 row or add a \
                     reasoned exemption",
                    needle, repo_rel
                );
            }
        }
        for (exempt_file, exempt_needle, _) in NEEDLE_SWEEP_EXEMPTIONS {
            assert!(
                needles.contains(exempt_needle),
                "stale exemption for {exempt_file}:{exempt_needle} -- needle no longer registered"
            );
        }
    }

    /// Generated projection is deterministic (second-run clean) and matches
    /// the committed golden packet.
    #[test]
    fn parse_effect_sink_projection_second_run_clean() {
        let first = render_inventory_projection();
        let second = render_inventory_projection();
        assert_eq!(first, second, "projection must be second-run clean");

        let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".spec")
            .join("11672-parse-effect-sink-contract")
            .join("inventory.md");
        assert!(
            golden_path.exists(),
            "committed projection missing at {} -- regenerate with PERL_EFFECT_SINK_DUMP=1",
            golden_path.display()
        );
        let golden =
            crate::must_with(std::fs::read_to_string(&golden_path), "read committed inventory.md")
                .replace("\r\n", "\n");
        assert_eq!(
            first, golden,
            "committed .spec inventory.md drifted from the checked inventory"
        );
    }

    /// Regenerate the committed projection with:
    /// `PERL_EFFECT_SINK_DUMP=1 cargo test -p perl-lsp-rs \
    ///  parse_effect_sink_projection_dump -- --nocapture`
    ///
    /// Writes `.spec/11672-parse-effect-sink-contract/inventory.md` from the
    /// checked inventory; the second-run test above fails if that committed
    /// file drifts.
    #[test]
    fn parse_effect_sink_projection_dump() {
        if std::env::var_os("PERL_EFFECT_SINK_DUMP").is_some() {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join(".spec")
                .join("11672-parse-effect-sink-contract")
                .join("inventory.md");
            crate::must_with(
                std::fs::create_dir_all(crate::must_some_with(
                    path.parent(),
                    "parent of inventory.md",
                )),
                "create spec dir",
            );
            crate::must_with(
                std::fs::write(&path, render_inventory_projection()),
                "write inventory.md",
            );
        }
    }

    /// The closed V1 outcome catalog. Adding a variant to
    /// [`ParseEffectCommitOutcomeV1`] breaks compilation of
    /// `assert_outcome_catalog_is_exhaustive` below until it is added here and
    /// classified, so the vocabulary cannot grow without entering this proof.
    #[cfg(test)]
    const OUTCOME_VARIANTS_V1: &[ParseEffectCommitOutcomeV1] = &[
        ParseEffectCommitOutcomeV1::CommittedCurrent,
        ParseEffectCommitOutcomeV1::RejectedStaleTicket,
        ParseEffectCommitOutcomeV1::RejectedWrongDocumentInstance,
        ParseEffectCommitOutcomeV1::RejectedSourceProjectionOrConfiguration,
        ParseEffectCommitOutcomeV1::RejectedSinkGenerationAdvanced,
        ParseEffectCommitOutcomeV1::RejectedLifecycleState,
        ParseEffectCommitOutcomeV1::SupersededBeforeMutation,
        ParseEffectCommitOutcomeV1::NoEffectRequired,
        ParseEffectCommitOutcomeV1::SafeClearCommitted,
        ParseEffectCommitOutcomeV1::SinkUnavailable,
        ParseEffectCommitOutcomeV1::ProductFailure,
        ParseEffectCommitOutcomeV1::InstrumentOrSchemaFailure,
        ParseEffectCommitOutcomeV1::NotProven,
    ];

    /// Compile-time forcing function: the match has no wildcard arm, so a new
    /// enum variant fails to compile here before any test can run.
    #[cfg(test)]
    fn assert_outcome_catalog_is_exhaustive(outcome: ParseEffectCommitOutcomeV1) {
        match outcome {
            ParseEffectCommitOutcomeV1::CommittedCurrent => {}
            ParseEffectCommitOutcomeV1::RejectedStaleTicket => {}
            ParseEffectCommitOutcomeV1::RejectedWrongDocumentInstance => {}
            ParseEffectCommitOutcomeV1::RejectedSourceProjectionOrConfiguration => {}
            ParseEffectCommitOutcomeV1::RejectedSinkGenerationAdvanced => {}
            ParseEffectCommitOutcomeV1::RejectedLifecycleState => {}
            ParseEffectCommitOutcomeV1::SupersededBeforeMutation => {}
            ParseEffectCommitOutcomeV1::NoEffectRequired => {}
            ParseEffectCommitOutcomeV1::SafeClearCommitted => {}
            ParseEffectCommitOutcomeV1::SinkUnavailable => {}
            ParseEffectCommitOutcomeV1::ProductFailure => {}
            ParseEffectCommitOutcomeV1::InstrumentOrSchemaFailure => {}
            ParseEffectCommitOutcomeV1::NotProven => {}
        }
    }

    /// The outcome vocabulary stays closed and fully classified: every
    /// variant lands in exactly one evidence class, stale/superseded tickets
    /// are typed non-applications (not silent success), absent evidence is
    /// `NotProven`, and instrument/schema failure stays a distinct typed
    /// outcome that is never conflated with either.
    #[test]
    fn parse_effect_sink_outcome_vocabulary_closed_partition() {
        assert_eq!(
            OUTCOME_VARIANTS_V1.len(),
            13,
            "the V1 outcome vocabulary is closed at 13 variants; growing it is a \
             contract change that must update this catalog and its classification"
        );
        let mut seen = Vec::new();
        for outcome in OUTCOME_VARIANTS_V1.iter().copied() {
            assert_outcome_catalog_is_exhaustive(outcome);
            assert!(!seen.contains(&outcome), "{outcome:?} listed twice in catalog");
            seen.push(outcome);
            let classes = [
                outcome.is_committed(),
                outcome.is_non_application(),
                matches!(
                    outcome,
                    ParseEffectCommitOutcomeV1::SinkUnavailable
                        | ParseEffectCommitOutcomeV1::ProductFailure
                        | ParseEffectCommitOutcomeV1::InstrumentOrSchemaFailure
                        | ParseEffectCommitOutcomeV1::NotProven
                ),
            ];
            assert_eq!(
                classes.iter().filter(|flag| **flag).count(),
                1,
                "{outcome:?} must classify into exactly one evidence class"
            );
        }
        assert!(ParseEffectCommitOutcomeV1::SupersededBeforeMutation.is_non_application());
        assert!(ParseEffectCommitOutcomeV1::RejectedStaleTicket.is_non_application());
        assert!(!ParseEffectCommitOutcomeV1::NoEffectRequired.is_committed());
        assert!(
            !ParseEffectCommitOutcomeV1::NotProven.is_committed(),
            "unproven currentness must never count as a commit"
        );
        assert!(
            !ParseEffectCommitOutcomeV1::InstrumentOrSchemaFailure.is_non_application(),
            "instrument failure must stay distinguishable from honest non-application"
        );
    }
}
