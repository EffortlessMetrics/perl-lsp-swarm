//! Mutation contracts for DAP variable editing.
//!
//! Domain types and validation only: no DAP wire parsing, no Perl rendering,
//! no debugger I/O.
//!
//! # Two profiles, one number authority
//!
//! Two versioned value profiles live here and are mechanically separate — no
//! conversion exists between [`MutationValue`] and [`StructuredValue`], and
//! [`MutationValueProfile`] is what a receipt records so one can never be
//! presented as the other:
//!
//! ```text
//! MutationValueText.v1        scalar core        (#8364 train, S0 = #10736)
//! MutationStructuredValue.v1  optional breadth   (#11326 train, S0 = #11327)
//! ```
//!
//! They do, however, share one exact-number carrier. [`ExactDecimal`] is the
//! crate's **single** exact-decimal authority: it was checked in with the
//! structured profile and the scalar profile reuses it rather than cloning a
//! second number model. That is what #11327's start conditions asked for
//! ("reuse exact-number/string types from the scalar model rather than cloning
//! them"), and it is what keeps `f64` out of both profiles.
//!
//! # Redaction is structural
//!
//! Debuggee data — an assigned value, a hash key, an observed read-back — is
//! private. Rather than rely on every future caller remembering to redact,
//! none of the payload-bearing types implement [`serde::Serialize`]: not
//! [`MutationValue`], [`MutationMember`], [`MutationLocationProvenance`],
//! [`MutationTarget`], [`MutationOperation`], [`ObservedReadBack`], or
//! [`MutationOutcome`]. The `*Receipt` projections do, and they carry cohort,
//! identity, class, and size only. So the way to serialize any of this is
//! `receipt_projection()`, and reaching for the raw type instead is a
//! compile error rather than a silent leak.
//!
//! `Debug` is still derived, because it is load-bearing for tests and
//! assertions; keep debuggee payload out of logs by logging receipts.
//!
//! # Identity
//!
//! [`MutationLocationProvenance`], [`InspectedValueIdentity`], and
//! [`MutationTarget`] are three distinct propositions; collapsing any two of
//! them is what makes a debugger write to the wrong storage.

mod operation;
mod outcome;
mod scalar_value;
mod structured_value;
mod target;

pub use operation::{
    MutationDeadline, MutationOperation, MutationOperationReceipt, MutationOrigin,
    ResponseValueFormat,
};
pub use outcome::{
    MutationOutcome, MutationOutcomeReceipt, ObservedReadBack, PossibleApplication,
    UnsupportedCellReceipt,
};
pub use scalar_value::{
    ExactInteger, MUTATION_SCALAR_VALUE_SCHEMA_VERSION, MutationValue, MutationValueKind,
    MutationValueProfile, MutationValueReceipt,
};
pub use structured_value::{
    ExactDecimal, FreshReferentKind, MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION,
    MutationStructuredValueV1, STRUCTURED_PREFIX, StructuredMutationLimits, StructuredRefusal,
    StructuredValue, fresh_referent_kind, parse_structured_mutation, structured_payload,
};
pub use target::{
    InspectedValueIdentity, MUTATION_TARGET_PROFILE_VERSION, MutationLocationKind,
    MutationLocationProvenance, MutationMember, MutationTarget, MutationTargetBindingError,
    MutationTargetCandidate, MutationTargetCohort, MutationTargetReceipt, RefusedWritability,
    WritabilityDisposition,
};
