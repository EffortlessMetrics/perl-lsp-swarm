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

    /// Drop each line's comment, keeping any code that precedes it. Trailing
    /// comments matter as much as whole-line ones: the module doc names the
    /// retired constructs to say where they belong, and a trailing `// uses
    /// WorkspaceIndex` would otherwise reach the needle scan as if it were code.
    fn strip_line_comments(source: &str) -> String {
        source
            .lines()
            .map(|line| match line.find("//") {
                Some(index) => &line[..index],
                None => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// This file's production half, with comments stripped, so the guard checks
    /// code and not the prose that documents the boundary.
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
        let hazards = literal_comment_hazards(production);
        assert!(
            hazards.is_empty(),
            "these lines mix a string literal with `//`, which this guard cannot split correctly: \
             {hazards:?}. Re-derive the stripping rule rather than letting it cut code out of the \
             scanned region"
        );
        strip_line_comments(production)
    }

    /// Lines `strip_line_comments` could mis-cut: a `//` inside a string literal
    /// makes it remove *code* rather than prose, which is the direction that
    /// could hide a real violation.
    ///
    /// Whole-line comments are exempt. They contain `//` by definition and are
    /// removed in full, so a doc comment that happens to quote something with an
    /// ASCII `"` carries no hazard — flagging those would fail the guard on
    /// ordinary prose while catching nothing.
    fn literal_comment_hazards(production: &str) -> Vec<&str> {
        production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| line.contains('"') && line.contains("//"))
            .collect()
    }

    #[test]
    fn a_doc_comment_quoting_prose_is_not_a_stripping_hazard() {
        // The fail-closed rule must not fire on ordinary documentation. Every
        // `///` and `//!` line contains `//`, so a rule applied before comment
        // stripping would reject any doc comment using an ASCII quote.
        let documented = concat!(
            "//! Module doc explaining why \"visible imports\" moved above this layer.\n",
            "/// Returns the \"resolved\" symbol, or None.\n",
            "pub fn resolved_symbol_at(&self, position: usize) -> Option<ResolvedSymbol> {\n",
            "    self.model.definition_at(position).map(ResolvedSymbol::from)\n",
            "}\n"
        );

        assert_eq!(
            literal_comment_hazards(documented),
            Vec::<&str>::new(),
            "documentation quoting prose is stripped in full and cannot mis-cut code"
        );
    }

    #[test]
    fn a_code_line_holding_a_url_literal_is_a_stripping_hazard() {
        // The direction that matters: `//` inside a literal on a code line would
        // make the stripper cut real code out of the scanned region.
        let hazardous = concat!(
            "pub fn spec_link() -> &'static str {\n",
            "    \"https://example.invalid/spec\"\n",
            "}\n"
        );

        assert_eq!(
            literal_comment_hazards(hazardous),
            vec!["    \"https://example.invalid/spec\""],
            "a literal `//` on a code line must fail closed, naming the line"
        );
    }

    #[test]
    fn comment_stripping_removes_prose_and_keeps_code() {
        // Owns the stripping proof, on controlled input. Asserting instead that
        // the real file's stripped text contains no `//` at all would forbid
        // any future trailing comment or `//` inside a production string
        // literal, and would be tautological once stripping cuts to end of line.
        let source = concat!(
            "//! Module doc naming WorkspaceIndex as the canonical cross-file owner.\n",
            "/// Doc comment mentioning std::fs and workspace_index.\n",
            "pub fn keep_me() -> usize { 1 } // trailing comment naming Parser::new\n",
            "pub fn also_keep() -> usize { 2 }\n",
        );

        let stripped = strip_line_comments(source);

        assert!(
            stripped.contains("pub fn keep_me"),
            "code preceding a trailing comment must survive stripping"
        );
        assert!(stripped.contains("pub fn also_keep"), "uncommented code must survive stripping");
        for (needle, _) in FORBIDDEN_IN_PRODUCTION {
            assert!(
                !stripped.contains(needle),
                "`{needle}` written only in prose must not survive stripping, or the boundary \
                 guard would be scanning comments instead of code"
            );
        }
    }

    #[test]
    fn the_scanned_region_is_the_production_half_of_this_file() {
        // Guards the guard. An empty or truncated region would make the
        // boundary check below pass for the wrong reason.
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
    }

    /// Every forbidden construct a synthetic file's production half reaches,
    /// routed through the same split-and-strip pipeline the real guard uses.
    /// The controls below run this rather than matching needles against a bare
    /// string, so a broken split or stripper fails them too.
    fn offenders_in(source: &str) -> Vec<&'static str> {
        let code = production_code(source);
        FORBIDDEN_IN_PRODUCTION
            .iter()
            .filter(|(needle, _)| code.contains(needle))
            .map(|(needle, _)| *needle)
            .collect()
    }

    /// Wrap a synthetic production half in a test module so `production_code`
    /// sees the same shape it sees in the real file. The marker is assembled at
    /// runtime so this file still contains exactly one literal marker.
    fn as_source_file(production: &str) -> String {
        format!("{production}\n{TEST_MODULE_MARKER}\n    // test half, never scanned\n}}\n")
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
        // Negative control: a facade that takes the workspace index back must
        // fail, rather than the guard passing for any input at all.
        let reintroduced = as_source_file(concat!(
            "use crate::workspace_index::WorkspaceIndex;\n",
            "pub fn visible_imports(&self, workspace: &WorkspaceIndex) -> Vec<String> {}\n"
        ));

        assert_eq!(
            offenders_in(&reintroduced),
            vec!["WorkspaceIndex", "workspace_index"],
            "the guard must name exactly the reintroduced workspace dependency"
        );
    }

    #[test]
    fn the_boundary_guard_accepts_a_purely_per_file_facade() {
        // The opposite direction: legitimate per-file code must not fire, or
        // the guard would block the intended design. The prose here names every
        // forbidden construct, so this also pins that a comment cannot trip it.
        let per_file = as_source_file(concat!(
            "/// Resolves from the model alone; WorkspaceIndex, workspace_index,\n",
            "/// std::fs and Parser::new all belong elsewhere.\n",
            "pub fn resolved_symbol_at(&self, position: usize) -> Option<ResolvedSymbol> {\n",
            "    self.model.definition_at(position).map(ResolvedSymbol::from) // not Parser::new\n",
            "}\n"
        ));

        assert_eq!(
            offenders_in(&per_file),
            Vec::<&str>::new(),
            "per-file code must not trip the guard, and prose naming the constructs must not either"
        );
    }
}
