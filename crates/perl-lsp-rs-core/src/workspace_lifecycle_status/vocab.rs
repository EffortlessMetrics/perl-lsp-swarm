//! Closed status vocabularies for independent lifecycle dimensions.
//!
//! Exact Rust names are implementation latitude; the propositions and
//! separation are not. Unknown wire tokens fail closed. These enums are not
//! the live #7932/#6736/#7900/#3099/#7892/#7454 state machines.

use serde::{Deserialize, Serialize};

/// Root/runtime lifecycle projection vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootLifecycleState {
    /// Replacement generation exists but has not admitted domain work.
    Creating,
    /// Current and available for exact use.
    Active,
    /// Accepted inputs are changing.
    Reconfiguring,
    /// Terminal cleanup is in progress.
    Removing,
    /// Detached; no work may publish.
    Detached,
    /// Application session is shutting down.
    Shutdown,
    /// Current only in an explicit limited state.
    Limited,
}

impl RootLifecycleState {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Active => "active",
            Self::Reconfiguring => "reconfiguring",
            Self::Removing => "removing",
            Self::Detached => "detached",
            Self::Shutdown => "shutdown",
            Self::Limited => "limited",
        }
    }
}

/// Configuration observation vocabulary. `not_proven` is explicit, never a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationState {
    /// Current accepted configuration is valid.
    Valid,
    /// A reload of configuration is in flight.
    Reloading,
    /// Current configuration/adapter input is invalid.
    InvalidCurrent,
    /// Configuration was deleted and defaults apply.
    DeletedOrDefaulted,
    /// Observation is known stale relative to the subject generation.
    Stale,
    /// Configuration state was not proven by instrumentation.
    NotProven,
}

impl ConfigurationState {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Reloading => "reloading",
            Self::InvalidCurrent => "invalid_current",
            Self::DeletedOrDefaulted => "deleted_or_defaulted",
            Self::Stale => "stale",
            Self::NotProven => "not_proven",
        }
    }
}

/// Watch/reload observation vocabulary. Registration is not invalidation or acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchReloadState {
    /// No watch transport is available.
    Unavailable,
    /// A watch transport is registered.
    Registered,
    /// Prior output has been invalidated.
    Invalidated,
    /// A reload is pending evaluation.
    Pending,
    /// A reload is currently evaluating.
    Evaluating,
    /// A reload was accepted.
    Accepted,
    /// A signal was observed as a no-op.
    NoOp,
    /// Reload evaluation failed.
    Failed,
}

impl WatchReloadState {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Registered => "registered",
            Self::Invalidated => "invalidated",
            Self::Pending => "pending",
            Self::Evaluating => "evaluating",
            Self::Accepted => "accepted",
            Self::NoOp => "no_op",
            Self::Failed => "failed",
        }
    }
}

/// Readiness for one independent scope. Store availability cannot fill this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessState {
    /// Readiness was not proven.
    NotProven,
    /// The scope is not ready.
    NotReady,
    /// Bounded work is still pending.
    Pending,
    /// The scope is ready.
    Ready,
    /// The scope is ready with explicit limitations.
    ReadyLimited,
    /// Instrumentation failed to observe readiness.
    InstrumentFailed,
}

impl ReadinessState {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotProven => "not_proven",
            Self::NotReady => "not_ready",
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::ReadyLimited => "ready_limited",
            Self::InstrumentFailed => "instrument_failed",
        }
    }
}

/// Snapshot storage administration vocabulary. Availability is not compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageState {
    /// Storage is disabled (for example cache-off cold start).
    Disabled,
    /// Storage cannot be reached.
    Unavailable,
    /// Storage is busy (another process or lock).
    Busy,
    /// Storage is reachable and empty.
    Empty,
    /// Storage is reachable and has content. This does not imply adoption.
    Available,
    /// A candidate exists but is incompatible.
    Incompatible,
    /// Storage is quarantined (corrupt/isolated).
    Quarantined,
}

impl StorageState {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
            Self::Busy => "busy",
            Self::Empty => "empty",
            Self::Available => "available",
            Self::Incompatible => "incompatible",
            Self::Quarantined => "quarantined",
        }
    }
}

/// Hydration/adoption vocabulary. Distinct from storage administration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HydrationState {
    /// Hydration was not attempted.
    NotAttempted,
    /// A candidate is loading.
    Loading,
    /// A candidate is being validated.
    Validating,
    /// Some facts were adopted.
    AdoptedPartial,
    /// Adoption completed.
    AdoptedComplete,
    /// The candidate was rejected.
    Rejected,
    /// Live analysis proceeds from a cold fallback.
    ColdFallback,
    /// Hydration was not proven.
    NotProven,
}

impl HydrationState {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotAttempted => "not_attempted",
            Self::Loading => "loading",
            Self::Validating => "validating",
            Self::AdoptedPartial => "adopted_partial",
            Self::AdoptedComplete => "adopted_complete",
            Self::Rejected => "rejected",
            Self::ColdFallback => "cold_fallback",
            Self::NotProven => "not_proven",
        }
    }
}

