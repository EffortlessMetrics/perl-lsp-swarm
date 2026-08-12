//! Canonical versioned fixture-expectation sidecar model and root-bound validation.
//!
//! Filesystem authority is always carried by [`SidecarValidationContext`].
//! Sidecar and fixture identities are root-relative and content-bound; every
//! runtime binding rechecks containment, handles, digests, schema, topology,
//! population membership, and regular file type before retained bytes become
//! corpus evidence.

mod context;
mod model;
mod parse;

pub use context::{
    SidecarPairIdentity, SidecarValidationContext, ValidatedSidecarPair,
};
pub use model::{
    ConceptRegistry, ExpectationMode, FIXTURE_EXPECTATION_SCHEMA, FixtureExpectationSidecar,
    FixtureExpectationV1, SidecarConcept, SidecarExpect, SidecarMetrics, SidecarSnapshots,
    SidecarValidation,
};
pub use parse::{
    load_and_validate_sidecar, parse_sidecar, parse_sidecar_str, parse_validated_sidecar,
    validate_sidecar, validate_validated_sidecar,
};
