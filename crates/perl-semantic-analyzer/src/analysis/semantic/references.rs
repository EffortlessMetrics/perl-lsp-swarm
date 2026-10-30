//! Reference resolution and symbol visibility for Navigate/Analyze workflows.

use crate::SourceLocation;
use crate::symbol::{ScopeId, Symbol, SymbolKind};

use super::SemanticAnalyzer;

impl SemanticAnalyzer {
    /// Resolve a reference to its symbol definitions, handling cross-package lookups.
    pub(crate) fn resolve_reference_to_symbols(
        &self,
        reference: &crate::symbol::SymbolReference,
    ) -> Vec<&Symbol> {
        // Handle qualified names like Foo::bar
        if let Some((pkg, name)) = reference.name.rsplit_once("::") {
            if let Some(pkg_syms) = self.symbol_table.symbols.get(pkg) {
                let mut results = Vec::new();
                for sym in pkg_syms {
                    if sym.kind == SymbolKind::Package {
                        // Find the scope associated with this package symbol
                        let pkg_scope = self
                            .symbol_table
                            .scopes
                            .values()
                            .find(|s| {
                                s.kind == crate::symbol::ScopeKind::Package
                                    && s.location.start == sym.location.start
                                    && s.location.end == sym.location.end
                            })
                            .map(|s| s.id)
                            .unwrap_or(sym.scope_id);
                        // Symbols may live in an inner block scope
                        let search_scope = self
                            .symbol_table
                            .scopes
                            .values()
                            .find(|s| s.parent == Some(pkg_scope))
                            .map(|s| s.id)
                            .unwrap_or(pkg_scope);
                        results.extend(self.symbol_table.find_symbol(
                            name,
                            search_scope,
                            reference.kind,
                        ));
                    }
                }
                results
            } else {
                self.symbol_table.find_symbol(name, reference.scope_id, reference.kind)
            }
        } else {
            self.symbol_table.find_symbol(&reference.name, reference.scope_id, reference.kind)
        }
    }

    /// Whether a use precedes a lexical candidate's declaration token, so
    /// it cannot bind to that candidate at all (#18115). A use inside the
    /// declaring statement (an initializer RHS) is decided by the scope
    /// walk instead: it binds to an outer declaration when one exists.
    fn not_yet_effective(
        &self,
        candidate: &Symbol,
        reference: &crate::symbol::SymbolReference,
    ) -> bool {
        matches!(candidate.declaration.as_deref(), Some("my" | "state"))
            && reference.location.start < candidate.location.start
    }

    /// Whether a use sits inside a lexical candidate's own declaring
    /// statement (an initializer RHS), so it cannot bind to that candidate
    /// either: it binds to an outer declaration or the package global.
    fn inside_declaring_statement(
        &self,
        candidate: &Symbol,
        reference: &crate::symbol::SymbolReference,
    ) -> bool {
        if !matches!(candidate.declaration.as_deref(), Some("my" | "state")) {
            return false;
        }
        let statement_end = self
            .symbol_table
            .declaration_statement_ends
            .get(&candidate.location.start)
            .copied()
            .unwrap_or(candidate.location.end);
        candidate.location.start <= reference.location.start
            && reference.location.start < statement_end
    }

