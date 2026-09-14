//! Bounded observable predicates for the Helix hosted-session contract
//! (#12832), tracking the #11409 shape. A hosted session has exactly two
//! settle-able waits today: the language-server handshake, observable only in
//! the instrument plane, and the terminal state of the spawned server after
//! host exit.

use serde::{Deserialize, Serialize};

/// The generation dimensions a hosted-session observation can be current
/// against. `host` counts isolated-host launches, `process` counts language-
/// server process lifecycles, and `session` counts settled LSP handshakes of
/// the current run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationDimension {
    Host,
    Process,
    Session,
}

pub const GENERATION_DIMENSIONS: &[GenerationDimension] =
    &[GenerationDimension::Host, GenerationDimension::Process, GenerationDimension::Session];

/// The per-dimension currentness snapshot carried by observations and by
/// predicate settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationSnapshot {
    pub host: u32,
    pub process: u32,
    pub session: u32,
}

impl GenerationSnapshot {
    pub fn zeroed() -> Self {
        Self::default()
    }

    pub fn dimension(&self, dimension: GenerationDimension) -> u32 {
        match dimension {
            GenerationDimension::Host => self.host,
            GenerationDimension::Process => self.process,
            GenerationDimension::Session => self.session,
        }
    }
}

/// The bounded observable predicate kinds this contract requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateKind {
    /// The spawned server completed its initialize handshake inside the
    /// budget (instrument-plane evidence).
    ServerHandshakeSettled,
    /// The exact server process reached a terminal state with no orphaned
    /// child left behind (process-supervision evidence).
    ServerTerminalState,
}

/// One required predicate and its explicit wait budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PredicateRequirement {
    pub kind: PredicateKind,
    pub max_wait_ms: u64,
}

/// A substitution is never state: every kind names a specific forgery the
/// validation laws reject outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstitutionKind {
    FixedSleep,
    ElapsedOnlySatisfaction,
    LogTextAsUiState,
    AnyResultMatch,
}

/// Evidence for one required predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "predicate", rename_all = "snake_case", deny_unknown_fields)]
pub enum PredicateEvidence {
    Satisfied {
        kind: PredicateKind,
        /// Bounded digest naming the state that settled the predicate;
        /// elapsed time alone is never satisfaction.
        settled_state_digest: String,
        settled_generations: GenerationSnapshot,
        polls: u64,
        waited_ms: u64,
    },
    TimedOut {
        kind: PredicateKind,
        polls: u64,
        waited_ms: u64,
    },
    Substituted {
        kind: PredicateKind,
        substitution: SubstitutionKind,
    },
}

impl PredicateEvidence {
    pub fn kind(&self) -> PredicateKind {
        match self {
            PredicateEvidence::Satisfied { kind, .. }
            | PredicateEvidence::TimedOut { kind, .. }
            | PredicateEvidence::Substituted { kind, .. } => *kind,
        }
    }
}

/// True for stable, bounded predicate/settle tokens used inside digests.
pub fn is_stable_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= super::observation::MAX_TOKEN_BYTES
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
