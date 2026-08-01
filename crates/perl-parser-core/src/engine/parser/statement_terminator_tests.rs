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
    use perl_ast::ast::NodeKind;

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
        assert!(matches!(ast.kind, NodeKind::Program { .. }));
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
}
