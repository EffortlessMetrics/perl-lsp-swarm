//! Typed host work status domain (#11664, WIP-01).
//!
//! Pure domain over supplied typed observations: exact host-work subject,
//! four independent WIP dimensions (logical, mutation, compute, storage), a
//! closed lifecycle vocabulary, mechanical classification laws, aggregate
//! observations, cleanup-readiness fields, and exhaustive provider adapters
//! over currently landed typed owners (#10256/#10263 worktree plans,
//! #3957/#11617 writer-admission reports).
//!
//! This module performs no Git, filesystem, process, network, GitHub, or
//! cleanup operation, and owns no scheduler. Providers without a landed
//! typed owner (#11650/#11653/#11659/#11661) must be declared missing via
//! [`adapter::declare_missing_provider`] so their absence stays visible.

// WIP-01 (#11664) ships the pure domain ahead of its production consumer:
// #11666 wires the live read-only observation command onto these types. Until
// that successor lands, the module is reachable from tests only, so the
// re-export surface below is intentionally kept with `unused_imports`
// silenced and dead code allowed, scoped to this module alone.
#![allow(unused_imports, dead_code)]

mod adapter;
mod dimension;
mod lifecycle;
mod status;
mod subject;

pub use adapter::{
    AdapterError, AdmissionAdapterOutcome, MissingProviderDeclaration, SubjectObservations,
    WorktreePlanAdapterOutcome, adapt_admission_report, adapt_worktree_cleanup_plan,
    declare_missing_provider, missing_capacity_reservation, missing_executor_allocation,
    missing_process_observation, process_tree_compute_observation,
    reservation_only_compute_observation, storage_scope_observation,
};
pub use dimension::{
    Attribution, CapacityFact, ClaimRelationship, ComputeWorkObservation, DurableState,
    FootprintFact, Freshness, HostWorkObservationSet, IndexState, InitiatorReturn, Instrument,
    LogicalWorkObservation, MutationOwnership, MutationWorkObservation, OrphanPremises,
    ProcessTreeFact, PushState, ReclaimClass, ReservationFact, RootClass, Settlement,
    StorageDisposition, StorageWorkObservation, UnknownVariantRecord, VolumeIdentity,
};
pub use lifecycle::{
    CleanupReadiness, Dimension, DimensionEvidence, HostWorkClassification, HostWorkLifecycle,
    HostWorkObservationToken, HostWorkReason, classify_compute, classify_logical,
    classify_mutation, classify_storage, merge_classifications,
};
pub use status::{HostWorkStatus, StatusError};
pub use subject::{
    HOST_WORK_STATUS_SCHEMA_VERSION, HostWorkSubject, ObservationScope, ProviderFamily, ProviderId,
    WorktreeIdentity,
};

#[cfg(test)]
mod tests;
