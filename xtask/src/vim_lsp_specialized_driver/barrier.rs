//! Observable-state barriers and generation identities for the specialized
//! Vim/vim-lsp driver (#11380).
//!
//! A barrier replaces a fixed sleep with an explicit wait over named state.
//! Its evidence is typed: either the named state was observed (with the
//! generation snapshot that settled it), the bounded wait expired (typed
//! instrument evidence that must classify `not_proven`), or someone offered a
//! known substitution for the state (fixed sleep, event-log artifact, raw
//! protocol response, bare process existence, server restart, manual format,
//! pre-forced filetype) — which is always a validation failure, never a pass.

use serde::{Deserialize, Serialize};

use crate::client_compat_fixture::is_reason_token;

/// Tracked generation dimensions. Every observation carries one snapshot and
/// every semantic result binds the snapshot it was computed against, so a
/// stale-generation result cannot be accepted as current.
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

/// One generation snapshot: the host instance, perllsp process generation,
/// document sync generation, workspace root generation, source-tree
/// generation, and configuration generation the observation settled against.
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

/// Named state a barrier waits for. The names are the #11380 observable-state
/// barrier vocabulary; adapters implement the wait, the vocabulary owns the
/// meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarrierKind {
    /// Exact server generation initialized (protocol initialize/initialized).
    ServerGenerationInitialized,
    /// Buffer enabled for the exact server generation.
    BufferEnabled,
    /// Expected document/source/config generation accepted by the server.
    DocumentGenerationAccepted,
    /// Exact diagnostic/provider result predicate met.
    DiagnosticPredicateMet,
    /// Save event observed and the selected formatting owner settled.
    SaveEventAndOwnerSettled,
    /// Buffer/file digest reached the expected state.
    DigestReached,
    /// Old/new server process generation disposition observed.
    ProcessGenerationDisposed,
    /// Host instance changed (full host replacement, not a server restart).
    HostInstanceChanged,
    /// Pending request/action settled or invalidated.
    PendingActionSettled,
    /// Process exited and the cleanup ledger settled.
    ProcessExitedCleanupSettled,
    /// vim-lsp service attached to the buffer.
    ServiceAttached,
    /// Native `&filetype` detection settled after open.
    NativeFiletypeDetected,
}

/// One barrier requirement: the named state plus the bounded wait budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarrierRequirement {
    pub kind: BarrierKind,
    pub max_wait_ms: u64,
}

/// The known false-substitution shapes. Each is load-bearing evidence in the
/// #11380 negative controls: a driver that accepts one of these as state has
/// manufactured the observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstitutionKind {
    /// A fixed sleep was offered instead of the required state.
    FixedSleep,
    /// An event/log/registration artifact was offered instead of semantic state.
    EventLogOnly,
    /// A raw LSP protocol response was offered instead of a user-action result.
    RawProtocolResponse,
    /// Bare new-PID process existence was offered instead of an
    /// initialized/replayed server generation.
    ProcessExistenceOnly,
    /// A server restart was offered instead of a full host instance change.
    ServerRestartOnly,
    /// A manual format request was offered instead of a save-triggered format.
    ManualFormatOnly,
    /// The native filetype was pre-forced before the open completed.
    PreForcedFiletype,
}

/// Typed evidence for one barrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "evidence", rename_all = "snake_case")]
pub enum BarrierEvidence {
    /// The named state was observed; carries the snapshot that settled it and
    /// how long the wait took inside the budget.
    Satisfied { kind: BarrierKind, settled_generations: GenerationSnapshot, waited_ms: u64 },
    /// The bounded wait expired. Lawful, but the owning action must classify
    /// `not_proven` with a limitation.
    TimedOut { kind: BarrierKind, waited_ms: u64 },
    /// A known substitution was offered for the required state. Always a hard
    /// validation failure: the observation is dishonest, not late.
    Substituted { kind: BarrierKind, substitution: SubstitutionKind },
}

impl BarrierEvidence {
    /// The barrier kind this evidence speaks for.
    pub fn kind(&self) -> BarrierKind {
        match self {
            BarrierEvidence::Satisfied { kind, .. }
            | BarrierEvidence::TimedOut { kind, .. }
            | BarrierEvidence::Substituted { kind, .. } => *kind,
        }
    }
}

/// Validate one evidence record structurally.
pub fn validate_barrier_evidence(evidence: &BarrierEvidence) -> Result<(), String> {
    match evidence {
        BarrierEvidence::Satisfied { waited_ms, .. } => {
            if *waited_ms == u64::MAX {
                return Err("satisfied barrier evidence carries an unbounded wait".to_string());
            }
            Ok(())
        }
        BarrierEvidence::TimedOut { waited_ms, .. } => {
            if *waited_ms == 0 {
                return Err("timed-out barrier evidence must record the bounded wait".to_string());
            }
            Ok(())
        }
        BarrierEvidence::Substituted { substitution, .. } => Err(format!(
            "barrier substitution offered ({substitution:?}); a substitution is never state"
        )),
    }
}

/// True when the token is a stable reason token (shared vocabulary rule).
pub fn is_stable_token(value: &str) -> bool {
    is_reason_token(value)
}
