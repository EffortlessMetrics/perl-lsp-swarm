//! Dancer2 hover cell (#8928).
//!
//! For a canonical route declaration exposes source-backed information:
//! methods, pattern/effective pattern, route name when present,
//! application/package, static parameters/captures when available, handler
//! kind, and framework/generated provenance with limitations. For imported
//! DSL keywords exposes provider/version and global versus request-scoped
//! availability, reported from the same canonical handler-context query the
//! completion and diagnostics cells use (#13604), naming route versus hook
//! context and staying explicit when the reviewed contract establishes
//! neither. For hooks exposes normalized hook identity/alias provenance
//! where #8924 proves it. Never invents bodies or locations.

use super::activation::Dancer2FileActivations;
use super::facts::CanonicalDancer2FileFacts;
use perl_semantic_facts::framework_adapters::dancer2::{
    DANCER2_DSL_CONTRACT_VERSION, Dancer2KeywordState, DslKeywordScope,
};
use perl_semantic_facts::hook::HookNameSelection;
use perl_semantic_facts::route::{HandlerContextKind, RouteFact, RouteParameterKind};
use perl_semantic_facts::route::{
    RouteEffectivePattern, RouteHandler, RouteMethodSet, RouteNameSelection, RoutePatternKind,
};

/// One hover projection derived from canonical facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteHoverProjection {
    /// Hover over a canonical route declaration.
    Route {
        /// Rendered markdown content.
        content: String,
        /// Whether every contributing fact was exact.
        exact: bool,
    },
    /// Hover over an imported DSL keyword.
    Keyword {
        /// Rendered markdown content.
        content: String,
    },
    /// Hover over a hook declaration.
    Hook {
        /// Rendered markdown content.
        content: String,
        /// Whether the hook identity was exact.
        exact: bool,
    },
}

/// Build the hover projection at `offset`, if the Dancer2 slice owns it.
#[must_use]
pub fn hover_projection_at(
    activations: &Dancer2FileActivations,
    facts: &CanonicalDancer2FileFacts,
    ast: &perl_parser_core::Node,
    package: &str,
    offset: usize,
) -> Option<RouteHoverProjection> {
    let activation = activations.for_package(package)?;
    if !activation.facts.is_exact() {
        return None;
    }
    let version = exact_version(&activation.facts)?;

    if let Some(hook) = facts.hook_at(offset) {
        return Some(hook_hover(hook, &version));
    }
    if let Some(route) = facts.route_at(offset) {
        return Some(route_hover(route, facts, &version));
    }
    keyword_hover(&activation.facts, facts, ast, offset, &version, package)
}

/// `(package, name)` pairs of subroutine declarations in the AST.
///
/// A local `sub <name>` in the cursor's package owns the name: hover then
/// belongs to the ordinary Perl path, not to the DSL keyword.
fn declared_sub_names(
    node: &perl_parser_core::Node,
) -> std::collections::HashSet<(String, String)> {
    let mut names = std::collections::HashSet::new();
    fn walk(
        node: &perl_parser_core::Node,
        package: &mut String,
        names: &mut std::collections::HashSet<(String, String)>,
    ) {
        if let perl_parser_core::NodeKind::Package { name, .. } = &node.kind {
            *package = name.clone();
        }
        if let perl_parser_core::NodeKind::Subroutine { name: Some(name), .. } = &node.kind {
            names.insert((package.clone(), name.clone()));
        }
        for child in node.children() {
            walk(child, package, names);
        }
    }
    let mut package = "main".to_string();
    walk(node, &mut package, &mut names);
    names
}

fn exact_version(
    facts: &perl_semantic_facts::framework_adapters::dancer2::Dancer2ActivationFacts,
) -> Option<String> {
    match &facts.state {
        perl_semantic_facts::framework_adapters::dancer2::Dancer2ActivationState::Exact {
            framework_version,
            ..
        } => Some(framework_version.clone()),
        _ => None,
    }
}

