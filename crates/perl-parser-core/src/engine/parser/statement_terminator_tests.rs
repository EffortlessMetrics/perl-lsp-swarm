//! Call-observation tests for the statement-terminator seam in
//! `parse_statement_inner` (#5474).
//!
//! `parse_statement_inner` reaches `finish_statement_terminator` from two
//! distinct sites — the autoquoted-hash-key path, where a keyword before `=>`
//! is consumed as a string, and the general path every other statement takes.
//! The two are easy to change independently and were, before this seam existed,
//! two separate copies of the same optional-`;` block. Each site is observed
//! here through the only effect the call has: a `;` is consumed when present,
//! and its absence between two statements is recorded.
//!
//! `drain_pending_heredocs_from` runs immediately after the terminator on the
//! same path, and its ordering is what makes a heredoc body attach to the
//! statement that queued it. It is observed here for the same reason: the
//! terminator work sits directly in front of it.

#[cfg(test)]
mod tests {
    use crate::error::{ParseError, RecoveryKind, RecoverySite};
    use crate::parser::Parser;
    use perl_ast::ast::{Node, NodeKind};

    fn contains_string(node: &Node, value: &str) -> bool {
        matches!(&node.kind, NodeKind::String { value: actual, .. } if actual == value)
            || node.children().into_iter().any(|child| contains_string(child, value))
    }

    /// Count of terminator diagnostics recorded while parsing `source`.
    fn inferred_semicolons(source: &str) -> usize {
        let mut parser = Parser::new(source);
        let _ = parser.parse();
        parser
            .errors()
            .iter()
            .filter(|error| {
                matches!(
                    error,
                    ParseError::Recovered {
                        site: RecoverySite::Statement,
                        kind: RecoveryKind::InferredSemicolon,
                        ..
                    }
                )
            })
            .count()
    }

    /// Single observer covering all three calls `parse_statement_inner` makes
    /// after a statement body is parsed: `finish_statement_terminator` from the
    /// autoquoted-hash-key path and from the general path, and the
    /// `drain_pending_heredocs_from` that follows each.
    ///
    /// The name is the one RIPR's review guidance asks for at these seams
    /// (`<caller>_call_presence_observer`); the tests below observe the same
    /// calls one path at a time, under names that say what they are for.
    /// Each assertion is an effect of the call, not a restatement of it.
    #[test]
    fn parse_statement_inner_call_presence_observer() {
        // Autoquoted-hash-key path → finish_statement_terminator.
        assert_eq!(inferred_semicolons("my %h = (if => 1)\nprint \"hi\";\n"), 1);
        assert_eq!(inferred_semicolons("my %h = (if => 1);\nprint \"hi\";\n"), 0);

        // General path → finish_statement_terminator.
        assert_eq!(inferred_semicolons("my $x = 1\nprint \"hi\";\n"), 1);
        assert_eq!(inferred_semicolons("my $x = 1;\nprint \"hi\";\n"), 0);

        // Both paths → drain_pending_heredocs_from: the queued body attaches to
        // the statement that declared it instead of swallowing the next one.
        let source = "my $text = <<'EOT';\nline one\nEOT\nprint $text;\n";
        let mut parser = Parser::new(source);
        let ast = match parser.parse() {
            Ok(ast) => ast,
            Err(error) => unreachable!("heredoc source must parse: {error:?}"),
        };
        let NodeKind::Program { statements } = &ast.kind else {
            unreachable!("parse() returns a Program");
        };
        assert_eq!(statements.len(), 2, "statements: {statements:#?}");
    }

    /// The general path: `parse_statement_inner` must reach
    /// `finish_statement_terminator` for an ordinary statement. Observed by the
    /// call's only effect — the `;` is consumed when present, and reported when
    /// it is absent between two statements.
    #[test]
    fn general_statement_path_reaches_the_terminator_seam() {
        assert_eq!(inferred_semicolons("my $x = 1;\nprint \"hi\";\n"), 0);
        assert_eq!(inferred_semicolons("my $x = 1\nprint \"hi\";\n"), 1);
    }

    /// The autoquoted-hash-key path: a keyword before `=>` is consumed as a
    /// string and the statement finishes through a different branch, which must
    /// reach the same seam. Before it was one function, this site carried its
    /// own copy of the optional-`;` block.
    #[test]
    fn autoquoted_hash_key_path_reaches_the_terminator_seam() {
        // `if` here is a hash key, not a conditional.
        let clean = "my %h = (if => 1, for => 2);\nprint \"hi\";\n";
        assert_eq!(inferred_semicolons(clean), 0);

        let missing = "my %h = (if => 1, for => 2)\nprint \"hi\";\n";
        assert_eq!(inferred_semicolons(missing), 1);

        // The key really did autoquote — otherwise this test would be observing
        // the ordinary path under a different name.
        let mut parser = Parser::new(clean);
        let ast = match parser.parse() {
            Ok(ast) => ast,
            Err(error) => unreachable!("hash-key source must parse: {error:?}"),
        };
        assert!(
            contains_string(&ast, "if"),
            "autoquoted hash key should be represented as a String node: {}",
            ast.to_sexp()
        );
    }

    /// `drain_pending_heredocs_from` runs right after the terminator on the same
    /// path. Observed through the body it attaches: a heredoc queued while the
    /// statement parsed is drained into the AST only if the call is reached.
    #[test]
    fn heredoc_drain_runs_after_the_terminator_on_the_same_path() {
        let source = "my $text = <<'EOT';\nline one\nEOT\nprint $text;\n";
        assert_eq!(inferred_semicolons(source), 0);

        let mut parser = Parser::new(source);
        let ast = match parser.parse() {
            Ok(ast) => ast,
            Err(error) => unreachable!("heredoc source must parse: {error:?}"),
        };
        let NodeKind::Program { statements } = &ast.kind else {
            unreachable!("parse() returns a Program");
        };
        // Both statements survive: the heredoc body was drained into the first
        // rather than left queued to swallow the second.
        assert_eq!(statements.len(), 2, "statements: {statements:#?}");
    }