    /// The declaration a use actually binds to: the first candidate the
    /// reference is visible from and not shadowed away from (#18115).
    pub(super) fn bound_symbol<'a>(
        &'a self,
        symbols: &[&'a Symbol],
        reference: &crate::symbol::SymbolReference,
    ) -> Option<&'a Symbol> {
        symbols
            .iter()
            .copied()
            .find(|candidate| {
                self.is_symbol_visible(candidate, reference.scope_id)
                    && !self.is_shadowed_by_inner_declaration(candidate, reference)
            })
            .or_else(|| {
                // Never resurrect a lexical the use textually precedes or
                // whose own declaring statement contains the use (an
                // initializer RHS): it binds outward or to the package
                // global instead.
                symbols.iter().copied().find(|candidate| {
                    !self.not_yet_effective(candidate, reference)
                        && !self.inside_declaring_statement(candidate, reference)
                })
            })
    }

    /// Find all references to a symbol at a given position for Navigate/Analyze workflows.
    pub fn find_all_references(
        &self,
        position: usize,
        include_declaration: bool,
    ) -> Vec<SourceLocation> {
        // First find the symbol at this position (either definition or reference)
        let symbol = if let Some(def) = self.find_definition(position) {
            Some(def)
        } else {
            // Check if we're on a reference
            for refs in self.symbol_table.references.values() {
                for reference in refs {
                    if reference.location.start <= position && reference.location.end >= position {
                        // Found a reference, get its definition to get the symbol ID
                        let symbols = self.symbol_table.find_symbol(
                            &reference.name,
                            reference.scope_id,
                            reference.kind,
                        );
                        if let Some(symbol) = self.bound_symbol(&symbols, reference) {
                            return self
                                .find_all_references_for_symbol(symbol, include_declaration);
                        }
                    }
                }
            }
            None
        };

        if let Some(symbol) = symbol {
            return self.find_all_references_for_symbol(symbol, include_declaration);
        }

        Vec::new()
    }

    /// Find all references for a specific symbol.
    pub(super) fn find_all_references_for_symbol(
        &self,
        symbol: &Symbol,
        include_declaration: bool,
    ) -> Vec<SourceLocation> {
        let mut locations = Vec::new();

        // Include the declaration if requested
        if include_declaration {
            locations.push(symbol.location);
        }

        // Find all references to this symbol by name
        if let Some(refs) = self.symbol_table.references.get(&symbol.name) {
            for reference in refs {
                // Only include references of the same kind and in scope where the symbol is visible
                if reference.kind == symbol.kind {
                    // Check if the symbol is visible from this reference's scope
                    if self.is_symbol_visible(symbol, reference.scope_id)
                        && !self.is_shadowed_by_inner_declaration(symbol, reference)
                    {
                        locations.push(reference.location);
                    }
                }
            }
        }

        locations
    }

    /// Check whether a reference that is textually inside `symbol`'s
    /// visibility region actually binds to a different declaration of the
    /// same name and kind (#18115).
    ///
    /// Walks the scope chain from the reference's scope outward. In each
    /// scope, the latest capturing declaration whose statement has ended
    /// at or before the reference owns the binding; the reference is
    /// shadowed when that owner is not `symbol`. A lexical's scope begins
    /// only after its declaring statement, so the initializer in
    /// `my $x = $x` still binds outward. `my`/`state` declarations always
    /// capture; `our` aliases the package variable, so it captures only
    /// when the queried symbol is itself lexical; `local` dynamically
    /// rebinds the same slot and never captures.
    pub(super) fn is_shadowed_by_inner_declaration(
        &self,
        symbol: &Symbol,
        reference: &crate::symbol::SymbolReference,
    ) -> bool {
        let Some(candidates) = self.symbol_table.symbols.get(&symbol.name) else {
            return false;
        };
        let symbol_is_lexical = matches!(symbol.declaration.as_deref(), Some("my" | "state"));
        // A use that textually precedes the lexical's declaration token
        // cannot bind to it: it binds to an outer declaration or the
        // package global (#18115).
        if symbol_is_lexical && self.not_yet_effective(symbol, reference) {
            return true;
        }
        let mut current_scope = reference.scope_id;
        // True when the reference sits inside a lexical's declaring
        // statement on the chain (its initializer): with no outer binding
        // it binds to the package global, never to that lexical.
        let mut in_declaring_statement = false;
        loop {
            in_declaring_statement |= candidates.iter().any(|candidate| {
                candidate.kind == symbol.kind
                    && candidate.scope_id == current_scope
                    && matches!(candidate.declaration.as_deref(), Some("my" | "state"))
                    && candidate.location.start <= reference.location.start
                    && reference.location.start
                        < self
                            .symbol_table
                            .declaration_statement_ends
                            .get(&candidate.location.start)
                            .copied()
                            .unwrap_or(candidate.location.end)
            });
            let binding = candidates
                .iter()
                .filter(|candidate| {
                    candidate.kind == symbol.kind
                        && candidate.scope_id == current_scope
                        && match candidate.declaration.as_deref() {
                            Some("my" | "state") => true,
                            // `our` captures a lexical query (it rebinds the
                            // name to the package variable), and it always
                            // owns uses when it is the queried declaration
                            // itself: an outer `my` does not bind through it.
                            Some("our") => {
                                symbol_is_lexical
                                    || candidate.location.start == symbol.location.start
                                    // An `our` of the same package variable
                                    // aliases the queried `our` itself: it
                                    // is the same owner, not a shadow.
                                    || (symbol.declaration.as_deref() == Some("our")
                                        && candidate.qualified_name == symbol.qualified_name)
                            }
                            _ => false,
                        }
                })
                .filter(|candidate| {
                    let statement_end = self
                        .symbol_table
                        .declaration_statement_ends
                        .get(&candidate.location.start)
                        .copied()
                        .unwrap_or(candidate.location.end);
                    reference.location.start >= statement_end
                })
                .max_by_key(|candidate| candidate.location.start);
            if let Some(binding) = binding {
                // An inner `our` aliasing the same package variable is the
                // queried symbol's own binding, never its shadow.
                let same_our_alias = symbol.declaration.as_deref() == Some("our")
                    && binding.declaration.as_deref() == Some("our")
                    && binding.qualified_name == symbol.qualified_name;
                return !same_our_alias
                    && binding.location.start != symbol.location.start;
            }
            // No capturing declaration has taken effect in this scope yet
            // (the reference precedes the end of its declaring statement,
            // as an initializer RHS does): the binding lives further out,
            // so keep walking instead of crediting `symbol`.
            let Some(scope) = self.symbol_table.scopes.get(&current_scope) else {
                return in_declaring_statement && symbol_is_lexical;
            };
            let Some(parent) = scope.parent else {
                return in_declaring_statement && symbol_is_lexical;
            };
            current_scope = parent;
        }
    }

    /// Check if a symbol is visible from a given scope.
    pub(super) fn is_symbol_visible(&self, symbol: &Symbol, scope_id: ScopeId) -> bool {
        // For now, simple visibility check:
        // - Symbols in the same scope are visible
        // - Symbols in parent scopes are visible
        // - Package-level symbols are visible from package scopes

        if symbol.scope_id == scope_id {
            return true;
        }

        // Check if scope_id is a descendant of symbol.scope_id
        let mut current_scope = scope_id;
        while let Some(scope) = self.symbol_table.scopes.get(&current_scope) {
            if scope.parent == Some(symbol.scope_id) {
                return true;
            }
            if let Some(parent) = scope.parent {
                current_scope = parent;
            } else {
                break;
            }
        }

        // For package-level symbols (scope_id 0), always visible
        symbol.scope_id == 0
    }
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test setup: literal sources must parse and contain the named token"
)]
mod tests {
    use super::super::SemanticAnalyzer;
    use crate::parser::Parser;

