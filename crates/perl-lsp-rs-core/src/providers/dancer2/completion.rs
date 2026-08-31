//! Dancer2 keyword completion cell (#8928).
//!
//! Offers imported default-DSL keywords from the canonical
//! [`Dancer2KeywordImportFact`]s under exact activation:
//!
//! - `!keyword` exclusions are honored: an excluded keyword is never offered;
//! - request-scoped keywords (the reviewed `is_global => 0` vocabulary) are
//!   offered only where the canonical handler-context facts establish request
//!   context: inside an exact inline route handler (#8921) or inside an
//!   admitted inline hook handler (#13604). One query answers both, so this
//!   cell keeps no syntax heuristic of its own;
//! - keywords never swamp ordinary lexical completion: every keyword item
//!   carries [`KEYWORD_RANK_PENALTY`] so the runtime sorts local
//!   variables/subroutines ahead of framework keywords, and keywords whose
//!   name is locally declared as a subroutine in the file are suppressed
//!   (the local declaration owns the name);
//! - custom/dynamic DSL classes never receive default-keyword facts.

use super::activation::Dancer2FileActivations;
use super::facts::CanonicalDancer2FileFacts;
use perl_semantic_facts::framework_adapters::dancer2::{
    DANCER2_DSL_CONTRACT_VERSION, Dancer2KeywordState, DslKeywordScope,
};
use perl_semantic_facts::route::HandlerContextKind;

/// Sort penalty applied to Dancer2 keyword completion items so ordinary
/// lexical/workspace results rank ahead of framework keywords.
pub const KEYWORD_RANK_PENALTY: u32 = 1_000;

/// One Dancer2 keyword completion candidate derived from canonical facts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2CompletionCandidate {
    /// Keyword label (`get`, `prefix`, ...).
    pub label: String,
    /// Reviewed availability scope.
    pub scope: DslKeywordScope,
    /// Rank penalty to add (framework keywords rank below local results).
    pub rank_penalty: u32,
    /// One-line detail: provider, version, and scope.
    pub detail: String,
    /// Versioned DSL contract provenance.
    pub dsl_contract_version: &'static str,
}

/// Build keyword completion candidates for the package at `offset`.
///
/// Returns an empty vector unless that package's activation is exact with
/// the default DSL. `locally_declared_subnames` suppresses keywords whose
/// name a local `sub` declaration already owns in this file.
#[must_use]
pub fn keyword_completion_candidates(
    activations: &Dancer2FileActivations,
    facts: &CanonicalDancer2FileFacts,
    package: &str,
    offset: usize,
    locally_declared_subnames: &dyn Fn(&str) -> bool,
) -> Vec<Dancer2CompletionCandidate> {
    let Some(activation) = activations.for_package(package) else {
        return Vec::new();
    };
    if !activation.facts.is_exact() {
        return Vec::new();
    }
    let version = match &activation.facts.state {
        perl_semantic_facts::framework_adapters::dancer2::Dancer2ActivationState::Exact {
            framework_version,
            ..
        } => framework_version.clone(),
        _ => return Vec::new(),
    };
    // One canonical context query: route handlers and admitted hook handlers
    // both establish request context, and nothing else does (#13604).
    let request_context = facts.request_context_at(offset);
    // The only context that may offer a request-scoped keyword is one that
    // establishes request context, so narrow to that once. Keeping the
    // narrowed context rather than a separate boolean means the decision to
    // offer the keyword and the description of where it is offered cannot
    // disagree: both read this single value.
    let established_context =
        request_context.filter(|context| context.establishes_request_context());
    let mut candidates = Vec::new();
    for keyword in &activation.facts.keywords {
        if keyword.state != Dancer2KeywordState::Imported {
            // `!keyword` at the activating import: never offered.
            continue;
        }
        if locally_declared_subnames(&keyword.keyword) {
            // A same-named local subroutine owns the name in this file;
            // ordinary Perl completion covers it.
            continue;
        }
        // The rendered scope names where the keyword is available *here*.
        // Saying "route handler only" inside an admitted hook handler would
        // contradict the very position that just offered it.
        let scope_detail = match keyword.scope {
            DslKeywordScope::Global => "global",
            DslKeywordScope::RouteHandlerOnly => {
                // A request-scoped keyword is offered only from a context that
                // establishes request context; without one there is nothing
                // honest to say about where it applies, so it is not offered.
                // Deciding that here, rather than in a separate guard, is what
                // keeps an unreachable description from existing at all.
                let Some(context) = established_context else {
                    continue;
                };
                match context.handler_kind {
                    HandlerContextKind::Hook => "request-scoped, in this hook handler",
                    _ => "request-scoped, in this route handler",
                }
            }
            _ => "unknown",
        };
        candidates.push(Dancer2CompletionCandidate {
            label: keyword.keyword.clone(),
            scope: keyword.scope,
            rank_penalty: KEYWORD_RANK_PENALTY,
            detail: format!(
                "Dancer2 {} keyword ({} — {})",
                &version, scope_detail, DANCER2_DSL_CONTRACT_VERSION
            ),
            dsl_contract_version: DANCER2_DSL_CONTRACT_VERSION,
        });
    }
    candidates
}

