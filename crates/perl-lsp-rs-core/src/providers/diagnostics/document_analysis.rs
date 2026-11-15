//! Compatibility path for generation-owned document diagnostic analysis.
//!
//! The implementation belongs to `perl-semantic-analyzer` (#19440), below
//! diagnostic providers and the native critic. This re-export preserves the
//! existing path and the exact shared type; it introduces no second cache.

pub use perl_semantic_analyzer::DocumentDiagnosticAnalysis;

#[cfg(test)]
mod tests {
    use super::DocumentDiagnosticAnalysis;
    use perl_parser_core::{Node, Parser};
    use std::sync::Arc;

    fn parse(source: &str) -> Arc<Node> {
        let mut parser = Parser::new(source);
        Arc::new(perl_tdd_support::must(parser.parse()))
    }

    #[test]
    fn core_and_semantic_paths_share_the_same_analysis_type() {
        let source = "use strict;\nmy $x = 1;\nprint $x;\n";
        let ast = parse(source);
        let analysis = perl_semantic_analyzer::DocumentDiagnosticAnalysis::build(&ast, source);
        let through_core: &DocumentDiagnosticAnalysis = &analysis;
        let through_semantic: &perl_semantic_analyzer::DocumentDiagnosticAnalysis = through_core;
        assert!(std::ptr::eq(through_semantic, &analysis));
    }

    /// #7286 configuration isolation: the folder-owned project version must
    /// still change PL900 output while the shared analysis is reused and never
    /// rebuilt.
    ///
    /// This is the seam where a caching change could plausibly do real damage.
    /// The analysis holds only source/AST-derived facts; `project_version` is
    /// *configuration*. If configuration ever leaked into the shared facts, two
    /// folders with the same file and different `[perl].version` would answer
    /// from whichever evaluation warmed the cell first — a wrong diagnostic, not
    /// a slow one. So this asserts both halves at once: varying configuration
    /// varies the output, and the facts underneath are the same ones, built once.
    ///
    /// `use builtin 'inf'` with no `use VERSION` is the fixture because `inf`
    /// arrived in v5.40, so the project fallback is what decides PL900.
    #[test]
    fn project_version_varies_output_while_the_shared_facts_are_reused() {
        let source = "use builtin 'inf'; builtin::inf();\n";
        let ast = parse(source);
        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        let provider = super::super::DiagnosticsProvider::new();

        let pl900 = |diags: &[crate::providers::diagnostics::Diagnostic]| -> usize {
            diags.iter().filter(|d| d.code.as_deref() == Some("PL900")).count()
        };

        let below = provider.get_diagnostics_with_path_with_analysis(
            &ast,
            &[],
            source,
            None,
            &[],
            None,
            Some("5.38"),
            Some(&analysis),
        );
        // Pin the exact fact slice the first evaluation used.
        let pragma_ptr = analysis.pragma_map().as_ptr();

        let at_or_above = provider.get_diagnostics_with_path_with_analysis(
            &ast,
            &[],
            source,
            None,
            &[],
            None,
            Some("5.40"),
            Some(&analysis),
        );

        assert!(
            pl900(&below) > 0,
            "fixture invariant: a 5.38 project must report PL900 for `builtin 'inf'`, or the \
             comparison below is vacuous: {below:?}"
        );
        assert_eq!(
            pl900(&at_or_above),
            0,
            "a 5.40 project must accept `builtin 'inf'`; configuration must still decide PL900 \
             even though both evaluations shared one analysis: {at_or_above:?}"
        );

        // The second evaluation must have read the same facts, not rebuilt them:
        // a rebuild would hand back a different allocation.
        assert!(
            std::ptr::eq(analysis.pragma_map().as_ptr(), pragma_ptr),
            "both evaluations must consume one pragma timeline; a new allocation means \
             configuration variation forced a source-fact rebuild"
        );
        assert!(
            !analysis.pragma_map().is_empty(),
            "fixture must produce a non-empty pragma map, or the identity check is vacuous"
        );
    }