fn route_hover(
    fact: &RouteFact,
    facts: &CanonicalDancer2FileFacts,
    version: &str,
) -> RouteHoverProjection {
    let route = &fact.route;
    let mut lines = Vec::new();
    lines.push(format!("**Dancer2 route** (`Dancer2` {})", version));

    match &route.methods {
        RouteMethodSet::Exact(names) => {
            lines.push(format!("- methods: {}", names.join(", ")));
        }
        RouteMethodSet::Dynamic { reason } => {
            lines.push(format!("- methods: computed (dynamic boundary: {reason})"));
        }
        _ => lines.push("- methods: unknown".to_string()),
    }
    if let Some(pattern) = &route.pattern.value {
        lines.push(format!(
            "- pattern: `{pattern}` ({})",
            match route.pattern.kind {
                RoutePatternKind::Literal => "literal",
                RoutePatternKind::Regex => "regex",
                RoutePatternKind::Dynamic => "dynamic",
                _ => "unknown",
            }
        ));
    }
    match &route.effective_pattern {
        RouteEffectivePattern::Composed { value, .. } => {
            lines.push(format!("- effective pattern: `{value}`"));
        }
        RouteEffectivePattern::Local { value } => {
            lines.push(format!("- effective pattern: `{value}`"));
        }
        RouteEffectivePattern::Boundary { reason } => {
            lines.push(format!("- effective pattern: not proven ({reason})"));
        }
        _ => lines.push("- effective pattern: unknown".to_string()),
    }
    match &route.route_name {
        RouteNameSelection::Literal(name) => lines.push(format!("- route name: `{}`", name.value)),
        RouteNameSelection::Dynamic { reason, .. } => {
            lines.push(format!("- route name: computed (dynamic boundary: {reason})"));
        }
        RouteNameSelection::Absent => {}
        _ => {}
    }
    lines.push(format!("- application/package: `{}`", fact.application_name));

    let declaration_index = route.declaration_index;
    let parameters: Vec<&str> = facts
        .parameters
        .iter()
        .filter(|parameter| parameter.route_declaration_index == declaration_index)
        .filter_map(|parameter| parameter.parameter.name.as_deref())
        .collect();
    if !parameters.is_empty() {
        lines.push(format!("- parameters: {}", parameters.join(", ")));
    }
    let unsupported = facts.parameters.iter().any(|parameter| {
        parameter.route_declaration_index == declaration_index
            && matches!(parameter.parameter.kind, RouteParameterKind::CaptureUnsupported)
    });
    if unsupported {
        lines.push("- captures: includes unsupported capture tokens (bounded)".to_string());
    }

    lines.push(format!(
        "- handler: {}",
        match &route.handler {
            RouteHandler::InlineSub { .. } => "inline anonymous sub (exact anchor)".to_string(),
            RouteHandler::StaticCoderef { name, .. } => {
                format!("static coderef `\\&{name}` (exact declaration target)")
            }
            RouteHandler::Bounded { reason, .. } => {
                format!("bounded relation ({reason})")
            }
            _ => "unknown".to_string(),
        }
    ));
    lines.push(format!(
        "- provenance: canonical framework fact, DSL contract `{}`; generated/framework \
         projection anchored to source — no fictional body",
        DANCER2_DSL_CONTRACT_VERSION
    ));
    let exact = fact.status() == perl_semantic_facts::SemanticFactStatus::Exact;
    if !exact {
        lines.push(
            "- limitations: payload carries a typed boundary; values above may be partial"
                .to_string(),
        );
    }
    RouteHoverProjection::Route { content: lines.join("\n"), exact }
}

