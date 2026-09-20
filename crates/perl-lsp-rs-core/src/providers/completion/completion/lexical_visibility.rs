//! Cursor lexical-visibility admission for local variable completion (#8941).
//!
//! Visibility is an admission decision applied BEFORE candidate construction,
//! not a ranking signal: a binding that is not visible at the cursor must not
//! appear at any rank. This module owns that one explicit decision for every
//! local variable-completion path over the compatibility `SymbolTable`.
//!
//! The decision is equivalent to:
//!
//! ```text
//! binding kind + declaration role
//! + declaration/effective range
//! + cursor scope and position
//! + ancestor relationship
//! + shadowing
//! → visible | not visible | bounded/unknown
//! ```
//!
//! Per-class rulings (bounded where current facts are insufficient; producer
//! gaps transfer to #7423/#7424 rather than growing text heuristics):
//!
//! - `my` / `state` / signature parameters / loop lexicals: visible only when
//!   the declaration precedes the cursor AND the declaring scope is the cursor
//!   scope or an ancestor of it.
//! - `our`: package-global alias whose strict-mode validity spans the
//!   declaring scope's lexical extent — the declaration must precede the
//!   cursor and the declaring scope must be the cursor scope or an ancestor.
//!   Package identity is not represented in the compatibility SymbolTable, so
//!   cross-package admission is a documented bound, not authority.
//! - `local`: dynamic alias of a package variable, modeled by the same static
//!   declaration-order and extent facts (conservative; dynamic-extent facts
//!   beyond the declaring block are not represented, so admission cannot
//!   follow a runtime call path).
//!
//! Shadowing is resolved as exact identity selection among admitted bindings
//! sharing one resolved slot (name + kind + qualified target): innermost
//! declaring scope wins, ties break to the latest declaration anchor. A
//! shadowed outer binding is dropped, never down-ranked.
//!
//! Boundary: #8941 owns only whether ONE local binding is visible/admissible.
//! The shared candidate envelope, final rank, cap, runtime route, and protocol
//! outcome belong to #11002/#10229/#10914/#10230. The admission decision and
//! identity selection are exposed crate-internally as pure functions over the
//! generation-current SymbolTable; #11002 consumes this API within
//! perl-lsp-rs-core and owns any cross-crate projection requiring generation
//! handles.

use perl_semantic_analyzer::symbol::{ScopeId, Symbol, SymbolKind, SymbolTable};
use std::collections::HashMap;

/// Why a binding was admitted or rejected at the completion cursor (#8941).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisibilityReason {
    /// Lexical whose declaring scope contains the cursor scope in its
    /// descendant tree and whose declaration precedes the cursor.
    LexicalActiveAtCursor,
    /// `our` package-global alias: visible across the package regardless of
    /// declaration extent (bounded per-class behavior).
    PackageGlobalAlias,
    /// `local` dynamic alias of a package variable: same name-resolution
    /// surface as the underlying global (bounded; no dynamic-extent facts).
    DynamicLocalAlias,
    /// Lexical declared textually after the cursor position.
    DeclaredAfterCursor,
    /// Declaring scope is neither the cursor scope nor an ancestor of it:
    /// sibling, child, or otherwise already-ended scope.
    ///
    /// Conservative fallback (#8941): this reason is overloaded with the
    /// bounded/unknown case. When the cursor scope id or the declaring scope id
    /// is not resolvable in the generation-current SymbolTable — or the
    /// bounded-hop guard in `scope_chain_contains` exhausts — containment
    /// cannot be proven, and admission falls back to
    /// `NotVisible(ScopeNotVisibleFromCursor)` rather than carrying a separate
    /// variant. Readers (#11002) must therefore treat this reason as
    /// "proven-invisible OR unresolved-scope", never as standalone proof of
    /// invisibility; separating the two requires generation-stable scope
    /// handles owned by the #11002 projection.
    ScopeNotVisibleFromCursor,
}

/// Admission outcome for one binding at the cursor (#8941).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admission {
    /// Binding may be constructed as a candidate before ranking.
    Visible(VisibilityReason),
    /// Binding must not appear at any rank.
    NotVisible(VisibilityReason),
}

impl Admission {
    pub(crate) fn is_visible(self) -> bool {
        matches!(self, Self::Visible(_))
    }
}

/// Declaration roles relevant to visibility admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclarationRole {
    /// `my` / `state` / signature parameter / implicit loop lexical.
    Lexical,
    /// `our` — package-global alias.
    OurAlias,
    /// `local` — dynamic alias of a package variable.
    LocalAlias,
}

