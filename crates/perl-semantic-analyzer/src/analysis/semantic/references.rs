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

    /// Byte offset at which a declaration's statement ends; a lexical is
    /// not in scope before it (#18115).
    fn declaration_statement_end(&self, symbol: &Symbol) -> usize {
        self.symbol_table
            .declaration_statement_ends
            .get(&symbol.location.start)
            .copied()
            .unwrap_or(symbol.location.end)
    }

    /// Whether a lexical candidate's declaring statement has not ended at
    /// the reference yet, so the reference cannot bind to it (#18115).
    fn not_yet_effective(
        &self,
        candidate: &Symbol,
        reference: &crate::symbol::SymbolReference,
    ) -> bool {
        matches!(candidate.declaration.as_deref(), Some("my" | "state"))
            && reference.location.start < self.declaration_statement_end(candidate)
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
                // Never resurrect a lexical whose statement has not ended:
                // the use binds outward or to the package global instead.
                symbols
                    .iter()
                    .copied()
                    .find(|candidate| !self.not_yet_effective(candidate, reference))
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
        // A use before the end of the lexical's own declaring statement
        // cannot bind to it (initializer RHS, pre-declaration use): it
        // binds to an outer declaration or the package global (#18115).
        if symbol_is_lexical && self.not_yet_effective(symbol, reference) {
            return true;
        }
        let mut current_scope = reference.scope_id;
        loop {
            let binding = candidates
                .iter()
                .filter(|candidate| {
                    candidate.kind == symbol.kind
                        && candidate.scope_id == current_scope
                        && match candidate.declaration.as_deref() {
                            Some("my" | "state") => true,
                            Some("our") => symbol_is_lexical,
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
                return binding.location.start != symbol.location.start;
            }
            // No capturing declaration has taken effect in this scope yet
            // (the reference precedes the end of its declaring statement,
            // as an initializer RHS does): the binding lives further out,
            // so keep walking instead of crediting `symbol`.
            let Some(scope) = self.symbol_table.scopes.get(&current_scope) else {
                return false;
            };
            let Some(parent) = scope.parent else {
                return false;
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
