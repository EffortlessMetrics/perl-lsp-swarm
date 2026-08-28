//! Bounded Dancer2 diagnostics cell (#8928).
//!
//! Only diagnostics whose truth is fully established by canonical facts:
//!
//! 1. **excluded keyword used** — a route declaration whose keyword the
//!    activating import excluded with a literal `!keyword`. The exclusion
//!    and the declaration are both exact source facts.
//! 2. **handler-only keyword used outside an exact route handler** — under
//!    the default DSL, the reviewed route-handler-only vocabulary is
//!    available only inside an exact inline handler; a use outside every
//!    minted handler context is a typed diagnostic.
//!
//! Deliberately absent: generic duplicate-route and route-reachability
//! diagnostics (out of scope), and unknown literal named-route references
//! (the canonical reference/query producer does not exist yet; that class
//! stays unreported rather than guessed).

use super::activation::Dancer2FileActivations;
use super::facts::CanonicalDancer2FileFacts;
use perl_parser_core::{Node, NodeKind};
use perl_semantic_analyzer::declaration::current_package_at;
use perl_semantic_facts::framework_adapters::dancer2::{
    DANCER2_DSL_CONTRACT_VERSION, Dancer2KeywordState, DslKeywordScope,
};

/// One bounded Dancer2 diagnostic.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2BoundedDiagnostic {
    /// Diagnostic code (`dancer2.excluded-keyword-used`,
    /// `dancer2.handler-only-keyword-outside-handler`).
    pub code: &'static str,
    /// Human message.
    pub message: String,
    /// Span start (byte offset).
    pub start: u32,
    /// Span end (byte offset).
    pub end: u32,
}

/// Compute the bounded Dancer2 diagnostics for one document.
///
/// Returns an empty vector unless a package is exactly activated with the
/// default DSL.
#[must_use]
pub fn bounded_diagnostics(
    ast: &Node,
    activations: &Dancer2FileActivations,
    facts: &CanonicalDancer2FileFacts,
) -> Vec<Dancer2BoundedDiagnostic> {
    let mut diagnostics = Vec::new();
    if !activations.has_exact() {
        return diagnostics;
    }

    for activation in &activations.packages {
        if !activation.facts.is_exact() {
            continue;
        }
        let package = activation.package.as_str();

        // (1) excluded route keyword used by a declaration.
        for declaration in &facts.extracted_routes {
            if declaration.package.as_deref() != Some(package) {
                continue;
            }
            let Some(keyword_fact) = activation
                .facts
                .keywords
                .iter()
                .find(|keyword| keyword.keyword == declaration.route.keyword)
            else {
                continue;
            };
            if keyword_fact.state == Dancer2KeywordState::Excluded {
                diagnostics.push(Dancer2BoundedDiagnostic {
                    code: "dancer2.excluded-keyword-used",
                    message: format!(
                        "`{}` was excluded by this activation's `!{}` import; the declaration \
                         is not a route of this application (DSL contract \
                         {DANCER2_DSL_CONTRACT_VERSION})",
                        declaration.route.keyword, declaration.route.keyword
                    ),
                    start: declaration.route.keyword_anchor.start_byte,
                    end: declaration.route.keyword_anchor.end_byte,
                });
            }
        }

        // (2) handler-only keyword used outside every exact handler context.
        let handler_only: Vec<&str> = activation
            .facts
            .keywords
            .iter()
            .filter(|keyword| {
                keyword.state == Dancer2KeywordState::Imported
                    && keyword.scope == DslKeywordScope::RouteHandlerOnly
            })
            .map(|keyword| keyword.keyword.as_str())
            .collect();
        if !handler_only.is_empty() {
            let mut usages = Vec::new();
            collect_keyword_usages(ast, &handler_only, &mut usages);
            let declared = declared_sub_names(ast);
            let hook_handler_spans: Vec<(u32, u32)> = facts
                .hooks
                .iter()
                .filter_map(|hook| match &hook.hook.handler {
                    perl_semantic_facts::handler::FrameworkHandler::InlineSub { anchor }
                    | perl_semantic_facts::handler::FrameworkHandler::StaticCoderef {
                        anchor,
                        ..
                    } => Some((anchor.start_byte, anchor.end_byte)),
                    perl_semantic_facts::handler::FrameworkHandler::Bounded { anchor, .. } => {
                        anchor.map(|span| (span.start_byte, span.end_byte))
                    }
                    _ => None,
                })
                .collect();
            for (name, start, end) in usages {
                if declared.contains(&name) {
                    // A local `sub <name>` declaration owns the name: using
                    // it is ordinary Perl, not a framework keyword use.
                    continue;
                }
                let offset = usize::try_from(start).unwrap_or(0);
                if facts.inside_handler_context(offset) {
                    continue;
                }
                // Hook bodies are an unmodeled request context (#8924 owns
                // hook facts; no handler-context fact exists for them), so a
                // handler-only keyword inside a hook handler stays unreported
                // rather than misleadingly flagged.
                if hook_handler_spans.iter().any(|(hook_start, hook_end)| {
                    usize::try_from(*hook_start).ok() <= Some(offset)
                        && offset < usize::try_from(*hook_end).unwrap_or(0)
                }) {
                    continue;
                }
                if current_package_at(ast, offset) != package {
                    continue;
                }
                diagnostics.push(Dancer2BoundedDiagnostic {
                    code: "dancer2.handler-only-keyword-outside-handler",
                    message: format!(
                        "`{name}` is a route-handler-only Dancer2 keyword; outside an exact \
                         route handler it has no defined meaning (DSL contract \
                         {DANCER2_DSL_CONTRACT_VERSION})"
                    ),
                    start,
                    end,
                });
            }
        }
    }
    diagnostics.sort_by_key(|diagnostic| diagnostic.start);
    diagnostics
}