fn declaration_role(declaration: Option<&str>) -> DeclarationRole {
    match declaration {
        Some("our") => DeclarationRole::OurAlias,
        Some("local") => DeclarationRole::LocalAlias,
        // "my", "state", signature/loop lexicals recorded via the same
        // declarator seam, and any future declarator default to lexical
        // admission — the conservative reading of Perl scoping rules.
        _ => DeclarationRole::Lexical,
    }
}

/// Decide whether one binding is visible/admissible at the cursor.
///
/// Pure over `(SymbolTable, cursor scope, cursor position, symbol)`; no
/// mutable state, no source re-scanning.
pub(crate) fn admit(
    symbol_table: &SymbolTable,
    cursor_scope_id: ScopeId,
    cursor_position: usize,
    symbol: &Symbol,
) -> Admission {
    let role = declaration_role(symbol.declaration.as_deref());
    // Declaration order applies to every class: a declaration textually after
    // the cursor cannot admit a binding of any kind — an `our` alias in a
    // later statement is not valid yet, and a `local` that has not run has
    // not localized anything.
    if symbol.location.start() > cursor_position {
        return Admission::NotVisible(VisibilityReason::DeclaredAfterCursor);
    }
    // Extent applies to every class with the facts this module has: the
    // declaring scope must be the cursor scope or an ancestor of it. For
    // `our` this is the alias's strict-mode lexical extent; for `local` it is
    // the conservative static model of the dynamic extent (a sibling block or
    // an already-ended block cannot reactivate either). Package identity is
    // not represented in the compatibility SymbolTable, so cross-package
    // admission remains a documented bound, not silent authority.
    if !scope_chain_contains(symbol_table, cursor_scope_id, symbol.scope_id) {
        return Admission::NotVisible(VisibilityReason::ScopeNotVisibleFromCursor);
    }
    match role {
        DeclarationRole::OurAlias => Admission::Visible(VisibilityReason::PackageGlobalAlias),
        DeclarationRole::LocalAlias => Admission::Visible(VisibilityReason::DynamicLocalAlias),
        DeclarationRole::Lexical => Admission::Visible(VisibilityReason::LexicalActiveAtCursor),
    }
}

/// Whether `target` is `cursor_scope` itself or an ancestor of it.
///
/// Walks the parent chain upward with the same bounded-hop guard used by the
/// scope-distance helpers so malformed trees cannot loop forever.
fn scope_chain_contains(
    symbol_table: &SymbolTable,
    cursor_scope: ScopeId,
    target: ScopeId,
) -> bool {
    // An equality between two unresolvable ids is not containment: the cursor
    // scope must exist in the generation-current table before any comparison,
    // or `cursor == target == 9` would admit a binding whose scope the table
    // never recorded.
    if !symbol_table.scopes.contains_key(&cursor_scope) {
        return false;
    }
    let mut current = cursor_scope;
    let mut hops = 0u32;

    loop {
        if current == target {
            return true;
        }

        let Some(scope) = symbol_table.scopes.get(&current) else {
            break;
        };
        let Some(parent) = scope.parent else {
            break;
        };

        hops = hops.saturating_add(1);
        if hops > 100 {
            break;
        }
        current = parent;
    }

    false
}

