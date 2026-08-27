//! Canonical Dancer2 named-route reference occurrences (#8931).
//!
//! Extracts the admitted named-route reference family from source: unqualified
//! `uri_for_route(...)` calls whose first operand statically denotes a route
//! name. This is the occurrence side of the canonical route-reference
//! contract — the target side is the exact [`RouteNameSelection::Literal`]
//! name on a minted `RouteFact` (#8918/#8921), matched by application/root
//! identity, never by label alone.
//!
//! Exactness boundary (mirrors the route-pattern operand rules): quoted
//! operands map byte-for-byte onto the token interior only when escape-free;
//! escapes and interpolated operands are typed dynamic boundaries, never
//! guessed values. A non-string first operand is likewise dynamic.

use super::dancer2_routes::{StaticString, interpolated_value_is_dynamic, static_string};
use crate::ast::{Node, NodeKind};
use perl_semantic_facts::{AnchorId, FileId, SourceAnchor};

/// The one admitted named-route reference API in this slice (#8931).
pub(crate) const NAMED_ROUTE_REFERENCE_API: &str = "uri_for_route";

/// One source occurrence of the admitted named-route reference family.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2NamedRouteCallOccurrence {
    /// Package scope the call lexically belongs to; binds to the exact
    /// activated package's route facts.
    pub package: Option<String>,
    /// Exact source range of the name string operand (the raw quoted token,
    /// including its quotes).
    pub operand_anchor: SourceAnchor,
    /// Statically claimable name selection.
    pub selection: Dancer2NamedRouteName,
}

/// Static selection of one name operand.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dancer2NamedRouteName {
    /// Literal route-name value (unquoted, byte-exact token interior).
    Literal(String),
    /// The operand is not statically a literal value. This occurrence stays
    /// outside every admitted edit set; it is recorded so consumers can refuse
    /// completeness honestly rather than silently ignoring the site.
    Dynamic {
        /// Bounded reason.
        reason: String,
    },
}

impl Dancer2NamedRouteCallOccurrence {
    /// Whether this occurrence names an exact literal route name.
    #[must_use]
    pub fn literal_value(&self) -> Option<&str> {
        match &self.selection {
            Dancer2NamedRouteName::Literal(value) => Some(value),
            Dancer2NamedRouteName::Dynamic { .. } => None,
        }
    }
}

/// Whether `name` is the bareword unqualified DSL form of the admitted API.
fn is_admitted_call_name(name: &str) -> bool {
    name == NAMED_ROUTE_REFERENCE_API && !name.contains("::")
}

/// Classify one first-operand node of the admitted call form.
fn classify_operand(node: Option<&Node>) -> Dancer2NamedRouteName {
    match node.map(|node| &node.kind) {
        Some(NodeKind::String { value, interpolated }) => {
            if *interpolated && interpolated_value_is_dynamic(value) {
                return Dancer2NamedRouteName::Dynamic {
                    reason: "interpolated name operand".to_string(),
                };
            }
            match static_string(value) {
                StaticString::Exact(value) => Dancer2NamedRouteName::Literal(value),
                StaticString::Empty => Dancer2NamedRouteName::Dynamic {
                    reason: "empty name operand".to_string(),
                },
                StaticString::Escaped => Dancer2NamedRouteName::Dynamic {
                    reason: "escaped name operand".to_string(),
                },
            }
        }
        Some(_) => Dancer2NamedRouteName::Dynamic {
            reason: "non-literal name operand".to_string(),
        },
        None => Dancer2NamedRouteName::Dynamic {
            reason: "missing name operand".to_string(),
        },
    }
}

struct WalkState {
    file_id: FileId,
    current_package: Option<String>,
    occurrences: Vec<Dancer2NamedRouteCallOccurrence>,
}

/// Extract every admitted named-route reference occurrence from `ast`, in
/// source order.
///
/// Unlike the load-time route declaration walk, this walk descends into
/// subroutine bodies: `uri_for_route` executes at request time inside
/// handlers, where the DSL functions imported by `use Dancer2` remain in the
/// declaring package's lexical scope.
#[must_use]
pub fn extract_dancer2_named_route_occurrences(
    ast: &Node,
    file_id: FileId,
) -> Vec<Dancer2NamedRouteCallOccurrence> {
    let mut state = WalkState { file_id, current_package: Some("main".to_string()), occurrences: Vec::new() };
    walk_node(ast, &mut state);
    state.occurrences.sort_by_key(|occ| occ.operand_anchor.start_byte);
    state.occurrences
}

