//! Read-only per-file semantic query facade over one accepted AST and source.
//!
//! Every query this facade answers is derived solely from the `(root, source)`
//! pair supplied to [`SemanticQueryFacade::build`]. The facade performs no
//! cross-file lookup, no workspace-store access, no filesystem read, and no
//! parsing of its own.
//!
//! Cross-file and project-wide questions — visible imports, file dependencies,
//! workspace symbol search — are **not** answered here. They belong to the
//! project/live-workspace query owners above this layer; `WorkspaceIndex`
//! remains their canonical surface. See #7588 for the boundary ruling.

use std::ops::Range;

use crate::SourceLocation;
use crate::ast::Node;
use crate::pragma_tracker::{PragmaState, PragmaTracker};
use crate::symbol::{Symbol, SymbolKind};

use super::SemanticModel;

/// Stable read-only per-file semantic query surface for incremental consumer
/// adoption.
///
/// Answers are a pure function of the AST and source handed to
/// [`SemanticQueryFacade::build`]; constructing one requires no workspace
/// index, no project model, and no I/O.
#[derive(Debug)]
pub struct SemanticQueryFacade {
    model: SemanticModel,
    pragma_map: Vec<(Range<usize>, PragmaState)>,
}

/// Read-only symbol projection returned by semantic query lookups.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResolvedSymbol {
    /// Symbol name (without sigil for variables).
    pub name: String,
    /// Fully qualified symbol name when known.
    pub qualified_name: String,
    /// Symbol kind.
    pub kind: SymbolKind,
    /// Definition source location.
    pub location: SourceLocation,
    /// Variable declaration type (`my`, `our`, `local`, `state`) when known.
    pub declaration: Option<String>,
    /// Extracted POD or comment documentation when present.
    pub documentation: Option<String>,
}

/// Definition location that may include a workspace URI.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DefinitionLocation {
    /// File URI if known for this definition.
    pub uri: Option<String>,
    /// Byte-range location inside the source file.
    pub location: SourceLocation,
}

/// Ordered inheritance information for a class.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParentChain {
    /// Class/package name that owns the chain.
    pub class_name: String,
    /// Ancestors in method-resolution order.
    pub ancestors: Vec<String>,
}

/// Effective pragma state at a byte offset.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct EffectivePragmaState {
    /// Byte offset where state was requested.
    pub offset: usize,
    /// Effective tracked pragma state.
    pub state: PragmaState,
}

impl SemanticQueryFacade {
    /// Build a read-only query facade from parser output and source text.
    pub fn build(root: &Node, source: &str) -> Self {
        Self { model: SemanticModel::build(root, source), pragma_map: PragmaTracker::build(root) }
    }

    /// Access the underlying semantic model for incremental migration.
    pub fn semantic_model(&self) -> &SemanticModel {
        &self.model
    }

    /// Resolve a symbol definition at `position`.
    pub fn resolved_symbol_at(&self, position: usize) -> Option<ResolvedSymbol> {
        self.model.definition_at(position).map(ResolvedSymbol::from)
    }

    /// Resolve a definition location at `position`.
    pub fn definition_location_at(
        &self,
        position: usize,
        current_uri: Option<&str>,
    ) -> Option<DefinitionLocation> {
        self.model.definition_at(position).map(|symbol| DefinitionLocation {
            uri: current_uri.map(std::string::ToString::to_string),
            location: symbol.location,
        })
    }

    /// Return class parent chain in analyzer-configured resolution order.
    pub fn parent_chain(&self, class_name: &str) -> Option<ParentChain> {
        self.model
            .parent_chain(class_name)
            .map(|ancestors| ParentChain { class_name: class_name.to_string(), ancestors })
    }

    /// Resolve inherited method origin for a class and method name.
    pub fn inherited_origin(
        &self,
        class_name: &str,
        method_name: &str,
        current_uri: Option<&str>,
    ) -> Option<DefinitionLocation> {
        self.model.resolve_inherited_method_location(class_name, method_name).map(|location| {
            DefinitionLocation { uri: current_uri.map(std::string::ToString::to_string), location }
        })
    }

    /// Return effective tracked pragma state for `offset`.
    pub fn effective_pragma_state(&self, offset: usize) -> EffectivePragmaState {
        EffectivePragmaState {
            offset,
            state: PragmaTracker::state_for_offset(&self.pragma_map, offset),
        }
    }
}

