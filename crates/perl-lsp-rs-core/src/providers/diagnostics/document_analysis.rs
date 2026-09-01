//! Generation-owned, transport-neutral document diagnostic analysis (#7286).
//!
//! Production push (`crates/perl-lsp-rs/src/runtime/diagnostics.rs`) and pull
//! (`crates/perl-lsp-rs/src/features/diagnostics/pull.rs`) diagnostics each used
//! to rebuild the same three AST-wide, source-only passes -- pragma tracking,
//! scope analysis, and symbol extraction -- independently, on every final
//! diagnostic evaluation of one accepted document generation. This module
//! extracts those three passes into one reusable, transport-neutral type so a
//! caller that owns a document generation (currently
//! `perl_lsp_rs::state::document::ParsedSnapshot`) can build the analysis at
//! most once per generation and hand out the shared facts to as many
//! diagnostic evaluations as it needs.
//!
//! This type deliberately contains only source/AST-derived facts: no
//! configuration, no workspace/semantic-query results, no external tool
//! output, and no LSP-facing types. It has no knowledge of push vs. pull, or
//! of the caller's transport.

use std::ops::Range;
use std::sync::{Arc, OnceLock};

use perl_parser_core::Node;
use perl_pragma::{PragmaState, PragmaTracker};
use perl_semantic_analyzer::scope_analyzer::{ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::symbol::{SymbolExtractor, SymbolTable};

/// Reusable, generation-scoped, source/AST-derived diagnostic facts for one
/// document.
///
/// Owns the tree and source for one document generation and hands out the
/// exact outputs of the three passes that used to run inline in
/// `get_diagnostics_with_path_and_semantics_impl` on every diagnostic
/// evaluation: the pragma-state map, the scope-analysis issues, and the
/// extracted symbol table. Sharing one of these across every diagnostic
/// evaluation of an accepted generation is the hard contract of #7286: one
/// accepted generation, at most one run of each pass.
///
/// Each fact is computed on first request and never again -- construction
/// itself runs no pass at all. That matters because the three facts do not
/// share a consumer set: `DiagnosticsProvider` skips its whole
/// pragma/scope/symbol block for a document with a blocking parse error,
/// native critic composition consumes only the pragma map and scope issues,
/// and the legacy (`BuiltInAnalyzer`) critic engine consumes none of them. If
/// construction ran all three eagerly, a generation whose consumers want two
/// of them -- or none -- would pay for passes nobody reads, which for a
/// malformed mid-edit document is strictly *worse* than the per-evaluation
/// rebuilding this type exists to remove. Deferring per fact makes the
/// question "which consumer will run against this generation?" irrelevant at
/// every call site, so no caller has to predict it.
#[derive(Debug)]
pub struct DocumentDiagnosticAnalysis {
    /// The exact tree these facts are derived from, retained both so
    /// [`Self::matches`] can prove tree identity by pointer (the pointer can
    /// never be recycled by an unrelated later allocation while this analysis
    /// is alive) and so a deferred pass still has its input when first
    /// requested.
    ast: Arc<Node>,
    /// The exact source `ast` was parsed against, retained for the same two
    /// reasons: [`Self::matches_source`] compares against it, and the
    /// deferred scope/symbol passes need it. Held as `Arc<str>` so a caller
    /// that already owns the document text this way (`ParsedSnapshot`) shares
    /// it rather than copying the whole document per generation.
    source: Arc<str>,
    pragma_map: OnceLock<Vec<(Range<usize>, PragmaState)>>,
    scope_issues: OnceLock<Vec<ScopeIssue>>,
    symbol_table: OnceLock<SymbolTable>,
}

impl DocumentDiagnosticAnalysis {
    /// Bind an analysis to `ast` and `source`.
    ///
    /// Runs no analysis pass: this is O(1) plus, for a `&str` argument, one
    /// copy of the source. Each pass runs on its first accessor call and is
    /// then cached -- see the type docs for why deferring is the point rather
    /// than an optimization detail.
    ///
    /// The passes themselves reproduce exactly those previously inlined in
    /// `get_diagnostics_with_path_and_semantics_impl`'s
    /// `!has_blocking_parse_error` block, with the same inputs, so the facts
    /// produced are identical to what that inline code produced. Callers must
    /// pass the exact source text `ast` was parsed from -- see
    /// [`Self::matches`] for how a consumer verifies both the tree and the
    /// source before trusting a prebuilt analysis.
    ///
    /// Accepts anything convertible into `Arc<str>`, so a caller holding an
    /// `Arc<str>` document source passes it without copying and a caller
    /// holding a `&str` still works unchanged.
    #[must_use]
    pub fn build(ast: &Arc<Node>, source: impl Into<Arc<str>>) -> Self {
        Self {
            ast: Arc::clone(ast),
            source: source.into(),
            pragma_map: OnceLock::new(),
            scope_issues: OnceLock::new(),
            symbol_table: OnceLock::new(),
        }
    }

    /// Whether this analysis describes exactly this `ast` **and** this
    /// `source`.
    ///
    /// This is the check a consumer must use before trusting a prebuilt
    /// analysis. [`Self::matches_source`] alone binds only the source bytes,
    /// which is not sufficient: the public `*_with_analysis` entry points take
    /// the tree and the analysis as independent arguments, so a caller could
    /// pair an analysis with a *different* tree that happens to have been
    /// parsed from identical text. Tree identity is compared by
    /// [`Arc::ptr_eq`], and this analysis holds its own strong reference to
    /// that tree, so the pointer cannot be recycled by a later allocation
    /// while the comparison is meaningful.
    #[must_use]
    pub fn matches(&self, ast: &Arc<Node>, source: &str) -> bool {
        Arc::ptr_eq(&self.ast, ast) && self.matches_source(source)
    }

    /// Whether this analysis is bound to exactly `source`.
    ///
    /// Binds the source bytes only. Prefer [`Self::matches`], which also binds
    /// the tree; use this when the caller genuinely has no tree to compare.
    ///
    /// Because the analysis retains its source (the deferred passes need it),
    /// this is an exact byte comparison rather than a fingerprint: same cost
    /// class as hashing the same bytes, with no collision to reason about, and
    /// it short-circuits on differing length.
    #[must_use]
    pub fn matches_source(&self, source: &str) -> bool {
        &*self.source == source
    }

    /// The pragma-state map for this analysis's AST, keyed by byte range. See
    /// `perl_pragma::PragmaTracker::build`.
    ///
    /// Runs `PragmaTracker::build` on first call and returns the cached map
    /// thereafter.
    #[must_use]
    pub fn pragma_map(&self) -> &[(Range<usize>, PragmaState)] {
        self.pragma_map.get_or_init(|| PragmaTracker::build(&self.ast))
    }

    /// The scope-analysis issues (undeclared/unused/shadowed variables, etc.)
    /// for this analysis's AST and source.
    ///
    /// Runs `ScopeAnalyzer::analyze` on first call and returns the cached
    /// issues thereafter. Scope analysis consumes the pragma map, so this
    /// materializes [`Self::pragma_map`] as well -- once, through the same
    /// cell any other consumer reads.
    #[must_use]
    pub fn scope_issues(&self) -> &[ScopeIssue] {
        // Evaluate the dependency before entering this cell's initializer: a
        // `OnceLock` initializer that re-enters its own cell deadlocks, and
        // while these are two distinct cells today, keeping the ordering
        // explicit rather than buried in the closure body keeps it that way.
        let pragma_map = self.pragma_map();
        self.scope_issues
            .get_or_init(|| ScopeAnalyzer::new().analyze(&self.ast, &self.source, pragma_map))
    }

    /// The symbol table extracted from this analysis's AST and source.
    ///
    /// Runs `SymbolExtractor::extract` on first call and returns the cached
    /// table thereafter. Nothing else materializes it: a consumer that reads
    /// only the pragma map and scope issues (native critic composition) never
    /// pays for this pass.
    #[must_use]
    pub fn symbol_table(&self) -> &SymbolTable {
        self.symbol_table
            .get_or_init(|| SymbolExtractor::new_with_source(&self.source).extract(&self.ast))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use std::sync::Arc;

    fn parse(source: &str) -> Arc<Node> {
        let mut parser = Parser::new(source);
        Arc::new(perl_tdd_support::must(parser.parse()))
    }

    /// Comparable projection of a [`ScopeIssue`].
    ///
    /// `ScopeIssue` is not `PartialEq`, and comparing only `.len()` is a weak
    /// oracle: it passes against an implementation that returns the right
    /// *number* of wrong issues. Project the identifying fields instead so the
    /// comparison is on content.
    fn issue_keys(issues: &[ScopeIssue]) -> Vec<(String, usize, (usize, usize), String)> {
        issues
            .iter()
            .map(|i| (i.variable_name.clone(), i.line, i.range, i.description.clone()))
            .collect()
    }

    /// Comparable projection of a [`SymbolTable`]: every symbol name paired
    /// with how many definitions it carries, sorted for stable comparison.
    fn symbol_keys(table: &SymbolTable) -> Vec<(String, usize)> {
        let mut keys: Vec<(String, usize)> =
            table.symbols.iter().map(|(name, defs)| (name.clone(), defs.len())).collect();
        keys.sort();
        keys
    }

    #[test]
    fn build_matches_inline_passes_on_clean_code() {
        let source = "use strict;\nmy $x = 1;\nprint $x;\n";
        let ast = parse(source);

        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);

        let expected_pragma_map = PragmaTracker::build(&ast);
        let expected_scope_issues =
            ScopeAnalyzer::new().analyze(&ast, source, &expected_pragma_map);
        let expected_symbol_table = SymbolExtractor::new_with_source(source).extract(&ast);

        assert_eq!(analysis.pragma_map(), expected_pragma_map.as_slice());
        assert_eq!(issue_keys(analysis.scope_issues()), issue_keys(&expected_scope_issues));
        assert_eq!(symbol_keys(analysis.symbol_table()), symbol_keys(&expected_symbol_table));
        assert!(
            !analysis.symbol_table().symbols.is_empty(),
            "fixture must extract at least one symbol, or the symbol comparison is vacuous"
        );
    }

    #[test]
    fn build_matches_inline_passes_on_scope_issues() {
        let source = "sub f { my $unused = 1; print $undeclared; }\n";
        let ast = parse(source);

        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        let expected_pragma_map = PragmaTracker::build(&ast);
        let expected_scope_issues =
            ScopeAnalyzer::new().analyze(&ast, source, &expected_pragma_map);

        assert_eq!(issue_keys(analysis.scope_issues()), issue_keys(&expected_scope_issues));
        assert!(
            !analysis.scope_issues().is_empty(),
            "fixture must produce at least one scope issue, or the comparison is vacuous"
        );
    }

    #[test]
    fn matches_source_true_for_identical_source() {
        let source = "my $x = 1;\n";
        let ast = parse(source);
        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        assert!(analysis.matches_source(source));
    }

    #[test]
    fn matches_source_false_for_different_content() {
        let source = "my $x = 1;\n";
        let ast = parse(source);
        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        assert!(!analysis.matches_source("my $y = 2;\n"));
    }

    #[test]
    fn matches_source_false_for_same_length_different_content() {
        // Same byte length, different bytes -- must not be conflated by the
        // freshness guard.
        let source = "my $xxxxx = 1;\n";
        let ast = parse(source);
        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        let same_length_other = "my $yyyyy = 2;\n";
        assert_eq!(source.len(), same_length_other.len());
        assert!(!analysis.matches_source(same_length_other));
    }

    #[test]
    fn matches_rejects_a_different_tree_parsed_from_identical_source() {
        // Two independent parses of the same text produce two distinct trees.
        // `matches_source` cannot tell them apart -- only the tree binding in
        // `matches` can. This is the control for the "cached facts can describe
        // another tree" gap: the public `*_with_analysis` entry points take the
        // tree and the analysis as independent arguments, so nothing but this
        // check prevents pairing them across trees.
        let source = "sub f { my $unused = 1; }\n";
        let tree_a = parse(source);
        let tree_b = parse(source);
        assert!(
            !Arc::ptr_eq(&tree_a, &tree_b),
            "fixture invariant: the two parses must be distinct allocations"
        );

        let analysis = DocumentDiagnosticAnalysis::build(&tree_a, source);

        assert!(analysis.matches(&tree_a, source), "must accept its own tree and source");
        assert!(
            !analysis.matches(&tree_b, source),
            "must reject a different tree even when the source bytes are identical"
        );
        assert!(
            analysis.matches_source(source),
            "source-only check still passes -- which is exactly why `matches` binds the tree too"
        );
    }

    #[test]
    fn matches_rejects_its_own_tree_with_different_source() {
        let source = "my $x = 1;\n";
        let ast = parse(source);
        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        assert!(!analysis.matches(&ast, "my $y = 2;\n"));
    }

    /// #7286: construction must run no pass at all.
    ///
    /// The three facts have different consumer sets -- the legacy critic
    /// engine reads none of them, and native critic composition reads two of
    /// three -- so an eager constructor would make a generation pay for
    /// passes nobody reads. Asserted on the cells directly rather than by
    /// timing, which would be a flaky oracle.
    #[test]
    fn construction_runs_no_pass() {
        let source = "use strict;\nsub f { my $unused = 1; print $undeclared; }\n";
        let ast = parse(source);

        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);

        assert!(analysis.pragma_map.get().is_none(), "construction must not build the pragma map");
        assert!(analysis.scope_issues.get().is_none(), "construction must not run scope analysis");
        assert!(
            analysis.symbol_table.get().is_none(),
            "construction must not run symbol extraction"
        );
    }

    /// #7286: reading the two facts native critic composition consumes must
    /// not drag in the symbol-extraction pass it never reads.
    ///
    /// This is the regression that would make a malformed mid-edit document
    /// *more* expensive than before the change: the provider skips its whole
    /// block for a blocking parse error, so the critic's pragma+scope read is
    /// the only consumer, and an eager constructor would add a
    /// `SymbolExtractor` walk with no reader at all.
    #[test]
    fn reading_critic_facts_does_not_materialize_the_symbol_table() {
        let source = "use strict;\nsub f { my $unused = 1; print $undeclared; }\n";
        let ast = parse(source);

        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        let _ = analysis.pragma_map();
        let _ = analysis.scope_issues();

        assert!(
            analysis.symbol_table.get().is_none(),
            "a consumer that reads only the pragma map and scope issues must not pay for \
             symbol extraction"
        );
    }

    /// Scope analysis consumes the pragma map, so requesting scope issues
    /// alone must materialize the map -- through the same cell every other
    /// consumer reads, not a private second copy.
    #[test]
    fn scope_issues_materialize_the_shared_pragma_map() {
        let source = "use strict;\nsub f { my $unused = 1; }\n";
        let ast = parse(source);

        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        let _ = analysis.scope_issues();

        let cached = perl_test_must::must_some_with(
            analysis.pragma_map.get(),
            "scope analysis must materialize the shared pragma-map cell",
        );
        assert!(
            std::ptr::eq(cached.as_slice(), analysis.pragma_map()),
            "a later pragma-map reader must get the very slice scope analysis used, not a \
             rebuilt one"
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
            Some(&analysis),
        );

        assert!(analysis.pragma_map.get().is_none(), "no pragma pass may run for this evaluation");
        assert!(analysis.scope_issues.get().is_none(), "no scope pass may run for this evaluation");
        assert!(
            analysis.symbol_table.get().is_none(),
            "no symbol-extraction pass may run for this evaluation"
        );
    }

    /// #7286 hard contract at the fact level: each pass runs at most once,
    /// however many times its fact is read. Proven by identity of the
    /// returned references -- a rebuild would hand back a different
    /// allocation. `assert_eq!` on the contents would pass against an
    /// implementation that recomputes identical values on every call.
    #[test]
    fn each_fact_is_computed_at_most_once() {
        let source = "use strict;\nsub f { my $unused = 1; print $undeclared; }\n";
        let ast = parse(source);
        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);

        assert!(std::ptr::eq(analysis.pragma_map(), analysis.pragma_map()));
        assert!(std::ptr::eq(analysis.scope_issues(), analysis.scope_issues()));
        assert!(std::ptr::eq(analysis.symbol_table(), analysis.symbol_table()));

        assert!(
            !analysis.pragma_map().is_empty(),
            "fixture must produce a non-empty pragma map, or the pragma identity check is \
             vacuous against two empty-slice dangling pointers"
        );
        assert!(
            !analysis.scope_issues().is_empty(),
            "fixture must produce at least one scope issue, or the scope identity check is \
             vacuous"
        );
    }
}
