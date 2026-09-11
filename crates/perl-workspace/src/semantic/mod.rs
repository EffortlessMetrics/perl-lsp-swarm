//! Semantic substrate module tree for workspace-level semantic analysis.
//!
//! This module groups the canonical semantic population, indexing, and query
//! infrastructure that replaces per-provider semantic approximations with a
//! single layered substrate.
//!
//! # Submodules
//!
//! - [`facts`] — canonical `FileFactShard` population from semantic fact producers.

/// Canonical `FileFactShard` population from semantic fact producers.
pub mod facts;

/// Typed reference index for cross-file reference lookups.
pub mod references;

/// Import/export index for cross-file import and export lookups.
pub mod imports;

/// Scorecard aggregation for semantic shadow-compare receipts.
pub mod scorecard;

/// Visibility resolution for symbols at a given query point.
pub mod visibility;

/// Package graph index for cross-file inheritance and role-composition lookups.
pub mod package_graph;

/// Value-shape index mapping entity IDs to lightweight type approximations.
pub mod value_shape;

/// Per-category semantic fact invalidation planning.
pub mod invalidation;

/// Semantic query facade: `SemanticQueries` trait and `WorkspaceSemanticQueries` impl.
pub mod queries;

/// Literal-eval sub extractor for dynamic boundary evidence.
pub mod eval_sub_extractor;

#[path = "generated_member_extractor.rs"]
mod generated_member_extractor_core;

#[allow(unreachable_pub)]
#[path = "workspace_import_extractor.rs"]
mod workspace_import_extractor_core;

mod quickorm;

/// Framework-generated member extraction for package-level declarations.
pub mod generated_member_extractor {
    use crate::Node;
    use perl_semantic_facts::FileId;

    pub(crate) use super::generated_member_extractor_core::GeneratedMemberFact;

    /// Extract generated-member facts from the canonical framework producers.
    pub(crate) fn extract_generated_member_facts(
        ast: &Node,
        file_id: FileId,
    ) -> Vec<GeneratedMemberFact> {
        let mut facts =
            super::generated_member_extractor_core::extract_generated_member_facts(ast, file_id);
        facts.extend(super::quickorm::extract_generated_member_facts(ast, file_id));
        facts
    }

    /// Extract generated members with the source text available to adapters
    /// whose parser representation intentionally omits separators.
    pub(crate) fn extract_generated_member_facts_with_source(
        ast: &Node,
        file_id: FileId,
        source: &str,
    ) -> Vec<GeneratedMemberFact> {
        let mut facts = extract_generated_member_facts(ast, file_id);
        facts.extend(super::quickorm::extract_generated_member_facts_with_source(
            ast, file_id, source,
        ));
        facts
    }
}

/// Import-spec extraction for `ImportExportIndex` population during `index_file`.
pub mod workspace_import_extractor {
    use crate::Node;
    use perl_semantic_facts::{FileId, ImportSpec};

    pub use super::workspace_import_extractor_core::extract_use_lib_facts;

    /// Extract import facts and apply bounded framework-specific import semantics.
    pub fn extract_import_specs(ast: &Node, file_id: FileId) -> Vec<ImportSpec> {
        let mut specs = super::workspace_import_extractor_core::extract_import_specs(ast, file_id);
        super::quickorm::normalize_import_specs(ast, &mut specs);
        specs
    }

    /// Extract import facts with source text available for exact framework
    /// syntax checks that the normalized AST cannot express.
    pub fn extract_import_specs_with_source(
        ast: &Node,
        file_id: FileId,
        source: &str,
    ) -> Vec<ImportSpec> {
        let mut specs = super::workspace_import_extractor_core::extract_import_specs(ast, file_id);
        super::quickorm::normalize_import_specs_with_source(ast, &mut specs, source);
        specs
    }
}

/// Non-published DBIx::QuickORM table-column field candidates.
///
/// This module has no canonical-shard or provider consumer. Promotion requires
/// a separate admission receipt and must not reinterpret columns as generated
/// methods.
#[path = "generated_member_extractor_quickorm.rs"]
pub(crate) mod dbix_quickorm_candidate;

/// Per-provider scorecard gate fixture suites (test-only).
#[cfg(test)]
mod scorecard_gate_fixtures;