impl From<&Symbol> for ResolvedSymbol {
    fn from(value: &Symbol) -> Self {
        Self {
            name: value.name.clone(),
            qualified_name: value.qualified_name.clone(),
            kind: value.kind,
            location: value.location,
            declaration: value.declaration.clone(),
            documentation: value.documentation.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use perl_tdd_support::{must, must_some};

    use super::*;
    use crate::parser::Parser;

    #[test]
    fn query_facade_resolves_symbol_and_pragmas() {
        let code = "use strict;\nmy $value = 1;\n$value;\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());

        let facade = SemanticQueryFacade::build(&ast, code);
        let usage_offset = must_some(code.rfind("$value"));

        let symbol = must_some(facade.resolved_symbol_at(usage_offset));
        assert_eq!(symbol.name, "value");

        let pragma_state = facade.effective_pragma_state(usage_offset);
        assert!(pragma_state.state.strict_vars);
    }

    #[test]
    fn query_facade_reads_parent_chain_from_the_file_alone() {
        // `use parent 'Base'` is an inheritance fact carried by this file's own
        // AST. Resolving it needs no workspace index — the previous revision of
        // this test built one only to exercise the retired `visible_imports`
        // wrapper, not because the parent chain required it (#7588).
        let code = "package Child;\nuse parent 'Base';\n1;\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());

        let facade = SemanticQueryFacade::build(&ast, code);

        let chain = must_some(facade.parent_chain("Child"));
        assert_eq!(chain.ancestors, vec!["Base"]);
    }

    #[test]
    fn query_facade_resolved_symbol_carries_declaration_and_docs() {
        // Verify that declaration and documentation fields are propagated
        // from Symbol through the From impl — not silently dropped.
        let code = "my $x = 1;\n$x;\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let facade = SemanticQueryFacade::build(&ast, code);

        let usage_offset = must_some(code.rfind("$x"));
        let symbol = must_some(facade.resolved_symbol_at(usage_offset));
        // `my $x` should produce declaration = Some("my")
        assert_eq!(
            symbol.declaration.as_deref(),
            Some("my"),
            "declaration field must be propagated from Symbol, not silently dropped"
        );
    }

    #[test]
    fn query_facade_resolved_symbol_at_past_end_returns_none() {
        // Out-of-range offset must return None gracefully — no panic.
        let code = "my $x = 1;\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let facade = SemanticQueryFacade::build(&ast, code);

        assert!(
            facade.resolved_symbol_at(usize::MAX).is_none(),
            "out-of-range offset must return None, not panic"
        );
    }

    #[test]
    fn query_facade_parent_chain_unknown_class_returns_none() {
        // A class that doesn't appear in class models must return None.
        let code = "package Foo; sub bar {} 1;\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let facade = SemanticQueryFacade::build(&ast, code);

        assert!(
            facade.parent_chain("NonExistentClass").is_none(),
            "unknown class must return None from parent_chain"
        );
    }

    #[test]
    fn query_facade_effective_pragma_state_no_pragmas() {
        // A file with no pragmas must return a default (all-false) pragma state.
        let code = "my $x = 1;\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let facade = SemanticQueryFacade::build(&ast, code);

        let state = facade.effective_pragma_state(0);
        assert!(!state.state.strict_vars, "strict_vars must be false when no use strict");
        assert!(!state.state.strict_subs, "strict_subs must be false when no use strict");
        assert!(!state.state.strict_refs, "strict_refs must be false when no use strict");
        assert!(!state.state.warnings, "warnings must be false when no use warnings");
    }

    #[test]
    fn query_facade_answers_every_query_from_ast_and_source_alone() {
        // Acceptance proof for #7588: the facade is constructible and fully
        // answerable with no workspace index, no project model, and no I/O.
        // If any query regains a workspace parameter this stops compiling.
        let code = "use strict;\npackage Child;\nuse parent 'Base';\nmy $value = 1;\n$value;\n1;\n";
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());

        // The AST and the source are the only inputs.
        let facade = SemanticQueryFacade::build(&ast, code);
        let usage_offset = must_some(code.rfind("$value"));

        let symbol = must_some(facade.resolved_symbol_at(usage_offset));
        assert_eq!(symbol.name, "value");

        let definition =
            must_some(facade.definition_location_at(usage_offset, Some("file:///test/child.pm")));
        assert_eq!(definition.uri.as_deref(), Some("file:///test/child.pm"));

        let chain = must_some(facade.parent_chain("Child"));
        assert_eq!(chain.ancestors, vec!["Base"]);

        assert!(facade.effective_pragma_state(usage_offset).state.strict_vars);

        // Exercised for reachability without a workspace: an unknown inherited
        // method resolves to None rather than consulting a cross-file store.
        assert!(facade.inherited_origin("Child", "method_defined_nowhere", None).is_none());

        assert!(facade.semantic_model().definition_at(usage_offset).is_some());
    }

    // ---------------------------------------------------------------------
    // Recurrence guard: the per-file boundary, checked against this module's
    // own production source rather than against prose describing it (#7588).
    // ---------------------------------------------------------------------

    /// This module's source, embedded at compile time so the guard itself
    /// performs no runtime filesystem read.
    const FACADE_SOURCE: &str = include_str!("query_facade.rs");

    /// Exact opening of this file's test module. The needles below are written
    /// in the test half, so the split keeps the scan pointed at production code.
    const TEST_MODULE_MARKER: &str = "#[cfg(test)]\nmod tests {";

    /// Constructs the per-file facade must not reach for, and why each is out
    /// of bounds.
    const FORBIDDEN_IN_PRODUCTION: &[(&str, &str)] = &[
        ("WorkspaceIndex", "cross-file workspace storage belongs above this facade"),
        ("workspace_index", "the facade must not import the workspace store module"),
        ("std::fs", "the facade answers from the supplied source, never the filesystem"),
        ("Parser::new", "the facade consumes an accepted AST; it must not parse"),
    ];

    /// Production source with comment lines removed, so the guard checks code
    /// and not the prose that documents the boundary.
    fn production_code(source: &str) -> String {
        let (production, _) = must_some(source.split_once(TEST_MODULE_MARKER));
        assert_eq!(
            source.matches(TEST_MODULE_MARKER).count(),
            1,
            "expected exactly one test-module marker so the production/test split is unambiguous; \
             re-derive this guard deliberately rather than letting it scan a truncated region"
        );
        assert!(
            !production.contains("/*"),
            "this guard strips line comments only; a block comment appeared in production code, so \
             re-derive the stripping rule rather than letting prose reach the needle scan"
        );
        production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_scanned_region_is_production_code_with_prose_removed() {
        // Guards the guard. An empty, truncated, or un-stripped region would
        // make the boundary check below pass for the wrong reason.
        let code = production_code(FACADE_SOURCE);

        assert!(
            code.contains("pub fn resolved_symbol_at"),
            "scanned region must contain the facade's real queries"
        );
        assert!(
            code.contains("pub fn effective_pragma_state"),
            "scanned region must reach the last query in the impl block"
        );
        assert!(
            !code.contains("fn production_code"),
            "scanned region must exclude this test module"
        );
        // Stripping is proven by the absence of any comment marker rather than
        // by matching particular prose, so rewording the module doc that names
        // the retired boundary cannot fail this test spuriously.
        assert!(
            !code.contains("//"),
            "comment stripping must remove every line comment, otherwise the needle scan would be \
             checking the prose that documents the boundary instead of the code"
        );
    }

    #[test]
    fn facade_production_code_reaches_no_workspace_filesystem_or_parser_construct() {
        let code = production_code(FACADE_SOURCE);

        for (needle, reason) in FORBIDDEN_IN_PRODUCTION {
            assert!(
                !code.contains(needle),
                "per-file query facade must not reference `{needle}`: {reason} (#7588)"
            );
        }
    }

    #[test]
    fn the_boundary_guard_rejects_a_reintroduced_workspace_dependency() {
        // Negative control: the needle set must fail a facade that takes the
        // workspace index back, rather than passing for any input at all.
        let reintroduced = concat!(
            "use crate::workspace_index::WorkspaceIndex;\n",
            "pub fn visible_imports(&self, workspace: &WorkspaceIndex) -> Vec<String> {}\n"
        );

        let offenders: Vec<&str> = FORBIDDEN_IN_PRODUCTION
            .iter()
            .filter(|(needle, _)| reintroduced.contains(needle))
            .map(|(needle, _)| *needle)
            .collect();

        assert_eq!(
            offenders,
            vec!["WorkspaceIndex", "workspace_index"],
            "the guard must name exactly the reintroduced workspace dependency"
        );
    }

    #[test]
    fn the_boundary_guard_accepts_a_purely_per_file_facade() {
        // Negative control's counterpart: the needle set must not fire on
        // legitimate per-file code, or it would block the intended design.
        let per_file = concat!(
            "pub fn resolved_symbol_at(&self, position: usize) -> Option<ResolvedSymbol> {\n",
            "    self.model.definition_at(position).map(ResolvedSymbol::from)\n",
            "}\n"
        );

        for (needle, _) in FORBIDDEN_IN_PRODUCTION {
            assert!(!per_file.contains(needle), "per-file code must not trip the `{needle}` guard");
        }
    }
}
