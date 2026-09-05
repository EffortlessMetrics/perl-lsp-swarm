//! Structured mutation contracts (#11327 train, S0).
//!
//! Domain types and validation only: no DAP wire parsing, no Perl rendering,
//! no debugger I/O.

mod structured_value;

pub use structured_value::{
    ExactDecimal, FreshReferentKind, MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION,
    MutationStructuredValueV1, STRUCTURED_PREFIX, StructuredMutationLimits, StructuredRefusal,
    StructuredValue, fresh_referent_kind, parse_structured_mutation, structured_payload,
};