/// Resolve exact identity among admitted bindings sharing one resolved slot.
///
/// Bindings group by `(name, kind, qualified_name)` — the third component
/// keeps distinct resolved targets (e.g. bare lexical vs `our` alias into a
/// named package) from competing over one label. Within a group the innermost
/// declaring scope wins; ties break to the latest declaration anchor, then to
/// the higher scope id so selection stays deterministic. Shadowed outers are
/// DROPPED, never down-ranked (#8941 negative control).
///
/// Survivors preserve first-occurrence order of their group in `candidates`.
pub(super) fn select_exact_identities<'a>(
    symbol_table: &SymbolTable,
    candidates: Vec<&'a Symbol>,
) -> Vec<&'a Symbol> {
    struct Group<'a> {
        best: &'a Symbol,
        best_depth: usize,
        first_index: usize,
    }

    let mut groups: HashMap<(String, SymbolKind, String), Group<'a>> = HashMap::new();
    let mut order: Vec<(String, SymbolKind, String)> = Vec::new();

    for (index, symbol) in candidates.into_iter().enumerate() {
        let key = (symbol.name.clone(), symbol.kind, symbol.qualified_name.clone());
        let depth = crate::providers::completion::completion::scope_distance::scope_depth(
            symbol_table,
            symbol.scope_id,
        );

        match groups.get_mut(&key) {
            Some(group) => {
                let challenger = (depth, symbol.location.start(), symbol.scope_id);
                let incumbent =
                    (group.best_depth, group.best.location.start(), group.best.scope_id);
                if challenger > incumbent {
                    group.best = symbol;
                    group.best_depth = depth;
                }
            }
            None => {
                order.push(key.clone());
                groups.insert(key, Group { best: symbol, best_depth: depth, first_index: index });
            }
        }
    }

    let mut survivors: Vec<(usize, &Symbol)> = order
        .into_iter()
        .filter_map(|key| groups.remove(&key).map(|g| (g.first_index, g.best)))
        .collect();
    survivors.sort_by_key(|(index, _)| *index);
    survivors.into_iter().map(|(_, symbol)| symbol).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::SourceLocation;
    use perl_semantic_analyzer::symbol::{Scope, ScopeKind};

    /// Hierarchy:
    ///
    /// ```text
    /// 0 Global (0..100)
    /// +-- 1 Subroutine outer (10..90)
    ///     +-- 2 Block inner (20..50)
    ///     +-- 3 Block sibling (55..80)
    /// ```
    fn table() -> SymbolTable {
        let mut table = SymbolTable::new();
        let scopes = [
            (0usize, None, ScopeKind::Global, 0usize, 100usize),
            (1, Some(0), ScopeKind::Subroutine, 10, 90),
            (2, Some(1), ScopeKind::Block, 20, 50),
            (3, Some(1), ScopeKind::Block, 55, 80),
        ];
        for (id, parent, kind, start, end) in scopes {
            table.scopes.insert(
                id,
                Scope {
                    id,
                    parent,
                    kind,
                    location: SourceLocation::new(start, end),
                    symbols: std::collections::HashSet::new(),
                },
            );
        }
        table
    }

    fn symbol(
        name: &str,
        kind: SymbolKind,
        declaration: &str,
        scope_id: ScopeId,
        start: usize,
    ) -> Symbol {
        Symbol {
            name: name.to_string(),
            qualified_name: name.to_string(),
            kind,
            location: SourceLocation::new(start, start + 4),
            scope_id,
            declaration: Some(declaration.to_string()),
            documentation: None,
            attributes: vec![],
        }
    }

    #[test]
    fn lexical_active_in_own_scope_is_visible() {
        let table = table();
        let s = symbol("x", SymbolKind::scalar(), "my", 2, 21);
        assert_eq!(
            admit(&table, 2, 30, &s),
            Admission::Visible(VisibilityReason::LexicalActiveAtCursor)
        );
    }

    #[test]
    fn lexical_from_ancestor_scope_is_visible() {
        let table = table();
        let s = symbol("x", SymbolKind::scalar(), "my", 0, 1);
        assert_eq!(
            admit(&table, 2, 30, &s),
            Admission::Visible(VisibilityReason::LexicalActiveAtCursor)
        );
    }

    #[test]
    fn state_binding_follows_lexical_rules() {
        let table = table();
        let s = symbol("once", SymbolKind::scalar(), "state", 2, 21);
        assert!(admit(&table, 2, 30, &s).is_visible());
        // Ended sibling scope cannot reactivate it.
        assert!(!admit(&table, 3, 70, &s).is_visible());
    }

    #[test]
    fn represented_loop_lexical_follows_lexical_rules() {
        // Loop lexicals recorded through the same declarator seam behave as
        // plain lexicals once a producer represents them (#7423/#7424 own the
        // missing bare-foreach recording).
        let table = table();
        let s = symbol("it", SymbolKind::scalar(), "my", 2, 21);
        assert_eq!(
            admit(&table, 2, 25, &s),
            Admission::Visible(VisibilityReason::LexicalActiveAtCursor)
        );
        assert_eq!(
            admit(&table, 3, 60, &s),
            Admission::NotVisible(VisibilityReason::ScopeNotVisibleFromCursor)
        );
    }

    #[test]
    fn our_alias_visible_within_declaring_extent_only() {
        let table = table();
        let declared_inner = symbol("pkg", SymbolKind::scalar(), "our", 3, 56);
        // Visible inside its declaring block after the declaration...
        assert_eq!(
            admit(&table, 3, 70, &declared_inner),
            Admission::Visible(VisibilityReason::PackageGlobalAlias)
        );
        // ...but not from a sibling block (a different strict-mode extent)...
        assert_eq!(
            admit(&table, 2, 70, &declared_inner),
            Admission::NotVisible(VisibilityReason::ScopeNotVisibleFromCursor)
        );
        // ...and not after the declaring block ended, even at global scope.
        assert_eq!(
            admit(&table, 0, 99, &declared_inner),
            Admission::NotVisible(VisibilityReason::ScopeNotVisibleFromCursor)
        );
    }

    #[test]
    fn our_alias_declared_after_cursor_is_rejected() {
        let table = table();
        let future = symbol("pkg", SymbolKind::scalar(), "our", 2, 40);
        assert_eq!(
            admit(&table, 2, 35, &future),
            Admission::NotVisible(VisibilityReason::DeclaredAfterCursor)
        );
    }

    #[test]
    fn local_alias_follows_static_extent_conservatively() {
        let table = table();
        let declared_inner = symbol("dyn", SymbolKind::scalar(), "local", 3, 56);
        assert_eq!(
            admit(&table, 3, 70, &declared_inner),
            Admission::Visible(VisibilityReason::DynamicLocalAlias)
        );
        // Dynamic-extent facts are not represented, so a sibling block cannot
        // reactivate the bare name and the conservative model stays closed.
        assert_eq!(
            admit(&table, 2, 70, &declared_inner),
            Admission::NotVisible(VisibilityReason::ScopeNotVisibleFromCursor)
        );
        let future = symbol("dyn", SymbolKind::scalar(), "local", 2, 40);
        assert_eq!(
            admit(&table, 2, 35, &future),
            Admission::NotVisible(VisibilityReason::DeclaredAfterCursor)
        );
    }

    #[test]
    fn declaration_after_cursor_is_rejected() {
        let table = table();
        let s = symbol("later", SymbolKind::scalar(), "my", 2, 40);
        assert_eq!(
            admit(&table, 2, 35, &s),
            Admission::NotVisible(VisibilityReason::DeclaredAfterCursor)
        );
    }

    #[test]
    fn sibling_scope_is_rejected_not_downranked() {
        let table = table();
        // Declared inside one block before the cursor moved into its sibling.
        let s = symbol("only_a", SymbolKind::scalar(), "my", 2, 21);
        assert_eq!(
            admit(&table, 3, 70, &s),
            Admission::NotVisible(VisibilityReason::ScopeNotVisibleFromCursor)
        );
    }

    #[test]
    fn unknown_scope_falls_back_conservatively_rejected() {
        // A recovered/partial table may reference a scope id that is absent;
        // without proof of visibility admission stays closed.
        let mut table = table();
        let s = symbol("ghost", SymbolKind::scalar(), "my", 9, 1);
        table.scopes.remove(&9);
        assert!(!admit(&table, 2, 30, &s).is_visible());
    }

    #[test]
    fn equal_but_absent_scope_ids_are_not_containment() {
        // cursor_scope == symbol.scope_id == 9 with scope 9 absent from the
        // table must not read as containment-by-equality.
        let table = table();
        let s = symbol("ghost", SymbolKind::scalar(), "my", 9, 1);
        assert_eq!(
            admit(&table, 9, 30, &s),
            Admission::NotVisible(VisibilityReason::ScopeNotVisibleFromCursor)
        );
    }

    #[test]
    fn identity_selection_prefers_innermost_shadow() {
        let table = table();
        let outer = symbol("value", SymbolKind::scalar(), "my", 0, 1);
        let inner = symbol("value", SymbolKind::scalar(), "my", 2, 21);

        let selected = select_exact_identities(&table, vec![&outer, &inner]);
        assert_eq!(selected.len(), 1);
        assert!(std::ptr::eq(selected[0], &inner), "innermost binding must survive");
    }

    #[test]
    fn identity_selection_same_scope_prefers_latest_anchor() {
        let table = table();
        let earlier = symbol("dup", SymbolKind::scalar(), "my", 2, 21);
        let later = symbol("dup", SymbolKind::scalar(), "my", 2, 31);

        let selected = select_exact_identities(&table, vec![&earlier, &later]);
        assert_eq!(selected.len(), 1);
        assert!(std::ptr::eq(selected[0], &later), "latest anchor must win within one scope");
    }

    #[test]
    fn identity_selection_keeps_distinct_kinds_and_targets() {
        let table = table();
        let scalar_outer = symbol("v", SymbolKind::scalar(), "my", 0, 1);
        let array_same_name = symbol("v", SymbolKind::array(), "my", 0, 2);
        let our_aliased = Symbol {
            qualified_name: "Pkg::v".to_string(),
            ..symbol("v", SymbolKind::Variable(perl_symbol::VarKind::Scalar), "our", 0, 3)
        };

        let selected =
            select_exact_identities(&table, vec![&scalar_outer, &array_same_name, &our_aliased]);
        assert_eq!(selected.len(), 3, "distinct kinds/targets must not shadow each other");
    }

    #[test]
    fn identity_selection_survivors_preserve_input_order() {
        let table = table();
        let first_scope_sym = symbol("aa", SymbolKind::scalar(), "my", 2, 21);
        let second_scope_sym = symbol("zz", SymbolKind::scalar(), "my", 3, 56);

        let selected = select_exact_identities(&table, vec![&second_scope_sym, &first_scope_sym]);
        assert_eq!(selected.len(), 2);
        assert!(std::ptr::eq(selected[0], &second_scope_sym));
        assert!(std::ptr::eq(selected[1], &first_scope_sym));
    }
}
