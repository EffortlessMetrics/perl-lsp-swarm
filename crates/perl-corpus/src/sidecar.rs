//! Canonical versioned fixture-expectation sidecar model and root-bound validation.
//!
//! Filesystem authority is always carried by [`SidecarValidationContext`].
//! Sidecar and fixture identities are root-relative, and every runtime binding
//! rechecks containment, symlink components, population membership, and regular
//! file type before source is read or treated as corpus evidence.

mod context;
mod model;
mod parse;

pub use context::{
    SidecarPairIdentity, SidecarValidationContext, ValidatedSidecarPair,
};
pub use model::{
    ConceptRegistry, ExpectationMode, FixtureExpectationSidecar, FixtureExpectationV1,
    SidecarConcept, SidecarExpect, SidecarMetrics, SidecarSnapshots, SidecarValidation,
    FIXTURE_EXPECTATION_SCHEMA,
};
pub use parse::{
    load_and_validate_sidecar, parse_sidecar, parse_sidecar_str, validate_sidecar,
};