fn hook_hover(fact: &perl_semantic_facts::hook::HookFact, version: &str) -> RouteHoverProjection {
    let mut lines = Vec::new();
    lines.push(format!("**Dancer2 hook** (`Dancer2` {})", version));
    match &fact.hook.name {
        HookNameSelection::Literal(name) => {
            lines.push(format!("- hook: `{}`", name.literal));
            if let Some(canonical) = name.canonical()
                && canonical != name.literal
            {
                lines.push(format!("- normalized identity: `{canonical}` (alias provenance)"));
            }
            if name.is_boundary() {
                lines.push("- normalization: boundary (unreviewed alias)".to_string());
            }
        }
        HookNameSelection::Dynamic { reason, .. } => {
            lines.push(format!("- hook name: computed (dynamic boundary: {reason})"));
        }
        _ => {}
    }
    lines.push(format!("- application: `{}`", fact.application_name));
    lines.push("- handler kind: see #8924 handler contract (inline/coderef/bounded)".to_string());
    lines.push("- provenance: canonical framework fact anchored to source".to_string());
    let exact = fact.status() == perl_semantic_facts::SemanticFactStatus::Exact;
    RouteHoverProjection::Hook { content: lines.join("\n"), exact }
}

fn keyword_hover(
    facts: &perl_semantic_facts::framework_adapters::dancer2::Dancer2ActivationFacts,
    file_facts: &CanonicalDancer2FileFacts,
    ast: &perl_parser_core::Node,
    offset: usize,
    version: &str,
    package: &str,
) -> Option<RouteHoverProjection> {
    // Hover on a DSL keyword usage (identifier or call) whose name the
    // activation imports. Scope availability comes from the canonical
    // keyword facts; handler-only keywords explain their availability.
    let vocabulary: std::collections::HashSet<&str> = facts
        .keywords
        .iter()
        .filter(|keyword| keyword.state == Dancer2KeywordState::Imported)
        .map(|keyword| keyword.keyword.as_str())
        .collect();
    let mut found = None;
    find_keyword_usage(ast, offset, &vocabulary, &mut found);
    let keyword_name = found?;
    let declared = declared_sub_names(ast);
    if declared.contains(&(package.to_string(), keyword_name.clone())) {
        // A local `sub <name>` declaration owns the name: the ordinary
        // Perl hover path covers it.
        return None;
    }
    let content = format!(
        "**Dancer2 DSL keyword `{keyword_name}`** (`Dancer2` {version})\n- availability: \
         {}\n- keyword contract: `{DANCER2_DSL_CONTRACT_VERSION}`\n- provenance: \
         canonical import fact of this activation (package `{package}`)",
        match facts.keywords.iter().find(|keyword| keyword.keyword == keyword_name)?.scope {
            DslKeywordScope::Global => {
                "global (available in any package scope that activated the DSL)".to_string()
            }
            DslKeywordScope::RouteHandlerOnly => request_context_availability(file_facts, offset),
            _ => "unknown scope".to_string(),
        },
    );
    Some(RouteHoverProjection::Keyword { content })
}

/// Describe request-scoped keyword availability at `offset`.
///
/// Availability comes from the one canonical handler-context query (#13604),
/// so hover, completion, and diagnostics agree by construction. The wording
/// names the owning declaration kind when there is one, and stays explicitly
/// non-committal for a handler position whose request context the reviewed
/// contract does not establish.
fn request_context_availability(facts: &CanonicalDancer2FileFacts, offset: usize) -> String {
    match facts.request_context_at(offset) {
        Some(context) if context.establishes_request_context() => match context.handler_kind {
            HandlerContextKind::Hook => {
                "request context (available here: this is an exact Dancer2 hook handler whose \
                 reviewed position runs with the current request)"
                    .to_string()
            }
            _ => "request context (available here: this is an exact Dancer2 route handler)"
                .to_string(),
        },
        Some(_) => "request context (this is an exact Dancer2 hook handler, but the reviewed hook \
                    contract does not establish request context for this hook position)"
            .to_string(),
        None => "request context (available inside exact route handlers and admitted hook \
                 handlers; this position is neither)"
            .to_string(),
    }
}

