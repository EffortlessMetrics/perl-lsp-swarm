//! Dependency-neutral in-memory domain model for maintained compiler
//! operating profiles (#12186).
//!
//! This module defines exact identity, closed row dispositions, imports,
//! subject selectors, evidence requirements, limitations, ownership,
//! invalidation, and claim ceilings — plus the closure law that makes a
//! profile's validity a conjunctive, per-axis boolean result:
//!
//! - every required applicable row is conjunctive across its declared proof
//!   axes; each axis demands evidence of its own and no axis ever satisfies
//!   another;
//! - conditional, optional, unsupported, and not-applicable are closed typed
//!   states carried by rows, never omissions;
//! - an import binds an exact lower-profile id/version/content digest and
//!   preserves every imported row and limitation verbatim;
//! - semantic fingerprints are FNV-1a 64 over a canonical, order-independent
//!   encoding, so any semantic change moves identity while insertion order
//!   cannot;
//! - there is no weighted or aggregate readiness score anywhere in this
//!   module, and no support/release/publication authority is derivable from a
//!   validated profile.
//!
//! Scope boundary: this layer is pure std-only in-memory modeling. No serde
//! derives, file loading, manifest syntax, CLI, receipt adaptation, status
//! generation, GitHub/workflow/LSP DTO types, initial repository row
//! inventory, or live product state belongs here. The wire-format task in
//! `crate::tasks::compiler_profile` stays separate by design.

mod dimensions;
mod fingerprint;
mod identity;
mod profile;
mod requirements;
mod rows;

pub use dimensions::ClaimFamily;
pub use dimensions::CompatibilityAcceptance;
pub use dimensions::EvidenceClass;
pub use dimensions::ExecutionStage;
pub use dimensions::ProofAxis;
pub use dimensions::SemanticSupportLevel;
pub use dimensions::SourceTier;
pub use dimensions::SubjectArea;
pub use dimensions::SubjectSelector;
pub use dimensions::SupportClaim;
pub use dimensions::UpstreamObservation;
pub use dimensions::WorkContext;
pub use dimensions::WorkPerformed;
pub use dimensions::WorkRequirement;
pub use dimensions::class_supports_family;
pub use identity::CompilerProfileId;
pub use identity::CompilerProfileRowId;
pub use identity::CompilerProfileVersion;
pub use identity::ProfileContentDigest;
pub use profile::CompilerProfileDefinition;
pub use profile::CompilerProfileImport;
pub use profile::ProfileRegistry;
pub use requirements::AllowedLimitation;
pub use requirements::ClaimCeiling;
pub use requirements::CollaborationSurface;
pub use requirements::CompletenessRequirement;
pub use requirements::CompletenessRule;
pub use requirements::CurrentnessRule;
pub use requirements::EvidenceRecord;
pub use requirements::ExternalProvenance;
pub use requirements::InvalidationInput;
pub use requirements::LegacyExitDimension;
pub use requirements::LegacyExitRequirement;
pub use requirements::OwnerAndWakeEvent;
pub use requirements::WakeEvent;
pub use rows::AxisProofSpec;
pub use rows::CompilerProfileRow;
pub use rows::ConditionalActivation;
pub use rows::RowDisposition;

