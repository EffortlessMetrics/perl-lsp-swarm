//! Dancer2 document/workspace symbol cell (#8928).
//!
//! Renders canonical route/hook entries as explicitly labeled Dancer2
//! framework, virtual, source-anchored project shape. Route identity stays
//! independent of generic Perl subroutine identity: the display name is the
//! method set plus pattern plus route name, and every entry is marked as a
//! framework projection anchored to its source declaration.

use super::facts::CanonicalDancer2FileFacts;
use perl_semantic_facts::hook::HookNameSelection;
use perl_semantic_facts::route::RouteMethodSet;

/// Route display suffix marking framework provenance.
pub const DANCER2_ROUTE_LABEL: &str = "[Dancer2 route]";
/// Hook display suffix marking framework provenance.
pub const DANCER2_HOOK_LABEL: &str = "[Dancer2 hook]";

/// One labeled Dancer2 document symbol.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2DocumentSymbol {
    /// Display name (`GET, HEAD /users/:id — user_show [Dancer2 route]`).
    pub name: String,
    /// Detail line with framework/version provenance.
    pub detail: String,
    /// Declaration span start (byte offset).
    pub start: u32,
    /// Declaration span end (byte offset).
    pub end: u32,
    /// Whether the underlying fact was exact.
    pub exact: bool,
    /// `true` for routes, `false` for hooks.
    pub is_route: bool,
}

/// Build the labeled Dancer2 document symbols for one file's canonical facts.
#[must_use]
pub fn dancer2_document_symbols(facts: &CanonicalDancer2FileFacts) -> Vec<Dancer2DocumentSymbol> {
    let mut symbols = Vec::new();
    for route in &facts.routes {
        let methods = match &route.route.methods {
            RouteMethodSet::Exact(names) => names.join(", "),
            RouteMethodSet::Dynamic { .. } => "ANY(computed)".to_string(),
            _ => "ANY".to_string(),
        };
        let pattern =
            route.route.pattern.value.clone().unwrap_or_else(|| route.route.keyword.clone());
        let name = match route.route.route_name_literal_value() {
            Some(route_name) => format!("{methods} {pattern} — {route_name} {DANCER2_ROUTE_LABEL}"),
            None => format!("{methods} {pattern} {DANCER2_ROUTE_LABEL}"),
        };
        symbols.push(Dancer2DocumentSymbol {
            name,
            detail: format!("Dancer2 {} route (canonical framework fact)", route.framework_version),
            start: route.envelope.anchor.start_byte,
            end: route.envelope.anchor.end_byte,
            exact: route.status() == perl_semantic_facts::SemanticFactStatus::Exact,
            is_route: true,
        });
    }
    for hook in &facts.hooks {
        let hook_name = match &hook.hook.name {
            HookNameSelection::Literal(name) => name.literal.clone(),
            HookNameSelection::Dynamic { .. } => "(computed)".to_string(),
            _ => hook.hook.keyword.clone(),
        };
        symbols.push(Dancer2DocumentSymbol {
            name: format!("{hook_name} {DANCER2_HOOK_LABEL}"),
            detail: format!("Dancer2 {} hook (canonical framework fact)", hook.framework_version),
            start: hook.envelope.anchor.start_byte,
            end: hook.envelope.anchor.end_byte,
            exact: hook.status() == perl_semantic_facts::SemanticFactStatus::Exact,
            is_route: false,
        });
    }
    symbols.sort_by_key(|symbol| symbol.start);
    symbols
}

/// One workspace-level entity projection for the generated-symbol index.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2WorkspaceEntity {
    /// Bare searchable name (route name or hook name when literal, else the
    /// pattern/keyword identity).
    pub bare_name: String,
    /// Qualified canonical name (`package::bare`).
    pub canonical_name: String,
    /// Declaration span start (byte offset).
    pub start: u32,
    /// Declaration span end (byte offset).
    pub end: u32,
    /// Whether the underlying fact was exact.
    pub exact: bool,
    /// `true` for route entities, `false` for hook entities (label provenance).
    pub is_route: bool,
}

