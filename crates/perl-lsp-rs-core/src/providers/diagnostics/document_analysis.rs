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

use perl_parser_core::Node;
use perl_parser_core::error::ParseError;
use perl_pragma::{PragmaState, PragmaTracker};
use perl_semantic_analyzer::scope_analyzer::{ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::symbol::{SymbolExtractor, SymbolTable};

use crate::hashing::fnv1a64;

/// Reusable, generation-scoped, source/AST-derived diagnostic facts for one
/// document.
///
/// Holds the exact outputs of the three passes that used to run inline in
/// `get_diagnostics_with_path_and_semantics_impl` on every diagnostic
/// evaluation: the pragma-state map, the scope-analysis issues, and the
/// extracted symbol table. Building this once per accepted document
/// generation and sharing it across every diagnostic evaluation of that
/// generation is the hard contract of #7286: one accepted generation, at most
/// one `DocumentDiagnosticAnalysis` construction.
#[derive(Debug)]
pub struct DocumentDiagnosticAnalysis {
    /// Non-cryptographic freshness fingerprint of the source this analysis
    /// was built from. See [`Self::matches_source`].
    source_fingerprint: u64,
    /// Byte length of the source this analysis was built from, checked
    /// alongside `source_fingerprint` in [`Self::matches_source`].
    source_len: usize,
    pragma_map: Vec<(Range<usize>, PragmaState)>,
    scope_issues: Vec<ScopeIssue>,
    symbol_table: SymbolTable,
}

impl DocumentDiagnosticAnalysis {
    /// Build the analysis from `ast` and `source`.
    ///
    /// Reproduces exactly the three passes previously inlined in
    /// `get_diagnostics_with_path_and_semantics_impl`'s
    /// `!has_blocking_parse_error` block, in the same order and with the same
    /// inputs, so the facts this produces are identical to what that inline
    /// code produced. Callers must pass the exact source text `ast` was
    /// parsed from -- see [`Self::matches_source`] for how a consumer can
    /// verify that before trusting a prebuilt analysis.
    #[must_use]
    pub fn build(ast: &Node, source: &str) -> Self {
        let pragma_map = PragmaTracker::build(ast);
        let scope_analyzer = ScopeAnalyzer::new();
        let scope_issues = scope_analyzer.analyze(ast, source, &pragma_map);
        let symbol_table = SymbolExtractor::new_with_source(source).extract(ast);
        Self {
            source_fingerprint: fnv1a64(source.as_bytes()),
            source_len: source.len(),
            pragma_map,
            scope_issues,
            symbol_table,
        }
    }

    /// Whether this analysis was built from exactly `source`.
    ///
    /// This is a freshness guard, not a cryptographic identity check: it
    /// compares an FNV-1a fingerprint of the bytes *and* the byte length, so
    /// same-length-but-different-content and different-length sources are
    /// both rejected. That is collision-resistant enough for the realistic
    /// case a consumer needs to reject -- a stale or mismatched analysis from
    /// a different generation -- but it is not adversarial-input-safe. A
    /// consumer that needs a cryptographic identity guarantee should use
    /// [`crate::hashing::sha256_hex`] instead; nothing in this crate
    /// currently needs that stronger guarantee for this purpose.
    #[must_use]
    pub fn matches_source(&self, source: &str) -> bool {
        self.source_len == source.len() && self.source_fingerprint == fnv1a64(source.as_bytes())
    }

    /// The pragma-state map built from this analysis's AST, keyed by byte
    /// range. See `perl_pragma::PragmaTracker::build`.
    #[must_use]
    pub fn pragma_map(&self) -> &[(Range<usize>, PragmaState)] {
        &self.pragma_map
    }

    /// The scope-analysis issues (undeclared/unused/shadowed variables, etc.)
    /// detected for this analysis's AST and source.
    #[must_use]
    pub fn scope_issues(&self) -> &[ScopeIssue] {
        &self.scope_issues
    }

    /// The symbol table extracted from this analysis's AST and source.
    #[must_use]
    pub fn symbol_table(&self) -> &SymbolTable {
        &self.symbol_table
    }
}

/// Whether these parse errors suppress the document-analysis / lint stack.
///
/// Shares the single existing authority for this rule
/// (`super::diagnostics::suppresses_semantic_analysis`, the
/// `has_blocking_parse_error` predicate) rather than duplicating it -- see
/// that function's doc comment for the `Recovered`-structured-recovery
/// carve-out rationale. A caller that owns a document generation (such as
/// `ParsedSnapshot`) uses this to decide whether building a
/// [`DocumentDiagnosticAnalysis`] would be wasted work, since production
/// diagnostics skip the entire pragma/scope/symbol block under the same
/// condition today.
#[must_use]
pub fn suppresses_document_analysis(parse_errors: &[ParseError]) -> bool {
    parse_errors.iter().any(super::diagnostics::suppresses_semantic_analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use perl_parser_core::error::{RecoveryKind, RecoverySite};
    use std::sync::Arc;

    fn parse(source: &str) -> Arc<Node> {
        let mut parser = Parser::new(source);
        Arc::new(perl_tdd_support::must(parser.parse()))
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
        assert_eq!(analysis.scope_issues().len(), expected_scope_issues.len());
        assert_eq!(analysis.symbol_table().symbols.len(), expected_symbol_table.symbols.len());
    }

    #[test]
    fn build_matches_inline_passes_on_scope_issues() {
        let source = "sub f { my $unused = 1; print $undeclared; }\n";
        let ast = parse(source);

        let analysis = DocumentDiagnosticAnalysis::build(&ast, source);
        let expected_pragma_map = PragmaTracker::build(&ast);
        let expected_scope_issues =
            ScopeAnalyzer::new().analyze(&ast, source, &expected_pragma_map);

        assert_eq!(analysis.scope_issues().len(), expected_scope_issues.len());
        assert!(!analysis.scope_issues().is_empty());
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
    fn suppresses_document_analysis_true_for_blocking_error() {
        let errors = vec![ParseError::UnexpectedEof];
        assert!(suppresses_document_analysis(&errors));
    }

    #[test]
    fn suppresses_document_analysis_false_for_recovered_error() {
        let errors = vec![ParseError::Recovered {
            site: RecoverySite::ArgList,
            kind: RecoveryKind::InsertedCloser,
            location: 0,
        }];
        assert!(!suppresses_document_analysis(&errors));
    }

    #[test]
    fn suppresses_document_analysis_false_for_no_errors() {
        assert!(!suppresses_document_analysis(&[]));
    }
}
