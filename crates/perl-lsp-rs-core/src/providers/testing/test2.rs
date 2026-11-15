//! Compatibility re-exports for the shared Test2 import/pragma analysis.
//!
//! The single implementation lives in semantic analysis (#19457/#7410).
//! Canonical framework/ProjectModel migration remains with #4907/#7661.

pub use perl_semantic_analyzer::{
    ResolvedImport, Test2Facts, Test2Pragmas, is_test2_bundle, is_test2_module,
    module_default_exports, resolve_import,
};

#[cfg(test)]
mod tests {
    use super::Test2Facts;

    #[test]
    fn compatibility_path_preserves_the_lower_type() {
        let lower = perl_semantic_analyzer::Test2Facts::from_source("use Test2::V0 -no_warnings;");
        let compatibility: Test2Facts = lower;
        assert!(compatibility.provides_pragma("strict"));
        assert!(!compatibility.provides_pragma("warnings"));
    }
}