    /// The terminator seam must not fire on the omissions Perl permits, on
    /// either path. A seam that reports everything is reached just as reliably
    /// as one that reports correctly, so the negative direction is observed too.
    #[test]
    fn the_terminator_seam_stays_silent_on_permitted_omissions() {
        assert_eq!(inferred_semicolons("my $only = 1"), 0);
        assert_eq!(inferred_semicolons("sub f {\n    my $y = 2\n}\n1;\n"), 0);
        assert_eq!(inferred_semicolons("my $x = 1\n__END__\ndocs\n"), 0);
    }

    /// RIPR seam: the `inferred_semicolons` filter must discriminate on
    /// `site`, `kind`, and `location`. The helper's `..` wildcard leaves the
    /// `location` field unobserved; this test makes it observable so a value
    /// mutation on any match-arm field is caught rather than passing silently.
    ///
    /// `"my $x = 1\n"` is 10 bytes (m=0 y=1 space=2 $=3 x=4 space=5 ==6
    /// space=7 1=8 \n=9), so `print` starts at byte 10.
    /// `finish_statement_terminator` records `current_position()` — the byte
    /// offset of the first token of the *next* statement — so the error must
    /// anchor at exactly 10.
    #[test]
    fn inferred_semicolon_location_is_observed() {
        let source = "my $x = 1\nprint \"hi\";\n";
        let mut parser = Parser::new(source);
        let _ = parser.parse();
        let locations: Vec<usize> = parser
            .errors()
            .iter()
            .filter_map(|error| {
                if let ParseError::Recovered {
                    site: RecoverySite::Statement,
                    kind: RecoveryKind::InferredSemicolon,
                    location,
                } = error
                {
                    Some(*location)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            locations,
            vec![10],
            "InferredSemicolon must be recorded at the byte offset of `print` (byte 10)"
        );
    }

    /// RIPR seam: `quote_like_body_end` must return `None` when the bytes at
    /// `index` do not start with any recognized quote-like operator. The `?`
    /// after `OPERATORS.iter().find()` (statements.rs line ~930) is the exact
    /// exit point; this test makes the `None` branch observable so a value
    /// mutation on the `?` is caught.
    ///
    /// The discriminating input for the `?`-removal mutation is a span that
    /// starts with a non-operator, NON-ALPHANUMERIC character such as `/`.
    /// Alphanumeric inputs (e.g. `x`) would still return `None` after the
    /// mutation due to the `is_ascii_alphanumeric()` guard — so they cannot
    /// discriminate the `?` specifically. A bare `/` bypasses that guard
    /// and would return `Some(wrong)` under the mutation.
    #[test]
    fn quote_like_body_end_returns_none_for_non_operator_prefix() {
        // 'x', 'z', 'h' are not recognized operator prefixes; function must
        // return None rather than treating them as operators.
        assert_eq!(
            Parser::quote_like_body_end(b"x/foo/", 0),
            None,
            "'x' is not a recognized quote-like operator"
        );
        assert_eq!(
            Parser::quote_like_body_end(b"(z/foo/)", 1),
            None,
            "'z' after '(' is not a recognized quote-like operator"
        );
        assert_eq!(
            Parser::quote_like_body_end(b" h(foo)", 1),
            None,
            "'h' after space is not a recognized quote-like operator"
        );
        // Discriminates the `?` early-return specifically: a bare `/` is not a
        // recognised operator and is also not alphanumeric, so the
        // `is_ascii_alphanumeric()` guard after the operator lookup would NOT
        // trigger. Under a mutation that replaces `?` with an empty-operator
        // fallback, `quote_like_body_end(b"/foo/", 0)` would continue and
        // return `Some(_)`; the original must return `None`.
        assert_eq!(
            Parser::quote_like_body_end(b"/foo/", 0),
            None,
            "bare '/' is not a quote-like operator"
        );
        assert_eq!(
            Parser::quote_like_body_end(b"+ 1", 0),
            None,
            "bare '+' is not a quote-like operator"
        );
        // Sanity: a valid operator must be recognized so the test proves the
        // None branch, not just a broken function.
        assert!(
            Parser::quote_like_body_end(b"q(foo)", 0).is_some(),
            "'q' is a recognized operator and must return Some"
        );
    }

    /// RIPR seam: the `inferred_semicolons` filter matches on BOTH
    /// `site: RecoverySite::Statement` AND `kind: RecoveryKind::InferredSemicolon`.
    /// If the filter is mutated to count ALL errors (not just InferredSemicolon
    /// at Statement), a source with a different recovery error must expose that.
    ///
    /// `my $x =;` — missing RHS after `=` — emits
    /// `Recovered { site: InfixRhs, kind: MissingOperand }`.
    /// That is NOT an InferredSemicolon at Statement, so `inferred_semicolons`
    /// must return 0 while the mutated "count all" version returns 1.
    #[test]
    fn inferred_semicolons_filter_does_not_count_other_recovery_sites() {
        // Emits InfixRhs/MissingOperand, NOT Statement/InferredSemicolon.
        assert_eq!(
            inferred_semicolons("my $x =;"),
            0,
            "InfixRhs error must not be counted as an InferredSemicolon"
        );
        // Control: a genuine missing semicolon must still count as 1.
        assert_eq!(
            inferred_semicolons("my $x = 1\nprint \"hi\";\n"),
            1,
            "a genuine missing-semicolon must still be counted"
        );
    }
}
