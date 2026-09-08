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
use perl_semantic_facts::framework_adapters::dancer2::{Dancer2KeywordState, DslKeywordScope};
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
    /// Where this keyword is available, phrased for the position that offered
    /// it — "global", or the request-context wording naming the owning route
    /// or hook handler.
    ///
    /// Renderers must use this rather than re-deriving availability from
    /// [`Self::scope`]. The `RouteHandlerOnly` discriminant records the
    /// reviewed *contract* scope, which since #13604 is satisfied by an
    /// admitted hook handler as well as a route handler; a renderer that
    /// matches on it directly will say "route handler only" inside a hook and
    /// contradict [`Self::detail`].
    pub availability: &'static str,
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
                &version, scope_detail, activation.facts.dsl_contract_version
            ),
            availability: scope_detail,
            dsl_contract_version: activation.facts.dsl_contract_version,
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
    use perl_test_must::{must_some_with, must_with};

    fn setup(source: &'static str) -> (Dancer2FileActivations, CanonicalDancer2FileFacts) {
        setup_with_version(source, "1.1.1")
    }

    fn setup_with_version(
        source: &'static str,
        framework_version: &str,
    ) -> (Dancer2FileActivations, CanonicalDancer2FileFacts) {
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let module = RuntimeDancer2Module::new("lib/Dancer2.pm", framework_version);
        let activations = file_activations(
            &ast,
            source,
            FileId(1),
            Some(&module),
            &SourceGeneration::known("g1"),
        );
        let facts = canonical_file_facts(&ast, FileId(1), &activations);
        (activations, facts)
    }

    fn none_declared(_: &str) -> bool {
        false
    }

    #[test]
    fn bare_activation_offers_complete_global_vocabulary_only() {
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
        let labels: Vec<&str> =
            candidates.iter().map(|candidate| candidate.label.as_str()).collect();
        for expected in
            ["get", "app", "dancer_version", "mime", "prepare_app", "to_app", "template"]
        {
            assert!(labels.contains(&expected), "missing {expected} in {labels:?}");
        }
        for handler_only in ["params", "uri_for", "redirect", "cookie", "content_type"] {
            assert!(
                !labels.contains(&handler_only),
                "handler-only keyword `{handler_only}` offered outside a handler: {labels:?}"
            );
        }
        for non_keyword in ["route", "before", "after", "body"] {
            assert!(
                !labels.contains(&non_keyword),
                "non-keyword `{non_keyword}` offered by the default DSL: {labels:?}"
            );
        }
    }

    #[test]
    fn inside_handler_offers_complete_request_context_vocabulary() {
        let source = "use Dancer2;\nget '/x' => sub { params; };\n";
        let (activations, facts) = setup(source);
        let handler_offset = must_some_with(source.find("params"), "handler body offset");
        let candidates = keyword_completion_candidates(
            &activations,
            &facts,
            "main",
            handler_offset,
            &none_declared,
        );
        let labels: Vec<&str> =
            candidates.iter().map(|candidate| candidate.label.as_str()).collect();
        for expected in [
            "params",
            "body_parameters",
            "query_parameters",
            "uri_for_route",
            "redirect",
            "cookie",
            "response_header",
            "splat",
        ] {
            assert!(labels.contains(&expected), "missing {expected} in {labels:?}");
        }
    }

    #[test]
    fn dancer2_1_0_omits_uri_for_route_and_uses_v1_0_contract() {
        let source = "use Dancer2;
get '/x' => sub { params; };
";
        let (activations, facts) = setup_with_version(source, "1.0.0");
        let handler_offset = must_some_with(source.find("params"), "handler body offset");
        let candidates = keyword_completion_candidates(
            &activations,
            &facts,
            "main",
            handler_offset,
            &none_declared,
        );
        assert!(
            candidates.iter().all(|candidate| candidate.label != "uri_for_route"),
            "Dancer2 1.0.x must not receive a v1.1 keyword"
        );
        let uri_for = must_some_with(
            candidates.iter().find(|candidate| candidate.label == "uri_for"),
            "v1.0 request helper",
        );
        assert_eq!(uri_for.dsl_contract_version, "dancer2-dsl.1-0.v3");
    }

    #[test]
    fn versioned_canonical_contexts_and_completion_share_the_activation_receipt()
    -> Result<(), String> {
        let source = "use Dancer2;\nget '/x' => sub { params; };\nhook before => sub { request; };";
        for (version, receipt) in [
            ("1.0.0", "dancer2-dsl.1-0.v3"),
            ("1.1.0", "dancer2-dsl.1-1.v3"),
            ("1.1.1", "dancer2-dsl.1-1.v3"),
        ] {
            let (activations, facts) = setup_with_version(source, version);
            let activation = activations.for_package("main").ok_or("missing activation")?;
            if !activation.facts.is_exact() || activation.facts.dsl_contract_version != receipt {
                return Err(format!("{version}: wrong activation receipt: {:?}", activation.facts));
            }
            if facts.handler_contexts.len() != 2 {
                return Err(format!("{version}: expected route and hook contexts: {facts:?}"));
            }
            for context in &facts.handler_contexts {
                if context.dsl_contract_version != receipt || context.framework_version != version {
                    return Err(format!("{version}: wrong canonical context receipt: {context:?}"));
                }
            }
            for (needle, owner) in [("params", "route handler"), ("request", "hook handler")] {
                let offset = source.find(needle).ok_or("missing handler offset")?;
                let candidates = keyword_completion_candidates(
                    &activations,
                    &facts,
                    "main",
                    offset,
                    &none_declared,
                );
                let candidate = candidates
                    .iter()
                    .find(|candidate| candidate.label == needle)
                    .ok_or("missing request keyword")?;
                if candidate.dsl_contract_version != receipt
                    || !candidate.detail.contains(receipt)
                    || !candidate.availability.contains(owner)
                {
                    return Err(format!(
                        "{version}: wrong completion receipt or scope: {candidate:?}"
                    ));
                }
                let has_later_keyword =
                    candidates.iter().any(|candidate| candidate.label == "uri_for_route");
                if has_later_keyword != (version != "1.0.0") {
                    return Err(format!("{version}: incorrect uri_for_route availability"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn unsupported_versions_publish_no_one_x_facts_or_completions() -> Result<(), String> {
        let source = "use Dancer2;\nget '/x' => sub { params; };\nhook before => sub { request; };";
        for version in ["", "1.1oops", "0.9.9", "3.0.0", "2.0.1"] {
            let (activations, facts) = setup_with_version(source, version);
            let offset = source.find("params").ok_or("missing handler offset")?;
            if activations.for_package("main").is_some_and(|a| a.facts.is_exact())
                || !facts.handler_contexts.is_empty()
                || !keyword_completion_candidates(
                    &activations,
                    &facts,
                    "main",
                    offset,
                    &none_declared,
                )
                .is_empty()
            {
                return Err(format!("{version}: unsupported 1.x version published facts"));
            }
            if version == "2.0.1" {
                let activation = activations
                    .two_x_packages
                    .iter()
                    .find(|a| a.package == "main")
                    .ok_or("missing 2.x control")?;
                let contexts: Vec<_> = facts
                    .two_x_route_facts
                    .iter()
                    .flat_map(|family| &family.handler_contexts)
                    .collect();
                if !activation.facts.is_exact() || contexts.len() != 1 {
                    return Err(format!(
                        "2.x control did not mint one comparison context: {facts:?}"
                    ));
                }
                for context in contexts {
                    if context.dsl_contract_version != activation.facts.dsl_contract_version {
                        return Err(format!("2.x context inherited wrong contract: {context:?}"));
                    }
                }
            } else if activations.two_x_packages.iter().any(|a| a.facts.is_exact()) {
                return Err(format!("{version}: unsupported version activated 2.x"));
            }
        }
        Ok(())
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
        // Every renderer of this candidate must be able to reach the same
        // wording. `availability` is what a renderer reads instead of
        // re-deriving from `scope`, so it has to agree with `detail` and carry
        // the hook phrasing itself — otherwise a second surface (the runtime's
        // `documentation` string) can contradict the first.
        assert!(
            request.detail.contains(request.availability),
            "detail must be built from availability: {} vs {}",
            request.detail,
            request.availability
        );
        assert!(
            request.availability.contains("hook handler")
                && !request.availability.contains("route handler only"),
            "availability must name the hook position: {}",
            request.availability
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
        let ast = must_with(parser.parse(), "fixture must parse");
        let activations =
            file_activations(&ast, source, FileId(1), None, &SourceGeneration::known("g1"));
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
        let labels: Vec<&str> =
            candidates.iter().map(|candidate| candidate.label.as_str()).collect();
        assert!(!labels.contains(&"get"), "excluded `get` offered: {labels:?}");
        assert!(labels.contains(&"post"));
    }

    #[test]
    fn excluded_reviewed_handler_keyword_is_never_offered() {
        let source = "use Dancer2 '!uri_for_route';\nget '/x' => sub { params; };\n";
        let (activations, facts) = setup(source);
        let handler_offset = must_some_with(source.find("params"), "handler body offset");
        let candidates = keyword_completion_candidates(
            &activations,
            &facts,
            "main",
            handler_offset,
            &none_declared,
        );
        let labels: Vec<&str> =
            candidates.iter().map(|candidate| candidate.label.as_str()).collect();
        assert!(!labels.contains(&"uri_for_route"), "excluded `uri_for_route` offered: {labels:?}");
        assert!(labels.contains(&"uri_for"), "unrelated request helper remains imported");
        assert!(labels.contains(&"params"), "unrelated request helper remains imported");
    }

    #[test]
    fn keywords_do_not_swamp_local_subs() {
        let (activations, facts) = setup("use Dancer2;\nsub get { 1 }\n");
        let candidates =
            keyword_completion_candidates(&activations, &facts, "main", 30, &|name: &str| {
                name == "get"
            });
        let labels: Vec<&str> =
            candidates.iter().map(|candidate| candidate.label.as_str()).collect();
        assert!(!labels.contains(&"get"), "local `sub get` owns the name");
        assert!(labels.contains(&"post"));
        assert!(
            candidates.iter().all(|candidate| candidate.rank_penalty >= KEYWORD_RANK_PENALTY),
            "every keyword carries the ranking penalty"
        );
    }

    #[test]
    fn without_activation_there_are_zero_keyword_candidates() {
        let source = "use Dancer2::Core;\nget '/x' => sub { 1 };\n";
        let mut parser = Parser::new(source);
        let ast = must_with(parser.parse(), "fixture must parse");
        let activations =
            file_activations(&ast, source, FileId(1), None, &SourceGeneration::known("g1"));
        let facts = canonical_file_facts(&ast, FileId(1), &activations);
        assert!(
            keyword_completion_candidates(&activations, &facts, "main", 30, &none_declared)
                .is_empty(),
            "no activation: zero framework completion"
        );
    }
}