/// Provider posture. Empty, not-ready, refusal, and instrument failure stay distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPosture {
    /// Exact current provider output.
    ExactCurrent,
    /// Bounded current provider output.
    BoundedCurrent,
    /// Provider is not ready.
    NotReady,
    /// Safe refusal with canonical evidence.
    SafeRefusal,
    /// Legitimate empty with canonical evidence. Never inferred from absence.
    LegitimateEmpty,
    /// Unsupported dynamic subject.
    UnsupportedDynamic,
    /// Product failure in the provider domain.
    ProductFailure,
    /// Observation/instrument failure, not a product failure.
    InstrumentFailure,
}

impl ProviderPosture {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExactCurrent => "exact_current",
            Self::BoundedCurrent => "bounded_current",
            Self::NotReady => "not_ready",
            Self::SafeRefusal => "safe_refusal",
            Self::LegitimateEmpty => "legitimate_empty",
            Self::UnsupportedDynamic => "unsupported_dynamic",
            Self::ProductFailure => "product_failure",
            Self::InstrumentFailure => "instrument_failure",
        }
    }
}

/// Closed owner roles for repair authority. Free-form strings are refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsibleOwnerRole {
    /// Root/runtime lifecycle (#7932).
    RootLifecycle,
    /// Configuration (#6736/#7900).
    Configuration,
    /// Reload/watch (#7893).
    Reload,
    /// Readiness (#3099).
    Readiness,
    /// Storage administration (#7892).
    Storage,
    /// Hydration/adoption (#7454).
    Hydration,
    /// Provider decisions (#3099).
    Provider,
    /// Environment/trust/binary identity (#4832/#4905).
    EnvironmentTrust,
}

impl ResponsibleOwnerRole {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RootLifecycle => "root_lifecycle",
            Self::Configuration => "configuration",
            Self::Reload => "reload",
            Self::Readiness => "readiness",
            Self::Storage => "storage",
            Self::Hydration => "hydration",
            Self::Provider => "provider",
            Self::EnvironmentTrust => "environment_trust",
        }
    }
}

/// Typed primary action. Priority policy is a separate input; this is not prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimaryActionKind {
    /// No repair is required (including healthy cold fallback).
    NoneRequired,
    /// Repair current project configuration.
    RepairConfiguration,
    /// Repair or disable an invalid adapter.
    RepairAdapter,
    /// Wait for an in-flight reload.
    WaitForReload,
    /// Wait for indexing/hydration work.
    WaitForIndex,
    /// Use the explicit manual reload route.
    ManualReload,
    /// Inspect storage path/permissions or process ownership locally.
    InspectStoragePermissions,
    /// Optional diagnostic export/clear of quarantined cache.
    ExportQuarantinedCache,
    /// Repair trust or binary/protocol identity mismatch.
    RepairTrustOrIdentity,
}

impl PrimaryActionKind {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoneRequired => "none_required",
            Self::RepairConfiguration => "repair_configuration",
            Self::RepairAdapter => "repair_adapter",
            Self::WaitForReload => "wait_for_reload",
            Self::WaitForIndex => "wait_for_index",
            Self::ManualReload => "manual_reload",
            Self::InspectStoragePermissions => "inspect_storage_permissions",
            Self::ExportQuarantinedCache => "export_quarantined_cache",
            Self::RepairTrustOrIdentity => "repair_trust_or_identity",
        }
    }
}

/// Optional local diagnostic/export action. Never a private filesystem path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalDiagnosticAction {
    /// Inspect local storage permissions or ownership.
    InspectStorage,
    /// Export a redacted diagnostic packet.
    ExportDiagnostic,
}

impl LocalDiagnosticAction {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InspectStorage => "inspect_storage",
            Self::ExportDiagnostic => "export_diagnostic",
        }
    }
}

/// Typed primary-action priority policy input. Not free-form ranking prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimaryActionPriorityPolicy {
    /// Canonical #7933 ordering: identity/trust, invalid config, root
    /// transition, product/quarantine, in-progress work, healthy cold,
    /// ready/ready-limited.
    CanonicalV1,
}

impl PrimaryActionPriorityPolicy {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CanonicalV1 => "canonical_v1",
        }
    }
}

/// Completeness of one status row. Absence is never `complete`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    /// Every independent dimension is observed.
    Complete,
    /// At least one dimension is partial, stale, or in progress.
    Partial,
    /// Completeness itself is not proven.
    NotProven,
}

impl Completeness {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::NotProven => "not_proven",
        }
    }
}

/// Instrument state for the row. Distinct from product failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentState {
    /// Instrumentation produced the observed dimensions.
    Ok,
    /// At least one dimension failed to be observed.
    Failed,
    /// Instrument state is not proven.
    NotProven,
}

impl InstrumentState {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::NotProven => "not_proven",
        }
    }
}

/// Public-safe vs local-diagnostic classification. Public output never carries
/// source, secrets, private paths, environment values, or logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyClass {
    /// Default public serialization class.
    PublicSafe,
    /// Local diagnostic class; still refuses hostile canaries by construction.
    LocalDiagnostic,
}

impl PrivacyClass {
    /// Frozen wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PublicSafe => "public_safe",
            Self::LocalDiagnostic => "local_diagnostic",
        }
    }
}