/// Collect identifier/function-call usages of the given keyword names.
///
/// A source observation over AST nodes only — string literals never appear
/// as identifiers, and the route declaration keywords themselves are not in
/// the handler-only vocabulary.
fn collect_keyword_usages(node: &Node, names: &[&str], out: &mut Vec<(String, u32, u32)>) {
    match &node.kind {
        NodeKind::Identifier { name } | NodeKind::FunctionCall { name, .. }
            if names.contains(&name.as_str()) =>
        {
            out.push((
                name.clone(),
                u32::try_from(node.location.start).unwrap_or(0),
                u32::try_from(node.location.end).unwrap_or(0),
            ));
        }
        _ => {}
    }
    for child in node.children() {
        collect_keyword_usages(child, names, out);
    }
}

/// Names of subroutine declarations in the file (any package).
fn declared_sub_names(node: &Node) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    collect_declared_sub_names(node, &mut names);
    names
}

fn collect_declared_sub_names(node: &Node, names: &mut std::collections::HashSet<String>) {
    if let NodeKind::Subroutine { name: Some(name), .. } = &node.kind {
        names.insert(name.clone());
    }
    for child in node.children() {
        collect_declared_sub_names(child, names);
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
    use perl_test_must::{must_some_with, must_with};

    fn setup(source: &'static str) -> (Dancer2FileActivations, CanonicalDancer2FileFacts, Node) {
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), &activations);
        (activations, facts, ast)
    }

    #[test]
    fn excluded_keyword_use_is_reported() {
        let source = "use Dancer2 '!get';\nget '/x' => sub { 1 };";
        let (activations, facts, ast) = setup(source);
        let diagnostics = bounded_diagnostics(&ast, &activations, &facts);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "dancer2.excluded-keyword-used");
        let declaration_get_offset =
            must_some_with(source.rfind("get"), "declaration keyword offset");
        assert_eq!(diagnostics[0].start as usize, declaration_get_offset);
    }

    #[test]
    fn handler_only_keyword_outside_handler_is_reported() {
        let source = "use Dancer2;\nmy $x = params;\nget '/x' => sub { splat; };";
        let (activations, facts, ast) = setup(source);
        let diagnostics = bounded_diagnostics(&ast, &activations, &facts);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "dancer2.handler-only-keyword-outside-handler");
        assert!(diagnostics[0].message.contains("params"));
        let params_offset = must_some_with(source.find("params"), "params offset");
        assert_eq!(diagnostics[0].start as usize, params_offset);
    }

    #[test]
    fn inside_handler_use_is_not_reported() {
        let source = "use Dancer2;\nget '/x' => sub { my $p = params; };";
        let (activations, facts, ast) = setup(source);
        assert!(bounded_diagnostics(&ast, &activations, &facts).is_empty());
    }

    #[test]
    fn no_activation_reports_nothing() {
        let source = "my $x = params;";
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let activations = file_activations(&ast, FileId(1), None, &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), &activations);
        assert!(bounded_diagnostics(&ast, &activations, &facts).is_empty());
    }

    #[test]
    fn ordinary_perl_names_are_never_reported() {
        let source = "use Dancer2;\nsub params { 1 }\nparams();";
        let (activations, facts, ast) = setup(source);
        let diagnostics = bounded_diagnostics(&ast, &activations, &facts);
        // A local `sub params` declaration owns the name; using it is
        // ordinary Perl, not a framework keyword use.
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
