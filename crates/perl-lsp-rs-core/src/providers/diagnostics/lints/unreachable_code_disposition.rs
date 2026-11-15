//! Compatibility exports for the semantic analyzer's shared flow disposition registry.
//!
//! The names and registry rows remain unchanged; there is one lower owner.

pub use perl_semantic_analyzer::{
    PL406_DISPOSITION_SCHEMA_VERSION, PL406_DISPOSITIONS, Pl406ConditionalRule, Pl406Disposition,
    Pl406ProofCeiling, Pl406Scope, Pl406SemanticClass, all_pl406_dispositions, pl406_disposition,
    pl406_disposition_of,
};