    /// #7286: the *provider* must reject an analysis built from a different
    /// tree — not merely `matches` in isolation.
    ///
    /// `matches_rejects_a_different_tree_parsed_from_identical_source` proves
    /// the predicate discriminates. It does not prove the consumer consults it,
    /// and no output comparison can: both trees are parsed from identical text,
    /// so a provider that wrongly trusted the foreign analysis would return
    /// byte-identical diagnostics. The fact cells are the only witness. If the
    /// provider rejected the pairing it built its own analysis locally and the
    /// foreign one stays cold; if it trusted it, the foreign cells warm up.
    #[test]
    fn the_provider_rejects_an_analysis_from_a_different_tree() {
        let source = "use strict;\nsub f { my $unused = 1; print $undeclared; }\n";
        let ast_a = parse(source);
        let output_b = Parser::new(source).parse_with_recovery();
        let ast_b = Arc::new(output_b.ast);
        let parse_errors_b = output_b.diagnostics;
        assert!(
            !Arc::ptr_eq(&ast_a, &ast_b),
            "fixture invariant: the two trees must be distinct allocations, or there is no \
             mismatch to reject"
        );

        let foreign = DocumentDiagnosticAnalysis::build(&ast_a, source);
        let _ = super::super::DiagnosticsProvider::new().get_diagnostics_with_path_with_analysis(
            &ast_b,
            &parse_errors_b,
            source,
            None,
            &[],
            None,
            None,
            Some(&foreign),
        );

        assert!(
            !foreign.pragma_map_materialized(),
            "the provider must not read a pragma timeline belonging to another tree"
        );
        assert!(
            !foreign.scope_issues_materialized(),
            "the provider must not read scope issues belonging to another tree"
        );
        assert!(
            !foreign.symbol_table_materialized(),
            "the provider must not read a symbol table belonging to another tree"
        );

        // Positive control: with the analysis's own tree, the very same call
        // does consume the shared facts. Without this, the assertions above
        // would also pass against a provider that ignores every supplied
        // analysis, which is the opposite defect.
        let matching = DocumentDiagnosticAnalysis::build(&ast_b, source);
        let _ = super::super::DiagnosticsProvider::new().get_diagnostics_with_path_with_analysis(
            &ast_b,
            &parse_errors_b,
            source,
            None,
            &[],
            None,
            None,
            Some(&matching),
        );
        assert!(
            matching.pragma_map_materialized() && matching.scope_issues_materialized(),
            "instrument liveness: a matching analysis must actually be consumed, otherwise the \
             rejection asserted above proves nothing"
        );
    }

    /// #7286 composition proof at the provider seam: handing a prebuilt
    /// analysis to an evaluation that cannot use it must cost nothing.
    ///
    /// `DiagnosticsProvider` skips its entire pragma/scope/symbol block for a
    /// document with a blocking parse error, so every cell must still be cold
    /// afterwards. This is the pairing that makes an eager constructor a
    /// regression rather than a wash: `ParsedSnapshot` hands the analysis over
    /// unconditionally, so "this consumer will not read it" has to be free.
    #[test]
    fn a_blocking_parse_error_evaluation_runs_no_pass() {
        // Unbalanced brace: recovery still yields an AST, and the resulting
        // errors are blocking.
        let source = "sub f { my $unused = 1; print $undeclared;\n";
        let output = Parser::new(source).parse_with_recovery();
        let ast = Arc::new(output.ast);
        let parse_errors = output.diagnostics;
        assert!(
            parse_errors.iter().any(super::super::diagnostics::suppresses_semantic_analysis),
            "fixture invariant: the parse errors must be blocking, or the provider would run \
             its block and this test would prove nothing"
        );

        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        let _ = super::super::DiagnosticsProvider::new().get_diagnostics_with_path_with_analysis(
            &ast,
            &parse_errors,
            source,
            None,
            &[],
            None,
            None,
            Some(&analysis),
        );

        assert!(!analysis.pragma_map_materialized(), "no pragma pass may run for this evaluation");
        assert!(!analysis.scope_issues_materialized(), "no scope pass may run for this evaluation");
        assert!(
            !analysis.symbol_table_materialized(),
            "no symbol-extraction pass may run for this evaluation"
        );
        assert!(
            !analysis.type_environment_materialized(),
            "no type-inference pass may run for this evaluation"
        );
        // The PL412 stray-cut lint deliberately runs OUTSIDE the #5089 gate
        // (diagnostics.rs), so a blocking-error evaluation MAY materialize
        // that one source-only pass; the gated pragma/scope/symbol/type
        // passes above must stay cold.
    }
}