fn walk_node(node: &Node, state: &mut WalkState) {
    match &node.kind {
        // Statement-form `package X;` switches the current package for the
        // following sibling statements (the #8914/#8918 walk contract); a
        // block form scopes lexically and restores.
        NodeKind::Package { name, block: Some(block), .. } => {
            let saved = state.current_package.clone();
            state.current_package = Some(name.clone());
            for child in block.children() {
                walk_node(child, state);
            }
            state.current_package = saved;
        }
        NodeKind::Package { name, block: None, .. } => {
            state.current_package = Some(name.clone());
        }
        NodeKind::FunctionCall { name, args } if is_admitted_call_name(name) => {
            // The cursor position never claims the callee token here; that
            // path keeps the generic import/definition behavior (#8928).
            if let Some(first) = args.first() {
                let selection = classify_operand(Some(first));
                let start = span_u32(first.location.start);
                let end = span_u32(first.location.end);
                state.occurrences.push(Dancer2NamedRouteCallOccurrence {
                    package: state.current_package.clone(),
                    operand_anchor: SourceAnchor::new(
                        Some(AnchorId(u64::from(start))),
                        state.file_id,
                        start,
                        end,
                    ),
                    selection,
                });
            }
            for child in node.children() {
                walk_node(child, state);
            }
        }
        _ => {
            for child in node.children() {
                walk_node(child, state);
            }
        }
    }
}

fn span_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    fn parse(source: &'static str) -> Node {
        let mut parser = Parser::new(source);
        parser.parse().expect("fixture must parse")
    }

    #[test]
    fn literal_operand_is_extracted_with_exact_range() {
        let source = "use Dancer2;\nget 'user_show', '/u', sub {\n  uri_for_route('user_show');\n};\n";
        let ast = parse(source);
        let occurrences = extract_dancer2_named_route_occurrences(&ast, FileId(7));
        assert_eq!(occurrences.len(), 1, "{occurrences:?}");
        assert_eq!(occurrences[0].package.as_deref(), Some("main"));
        assert_eq!(occurrences[0].literal_value(), Some("user_show"));
        let start = occurrences[0].operand_anchor.start_byte as usize;
        let end = occurrences[0].operand_anchor.end_byte as usize;
        assert_eq!(&source[start..end], "'user_show'");
    }

    #[test]
    fn occurrences_inside_subs_bind_to_the_declaring_package() {
        let source =
            "package App;\nuse Dancer2;\nsub handler { uri_for_route('show_one'); }\npackage Other;\nsub away { uri_for_route('show_two'); }\n";
        let ast = parse(source);
        let occurrences = extract_dancer2_named_route_occurrences(&ast, FileId(3));
        let packages: Vec<Option<&str>> =
            occurrences.iter().map(|occ| occ.package.as_deref()).collect();
        assert_eq!(
            packages,
            vec![Some("App"), Some("Other")],
            "each call binds to its own package scope"
        );
    }

    #[test]
    fn dynamic_operands_stay_dynamic_and_are_still_recorded() {
        // A zero-argument call carries no name operand at all, so it is not
        // an occurrence; operand-bearing sites are always recorded.
        let source = "use Dancer2;\nmy $n = 'x';\nuri_for_route($n);\nuri_for_route(\"v_$n\");\n";
        let ast = parse(source);
        let occurrences = extract_dancer2_named_route_occurrences(&ast, FileId(4));
        assert_eq!(occurrences.len(), 2, "all operand-bearing sites are observed");
        assert!(
            occurrences.iter().all(|occ| occ.literal_value().is_none()),
            "no dynamic site may pretend to be literal: {occurrences:?}"
        );
    }

    #[test]
    fn qualified_calls_are_not_the_admitted_family() {
        let source = "use Dancer2;\nSome::uri_for_route('x');\n";
        let ast = parse(source);
        // Qualified names arrive as different AST shapes or fully-qualified
        // call nodes; either way the bareword DSL form is what this family
        // admits. If any occurrence were produced it must not be the
        // qualified spelling.
        let occurrences = extract_dancer2_named_route_occurrences(&ast, FileId(5));
        assert!(occurrences.is_empty(), "qualified call leaked in: {occurrences:?}");
    }

    #[test]
    fn occurrences_are_source_ordered() {
        let source =
            "use Dancer2;\nget 'b', '/b', sub { uri_for_route('late') };\nget 'a', '/a', sub { uri_for_route('early') };\n";
        let ast = parse(source);
        let occurrences = extract_dancer2_named_route_occurrences(&ast, FileId(6));
        let values: Vec<Option<&str>> =
            occurrences.iter().map(|occ| occ.literal_value()).collect();
        assert_eq!(values.first().copied().flatten(), Some("late"), "source order");
    }
}

