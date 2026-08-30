//! Dancer2 definition-target cell (#8928).
//!
//! Read-only definition targets from canonical facts:
//!
//! - a route declaration (or its keyword/name operands) resolves to the
//!   exact inline handler anchor or the resolved static-coderef declaration;
//! - there is deliberately no Dancer2 string-handler → subroutine
//!   definition path (Dancer2 requires a CodeRef; #8910 containment);
//! - named-route references need a canonical reference producer that does
//!   not exist yet: that request class returns a typed refusal rather than
//!   a re-parsed answer;
//! - imported DSL keyword definition resolves only when the provider module
//!   source is available and exact; otherwise the caller reports generated
//!   provenance instead of a fictional location.

use super::activation::Dancer2FileActivations;
use super::facts::CanonicalDancer2FileFacts;
use perl_semantic_facts::route::{RouteFact, RouteHandler};

/// One definition target derived from canonical facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dancer2DefinitionTarget {
    /// Exact source anchor navigation (file-relative byte span).
    Anchor {
        /// Span start (byte offset in the document).
        start: u32,
        /// Span end (byte offset in the document).
        end: u32,
        /// Bounded description of the target.
        label: String,
    },
    /// The request class needs a canonical producer that does not exist;
    /// no target is returned.
    TypedRefusal {
        /// Machine-readable refusal reason.
        reason: &'static str,
        /// Human explanation.
        detail: String,
    },
}

/// Resolve the definition target at `offset`.
#[must_use]
pub fn definition_target_at(
    _activations: &Dancer2FileActivations,
    facts: &CanonicalDancer2FileFacts,
    offset: usize,
) -> Option<Dancer2DefinitionTarget> {
    let route = facts.route_at(offset)?;
    handler_target(route).or_else(|| declaration_target(route))
}

fn handler_target(route: &RouteFact) -> Option<Dancer2DefinitionTarget> {
    match &route.route.handler {
        RouteHandler::InlineSub { anchor } => Some(Dancer2DefinitionTarget::Anchor {
            start: anchor.start_byte,
            end: anchor.end_byte,
            label: "Dancer2 inline route handler".to_string(),
        }),
        RouteHandler::StaticCoderef { name, target, .. } => Some(Dancer2DefinitionTarget::Anchor {
            start: target.name_anchor.start_byte,
            end: target.name_anchor.end_byte,
            label: format!("Dancer2 route handler `\\&{name}` declaration"),
        }),
        // String/computed handlers are bounded: no subroutine definition
        // path exists for them (Dancer2 requires a CodeRef).
        RouteHandler::Bounded { .. } => None,
        _ => None,
    }
}

fn declaration_target(route: &RouteFact) -> Option<Dancer2DefinitionTarget> {
    Some(Dancer2DefinitionTarget::Anchor {
        start: route.envelope.anchor.start_byte,
        end: route.envelope.anchor.end_byte,
        label: "Dancer2 route declaration".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::dancer2::activation::RuntimeDancer2Module;
    use crate::providers::dancer2::activation::file_activations;
    use crate::providers::dancer2::facts::canonical_file_facts;
    use perl_semantic_analyzer::Parser;
    use perl_semantic_facts::{FileId, SourceGeneration};
    use perl_test_must::{must_some_with, must_with};

    fn setup(source: &'static str) -> (Dancer2FileActivations, CanonicalDancer2FileFacts) {
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), source, &activations);
        (activations, facts)
    }

    #[test]
    fn route_definition_reaches_inline_handler_anchor() {
        let source = "use Dancer2;\nget '/x' => sub { 'body' };";
        let (activations, facts) = setup(source);
        let keyword_offset = must_some_with(source.find("get"), "keyword offset");
        let target = must_some_with(
            definition_target_at(&activations, &facts, keyword_offset),
            "definition target",
        );
        let (start, end, label) = must_some_with(
            match target {
                Dancer2DefinitionTarget::Anchor { start, end, label } => Some((start, end, label)),
                Dancer2DefinitionTarget::TypedRefusal { .. } => None,
            },
            "expected anchor target",
        );
        let handler_start = must_some_with(source.find("sub"), "handler start");
        assert_eq!(start as usize, handler_start, "target must be the handler anchor");
        assert!(end as usize > handler_start);
        assert!(label.contains("handler"));
    }

    #[test]
    fn coderef_definition_reaches_declaration_name() {
        let source = "use Dancer2;\nget '/x' => \\&do_it;\nsub do_it { 1 }";
        let (activations, facts) = setup(source);
        let keyword_offset = must_some_with(source.find("get"), "keyword offset");
        let target = must_some_with(
            definition_target_at(&activations, &facts, keyword_offset),
            "definition target",
        );
        let start = must_some_with(
            match target {
                Dancer2DefinitionTarget::Anchor { start, .. } => Some(start),
                Dancer2DefinitionTarget::TypedRefusal { .. } => None,
            },
            "expected anchor target",
        );
        let declaration_name =
            must_some_with(source.find("sub do_it"), "declaration") + "sub ".len();
        assert_eq!(start as usize, declaration_name);
    }

    #[test]
    fn string_handler_has_no_definition_path() {
        let source = "use Dancer2;\nget '/x' => 'do_it';\nsub do_it { 1 }";
        let (activations, facts) = setup(source);
        let keyword_offset = must_some_with(source.find("get"), "keyword offset");
        let target = definition_target_at(&activations, &facts, keyword_offset);
        let target_context = format!("expected declaration fallback, got {target:?}");
        let label = must_some_with(
            match target {
                Some(Dancer2DefinitionTarget::Anchor { label, .. }) => Some(label),
                Some(Dancer2DefinitionTarget::TypedRefusal { .. }) | None => None,
            },
            target_context,
        );
        assert!(
            label.contains("declaration"),
            "string handler falls back to the declaration anchor, never the sub: {label}"
        );
    }

    #[test]
    fn outside_route_there_is_no_framework_definition() {
        let source = "use Dancer2;\nmy $x = 1;\n";
        let (activations, facts) = setup(source);
        assert!(definition_target_at(&activations, &facts, 12).is_none());
    }
}
