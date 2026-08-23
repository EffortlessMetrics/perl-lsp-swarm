//! Public checked framework-adapter SDK surface.
//!
//! The substrate vocabulary remains in `framework_raw.rs`. Detection input and
//! result types are wrapped here because a deserialized result cannot establish
//! presence or absence from its own assertions. Authoritative detection always
//! validates against the exact observed input and reason-specific evidence.

#[path = "framework_checked/model.rs"]
mod model;
#[path = "framework_raw.rs"]
mod raw;
#[path = "framework_checked/validation.rs"]
mod validation;
#[path = "framework_checked/validation_input.rs"]
mod validation_input;
#[path = "framework_checked/validation_outcome.rs"]
mod validation_outcome;
#[path = "framework_checked/version.rs"]
mod version;

pub use model::*;
pub use raw::{
    AdapterAuthorityError, AdapterBudget, AdapterCancellation, AdapterCancellationControl,
    AdapterDescriptor, AdapterDisposition, AdapterId, AdapterInput, AdapterOutcome, AdapterResult,
    AdapterSourceScope, DetectionAbsenceReason, DetectionOutcome, EmittedFact,
    FRAMEWORK_ADAPTER_SCHEMA_VERSION, FRAMEWORK_ADAPTER_SDK_LEGACY_VERSION,
    FRAMEWORK_ADAPTER_SDK_VERSION, FactClass, FactLimitation, FactSink, FactSinkId,
    ModuleActivationIdentity, ModuleVersionEvidence, NoopAdapterCancellationControl,
    UnavailableReason,
};