fn find_keyword_usage(
    node: &perl_parser_core::Node,
    offset: usize,
    vocabulary: &std::collections::HashSet<&str>,
    found: &mut Option<String>,
) {
    if found.is_some() {
        return;
    }
    // Innermost match wins. A route or hook declaration is itself a keyword
    // call whose span covers its whole handler body, so a top-down match
    // would answer `get` for a cursor sitting on `params` inside the body.
    // Descending first keeps the answer the keyword actually under the
    // cursor; the enclosing declaration still answers when the cursor is on
    // its own keyword token, because no child covers that offset.
    for child in node.children() {
        find_keyword_usage(child, offset, vocabulary, found);
        if found.is_some() {
            return;
        }
    }
    let covers = node.location.start <= offset && offset < node.location.end;
    if covers {
        let name = match &node.kind {
            perl_parser_core::NodeKind::Identifier { name }
            | perl_parser_core::NodeKind::FunctionCall { name, .. } => Some(name),
            _ => None,
        };
        if let Some(name) = name
            && vocabulary.contains(name.as_str())
        {
            *found = Some(name.clone());
        }
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

    struct Setup {
        activations: Dancer2FileActivations,
        facts: CanonicalDancer2FileFacts,
        ast: perl_parser_core::Node,
    }

    fn setup(source: &'static str) -> Setup {
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), source, &activations);
        Setup { activations, facts, ast }
    }

    #[test]
    fn get_route_hover_reports_get_head_and_name_pattern_separately() {
        let source = "use Dancer2;\nget '/users/:id' => sub { 1 };";
        let setup = setup(source);
        let offset = must_some_with(source.find("/users/:id"), "pattern offset");
        let projection = must_some_with(
            hover_projection_at(&setup.activations, &setup.facts, &setup.ast, "main", offset),
            "route hover",
        );
        let (content, exact) = must_some_with(
            match projection {
                RouteHoverProjection::Route { content, exact } => Some((content, exact)),
                RouteHoverProjection::Keyword { .. } | RouteHoverProjection::Hook { .. } => None,
            },
            "expected route projection",
        );
        assert!(exact);
        assert!(content.contains("GET, HEAD"), "{content}");
        assert!(content.contains("pattern: `/users/:id`"), "{content}");
        assert!(content.contains("parameters: id"), "{content}");
        assert!(content.contains("Dancer2 route"), "{content}");
    }

    #[test]
    fn named_route_hover_includes_name_and_pattern_separately() {
        let source = "use Dancer2;\nget 'user_show', '/users/:id' => sub { 1 };";
        let setup = setup(source);
        let offset = must_some_with(source.find("/users/:id"), "pattern offset");
        let projection = must_some_with(
            hover_projection_at(&setup.activations, &setup.facts, &setup.ast, "main", offset),
            "route hover",
        );
        let (content, _exact) = must_some_with(
            match projection {
                RouteHoverProjection::Route { content, exact } => Some((content, exact)),
                RouteHoverProjection::Keyword { .. } | RouteHoverProjection::Hook { .. } => None,
            },
            "expected route projection",
        );
        assert!(content.contains("route name: `user_show`"), "{content}");
        assert!(content.contains("pattern: `/users/:id`"), "{content}");
    }

    #[test]
    fn hook_hover_exposes_identity() {
        let source = "use Dancer2;\nhook 'before' => sub { 1 };";
        let setup = setup(source);
        let offset = must_some_with(source.find("before"), "hook name offset");
        let projection = must_some_with(
            hover_projection_at(&setup.activations, &setup.facts, &setup.ast, "main", offset),
            "hook hover",
        );
        let (content, exact) = must_some_with(
            match projection {
                RouteHoverProjection::Hook { content, exact } => Some((content, exact)),
                RouteHoverProjection::Route { .. } | RouteHoverProjection::Keyword { .. } => None,
            },
            "expected hook projection",
        );
        assert!(exact);
        assert!(content.contains("Dancer2 hook"), "{content}");
        assert!(content.contains("hook: `before`"), "{content}");
    }

    #[test]
    fn no_activation_yields_no_framework_hover() {
        let source = "get '/x' => sub { 1 };";
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let activations = file_activations(&ast, FileId(1), None, &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), source, &activations);
        assert!(hover_projection_at(&activations, &facts, &ast, "main", 5).is_none());
    }

    /// Hover content for a request-scoped keyword usage at `offset`.
    fn keyword_hover_content(source: &'static str, needle: &str) -> String {
        let setup = setup(source);
        let offset = must_some_with(source.find(needle), "keyword offset");
        let projection = must_some_with(
            hover_projection_at(&setup.activations, &setup.facts, &setup.ast, "main", offset),
            "keyword hover",
        );
        must_some_with(
            match projection {
                RouteHoverProjection::Keyword { content } => Some(content),
                RouteHoverProjection::Route { .. } | RouteHoverProjection::Hook { .. } => None,
            },
            "expected keyword projection",
        )
    }

    #[test]
    fn hover_answers_the_keyword_under_the_cursor_not_the_enclosing_declaration() {
        // A declaration's call span covers its whole handler body, so the
        // keyword lookup must resolve innermost-first. Both directions are
        // pinned: the inner keyword inside the body, and the declaration
        // keyword on its own token.
        let source = "use Dancer2;\nget '/x' => sub { params; };";
        assert!(
            keyword_hover_content(source, "params").contains("keyword `params`"),
            "cursor inside the body must answer `params`"
        );
        let setup = setup(source);
        let get_offset = must_some_with(source.find("get"), "get keyword offset");
        let projection = must_some_with(
            hover_projection_at(&setup.activations, &setup.facts, &setup.ast, "main", get_offset),
            "route hover",
        );
        // The route declaration itself still owns its own keyword token.
        assert!(matches!(projection, RouteHoverProjection::Route { .. }), "{projection:?}");
    }

    #[test]
    fn a_hook_name_operand_colliding_with_a_keyword_still_hovers_as_the_hook() {
        // `redirect` is both a real imported DSL keyword and, here, the hook's
        // own name operand. The declaration owns that token: hover must answer
        // the hook, not report a keyword usage at a position that is not one.
        let source = "use Dancer2;\nhook redirect => sub { 1 };";
        let setup = setup(source);
        let operand = must_some_with(source.find("redirect"), "hook name operand offset");
        let projection = must_some_with(
            hover_projection_at(&setup.activations, &setup.facts, &setup.ast, "main", operand),
            "hover at the hook name operand",
        );
        assert!(
            matches!(projection, RouteHoverProjection::Hook { .. }),
            "the hook declaration owns its name operand: {projection:?}"
        );
    }

    #[test]
    fn hover_inside_a_route_handler_names_the_route_request_context() {
        let content = keyword_hover_content("use Dancer2;\nget '/x' => sub { params; };", "params");
        assert!(content.contains("request context"), "{content}");
        assert!(content.contains("route handler"), "{content}");
    }

    #[test]
    fn hover_inside_an_admitted_hook_handler_names_the_hook_request_context() {
        let content =
            keyword_hover_content("use Dancer2;\nhook before => sub { params; };", "params");
        assert!(content.contains("request context"), "{content}");
        assert!(content.contains("hook handler"), "{content}");
        assert!(
            !content.contains("does not establish"),
            "an admitted position must not read as unproven: {content}"
        );
    }

    #[test]
    fn hover_inside_an_unadmitted_hook_handler_says_availability_is_not_established() {
        let content = keyword_hover_content(
            "use Dancer2;\nhook before_template_render => sub { params; };",
            "params",
        );
        assert!(
            content.contains("does not establish request context"),
            "an unproven position must say so rather than claim availability: {content}"
        );
    }

    #[test]
    fn hover_outside_every_handler_does_not_claim_availability_here() {
        let content = keyword_hover_content("use Dancer2;\nmy $p = params;", "params");
        assert!(content.contains("this position is neither"), "{content}");
    }

    #[test]
    fn handler_only_keyword_scope_is_documented() {
        let source = "use Dancer2;\nget '/x' => sub { params; };";
        let setup = setup(source);
        let _ = setup.facts;
        let activation = must_some_with(setup.activations.for_package("main"), "activation");
        let params = must_some_with(
            activation.facts.keywords.iter().find(|k| k.keyword == "params"),
            "params keyword",
        );
        assert_eq!(params.scope, DslKeywordScope::RouteHandlerOnly);
    }
}