/// The rank penalty re-exported for runtime ranking decisions.
#[must_use]
pub fn keyword_completion_rank_penalty(candidate: &Dancer2CompletionCandidate) -> u32 {
    candidate.rank_penalty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::dancer2::activation::RuntimeDancer2Module;
    use crate::providers::dancer2::activation::file_activations;
    use crate::providers::dancer2::facts::canonical_file_facts;
    use perl_semantic_analyzer::Parser;
    use perl_semantic_facts::{FileId, SourceGeneration};

    fn setup(source: &'static str) -> (Dancer2FileActivations, CanonicalDancer2FileFacts) {
        let mut parser = Parser::new(source);
        let ast = parser.parse().expect("fixture must parse");
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", "1.1.1");
        let activations =
            file_activations(&ast, FileId(1), Some(&module), &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), &activations);
        (activations, facts)
    }

    fn none_declared(_: &str) -> bool {
        false
    }

    #[test]
    fn bare_activation_offers_global_keywords() {
        let (activations, facts) = setup("use Dancer2;\nget '/x' => sub { 1 };\n");
        // Offset at the `get` route keyword: outside every handler body.
        let keyword_offset = "use Dancer2;\n".len();
        let candidates = keyword_completion_candidates(
            &activations,
            &facts,
            "main",
            keyword_offset,
            &none_declared,
        );
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        for expected in ["get", "post", "prefix", "hook", "set", "template"] {
            assert!(labels.contains(&expected), "missing {expected} in {labels:?}");
        }
        assert!(
            labels.iter().all(|label| *label != "params"),
            "handler-only keyword must not be offered outside a handler: {labels:?}"
        );
    }

    #[test]
    fn inside_handler_offers_handler_only_keywords() {
        let source = "use Dancer2;\nget '/x' => sub { params; };\n";
        let (activations, facts) = setup(source);
        let handler_offset = source.find("params").expect("handler body offset");
        let candidates = keyword_completion_candidates(
            &activations,
            &facts,
            "main",
            handler_offset,
            &none_declared,
        );
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"params"), "handler-only keyword offered inside handler");
        assert!(labels.contains(&"splat"), "splat offered inside handler");
    }

    #[test]
    fn inside_an_admitted_hook_handler_offers_request_scoped_keywords() {
        // The claim of #13604: an inline `hook before` body is a request
        // context, so the editor must offer the same request helpers it
        // offers inside a route handler.
        let source = "use Dancer2;\nhook before => sub { my $r = request; };\n";
        let (activations, facts) = setup(source);
        let inside = source.find("request").expect("hook body offset");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        for expected in ["request", "params", "redirect", "cookie", "session"] {
            assert!(
                labels.contains(&expected),
                "`{expected}` must be offered inside an admitted hook handler: {labels:?}"
            );
        }
    }

    #[test]
    fn the_offered_scope_detail_names_the_position_that_offered_it() {
        // The detail must not contradict the location: a keyword offered
        // inside a hook handler cannot describe itself as route-handler-only.
        let hook_source = "use Dancer2;\nhook before => sub { my $r = request; };\n";
        let (activations, facts) = setup(hook_source);
        let inside = hook_source.find("request").expect("hook body offset");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared);
        let request = candidates
            .iter()
            .find(|candidate| candidate.label == "request")
            .expect("request offered inside an admitted hook handler");
        assert!(request.detail.contains("hook handler"), "{}", request.detail);
        assert!(
            !request.detail.contains("route handler only"),
            "stale scope wording: {}",
            request.detail
        );

        // A route handler still says route, so the wording tracks the owning
        // context rather than being blanket-renamed.
        let route_source = "use Dancer2;\nget '/x' => sub { my $p = params; };\n";
        let (activations, facts) = setup(route_source);
        let inside = route_source.find("params").expect("route body offset");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared);
        let params = candidates
            .iter()
            .find(|candidate| candidate.label == "params")
            .expect("params offered inside a route handler");
        assert!(params.detail.contains("route handler"), "{}", params.detail);
    }

    #[test]
    fn nested_blocks_inside_a_hook_handler_stay_in_request_context() {
        let source = "use Dancer2;\nhook before => sub { if (1) { my $r = request; } };\n";
        let (activations, facts) = setup(source);
        let inside = source.find("request").expect("nested body offset");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"request"), "nested block keeps the context: {labels:?}");
    }

    #[test]
    fn a_hook_position_without_established_request_context_offers_nothing_extra() {
        // `before_template_render` is a reviewed canonical position, but the
        // reviewed contract does not establish request context there, so
        // availability must not be claimed.
        let source = "use Dancer2;\nhook before_template_render => sub { my $r = request; };\n";
        let (activations, facts) = setup(source);
        let inside = source.find("my $r").expect("hook body offset");
        // Guard against a vacuous pass: the interval must really exist and
        // really be unadmitted, not be missing because the hook never minted.
        let context = facts.request_context_at(inside).expect("hook handler interval exists");
        assert_eq!(context.handler_kind, HandlerContextKind::Hook);
        assert!(!context.establishes_request_context());
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(
            !labels.contains(&"request"),
            "unadmitted hook position must not offer request helpers: {labels:?}"
        );
        // Global keywords remain available: this is not a dead zone.
        assert!(labels.contains(&"get"), "global keywords stay offered: {labels:?}");
    }

    #[test]
    fn hook_spelling_alone_never_creates_a_request_context() {
        // No Dancer2 activation: `hook` is an ordinary bareword and nothing
        // about its shape may mint availability.
        let source = "hook before => sub { my $r = request; };\n";
        let mut parser = Parser::new(source);
        let ast = parser.parse().expect("fixture must parse");
        let activations = file_activations(&ast, FileId(1), None, &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), &activations);
        let inside = source.find("request").expect("body offset");
        assert!(
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared)
                .is_empty(),
            "hook-like spelling without activation offers nothing"
        );
    }

    #[test]
    fn a_comment_before_the_fat_comma_keeps_the_hook_request_context() {
        // Perl auto-quotes across a comment, so this is still `hook 'before'`
        // and its body is still a request context. Skipping only whitespace
        // would silently withhold the helpers here.
        let source = "use Dancer2;\nhook before # a note\n    => sub { my $r = request; };\n";
        let (activations, facts) = setup(source);
        let inside = source.find("my $r").expect("hook body offset");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.contains(&"request"),
            "a commented fat comma still yields an admitted hook: {labels:?}"
        );
    }

    #[test]
    fn a_comma_separated_bareword_hook_operand_establishes_no_request_context() {
        // `hook(before, sub {...})` calls `before()`; no fat comma, so no
        // auto-quoting and no proven hook identity. The body must not inherit
        // the admitted position's request context.
        let source = "use Dancer2;\nhook(before, sub { my $r = request; });\n";
        let (activations, facts) = setup(source);
        let inside = source.find("my $r").expect("hook body offset");
        let context =
            facts.request_context_at(inside).expect("an inline body still owns an interval");
        assert!(
            !context.establishes_request_context(),
            "an unproven hook name must not establish request context"
        );
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(
            !labels.contains(&"request"),
            "request helpers must not be offered here: {labels:?}"
        );
    }

    #[test]
    fn an_exclusion_still_wins_inside_an_admitted_hook_handler() {
        let source = "use Dancer2 '!request';\nhook before => sub { my $r = request; };\n";
        let (activations, facts) = setup(source);
        let inside = source.find("my $r").expect("hook body offset");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(
            !labels.contains(&"request"),
            "`!request` exclusion outranks hook request context: {labels:?}"
        );
        assert!(labels.contains(&"params"), "other helpers stay available: {labels:?}");
    }

    #[test]
    fn an_adjacent_ordinary_sub_is_not_a_request_context() {
        let source = "use Dancer2;\nhook before => sub { 1 };\nsub helper { my $r = request; }\n";
        let (activations, facts) = setup(source);
        let inside = source.find("my $r").expect("adjacent sub offset");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", inside, &none_declared);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(
            !labels.contains(&"request"),
            "an adjacent sub is outside the handler interval: {labels:?}"
        );
    }

    #[test]
    fn excluded_keyword_is_never_offered() {
        let (activations, facts) = setup("use Dancer2 '!get';\npost '/x' => sub { 1 };\n");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", 40, &none_declared);
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(!labels.contains(&"get"), "excluded `get` offered: {labels:?}");
        assert!(labels.contains(&"post"));
    }

    #[test]
    fn keywords_do_not_swamp_local_subs() {
        let (activations, facts) = setup("use Dancer2;\nsub get { 1 }\n");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", 30, &|name: &str| {
                name == "get"
            });
        let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(!labels.contains(&"get"), "local `sub get` owns the name");
        assert!(labels.contains(&"post"));
        assert!(
            candidates.iter().all(|c| c.rank_penalty >= KEYWORD_RANK_PENALTY),
            "every keyword carries the ranking penalty"
        );
    }

    #[test]
    fn without_activation_there_are_zero_keyword_candidates() {
        let source = "use Dancer2::Core;\nget '/x' => sub { 1 };\n";
        let mut parser = Parser::new(source);
        let ast = parser.parse().expect("fixture must parse");
        let activations = file_activations(&ast, FileId(1), None, &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), &activations);
        assert!(
            keyword_completion_candidates(&activations, &facts, "main", 30, &none_declared)
                .is_empty(),
            "no activation: zero framework completion"
        );
    }
}
