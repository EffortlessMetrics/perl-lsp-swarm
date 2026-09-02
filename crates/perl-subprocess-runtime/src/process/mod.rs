//! The supervised process domain: plan, events, result, and ports.
//!
//! ```text
//! domain operation + exact identities + authorization evidence
//!   -> ProcessPlan
//!   -> validate()            (pure; the only route to a startable plan)
//!   -> ValidatedProcessPlan
//!   -> ProcessSupervisor     (port)
//!   -> ordered ProcessEvent stream
//!   -> terminal ProcessResult
//!   -> domain result adapter
//! ```
//!
//! # What this module owns
//!
//! The structured mechanics of an authorized execution and the terminal truth
//! about it: what will run, with what exact identities, under what bounds,
//! and how the attempt ended.
//!
//! # What it does not own
//!
//! Formatter, critic, debug-adapter, test, compiler, parser, editor, and
//! release semantics; scheduling; and any claim about sandboxing, isolation,
//! or hermeticity. Types, timeouts, and process ownership do not constrain
//! what admitted code can reach.
//!
//! # Implementation status
//!
//! This is the domain contract only. It performs **no** operating-system
//! spawn: there is no production backend in this module, and the only
//! supervisor it ships is the deterministic [`fake::FakeSupervisor`]. Every
//! result the fake produces is marked [`EvidenceClass::Fake`], so fake
//! evidence can never be mistaken for evidence about a real process.

pub mod encoding;
pub mod environment;
pub mod event;
pub mod fake;
pub mod identity;
pub mod legacy;
pub mod plan;
pub mod port;
pub mod result;
pub mod validation;

pub use encoding::{ContentFingerprint, Fingerprint, PathFingerprint, PlanFingerprint};
pub use environment::{
    AmbientInheritance, CODE_LOADING_VARIABLES, CodeLoadingDisposition, EnvVarName,
    EnvironmentProjection, is_code_loading_variable,
};
pub use event::{
    EventAdmissionError, EventLedger, EventSequence, LimitEvidence, ProcessEvent, ProcessEventKind,
    StreamChunkEvidence, TerminationPhase,
};
pub use fake::{FAKE_BACKEND_NAME, FakeSupervisor, ScriptedOutcome, ScriptedRun};
pub use identity::{
    AuthorizationEvidence, AuthorizationStrength, CwdPolicy, EvidenceFreshness, ExecutableIdentity,
    ExecutableResolution, ExecutionProfile, OperationId, OwnerDomain, PlanId, PlatformRequirement,
    PrivateBytes, PrivatePath, ResolutionProvenance, RunId, SchemaVersion, SecretValue,
    SubjectIdentity, SubjectReference,
};
pub use legacy::{LEGACY_CONTAINMENT, LegacyContainment, LegacyUnsupportedCapability};
pub use plan::{
    CancellationPolicy, CaptureBudget, ClaimBoundary, DeadlinePolicy, MAX_CAPTURE_BUDGET_BYTES,
    OutputLimitAction, ProcessPlan, ProcessPlanBuilder, PublicProjection, RetentionPolicy,
    StdinPolicy, TerminationPolicy,
};
pub use port::{
    CancellationAcknowledgement, HandleDropDisposition, ProcessHandle, ProcessSupervisor,
    StdinWriteOutcome,
};
pub use result::{
    BackendIdentity, CancellationReason, CleanupDisposition, ControlState, DecodedViewLimitation,
    EvidenceClass, Limitation, ObservedSettlement, ProcessResult, SpawnFailureDetail,
    StreamChannel, StreamEvidence, TerminalDisposition, TreeDisposition, TruncationState,
    WorkMetadata,
};
pub use validation::{BudgetChannel, PlanRejection, ValidatedProcessPlan};

/// The version of this domain's types, discriminants, and canonical encoding.
///
/// Any change in meaning — a new field that participates in the canonical
/// encoding, a reused tag, a changed discriminant, a changed validation
/// outcome — must move this version. The locked canonical-encoding test in
/// the crate's test suite exists to make an unversioned change fail.
pub const PROCESS_DOMAIN_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1);
