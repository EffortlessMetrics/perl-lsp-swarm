//! Bounded observable-predicate waits for the native Neovim built-in-LSP
//! action contract (#11409).
//!
//! Deterministic wait law: every asynchronous action waits on a bounded
//! **observable predicate** tied to the exact subject/currentness identity. A
//! short polling interval may implement a bounded predicate, but elapsed time
//! alone can never become semantic success: a `Satisfied` predicate names the
//! exact state that settled it (as a bounded digest) plus the poll count and
//! bounded elapsed time inside the declared budget; a `TimedOut` predicate is
//! lawful typed evidence that forces `not_proven`; a `Substituted` predicate
//! (fixed sleep, global workspace idle, any-result satisfaction, log text,
//! server response where buffer/UI state is claimed, or a raw companion
//! request relabeled ordinary) is always a hard validation failure, never a
//! pass.

use serde::{Deserialize, Serialize};

use crate::client_compat_fixture::is_reason_token;

/// Tracked currentness dimensions (same spelling family as the Vim driver
/// model; kept local so this contract does not reach into another family's
/// internals). Every observation carries one snapshot and every observed
/// effect binds the snapshot it was computed against, so an old-generation
/// result can never satisfy a post-edit action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationDimension {
    Host,
    Process,
    Document,
    Root,
    Source,
    Config,
}

pub const GENERATION_DIMENSIONS: &[GenerationDimension] = &[
    GenerationDimension::Host,
    GenerationDimension::Process,
    GenerationDimension::Document,
    GenerationDimension::Root,
    GenerationDimension::Source,
    GenerationDimension::Config,
];

/// One currentness snapshot: the Neovim host instance, perllsp process
/// generation, document sync generation, workspace-root generation,
/// source-tree generation, and configuration generation an observation or
/// effect settled against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationSnapshot {
    pub host_generation: u32,
    pub process_generation: u32,
    pub document_generation: u32,
    pub root_generation: u32,
    pub source_generation: u32,
    pub config_generation: u32,
}

impl GenerationSnapshot {
    /// The zero snapshot used by the fake backend before any state settles.
    pub fn zeroed() -> Self {
        Self {
            host_generation: 0,
            process_generation: 0,
            document_generation: 0,
            root_generation: 0,
            source_generation: 0,
            config_generation: 0,
        }
    }

    /// Read one dimension. Unknown dimensions are a caller bug, so this
    /// returns the message rather than panicking.
    pub fn dimension(&self, dimension: GenerationDimension) -> u32 {
        match dimension {
            GenerationDimension::Host => self.host_generation,
            GenerationDimension::Process => self.process_generation,
            GenerationDimension::Document => self.document_generation,
            GenerationDimension::Root => self.root_generation,
            GenerationDimension::Source => self.source_generation,
            GenerationDimension::Config => self.config_generation,
        }
    }
}

/// The observable-predicate vocabulary. The first six kinds are the #11409
/// examples verbatim; `DocumentGenerationAccepted` carries the text-sync
/// currentness the issue's synchronization section requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateKind {
    /// The exact built-in-LSP client initialized against the selected
    /// perllsp process identity.
    ClientInitializedExactProcess,
    /// A specific diagnostic code/range at the current document generation.
    DiagnosticStateCurrent,
    /// A specific expected completion result (exact items, not any items).
    CompletionResultExact,
    /// A specific expected hover result.
    HoverResultExact,
    /// A specific expected definition/navigation target.
    NavigationResultExact,
    /// A specific applied buffer/source digest.
    AppliedBufferDigest,
    /// A specific applied file digest (applied edits, returned results).
    AppliedFileDigest,
    /// A specific parser/effect ticket, or a typed not-ready result where
    /// instrumented.
    ParserEffectTicket,
    /// A specific client/process terminal state.
    ClientTerminalState,
    /// The exact document generation was accepted by the server (didChange
    /// currentness, not global workspace idle).
    DocumentGenerationAccepted,
}

/// One predicate requirement: the named state plus the bounded wait budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PredicateRequirement {
    pub kind: PredicateKind,
    pub max_wait_ms: u64,
}

/// The known false-substitution shapes of #11409's falsifier list. Each is
/// load-bearing negative-control evidence: a driver that offers one of these
/// as readiness has manufactured the observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstitutionKind {
    /// A fixed sleep was offered instead of the required state.
    FixedSleep,
    /// Global workspace idle was offered instead of document-specific
    /// currentness.
    GlobalWorkspaceIdle,
    /// Any diagnostic/completion/hover result was offered where an exact
    /// expectation is claimed.
    AnyResultSatisfies,
    /// Method text in a log was offered instead of an actual
    /// request/consumption/application.
    LogTextOnly,
    /// A server response was offered where Neovim buffer/UI state is
    /// claimed.
    ServerResponseOnly,
    /// A raw companion request was offered as ordinary Neovim traffic.
    RawCompanionAsOrdinary,
}

/// Typed evidence for one predicate wait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "evidence", rename_all = "snake_case", deny_unknown_fields)]
pub enum PredicateEvidence {
    /// The named state was observed. The settling state is named by a
    /// bounded digest (elapsed time alone cannot be satisfaction), with the
    /// poll count, the bounded elapsed time, and the generation snapshot the
    /// predicate settled against.
    Satisfied {
        kind: PredicateKind,
        settled_state_digest: String,
        settled_generations: GenerationSnapshot,
        polls: u32,
        waited_ms: u64,
    },
    /// The bounded wait expired after at least one poll. Lawful typed
    /// evidence, but the owning observation must classify `not_proven`.
    TimedOut { kind: PredicateKind, polls: u32, waited_ms: u64 },
    /// A known substitution was offered for the required state. Always a
    /// hard validation failure: the observation is dishonest, not late.
    Substituted { kind: PredicateKind, substitution: SubstitutionKind },
}

impl PredicateEvidence {
    /// The predicate kind this evidence speaks for.
    pub fn kind(&self) -> PredicateKind {
        match self {
            PredicateEvidence::Satisfied { kind, .. }
            | PredicateEvidence::TimedOut { kind, .. }
            | PredicateEvidence::Substituted { kind, .. } => *kind,
        }
    }
}

/// True when the token is a stable reason token (shared vocabulary rule).
pub fn is_stable_token(value: &str) -> bool {
    is_reason_token(value)
}