    fn analyze(code: &str) -> SemanticAnalyzer {
        let mut parser = Parser::new(code);
        let ast = parser.parse().unwrap_or_else(|e| panic!("parse failed: {e}"));
        SemanticAnalyzer::analyze_with_source(&ast, code)
    }

    #[test]
    fn find_all_references_excludes_shadowed_inner_uses() {
        let code = "my $x = 1;\nprint $x;\n{ my $x = 2; print $x; }\nprint $x;\n";
        let analyzer = analyze(code);
        let outer_decl = code.find("$x").unwrap_or_else(|| panic!("outer $x"));
        let outer_refs = analyzer.find_all_references(outer_decl, false);
        assert_eq!(outer_refs.len(), 2, "outer uses only; got {outer_refs:?}");
        let inner_decl = code.find("my $x = 2").unwrap_or_else(|| panic!("inner")) + 3;
        let inner_refs = analyzer.find_all_references(inner_decl, false);
        assert_eq!(inner_refs.len(), 1, "inner use only; got {inner_refs:?}");
    }

    #[test]
    fn find_all_references_includes_initializer_rhs_of_shadowing_decl() {
        let code = "my $x = 1;\n{ my $x = $x; print $x; }\n";
        let analyzer = analyze(code);
        let outer_decl = code.find("$x").unwrap_or_else(|| panic!("outer $x"));
        let outer_refs = analyzer.find_all_references(outer_decl, false);
        assert_eq!(outer_refs.len(), 1, "only RHS use; got {outer_refs:?}");
        let rhs = code.find("= $x").unwrap_or_else(|| panic!("rhs")) + 2;
        assert!(outer_refs.iter().any(|loc| loc.start == rhs));
    }

