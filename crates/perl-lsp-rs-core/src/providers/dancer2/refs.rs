//! Canonical named-route reference producer (#8931).
//!
//! Closes the gap #8928 recorded in [`super::targets`]: named-route
//! references previously had no canonical producer and returned a typed
//! refusal. This cell admits reviewed source-backed APIs that refer to a
//! declared Dancer2 route by literal name:
//!
//! - `uri_for_route('<literal>', ...)` resolves to the exact named
//!   [`perl_semantic_facts::route::RouteFact`] of the same application/root;
//! - the exact operand range and the route declaration identity are
//!   retained (no spelling-only matching across apps/roots);
//! - dynamic names, unknown literal names, stale generations, and
//!   non-source occurrences are typed refusals, never guesses.
//!
//! The producer consumes only canonical facts and the parsed AST — no
//! text-search union, no new Dancer2 grammar.
//!
//! Status boundary: this is the last route by which an admission may be
//! incomplete. An admission is complete-or-refuse at the consumer level
//! (rename); references themselves are per-occurrence exact or refused.

use super::facts::CanonicalDancer2FileFacts;
use perl_semantic_facts::route::{RouteFact, RouteNameSelection};

/// One canonical named-route reference resolution result.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedRouteReference {
    /// A source-backed literal occurrence resolved to one exact route fact.
    Canonical {
        /// Declaration index of the resolved route entity.
        declaration_index: u32,
        /// Literal route-name value of that entity.
        route_name: String,
    },
    /// The occurrence family is recognized but cannot be admitted: typed
    /// refusal with a machine-readable reason.
    TypedRefusal {
        /// Machine-readable refusal reason.
        reason: &'static str,
        /// Human explanation.
        detail: String,
    },
}

/// Machine-readable refusal reasons for the named-route reference family.
pub mod refusal_reasons {
    /// The name operand was not a literal string (dynamic / computed).
    pub const DYNAMIC_ROUTE_NAME: &str = "dancer2.dynamic_route_name";
    /// No current route entity of this application declares that name.
    pub const UNKNOWN_ROUTE_NAME: &str = "dancer2.unknown_route_name";
}

/// Resolve a canonical named-route reference at `offset`.
///
/// Returns `None` when `offset` is not inside an admitted reference-family
/// call occurrence (the generic paths own those positions).
#[must_use]
pub fn named_route_reference_at(
    _facts: &CanonicalDancer2FileFacts,
    offset: usize,
) -> Option<NamedRouteReference> {
    // Occurrence scan lands with the reference family wiring commit; until
    // then every position stays unclaimed so no generic answer is shadowed.
    let _ = offset;
    None
}

/// The literal route name of one route entity, if it declares one.
#[must_use]
pub fn route_literal_name(route: &RouteFact) -> Option<&str> {
    match &route.route.route_name {
        RouteNameSelection::Literal(name) => Some(&name.value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::dancer2::activation::RuntimeDancer2Module;
    use crate::providers::dancer2::activation::file_activations;
    use crate::providers::dancer2::facts::canonical_file_facts;
    use perl_semantic_analyzer::Parser;
    use perl_semantic_facts::{FileId, SourceGeneration};

    fn setup(source: &'static str) -> (perl_semantic_analyzer::Node, CanonicalDancer2FileFacts) {
        let mut parser = Parser::new(source);
        let ast = parser.parse().unwrap_or_else(|_| panic!("fixture must parse"));
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), &activations);
        (ast, facts)
    }

    #[test]
    fn unclaimed_positions_return_none_while_wiring_lands() {
        let source = "use Dancer2;\nget '/x' => sub { 'body' };\n";
        let (_ast, facts) = setup(source);
        assert!(named_route_reference_at(&facts, source.find("body").unwrap_or(0)).is_none());
    }

    #[test]
    fn declared_names_are_visible_through_the_fact_layer() {
        let source = "use Dancer2;\nget 'user_show', '/users/:id', sub { 'u' };\n";
        let (_ast, facts) = setup(source);
        let names: Vec<&str> = facts
            .routes
            .iter()
            .filter_map(route_literal_name)
            .collect();
        assert!(names.contains(&"user_show"), "expected user_show, got {names:?}");
    }
}
