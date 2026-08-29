//! Dancer2 keyword completion cell (#8928).
//!
//! Offers imported default-DSL keywords from the canonical
//! [`Dancer2KeywordImportFact`]s under exact activation:
//!
//! - `!keyword` exclusions are honored: an excluded keyword is never offered;
//! - route-handler-only keywords (the reviewed `is_global => 0` vocabulary)
//!   are offered only inside an exact inline route handler, decided by the
//!   canonical #8921 handler-context facts;
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
    let inside_handler = facts.inside_handler_context(offset);
    let mut candidates = Vec::new();
    for keyword in &activation.facts.keywords {
        if keyword.state != Dancer2KeywordState::Imported {
            // `!keyword` at the activating import: never offered.
            continue;
        }
        if keyword.scope == DslKeywordScope::RouteHandlerOnly && !inside_handler {
            continue;
        }
        if locally_declared_subnames(&keyword.keyword) {
            // A same-named local subroutine owns the name in this file;
            // ordinary Perl completion covers it.
            continue;
        }
        candidates.push(Dancer2CompletionCandidate {
            label: keyword.keyword.clone(),
            scope: keyword.scope,
            rank_penalty: KEYWORD_RANK_PENALTY,
            detail: format!(
                "Dancer2 {} keyword ({} — {})",
                &version,
                match keyword.scope {
                    DslKeywordScope::Global => "global",
                    DslKeywordScope::RouteHandlerOnly => "route handler only",
                    _ => "unknown",
                },
                DANCER2_DSL_CONTRACT_VERSION
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
