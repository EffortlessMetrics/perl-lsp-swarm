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
            if matches!(reference.kind, SymbolKind::Variable(_)) {
                let qualified = format!("{pkg}::{name}");
                return self
                    .symbol_table
                    .symbols
                    .get(name)
                    .into_iter()
                    .flatten()
                    .filter(|symbol| {
                        symbol.kind == reference.kind
                            && symbol.declaration.as_deref() == Some("our")
                            && symbol.qualified_name == qualified
                    })
                    .collect();
            }
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

    /// Find all references to a symbol at a given position for Navigate/Analyze workflows.
    pub fn find_all_references(
        &self,
        position: usize,
        include_declaration: bool,
    ) -> Vec<SourceLocation> {
        if let Some(references) = self.find_lexical_references(position, include_declaration) {
            return references;
        }
        let symbol = self.find_definition(position);

        if let Some(symbol) = symbol {
            return self.find_all_references_for_symbol(symbol, include_declaration);
        }

        Vec::new()
    }

    /// Return same-file references when the cursor resolves to a lexical declaration.
    pub fn find_lexical_references(
        &self,
        position: usize,
        include_declaration: bool,
    ) -> Option<Vec<SourceLocation>> {
        let reference = self
            .symbol_table
            .references
            .values()
            .flatten()
            .filter(|r| r.location.start <= position && position < r.location.end)
            .min_by_key(|r| r.location.end.saturating_sub(r.location.start));
        let symbol = if let Some(reference) = reference {
            self.lexical_binding(reference)?
        } else {
            self.symbol_at(SourceLocation { start: position, end: position })?
        };
        if !self.can_resolve_lexically(symbol) {
            return None;
        }
        Some(self.find_all_references_for_symbol(symbol, include_declaration))
    }

    /// Find all references for a specific symbol.
    pub(super) fn find_all_references_for_symbol(
        &self,
        symbol: &Symbol,
        include_declaration: bool,
    ) -> Vec<SourceLocation> {
        let mut locations = Vec::new();
        let scope_sensitive = self.can_resolve_lexically(symbol);

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
                        && (!scope_sensitive
                            || self
                                .lexical_binding(reference)
                                .is_some_and(|resolved| std::ptr::eq(resolved, symbol)))
                    {
                        locations.push(reference.location);
                    }
                }
            }
        }

        locations
    }

    fn can_resolve_lexically(&self, symbol: &Symbol) -> bool {
        symbol.kind == SymbolKind::scalar()
            && matches!(symbol.declaration.as_deref(), Some("my" | "state"))
            && self.symbol_table.lexical_activation.contains_key(&symbol.location.start)
            && !self.has_indexed_reference(&symbol.name)
            && !self.symbol_table.symbols.get(&symbol.name).is_some_and(|symbols| {
                symbols.iter().any(|other| {
                    other.kind == SymbolKind::array()
                        || other.kind == SymbolKind::hash()
                        || (other.kind == symbol.kind
                            && matches!(other.declaration.as_deref(), Some("my" | "state" | "our"))
                            && !self
                                .symbol_table
                                .lexical_activation
                                .contains_key(&other.location.start))
                })
            })
    }

    fn has_indexed_reference(&self, name: &str) -> bool {
        // Without source text, or when scalar syntax denotes an array/hash
        // element, the symbol kind alone cannot establish lexical identity.
        self.source.is_empty()
            || self.symbol_table.references.get(name).is_some_and(|refs| {
                refs.iter().any(|reference| {
                    self.source.get(reference.location.end..).is_none_or(|tail| {
                        matches!(tail.trim_start().chars().next(), Some('[' | '{'))
                    })
                })
            })
    }

    fn lexical_binding(&self, reference: &crate::symbol::SymbolReference) -> Option<&Symbol> {
        let candidates = self.symbol_table.symbols.get(&reference.name)?;
        let mut scope_id = Some(reference.scope_id);
        while let Some(id) = scope_id {
            if let Some(symbol) = candidates
                .iter()
                .filter(|symbol| {
                    symbol.scope_id == id
                        && symbol.kind == reference.kind
                        && self
                            .symbol_table
                            .lexical_activation
                            .get(&symbol.location.start)
                            .is_some_and(|end| *end <= reference.location.start)
                })
                .max_by_key(|symbol| symbol.location.start)
            {
                return Some(symbol);
            }
            scope_id = self.symbol_table.scopes.get(&id).and_then(|scope| scope.parent);
        }
        None
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