    #[test]
    fn find_all_references_respects_same_scope_redeclaration() {
        let code = "my $x = 1;\nprint $x;\nmy $x = 2;\nprint $x;\n";
        let analyzer = analyze(code);
        let first_decl = code.find("$x").unwrap_or_else(|| panic!("first"));
        assert_eq!(analyzer.find_all_references(first_decl, false).len(), 1);
        let second_decl = code.rfind("my $x").unwrap_or_else(|| panic!("second")) + 3;
        assert_eq!(analyzer.find_all_references(second_decl, false).len(), 1);
    }

    #[test]
    fn find_all_references_from_use_binds_owning_declaration() {
        let code = "my $x = 1;\nprint $x;\nmy $x = 2;\nprint $x;\n";
        let analyzer = analyze(code);
        let second_use = code.rfind("print $x").unwrap_or_else(|| panic!("use")) + 6;
        let refs = analyzer.find_all_references(second_use, false);
        assert_eq!(refs.len(), 1, "second declaration only; got {refs:?}");
        assert!(refs.iter().any(|loc| loc.start == second_use));
    }

    #[test]
    fn find_all_references_initializer_without_outer_binds_global() {
        // No outer lexical: the RHS of `my $x = $x` is the package global,
        // so the inner declaration owns only the later use.
        let code = "{ my $x = $x; print $x; }";
        let analyzer = analyze(code);
        let inner_decl = code.find("my $x").unwrap_or_else(|| panic!("decl")) + 3;
        let inner_refs = analyzer.find_all_references(inner_decl, false);
        assert_eq!(inner_refs.len(), 1, "only print use; got {inner_refs:?}");
        let print_use = code.rfind("$x").unwrap_or_else(|| panic!("print use"));
        assert!(inner_refs.iter().any(|loc| loc.start == print_use));
    }

    #[test]
    fn find_all_references_from_initializer_rhs_without_outer_does_not_bind_inner() {
        // Querying the RHS itself: with no outer lexical it binds to the
        // package global, so it must not resolve to the inner declaration
        // or collect that declaration's later use.
        let code = "{ my $x = $x; print $x; }";
        let analyzer = analyze(code);
        let rhs = code.find("= $x").unwrap_or_else(|| panic!("rhs")) + 2;
        let refs = analyzer.find_all_references(rhs, false);
        let print_use = code.rfind("$x").unwrap_or_else(|| panic!("print use"));
        assert!(
            !refs.iter().any(|loc| loc.start == print_use),
            "RHS must not bind to the inner lexical; got {refs:?}"
        );
        assert!(refs.is_empty(), "RHS binds the package global; got {refs:?}");
    }

    #[test]
    fn find_all_references_from_initializer_rhs_binds_outer() {
        let code = "my $x = 1;\n{ my $x = $x; print $x; }\n";
        let analyzer = analyze(code);
        let rhs = code.find("= $x").unwrap_or_else(|| panic!("rhs")) + 2;
        let refs = analyzer.find_all_references(rhs, false);
        assert_eq!(refs.len(), 1, "outer RHS only; got {refs:?}");
        assert!(refs.iter().any(|loc| loc.start == rhs));
    }

    #[test]
    fn find_all_references_our_declaration_owns_its_uses() {
        // An inner `our $x` rebinds the name to the package variable: its
        // uses are not the outer lexical's, and they are the `our`'s.
        let code = "my $x = 1;\n{ our $x; print $x; }\nprint $x;\n";
        let analyzer = analyze(code);
        let our_decl = code.find("our $x").unwrap_or_else(|| panic!("our decl")) + 4;
        let our_refs = analyzer.find_all_references(our_decl, false);
        assert_eq!(our_refs.len(), 1, "only block use; got {our_refs:?}");
        let outer_decl = code.find("$x").unwrap_or_else(|| panic!("outer"));
        let outer_refs = analyzer.find_all_references(outer_decl, false);
        assert_eq!(outer_refs.len(), 1, "only final use; got {outer_refs:?}");
    }

