//! Compatibility path for the shared local-flow disposition registry.
//!
//! The semantic analyzer owns the registry and the one recursive flow walk.

pub use perl_semantic_analyzer::{
    PL406_DISPOSITION_SCHEMA_VERSION, PL406_DISPOSITIONS, Pl406ConditionalRule, Pl406Disposition,
    Pl406ProofCeiling, Pl406Scope, Pl406SemanticClass, all_pl406_dispositions, pl406_disposition,
    pl406_disposition_of,
};
