//! Compatibility exports for the shared Test2 source analysis.
//!
//! The existing reader is owned by `perl-semantic-analyzer`; critic and
//! completion consumers use that owner directly. These paths retain the same
//! public type and function identities for existing callers. The registered
//! FrameworkAdapter/ProjectModel migrations (#4907, #6946, #6948, #7661, #6981)
//! retain ownership of the compatibility reader's eventual replacement.

pub use perl_semantic_analyzer::{
    Test2Facts, Test2Pragmas, Test2ResolvedImport as ResolvedImport, is_test2_bundle,
    is_test2_module, resolve_test2_import as resolve_import,
    test2_module_default_exports as module_default_exports,
};
