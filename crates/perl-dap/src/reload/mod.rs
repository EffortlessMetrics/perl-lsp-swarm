//! Loaded-module reload semantic contract (reload train R01, #10097).
//!
//! This module freezes the reviewed contract for a bounded loaded-module
//! reload transaction against a live Perl debuggee. It is a typed, pure
//! semantic layer only: it performs no live reload, sends no debugger
//! command, advertises no capability, and defines no wire format. The
//! mechanism execution (#10098), the custom DAP family registration
//! (#10138), and the reconciliation/invalidation wiring (#10102) consume
//! these decisions; none of them may widen them silently.
//!
//! # Frozen decisions
//!
//! - **Exact subject identity** ([`LoadedModuleSubject`]): a reload subject
//!   is bound by session generation, suspension generation, `%INC` key and
//!   resolved runtime path, loaded-source observation generation, saved
//!   content digest, selected runtime identity, launch authority, module
//!   classification, and operation identity together. Path spelling,
//!   basename, `%INC` key alone, package name alone, or matching source
//!   bytes are each insufficient ([`subject::SubjectCandidate::bind`]
//!   refuses incomplete bindings with `insufficient_subject_identity`).
//! - **Closed eligibility vocabulary** ([`LoadedModuleReloadEligibility`]):
//!   exactly thirteen dispositions; the initial admitted cohort is
//!   `eligible_source_backed_perl_module` with no active target-module
//!   frame. Every other class fails closed with a deterministic
//!   precedence ([`eligibility::classify_reload_eligibility`]).
//! - **Possibly-applied boundary** ([`transaction`]): eight transaction
//!   phases; any timeout, transport loss, or ambiguous response at or
//!   after `runtime_mutation_begins` is `indeterminate_possibly_applied`
//!   and is never projected as a clean or empty result.
//! - **Generation semantics** ([`RuntimeModuleGeneration`]): a monotonic
//!   per-debuggee-process generation advanced by *both* `reloaded` and
//!   `indeterminate_possibly_applied`; refusals and pre-mutation failures
//!   advance nothing. It is independent of the suspension authority
//!   (`stopped_generation`, `debug_adapter/session.rs`) but composed with
//!   it in the invalidation table.
//! - **Invalidation table** ([`ReloadInvalidationPlan`]): a closed
//!   per-object-kind disposition for every enumerated DAP object kind for
//!   both terminal mutation outcomes. Thread references are adapter
//!   projections, not runtime facts; durable client breakpoint
//!   configuration is preserved and reconciled later (#10102).
//! - **Protocol requirements** ([`surface`]): a namespaced, versioned
//!   custom family with correlation identity that accepts no raw path,
//!   debugger command, or Perl expression, invents no standard DAP
//!   capability, and stays unadvertised until R04 proof. The wire format
//!   itself belongs to #10138.
//! - **Mechanism limits** ([`mechanism`]): comparative limitation records
//!   for the four candidate mechanisms. Compile success is never reload
//!   success, and no external module (for example Class::Refresh) becomes
//!   product authority merely by being available.
//! - **Live measurement** ([`measurement`], #10098): the controlled
//!   real-Perl harness that records each directly measurable mechanism's
//!   actual state limits as typed facts, and every boundary the harness
//!   cannot measure as a typed unmeasured boundary. Measurement adds
//!   evidence against the frozen record; it never rewrites the frozen
//!   vocabularies.
//!
//! # Authority
//!
//! Decision record: `docs/adr/0046-loaded-module-reload-semantics.md`.
//! Machine-checkable corpus: `.spec/10097-loaded-module-reload-contract/`
//! (schema, classification fixtures, transaction fixtures, negative
//! controls with expected error codes). The Rust vocabulary in this module
//! is the executable authority; the `.spec` schema mirrors it and is kept
//! in sync by a fixture-driven test.

mod eligibility;
mod generation;
mod invalidation;
mod measurement;
mod mechanism;
mod reconciliation;
mod subject;
mod surface;
mod transaction;

pub use eligibility::{
    LoadedModuleReloadEligibility, ReloadAdmissionObservation, classify_reload_eligibility,
};
pub use generation::{
    GenerationAdvance, GenerationEffect, RetainedModuleObservations, RuntimeModuleGeneration,
    RuntimeModuleGenerationClock,
};
pub use invalidation::{
    DapObjectKind, DapReferenceBinding, InvalidationDisposition, InvalidationPlanError,
    ReloadInvalidationPlan, invalidation_plan_for, reference_is_stale, verify_invalidation_plan,
};
pub use measurement::{
    MeasuredStateFact, MeasurementRecordError, MechanismMeasurement, UnmeasuredBoundary,
    measure_mechanism_on_real_perl, verify_measurement,
};
pub use mechanism::{
    MechanismClaim, MechanismClaims, MechanismRecordError, ReloadMechanism, ReloadMechanismRecord,
    mechanism_records, verify_mechanism_claims,
};
pub use reconciliation::{
    MAX_RETAINED_COMPLETIONS, MUTATION_INVALIDATED_AREAS, ObservationClaim, ReloadSessionWiring,
    ReloadWiringRefusal, RoutedReloadTerminal, outcome_is_mutating,
    reconciliation_dispositions_for, verify_reconciliation_claim,
};
pub use subject::{
    LoadedModuleSubject, ModuleClassification, SubjectBindingError, SubjectCandidate,
    SubjectCurrentnessView,
};
pub use surface::{
    ReloadCapabilityProjection, ReloadRequestPayload, ReloadRequestSurfaceDescriptor,
    SurfaceViolation, validate_request_surface,
};
pub use transaction::{
    IndeterminateCause, LoadedModuleReloadOutcome, LoadedModuleReloadPlan, PreMutationFailureCause,
    ReloadTransactionPhase, phase_permits_outcome, plan_reload, project_unknown_after_mutation,
};

#[cfg(test)]
mod fixture_tests;