/// Build workspace-level entity projections for the generated-symbol index.
#[must_use]
pub fn dancer2_workspace_entities(
    facts: &CanonicalDancer2FileFacts,
) -> Vec<Dancer2WorkspaceEntity> {
    let mut entities = Vec::new();
    for route in &facts.routes {
        let package = route.envelope.package.clone().unwrap_or_else(|| "main".to_string());
        let bare =
            route.route.route_name_literal_value().map(ToOwned::to_owned).unwrap_or_else(|| {
                route.route.pattern.value.clone().unwrap_or_else(|| route.route.keyword.clone())
            });
        entities.push(Dancer2WorkspaceEntity {
            bare_name: bare.clone(),
            canonical_name: format!("{package}::{bare}"),
            start: route.envelope.anchor.start_byte,
            end: route.envelope.anchor.end_byte,
            exact: route.status() == perl_semantic_facts::SemanticFactStatus::Exact,
            is_route: true,
        });
    }
    for hook in &facts.hooks {
        let package = hook.envelope.package.clone().unwrap_or_else(|| "main".to_string());
        if let HookNameSelection::Literal(name) = &hook.hook.name {
            entities.push(Dancer2WorkspaceEntity {
                bare_name: name.literal.clone(),
                canonical_name: format!("{package}::{}", name.literal),
                start: hook.envelope.anchor.start_byte,
                end: hook.envelope.anchor.end_byte,
                exact: hook.status() == perl_semantic_facts::SemanticFactStatus::Exact,
                is_route: false,
            });
        }
    }
    entities
}

/// LSP `SymbolKind` numeric value for one Dancer2 document symbol.
///
/// Routes and hooks are navigable function-like framework entries; the
/// `[Dancer2 route]` / `[Dancer2 hook]` label carries the provenance.
#[must_use]
pub fn dancer2_request_kind(is_route: bool) -> u32 {
    let _ = is_route;
    12 // LSP SymbolKind::Function
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::dancer2::activation::RuntimeDancer2Module;
    use crate::providers::dancer2::activation::file_activations;
    use crate::providers::dancer2::facts::canonical_file_facts;
    use perl_semantic_analyzer::Parser;
    use perl_semantic_facts::{FileId, SourceGeneration};
    use perl_test_must::must_with;

    fn file_facts(source: &'static str) -> CanonicalDancer2FileFacts {
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("g1"));
        canonical_file_facts(&ast, FileId(1), source, &activations)
    }

    #[test]
    fn route_symbol_shape_is_labeled_with_name_and_pattern_separately() {
        let facts = file_facts("use Dancer2;\nget 'user_show', '/users/:id' => sub { 1 };");
        let symbols = dancer2_document_symbols(&facts);
        assert_eq!(symbols.len(), 1);
        assert!(
            symbols[0].name.starts_with("GET, HEAD /users/:id — user_show"),
            "name: {}",
            symbols[0].name
        );
        assert!(symbols[0].name.contains(DANCER2_ROUTE_LABEL));
        assert!(symbols[0].exact);
    }

    #[test]
    fn hook_symbols_are_labeled() {
        let facts = file_facts("use Dancer2;\nhook 'before' => sub { 1 };");
        let symbols = dancer2_document_symbols(&facts);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, format!("before {DANCER2_HOOK_LABEL}"));
        assert!(!symbols[0].is_route);
    }

    #[test]
    fn unnamed_route_uses_pattern_identity() {
        let facts = file_facts("use Dancer2;\npost '/submit' => sub { 1 };");
        let symbols = dancer2_document_symbols(&facts);
        assert_eq!(symbols[0].name, format!("POST /submit {DANCER2_ROUTE_LABEL}"));
    }

    #[test]
    fn workspace_entities_preserve_package_identity() {
        let facts =
            file_facts("package App;\nuse Dancer2;\nget 'user_show', '/users/:id' => sub { 1 };");
        let entities = dancer2_workspace_entities(&facts);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].bare_name, "user_show");
        assert_eq!(entities[0].canonical_name, "App::user_show");
    }

    #[test]
    fn dynamic_route_name_falls_back_to_pattern_identity() {
        // A computed name operand is a boundary: the symbol identity uses
        // the pattern, never a guessed name.
        let facts = file_facts(
            "use Dancer2;
my $n = 'computed';
get $n, '/y' => sub { 1 };",
        );
        let symbols = dancer2_document_symbols(&facts);
        assert_eq!(symbols.len(), 1);
        assert!(
            symbols[0].name.starts_with("GET, HEAD /y"),
            "dynamic name falls back to the pattern identity: {}",
            symbols[0].name
        );
        assert!(!symbols[0].name.contains("—"), "no name part is synthesized");
    }
}