    #[test]
    fn find_definition_from_initializer_rhs_binds_outer_or_none() {
        // T2: find_definition itself (not only find_all_references) must
        // answer the outward binding for an initializer RHS.
        let code = "my $x = 1;\n{ my $x = $x; print $x; }\n";
        let analyzer = analyze(code);
        let rhs = code.find("= $x").unwrap_or_else(|| panic!("rhs")) + 2;
        let outer_decl = code.find("$x").unwrap_or_else(|| panic!("outer"));
        let definition = analyzer.find_definition(rhs);
        assert_eq!(
            definition.map(|symbol| symbol.location.start),
            Some(outer_decl),
            "RHS must define to the outer declaration"
        );

        let code = "{ my $x = $x; print $x; }";
        let analyzer = analyze(code);
        let rhs = code.find("= $x").unwrap_or_else(|| panic!("rhs")) + 2;
        assert!(
            analyzer.find_definition(rhs).is_none(),
            "RHS with no outer lexical binds the package global, not a symbol"
        );
    }

    #[test]
    fn find_all_references_from_rhs_with_declaration_flag() {
        // T1: include_declaration=true queried from the RHS position.
        let code = "my $x = 1;\n{ my $x = $x; print $x; }\n";
        let analyzer = analyze(code);
        let rhs = code.find("= $x").unwrap_or_else(|| panic!("rhs")) + 2;
        let refs = analyzer.find_all_references(rhs, true);
        assert_eq!(refs.len(), 2, "outer declaration plus RHS; got {refs:?}");
        let outer_decl = code.find("$x").unwrap_or_else(|| panic!("outer"));
        assert!(refs.iter().any(|loc| loc.start == outer_decl));
        assert!(refs.iter().any(|loc| loc.start == rhs));

        let code = "{ my $x = $x; print $x; }";
        let analyzer = analyze(code);
        let rhs = code.find("= $x").unwrap_or_else(|| panic!("rhs")) + 2;
        assert!(analyzer.find_all_references(rhs, true).is_empty());
    }

    #[test]
    fn find_definition_rhs_inside_named_sub_does_not_fall_back_to_the_sub() {
        // T6: the RHS binds the package global; the enclosing sub (whose
        // symbol spans the whole node) is not its definition.
        let code = "sub run { my $x = $x; print $x; }\nrun();\n";
        let analyzer = analyze(code);
        let rhs = code.find("= $x").unwrap_or_else(|| panic!("rhs")) + 2;
        assert!(analyzer.find_definition(rhs).is_none());
        assert!(analyzer.find_all_references(rhs, false).is_empty());
        assert!(analyzer.find_all_references(rhs, true).is_empty());
        // Control: querying the call still selects the subroutine.
        let call = code.find("run();").unwrap_or_else(|| panic!("call"));
        let definition = analyzer.find_definition(call);
        assert_eq!(
            definition.map(|symbol| symbol.name.as_str()),
            Some("run"),
            "call must still define to the sub"
        );
    }

    #[test]
    fn find_all_references_outer_our_reaches_inner_our_alias_use() {
        // T7: an inner `our` aliases the same package variable as an outer
        // `our`, even across an intervening `my` of the same name.
        let code = "our $x = 10;\n{ my $x = 20; { our $x; print $x; } }\n";
        let analyzer = analyze(code);
        let outer_our = code.find("our $x").unwrap_or_else(|| panic!("outer our")) + 4;
        let refs = analyzer.find_all_references(outer_our, false);
        let inner_use = code.rfind("print $x").unwrap_or_else(|| panic!("use")) + 6;
        assert_eq!(refs.len(), 1, "inner alias use only; got {refs:?}");
        assert!(refs.iter().any(|loc| loc.start == inner_use));
    }

    #[test]
    fn find_all_references_excludes_pre_declaration_uses() {
        let code = "{ print $x; my $x = 2; print $x; }";
        let analyzer = analyze(code);
        let inner_decl = code.find("my $x").unwrap_or_else(|| panic!("decl")) + 3;
        let inner_refs = analyzer.find_all_references(inner_decl, false);
        assert_eq!(inner_refs.len(), 1, "post-declaration only; got {inner_refs:?}");
        let second_use = code.rfind("$x").unwrap_or_else(|| panic!("second use"));
        assert!(inner_refs.iter().any(|loc| loc.start == second_use));
    }
}
