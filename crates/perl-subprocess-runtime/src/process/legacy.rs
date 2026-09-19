//! Containment of the pre-domain `run_command` seam.
//!
//! [`crate::SubprocessRuntime`] and [`crate::OsSubprocessRuntime`] predate the
//! supervised process domain. They remain in the crate because live consumers
//! compile against them, and migrating those consumers is separately owned
//! work — not because they are a second production process authority.
//!
//! This module states that containment as data so that it can be asserted
//! rather than remembered.
//!
//! # What the legacy seam does not provide
//!
//! Everything in [`LEGACY_UNSUPPORTED_CAPABILITIES`]. In particular it cannot
//! express exact cwd or environment projection, execution authorization,
//! output budgets, cancellation, process-tree cleanup, or terminal-cause
//! precedence — so a caller that needs any of those must migrate rather than
//! be told the legacy path is adequate.
//!
//! # Why it is not simply deleted
//!
//! Deleting it would rewrite every live consumer inside a contract PR, which
//! is exactly the widening this lane is meant to avoid.

use super::identity::OwnerDomain;

/// A capability the legacy seam cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LegacyUnsupportedCapability {
    /// An exact working directory.
    ExactWorkingDirectory,
    /// A declared environment projection.
    EnvironmentProjection,
    /// Execution-authorization evidence.
    ExecutionAuthorization,
    /// Bounded output observation and retention.
    OutputBudgets,
    /// Separate observed and retained stream identities.
    ObservedVersusRetainedOutput,
    /// Cancellation.
    Cancellation,
    /// Process-group or process-tree cleanup.
    ProcessTreeCleanup,
    /// Deterministic terminal-cause precedence.
    TerminalCausePrecedence,
    /// An ordered event stream.
    OrderedEventStream,
    /// A public, redacted execution receipt.
    RedactedReceipt,
}

/// Everything the legacy seam cannot express.
pub const LEGACY_UNSUPPORTED_CAPABILITIES: &[LegacyUnsupportedCapability] = &[
    LegacyUnsupportedCapability::ExactWorkingDirectory,
    LegacyUnsupportedCapability::EnvironmentProjection,
    LegacyUnsupportedCapability::ExecutionAuthorization,
    LegacyUnsupportedCapability::OutputBudgets,
    LegacyUnsupportedCapability::ObservedVersusRetainedOutput,
    LegacyUnsupportedCapability::Cancellation,
    LegacyUnsupportedCapability::ProcessTreeCleanup,
    LegacyUnsupportedCapability::TerminalCausePrecedence,
    LegacyUnsupportedCapability::OrderedEventStream,
    LegacyUnsupportedCapability::RedactedReceipt,
];

/// The declared containment of a pre-domain process seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyContainment {
    /// The seam's public name.
    pub seam: &'static str,
    /// The owner domain a legacy invocation is attributed to.
    pub owner: OwnerDomain,
    /// The issue that owns removing the seam.
    pub removal_owner: &'static str,
    /// Whether new consumers may be written against the seam.
    pub open_to_new_consumers: bool,
    /// What the seam cannot express.
    pub unsupported: &'static [LegacyUnsupportedCapability],
}

/// The contained legacy seams in this crate.
pub const LEGACY_CONTAINMENT: &[LegacyContainment] = &[
    LegacyContainment {
        seam: "SubprocessRuntime::run_command",
        owner: OwnerDomain::LegacyAdapter,
        removal_owner: "#1975",
        open_to_new_consumers: false,
        unsupported: LEGACY_UNSUPPORTED_CAPABILITIES,
    },
    LegacyContainment {
        seam: "OsSubprocessRuntime",
        owner: OwnerDomain::LegacyAdapter,
        removal_owner: "#1975",
        open_to_new_consumers: false,
        unsupported: LEGACY_UNSUPPORTED_CAPABILITIES,
    },
];

/// Whether any contained seam is open to new consumers.
///
/// Always false: a contained seam that accepts new consumers is not
/// contained.
pub fn any_seam_open_to_new_consumers() -> bool {
    LEGACY_CONTAINMENT.iter().any(|entry| entry.open_to_new_consumers)
}