/// Classified validation failures for profile construction and closure.
///
/// Variants are precise so tests and callers can distinguish malformed
/// identity from cross-satisfaction attempts, stage/work violations, support
/// overstatements, disposition conflicts, rejected provenance, and import
/// resolution or preservation failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerProfileError {
    /// Malformed ids, versions, digests, or owner references.
    Identity {
        /// What is malformed and why.
        message: String,
    },
    /// Structural violations: empty statements/selectors/reasons/specs.
    Structure {
        /// What is malformed and why.
        message: String,
    },
    /// A required axis lacks its own conforming evidence.
    MissingRequiredEvidence {
        /// Row id whose axis went unsatisfied.
        row: String,
        /// The unsatisfied axis.
        axis: String,
        /// Why the evidence does not count for this axis.
        detail: String,
    },
    /// An evidence class can never back the offered axis family.
    CrossSatisfaction {
        /// Row id involved.
        row: String,
        /// What crossed which boundary.
        detail: String,
    },
    /// Observation stage below an axis floor.
    StageUnderflow {
        /// Row id involved.
        row: String,
        /// Required floor versus observed stage.
        detail: String,
    },
    /// Provenance tier below an axis floor.
    EvidenceTierBelowFloor {
        /// Row id involved.
        row: String,
        /// Required floor versus observed tier.
        detail: String,
    },
    /// Performed work misses the requirement (context or minimum).
    WorkMismatch {
        /// Row id involved.
        row: String,
        /// Required versus performed work.
        detail: String,
    },
    /// A support claim claims more than its typed inputs allow.
    SupportOverstatement {
        /// Row id involved.
        row: String,
        /// Which strengthening was rejected.
        detail: String,
    },
    /// A disposition state contradicts other row content.
    DispositionConflict {
        /// Row id involved.
        row: String,
        /// Which combination was rejected.
        detail: String,
    },
    /// Issue/PR/workflow state was offered as evidence and refused.
    RejectedProvenance {
        /// Which provenance kind was refused.
        detail: String,
    },
    /// An import could not be resolved to its bound identity/digest.
    ImportResolution {
        /// Importing profile name.
        importer: String,
        /// Imported profile name.
        imported: String,
        /// What failed during resolution.
        detail: String,
    },
    /// An import lost or altered rows or limitations.
    ImportPreservation {
        /// Importing profile name.
        importer: String,
        /// Imported profile name.
        imported: String,
        /// What was lost or altered.
        detail: String,
    },
}

impl std::fmt::Display for CompilerProfileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Identity { message } => {
                write!(formatter, "compiler-profile identity error: {message}")
            }
            Self::Structure { message } => {
                write!(formatter, "compiler-profile structure error: {message}")
            }
            Self::MissingRequiredEvidence { row, axis, detail } => write!(
                formatter,
                "compiler-profile closure error: row {row} axis {axis} lacks evidence: {detail}"
            ),
            Self::CrossSatisfaction { row, detail } => {
                write!(formatter, "compiler-profile cross-satisfaction at row {row}: {detail}")
            }
            Self::StageUnderflow { row, detail } => {
                write!(formatter, "compiler-profile stage underflow at row {row}: {detail}")
            }
            Self::EvidenceTierBelowFloor { row, detail } => write!(
                formatter,
                "compiler-profile provenance-tier underflow at row {row}: {detail}"
            ),
            Self::WorkMismatch { row, detail } => {
                write!(formatter, "compiler-profile work mismatch at row {row}: {detail}")
            }
            Self::SupportOverstatement { row, detail } => {
                write!(formatter, "compiler-profile support overstatement at row {row}: {detail}")
            }
            Self::DispositionConflict { row, detail } => {
                write!(formatter, "compiler-profile disposition conflict at row {row}: {detail}")
            }
            Self::RejectedProvenance { detail } => {
                write!(formatter, "compiler-profile rejected provenance: {detail}")
            }
            Self::ImportResolution { importer, imported, detail } => write!(
                formatter,
                "compiler-profile import failure ({importer} <- {imported}): {detail}"
            ),
            Self::ImportPreservation { importer, imported, detail } => write!(
                formatter,
                "compiler-profile import preservation failure ({importer} <- {imported}): {detail}"
            ),
        }
    }
}

impl std::error::Error for CompilerProfileError {}

/// Stable identifiers are lowercase tokens of `[a-z0-9._-]`, at most 128
/// characters, starting alphanumeric. Shared with `close_proof` semantics but
/// kept local so this module remains dependency-neutral.
pub(crate) fn is_stable_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    })
}
