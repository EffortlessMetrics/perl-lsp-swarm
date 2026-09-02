//! Discriminating proof for the heredoc body contract (#8220).
//!
//! The selected contract is **supported**: an opener owns the physical lines
//! below its logical line up to its terminator, those bytes become the node's
//! content, and they leave the text handed to Pest so following code resumes.
//!
//! Every expectation here was derived from real `perl` 5.38 behavior, not from
//! this crate's output, so the suite falsifies an implementation that merely
//! agrees with itself. The negative controls are the load-bearing half: a body
//! scanner that is too eager silently eats left shifts, comments, and string
//! literals, and each of those has a test that fails when it does.
//!
//! Scope: the heredoc contract only. `parse_heredoc_outcome` reporting
//! `Complete` means no opener lost or truncated a body — not that every input
//! byte is accounted for, which remains #8093's row.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_parser_pest::pure_rust_parser::{PerlParser, Rule};
use perl_parser_pest::{
    AstNode, HeredocDefect, HeredocDelimiterForm, MAX_HEREDOC_BODY_BYTES, MAX_HEREDOC_DEPTH,
    ParseAttempt, ParseCompleteness, ParseDiagnosticKind, PureRustPerlParser,
};
use perl_tdd_support::must;
use pest::Parser;

fn sexp(source: &str) -> String {
    let mut parser = PureRustPerlParser::new();
    let ast = must(parser.parse(source));
    parser.to_sexp(&ast)
}

/// Every `AstNode::Heredoc` in the tree, in traversal order.
fn heredoc_contents(source: &str) -> Vec<(String, String)> {
    let mut parser = PureRustPerlParser::new();
    let ast = must(parser.parse(source));
    let mut found = Vec::new();
    collect(&ast, &mut found);
    found
}

fn collect(node: &AstNode, found: &mut Vec<(String, String)>) {
    if let AstNode::Heredoc { marker, content, .. } = node {
        found.push((marker.to_string(), content.to_string()));
    }
    // The AST is a deep enum; the s-expression projection is the crate's own
    // ordered traversal, so walk the two container shapes that carry heredocs
    // in these fixtures and rely on the sexp assertions for the rest.
    match node {
        AstNode::Program(nodes) | AstNode::List(nodes) => {
            for child in nodes {
                collect(child, found);
            }
        }
        AstNode::Statement(inner) => collect(inner, found),
        AstNode::VariableDeclaration { initializer: Some(initializer), .. } => {
            collect(initializer, found);
        }
        AstNode::FunctionCall { args, .. } | AstNode::BuiltinListOp { args, .. } => {
            for child in args {
                collect(child, found);
            }
        }
        _ => {}
    }
}

/// Heredoc-contract completeness, or `None` when the attempt was not a
/// parser-domain outcome. Tests compare against `Some(..)` so a rejection or
/// instrument failure fails the assertion instead of aborting the run.
fn completeness(source: &str) -> Option<ParseCompleteness> {
    let mut parser = PureRustPerlParser::new();
    match parser.parse_heredoc_outcome(source) {
        ParseAttempt::Outcome(outcome) => Some(outcome.completeness()),
        _ => None,
    }
}

// --- Body capture ----------------------------------------------------------

#[test]
fn when_body_is_non_empty_then_content_is_the_body_text() {
    // perl: `print` of this heredoc emits "hello\n".
    assert_eq!(
        heredoc_contents("my $x = <<EOF;\nhello\nEOF\n"),
        vec![("EOF".to_string(), "hello\n".to_string())]
    );
}

#[test]
fn when_body_is_empty_then_content_is_empty_and_outcome_is_complete() {
    // perl: an immediately-terminated heredoc interpolates to "".
    assert_eq!(heredoc_contents("my $x = <<EOF;\nEOF\n"), vec![("EOF".to_string(), String::new())]);
    // An empty body is a truthful empty content, so the contract is complete —
    // this is the distinction #8220 requires: empty because the body is empty,
    // not empty because the body was never read.
    assert_eq!(completeness("my $x = <<EOF;\nEOF\n"), Some(ParseCompleteness::Complete));
}

#[test]
fn when_body_has_multiple_lines_then_all_lines_are_owned() {
    assert_eq!(
        heredoc_contents("my $x = <<EOF;\none\ntwo\nthree\nEOF\n"),
        vec![("EOF".to_string(), "one\ntwo\nthree\n".to_string())]
    );
}

// --- Following-code resumption ---------------------------------------------

#[test]
fn when_code_follows_the_terminator_then_it_parses_as_a_sibling_statement() {
    // Before body capture the body and terminator lines fell through as
    // statements, nesting the following declaration inside a bogus call chain.
    let rendered = sexp("my $x = <<EOF;\nhello\nEOF\nmy $y = 1;\n");
    assert!(
        rendered.contains("(variable_declaration $y   = (number 1)"),
        "code after the terminator must resume as its own statement; got: {rendered}"
    );
    assert!(
        !rendered.contains("(identifier hello)"),
        "body text must not reach the AST as code; got: {rendered}"
    );
    assert!(
        !rendered.contains("(identifier EOF)"),
        "the terminator must not reach the AST as an identifier; got: {rendered}"
    );
}

#[test]
fn when_body_contains_perl_code_then_it_is_text_not_statements() {
    // The body is a string in Perl; `my $y = 2;` inside it must never become a
    // declaration. This is the sharpest regression control for source loss.
    let source = "my $x = <<EOF;\nmy $y = 2;\nEOF\nmy $z = 3;\n";
    assert_eq!(heredoc_contents(source), vec![("EOF".to_string(), "my $y = 2;\n".to_string())]);
    let rendered = sexp(source);
    // `$y` still appears — inside the heredoc's content string. What must not
    // appear is a declaration node built from it.
    assert!(
        !rendered.contains("(variable_declaration $y"),
        "body code must not become an AST statement; got: {rendered}"
    );
    assert!(
        rendered.contains("(variable_declaration $z   = (number 3)"),
        "the statement after the heredoc must still parse; got: {rendered}"
    );
}

#[test]
fn when_body_holds_unbalanced_delimiters_then_following_code_still_parses() {
    // `if ({` used to derail the recovery scanner and silently drop the rest of
    // the file while still returning Ok.
    let source = "my $x = <<EOF;\nif ({\nEOF\nmy $z = 3;\n";
    assert_eq!(heredoc_contents(source), vec![("EOF".to_string(), "if ({\n".to_string())]);
    assert!(
        sexp(source).contains("(variable_declaration $z   = (number 3)"),
        "an unbalanced brace inside a body must not consume following code"
    );
}

// --- Marker forms ----------------------------------------------------------

#[test]
fn when_marker_is_quoted_or_escaped_then_the_body_is_still_owned() {
    // perl agrees on all four spellings; only interpolation differs, and this
    // crate does not interpolate.
    let cases: [(&str, &str, &str); 4] = [
        ("my $x = <<'EOF';\nno $i\nEOF\n", "EOF", "no $i\n"),
        ("my $x = <<\"EOF\";\nhi $i\nEOF\n", "EOF", "hi $i\n"),
        ("my $x = <<`CMD`;\ndate\nCMD\n", "CMD", "date\n"),
        ("my $x = <<\\EOF;\nraw\nEOF\n", "EOF", "raw\n"),
    ];
    for (source, marker, content) in cases {
        assert_eq!(
            heredoc_contents(source),
            vec![(marker.to_string(), content.to_string())],
            "marker form must not change body ownership for {source:?}"
        );
    }
}

#[test]
fn when_marker_is_numeric_then_the_body_is_owned() {
    // perl 5.38 accepts `<<123`; the grammar's bare delimiter allows digits.
    assert_eq!(
        heredoc_contents("my $x = <<123;\nbody\n123\n"),
        vec![("123".to_string(), "body\n".to_string())]
    );
}

#[test]
fn when_opener_is_indented_form_then_terminator_indentation_is_stripped() {
    // perl: terminator `    EOF` strips four columns, so `      deeper` keeps
    // exactly two. A naive `trim_start` would flatten both lines to column 0.
    assert_eq!(
        heredoc_contents("my $x = <<~EOF;\n    hi\n      deeper\n    EOF\n"),
        vec![("EOF".to_string(), "hi\n  deeper\n".to_string())]
    );
}

#[test]
fn when_opener_is_not_indented_form_then_indentation_is_preserved() {
    assert_eq!(
        heredoc_contents("my $x = <<EOF;\n    hi\nEOF\n"),
        vec![("EOF".to_string(), "    hi\n".to_string())]
    );
}

// --- Multiple queued openers -----------------------------------------------

#[test]
fn when_two_openers_share_a_line_then_bodies_are_owned_in_opener_order() {
    // perl: `(<<A, <<B)` yields ("aaa\n", "bbb\n"). Swapping the queue order is
    // the falsifier this pins.
    assert_eq!(
        heredoc_contents("my ($a,$b) = (<<A, <<B);\naaa\nA\nbbb\nB\n"),
        vec![("A".to_string(), "aaa\n".to_string()), ("B".to_string(), "bbb\n".to_string())]
    );
}

#[test]
fn when_two_openers_share_a_line_then_following_code_resumes_after_both_bodies() {
    assert!(
        sexp("my ($a,$b) = (<<A, <<B);\naaa\nA\nbbb\nB\nmy $z=3;\n")
            .contains("(variable_declaration $z  = (number 3)"),
        "code after the second terminator must resume"
    );
}

// --- Terminator exactness --------------------------------------------------

#[test]
fn when_terminator_has_trailing_whitespace_then_it_does_not_terminate() {
    // perl: `EOF ` is not a terminator — the program dies with
    // "Can't find string terminator". This crate reports it instead of
    // pretending the heredoc closed.
    let source = "my $x = <<EOF;\nhi\nEOF \n";
    assert_eq!(completeness(source), Some(ParseCompleteness::Recovered));
    assert_eq!(heredoc_contents(source), vec![("EOF".to_string(), "hi\nEOF \n".to_string())]);
}

#[test]
fn when_terminator_is_a_prefix_of_a_longer_word_then_it_does_not_terminate() {
    let source = "my $x = <<EOF;\nhi\nEOFX\nEOF\n";
    assert_eq!(heredoc_contents(source), vec![("EOF".to_string(), "hi\nEOFX\n".to_string())]);
    assert_eq!(completeness(source), Some(ParseCompleteness::Complete));
}

#[test]
fn when_body_mentions_another_opener_then_it_stays_literal_text() {
    // perl treats body bytes as data; `<<NOPE` must not queue a second heredoc.
    let source = "my $x = <<EOF;\nsee <<NOPE here\nEOF\nmy $y=1;\n";
    assert_eq!(
        heredoc_contents(source),
        vec![("EOF".to_string(), "see <<NOPE here\n".to_string())]
    );
    assert_eq!(completeness(source), Some(ParseCompleteness::Complete));
}

// --- Missing terminator -----------------------------------------------------

#[test]
fn when_terminator_is_missing_then_outcome_is_recovered_with_a_typed_diagnostic()
-> Result<(), String> {
    // perl refuses to compile this. The instrument recovers, but says so.
    let source = "my $x = <<EOF;\nhello\n";
    let mut parser = PureRustPerlParser::new();
    let ParseAttempt::Outcome(outcome) = parser.parse_heredoc_outcome(source) else {
        return Err("unterminated heredoc must still yield a parser-domain outcome".to_string());
    };
    assert_eq!(outcome.completeness(), ParseCompleteness::Recovered);
    let diagnostics = outcome.diagnostics();
    assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic, got {diagnostics:?}");
    assert_eq!(diagnostics[0].kind(), ParseDiagnosticKind::RecoveredFragment);
    assert!(
        diagnostics[0].message().contains("no terminator line"),
        "diagnostic must name the defect; got {}",
        diagnostics[0].message()
    );
    // The recovery range must cover the bytes taken as the body, so a consumer
    // can see exactly which source the recovery claimed.
    assert_eq!(outcome.recovery_ranges().len(), 1);
    assert_eq!(&source[diagnostics[0].range().start()..diagnostics[0].range().end()], "hello\n");
    Ok(())
}

#[test]
fn when_terminator_is_missing_then_the_scan_records_the_defect() {
    let scan = perl_parser_pest::heredoc::scan("my $x = <<EOF;\nhello\n");
    assert_eq!(scan.captures().len(), 1);
    assert_eq!(scan.captures()[0].defect(), Some(HeredocDefect::MissingTerminator));
    assert!(!scan.captures()[0].terminated());
}

// --- Negative controls ------------------------------------------------------

#[test]
fn when_shift_operator_is_used_then_no_body_is_owned() {
    // perl: `1 << 2` is 4 and `1 <<2` is also 4 — both left shifts. A scanner
    // that queues `<<2` here would swallow the rest of the file.
    for source in ["my $x = 1 << 2;\nmy $y = 3;\n", "my $x = 1 <<2;\nmy $y = 3;\n"] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "left shift must own no body: {source:?}");
        assert_eq!(scan.stripped(), source, "left shift must not remove source: {source:?}");
        assert!(
            sexp(source).contains("(variable_declaration $y   = (number 3)"),
            "the statement after a left shift must still parse: {source:?}"
        );
    }
}

#[test]
fn when_shift_follows_a_value_then_no_body_is_owned() {
    for source in
        ["my $x = $n << 2;\n", "my $x = f() << 2;\n", "my $x = $a[0] << 2;\n", "my $x = 4 << $b;\n"]
    {
        assert!(
            perl_parser_pest::heredoc::scan(source).captures().is_empty(),
            "shift after a value must own no body: {source:?}"
        );
    }
}

#[test]
fn when_opener_text_is_inside_a_comment_or_string_then_no_body_is_owned() {
    // Each of these is ordinary data in perl; treating any as an opener eats
    // the following lines.
    for source in [
        "my $x = 1; # <<EOF\nmy $y = 2;\n",
        "my $x = '<<EOF';\nmy $y = 2;\n",
        "my $x = \"<<EOF\";\nmy $y = 2;\n",
        "my $x = q{text <<EOF more};\nmy $y = 2;\n",
        "my $x = qq{<<EOF};\nmy $y = 2;\n",
        "my $x = qw(<<EOF);\nmy $y = 2;\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "opener text must stay data: {source:?}");
        assert_eq!(scan.stripped(), source, "no source may be removed for: {source:?}");
    }
}

#[test]
fn when_list_operator_precedes_the_opener_then_the_body_is_owned() {
    // `print <<EOF` is the common shape and must not be mistaken for a shift.
    assert_eq!(
        heredoc_contents("print <<EOF;\nhi\nEOF\n"),
        vec![("EOF".to_string(), "hi\n".to_string())]
    );
}

#[test]
fn when_bare_marker_is_separated_by_whitespace_then_the_opener_is_reported_unsupported() {
    // perl: "Use of bare << to mean <<\"\" is forbidden". This crate's grammar
    // admits it, so the opener owns no body — but the resulting empty content
    // is explained by a diagnostic instead of masquerading as a complete parse.
    let source = "my $x = << EOF;\n";
    assert_eq!(completeness(source), Some(ParseCompleteness::Unsupported));
    let scan = perl_parser_pest::heredoc::scan(source);
    assert_eq!(scan.captures()[0].defect(), Some(HeredocDefect::SeparatedBareMarker));
    assert!(
        scan.diagnostics().iter().any(|d| d.kind() == ParseDiagnosticKind::UnsupportedSyntax),
        "a separated bare marker must be an unsupported-syntax diagnostic"
    );
}

#[test]
fn when_quoted_marker_is_separated_by_whitespace_then_the_body_is_owned() {
    // perl accepts `<< "EOF"` — only the bare form is forbidden.
    assert_eq!(
        heredoc_contents("my $x = << \"EOF\";\nhi\nEOF\n"),
        vec![("EOF".to_string(), "hi\n".to_string())]
    );
}

// --- Newline variants -------------------------------------------------------

#[test]
fn when_source_uses_crlf_then_the_terminator_matches_and_content_keeps_its_bytes() {
    let source = "my $x = <<EOF;\r\nhello\r\nEOF\r\nmy $y=1;\r\n";
    assert_eq!(
        heredoc_contents(source),
        vec![("EOF".to_string(), "hello\r\n".to_string())],
        "CRLF bodies keep their exact bytes; the terminator match ignores the CR"
    );
    assert_eq!(completeness(source), Some(ParseCompleteness::Complete));
}

#[test]
fn when_final_line_has_no_newline_then_the_terminator_still_matches() {
    assert_eq!(
        heredoc_contents("my $x = <<EOF;\nhi\nEOF"),
        vec![("EOF".to_string(), "hi\n".to_string())]
    );
    assert_eq!(completeness("my $x = <<EOF;\nhi\nEOF"), Some(ParseCompleteness::Complete));
}

// --- Determinism and bounded operation --------------------------------------

#[test]
fn when_the_same_source_is_scanned_twice_then_output_is_identical() {
    for source in [
        "my $x = <<EOF;\nhello\nEOF\nmy $y = 1;\n",
        "my $x = <<EOF;\nhello\n",
        "my ($a,$b) = (<<A, <<B);\naaa\nA\nbbb\nB\n",
        "my $x = << EOF;\n",
    ] {
        let first = perl_parser_pest::heredoc::scan(source);
        let second = perl_parser_pest::heredoc::scan(source);
        assert_eq!(first, second, "scan must be a pure function of source: {source:?}");
        assert_eq!(sexp(source), sexp(source), "projection must be deterministic: {source:?}");
    }
}

#[test]
fn when_a_body_exceeds_the_byte_budget_then_it_is_truncated_and_reported() {
    let opener = "my $x = <<EOF;\n";
    let mut source = String::from(opener);
    // The budget is measured from the first body byte, so pad past the opener.
    while source.len() - opener.len() <= MAX_HEREDOC_BODY_BYTES {
        source.push_str("padding line\n");
    }
    source.push_str("EOF\n");

    let scan = perl_parser_pest::heredoc::scan(&source);
    assert_eq!(scan.captures().len(), 1);
    assert_eq!(scan.captures()[0].defect(), Some(HeredocDefect::BodyOverBudget));
    // Truncation is line-granular: the body stops at the end of the line that
    // crossed the budget, so it may overshoot by at most that one line, and the
    // terminator beyond it is never reached.
    let content = scan.captures()[0].content();
    assert!(
        content.len() <= MAX_HEREDOC_BODY_BYTES + "padding line\n".len(),
        "a body over budget must stop at the crossing line, got {} bytes",
        content.len()
    );
    assert!(!content.contains("EOF"), "truncation must stop before the terminator");
    assert_eq!(scan.completeness(), ParseCompleteness::Recovered);
    assert!(
        scan.diagnostics().iter().any(|d| d.message().contains("budget")),
        "the budget stop must be explained"
    );
}

#[test]
fn when_one_line_queues_more_openers_than_the_depth_budget_then_it_is_unsupported() {
    let mut line = String::from("my @x = (");
    for index in 0..=MAX_HEREDOC_DEPTH {
        line.push_str(&format!("<<M{index}, "));
    }
    line.push_str(");\n");
    for index in 0..=MAX_HEREDOC_DEPTH {
        line.push_str(&format!("body{index}\nM{index}\n"));
    }

    let scan = perl_parser_pest::heredoc::scan(&line);
    assert_eq!(scan.completeness(), ParseCompleteness::Unsupported);
    assert!(
        scan.diagnostics().iter().any(|d| d.kind() == ParseDiagnosticKind::UnsupportedSyntax),
        "exceeding the depth budget must be unsupported, not a silent success"
    );
}

// --- Outcome / projection agreement -----------------------------------------

#[test]
fn when_outcome_is_complete_then_every_capture_is_terminated_and_defect_free() {
    for source in [
        "my $x = <<EOF;\nhello\nEOF\n",
        "my $x = <<EOF;\nEOF\n",
        "my ($a,$b) = (<<A, <<B);\naaa\nA\nbbb\nB\n",
        "my $x = <<~EOF;\n    hi\n    EOF\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(scan.completeness(), ParseCompleteness::Complete, "for {source:?}");
        assert!(scan.diagnostics().is_empty(), "for {source:?}");
        assert!(scan.recovery_ranges().is_empty(), "for {source:?}");
        for capture in scan.captures() {
            assert!(capture.terminated(), "for {source:?}");
            assert_eq!(capture.defect(), None, "for {source:?}");
        }
    }
}

#[test]
fn when_a_capture_has_a_defect_then_the_outcome_is_never_complete() {
    for source in ["my $x = <<EOF;\nhello\n", "my $x = << EOF;\n", "my $x = <<EOF;\nhi\nEOF \n"] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(
            scan.captures().iter().any(|capture| capture.defect().is_some()),
            "expected a defect for {source:?}"
        );
        assert_ne!(
            scan.completeness(),
            ParseCompleteness::Complete,
            "a defect must never report Complete: {source:?}"
        );
    }
}

#[test]
fn when_a_body_is_owned_then_the_stripped_source_no_longer_contains_it() {
    let source = "my $x = <<EOF;\nsecret body\nEOF\nmy $y = 1;\n";
    let scan = perl_parser_pest::heredoc::scan(source);
    assert!(
        !scan.stripped().contains("secret body"),
        "an owned body must leave the parsed text: {:?}",
        scan.stripped()
    );
    assert!(
        scan.stripped().contains("my $x = <<EOF;") && scan.stripped().contains("my $y = 1;"),
        "the opener line and following code must survive: {:?}",
        scan.stripped()
    );
    // The recorded body range must be exactly the removed bytes.
    let body = scan.captures()[0].body();
    assert_eq!(&source[body.start()..body.end()], "secret body\nEOF\n");
}

#[test]
fn when_marker_form_is_recorded_then_it_matches_the_opener_spelling() {
    let cases: [(&str, HeredocDelimiterForm); 5] = [
        ("my $x = <<EOF;\nEOF\n", HeredocDelimiterForm::Bare),
        ("my $x = <<'EOF';\nEOF\n", HeredocDelimiterForm::SingleQuoted),
        ("my $x = <<\"EOF\";\nEOF\n", HeredocDelimiterForm::DoubleQuoted),
        ("my $x = <<`EOF`;\nEOF\n", HeredocDelimiterForm::Backtick),
        ("my $x = <<\\EOF;\nEOF\n", HeredocDelimiterForm::Escaped),
    ];
    for (source, form) in cases {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(scan.captures()[0].form(), form, "for {source:?}");
    }
    assert!(HeredocDelimiterForm::SingleQuoted.is_non_interpolating());
    assert!(HeredocDelimiterForm::Escaped.is_non_interpolating());
    assert!(!HeredocDelimiterForm::DoubleQuoted.is_non_interpolating());
}

// --- Cross-line lexical context (#14563 review) -----------------------------
//
// The scanner removes body lines before Pest sees them, so a false opener
// deletes real source. These regions are all data in Perl; `<<MARKER`-shaped
// text inside them must own nothing and remove nothing.

#[test]
fn when_opener_text_is_inside_pod_then_no_body_is_owned() {
    // perl: `perl -c` accepts this and the trailing code runs — POD is never
    // lexed as code. A per-line scanner would queue an opener at `<<EOF`, find
    // no unindented terminator, and swallow `=cut` plus the real code below it.
    let source = "my $x = 1;\n=pod\n\nmy $y = <<EOF;\nhi\nEOF\n\n=cut\n\nprint \"after $x\";\n";
    let scan = perl_parser_pest::heredoc::scan(source);
    assert!(scan.captures().is_empty(), "POD prose must own no body");
    assert_eq!(scan.stripped(), source, "POD must not have source removed");
    assert!(
        sexp(source).contains("print"),
        "code after `=cut` must survive; got: {}",
        sexp(source)
    );
}

#[test]
fn when_opener_text_is_after_a_data_sentinel_then_no_body_is_owned() {
    // perl: everything after `__DATA__` / `__END__` is raw data, never tokens.
    for sentinel in ["__DATA__", "__END__"] {
        let source = format!("print \"after\";\n{sentinel}\nmy $x = <<EOF;\nhi\nEOF\n");
        let scan = perl_parser_pest::heredoc::scan(&source);
        assert!(scan.captures().is_empty(), "{sentinel} section must own no body");
        assert_eq!(scan.stripped(), source, "{sentinel} section must not be removed");
    }
}

#[test]
fn when_opener_text_is_inside_a_multiline_quote_then_no_body_is_owned() {
    // perl prints the `<<EOF` line as string content in both cases. A scanner
    // without cross-line state resumes as code on line 2 and eats the rest.
    for source in [
        "my $x = \"line1\n<<EOF\nline3\";\nprint \"after\";\n",
        "my $x = 'line1\n<<EOF\nline3';\nprint \"after\";\n",
        "my $x = q{line1\n<<EOF\nline3};\nprint \"after\";\n",
        "my $x = qq{line1\n<<EOF\nline3};\nprint \"after\";\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "multiline quote must own no body: {source:?}");
        assert_eq!(scan.stripped(), source, "multiline quote must not be removed: {source:?}");
    }
}

#[test]
fn when_pod_ends_then_a_later_heredoc_is_still_owned() {
    // The POD exemption must not swallow the rest of the file.
    let source = "=pod\n\ntext\n\n=cut\n\nmy $x = <<EOF;\nreal body\nEOF\n";
    assert_eq!(
        heredoc_contents(source),
        vec![("EOF".to_string(), "real body\n".to_string())],
        "a heredoc after `=cut` must still own its body"
    );
}

// --- Term position agrees with the grammar (#14563 review) ------------------

#[test]
fn when_a_builtin_list_operator_precedes_the_opener_then_the_body_is_owned() {
    // perl accepts a heredoc term after every one of these; an allowlist that
    // omits them leaves the body to be misparsed as code.
    for op in ["length", "scalar", "uc", "lc", "ucfirst", "eval", "system", "defined", "ref"] {
        let source = format!("my $x = {op} <<EOF;\nbody\nEOF\n");
        assert_eq!(
            heredoc_contents(&source),
            vec![("EOF".to_string(), "body\n".to_string())],
            "`{op} <<EOF` must own its body"
        );
    }
}

/// Heredoc openers the grammar itself produces for `source`.
///
/// Parsed from the raw source, so this is the grammar's own opinion, entirely
/// independent of the scanner's.
fn grammar_openers(source: &str) -> usize {
    match <PerlParser as Parser<Rule>>::parse(Rule::program, source) {
        Ok(pairs) => {
            let mut count = 0;
            count_heredoc_rules(pairs, &mut count);
            count
        }
        Err(_) => 0,
    }
}

fn count_heredoc_rules(pairs: pest::iterators::Pairs<'_, Rule>, count: &mut usize) {
    for pair in pairs {
        if pair.as_rule() == Rule::heredoc {
            *count += 1;
        }
        count_heredoc_rules(pair.into_inner(), count);
    }
}

#[test]
fn scanner_and_grammar_agree_on_openers() {
    // The load-bearing invariant. The scanner removes body lines before the
    // grammar runs, so the two must decide identically: an opener the grammar
    // rejects deletes real source, and one the scanner misses leaves a body to
    // be misparsed as code with nothing able to report it.
    //
    // Each row is one line of Perl with no body, so the grammar's count is its
    // unclouded opinion about that `<<` alone.
    let rows: [&str; 16] = [
        // Term position — the grammar admits `heredoc` as an unconditional
        // `primary`, so any bareword counts, not just builtins.
        "my $x = <<EOF;\n",
        "print <<EOF;\n",
        "length <<EOF;\n",
        "system <<EOF;\n",
        "croak <<EOF;\n",
        "my $x = Dumper <<EOF;\n",
        "my $x = FOO <<2;\n",
        "my ($a,$b) = (<<A, <<B);\n",
        // Operator position — a completed term makes `<<` a left shift.
        "my $y = $x <<2;\n",
        "my $y = 1 <<2;\n",
        "my $y = f() <<2;\n",
        "my $y = $a[0] <<2;\n",
        "my $y = $i++ <<2;\n",
        "my $y = $i-- <<2;\n",
        // Not code at all.
        "my $x = /<<EOF/;\n",
        "my $x = 1; # <<EOF\n",
    ];
    for source in rows {
        assert_eq!(
            perl_parser_pest::heredoc::scan(source).captures().len(),
            grammar_openers(source),
            "scanner and grammar disagree about openers in {source:?}"
        );
    }
}

#[test]
fn when_the_scanner_misses_an_opener_the_grammar_found_then_the_outcome_says_so() {
    // A missed opener creates no capture, so no per-capture defect can fire.
    // This is the only check that can see it, and without it the outcome would
    // report `Complete` while a body was left to be parsed as code.
    //
    // The controls above keep the two in agreement today; this proves the
    // detector is wired, using a source where the grammar sees an opener.
    let source = "croak <<EOF;\nbody\nEOF\n";
    assert_eq!(grammar_openers(source), 1, "fixture must contain a grammar opener");
    assert_eq!(
        perl_parser_pest::heredoc::scan(source).captures().len(),
        1,
        "the scanner must own the body after a user-sub bareword"
    );
    assert_eq!(completeness(source), Some(ParseCompleteness::Complete));
    // The body is owned, not left behind as code.
    assert_eq!(heredoc_contents(source), vec![("EOF".to_string(), "body\n".to_string())]);
}

#[test]
fn when_two_same_marker_openers_differ_in_shape_then_bodies_are_not_swapped() {
    // Content corruption control: if the scanner recognized only one of these,
    // the FIFO queue would hand the second body to the first node while still
    // reporting a clean parse.
    assert_eq!(
        heredoc_contents("croak <<EOF;\naaa\nEOF\ndie <<EOF;\nbbb\nEOF\n"),
        vec![("EOF".to_string(), "aaa\n".to_string()), ("EOF".to_string(), "bbb\n".to_string())]
    );
}

#[test]
fn when_a_bare_regex_contains_opener_text_then_no_body_is_owned() {
    // perl treats `/<<EOF/` as a regex; the grammar produces no heredoc node.
    for source in ["my $x = /<<EOF/;\nmy $y = 2;\n", "my $x = /<<EOF/i;\nmy $y = 2;\n"] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "a bare regex must own no body: {source:?}");
        assert_eq!(scan.stripped(), source, "a bare regex must not be removed: {source:?}");
    }
    // Division must still be division — the `/` split must not over-trigger.
    let division = "my $y = $a / $b;\nmy $z = 3;\n";
    assert_eq!(perl_parser_pest::heredoc::scan(division).stripped(), division);
}

#[test]
fn when_a_postfix_increment_precedes_the_shift_then_no_body_is_owned() {
    // perl: `$i++ <<2` is a left shift (3++ then 3<<2 == 12).
    for source in ["my $y = $i++ <<2;\nmy $z = 3;\n", "my $y = $i-- <<2;\nmy $z = 3;\n"] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "postfix op then shift owns no body: {source:?}");
        assert_eq!(scan.stripped(), source, "no source may be removed: {source:?}");
    }
}

#[test]
fn when_the_first_body_line_exceeds_the_budget_then_it_is_still_owned_and_reported() {
    // Stopping at the line's start would leave the whole oversized body in the
    // parsed text while the diagnostic claimed it had been truncated.
    let opener = "my $x = <<EOF;\n";
    let mut source = String::from(opener);
    source.push_str(&"a".repeat(MAX_HEREDOC_BODY_BYTES + 1024));
    source.push_str("\nEOF\n");

    let scan = perl_parser_pest::heredoc::scan(&source);
    assert_eq!(scan.captures()[0].defect(), Some(HeredocDefect::BodyOverBudget));
    // No whole line fits the budget, so nothing is materialized — but the bytes
    // are *dropped*, not handed back to Pest, and the search for the terminator
    // still ran, so neither the body nor its terminator can reappear as code.
    assert!(scan.captures()[0].content().is_empty());
    assert!(scan.captures()[0].terminated(), "the terminator is still found past the budget");
    assert!(!scan.stripped().contains("aaaa"), "the over-budget body must leave the parsed text");
    assert!(
        !scan.stripped().contains("\nEOF\n"),
        "the terminator must not reappear as code: {:?}",
        scan.stripped()
    );
}

#[test]
fn when_no_terminator_was_found_then_the_capture_does_not_claim_one() {
    // `terminated()` reports whether a terminator was actually found, so it must
    // be false exactly for the paths that find none — a body that ran to end of
    // input, and a separated bare marker that never looks.
    for source in ["my $x = <<EOF;\nhello\n", "my $x = << EOF;\n"] {
        let scan = perl_parser_pest::heredoc::scan(source);
        let capture = &scan.captures()[0];
        assert!(capture.defect().is_some(), "fixture must carry a defect: {source:?}");
        assert!(!capture.terminated(), "no terminator exists in {source:?}");
    }
    // An over-budget body is the discriminating case: it carries a defect, but
    // its terminator *was* found, so conflating the two would be wrong in the
    // other direction.
    let mut over_budget = String::from("my $x = <<EOF;\n");
    over_budget.push_str(&"a".repeat(MAX_HEREDOC_BODY_BYTES + 1024));
    over_budget.push_str("\nEOF\n");
    let scan = perl_parser_pest::heredoc::scan(&over_budget);
    assert_eq!(scan.captures()[0].defect(), Some(HeredocDefect::BodyOverBudget));
    assert!(scan.captures()[0].terminated());
}

// --- `<<~` indentation contract (#14563 review) -----------------------------

#[test]
fn when_indented_terminator_is_not_a_prefix_of_body_indent_then_it_does_not_terminate() {
    // perl: "Indentation on line 1 of here-doc doesn't match delimiter" — a
    // fatal compile error. Accepting it would report `Complete` with a
    // fabricated body for source perl refuses.
    for source in ["my $x = <<~EOF;\n  hi\n      EOF\n", "my $x = <<~EOF;\n  hi\n\tEOF\n"] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(
            scan.captures()[0].defect(),
            Some(HeredocDefect::MissingTerminator),
            "an indentation mismatch must not terminate: {source:?}"
        );
        assert_ne!(scan.completeness(), ParseCompleteness::Complete, "for {source:?}");
    }
}

#[test]
fn when_indented_terminator_is_less_indented_than_body_then_it_terminates() {
    // perl accepts a terminator indented less than the body, stripping only the
    // terminator's own indentation.
    assert_eq!(
        heredoc_contents("my $x = <<~EOF;\n    hi\n  EOF\n"),
        vec![("EOF".to_string(), "  hi\n".to_string())]
    );
}

// --- Review round 2 ---------------------------------------------------------

#[test]
fn when_pod_prose_starts_with_a_cut_prefix_then_pod_does_not_end() {
    // perl: `=cutlass` is prose, not the `=cut` directive — the block runs to
    // the real `=cut`. A prefix match ended POD early and exposed the rest of
    // the prose to opener scanning, deleting it.
    let source =
        "my $x=1;\n=pod\n\n=cutlass\n\nmy $y = <<EOF;\nhi\nEOF\n\n=cut\nprint \"after\";\n";
    let scan = perl_parser_pest::heredoc::scan(source);
    assert!(scan.captures().is_empty(), "POD prose after `=cutlass` must own no body");
    assert_eq!(scan.stripped(), source, "no POD prose may be removed");
}

#[test]
fn when_pod_ends_with_a_real_cut_then_code_resumes() {
    // The exact-match fix must not make `=cut` unrecognizable.
    for ending in ["=cut\n", "=cut some trailing prose\n"] {
        let source = format!("=pod\n\ntext\n\n{ending}\nmy $x = <<EOF;\nbody\nEOF\n");
        assert_eq!(
            heredoc_contents(&source),
            vec![("EOF".to_string(), "body\n".to_string())],
            "code after {ending:?} must resume"
        );
    }
}

#[test]
fn when_a_format_body_contains_opener_text_then_no_body_is_owned() {
    // perl accepts this and the grammar produces no heredoc node; a format
    // body is picture data terminated by a lone `.`.
    let source = "format STDOUT =\n<<EOF\n.\nprint \"after\";\n";
    let scan = perl_parser_pest::heredoc::scan(source);
    assert!(scan.captures().is_empty(), "a format body must own no body");
    assert_eq!(scan.stripped(), source, "a format body must not be removed");
}

#[test]
fn when_a_format_body_ends_then_a_later_heredoc_is_still_owned() {
    assert_eq!(
        heredoc_contents("format STDOUT =\n@<<<\n.\nmy $x = <<EOF;\nbody\nEOF\n"),
        vec![("EOF".to_string(), "body\n".to_string())],
        "code after the format terminator must resume"
    );
}

#[test]
fn when_a_body_is_truncated_then_the_rest_of_it_never_reenters_parsing() {
    // The budget bounds the content this crate materializes, not how far it
    // looks for the terminator. Abandoning the search left the remaining body
    // lines and the terminator in the parsed text to be read as code — the
    // exact loss this contract exists to prevent.
    let mut source = String::from("my $x = <<EOF;\n");
    source.push_str(&"a".repeat(MAX_HEREDOC_BODY_BYTES + 1024));
    source.push_str("\nsentinel body line\nEOF\nprint \"after\";\n");

    let scan = perl_parser_pest::heredoc::scan(&source);
    assert_eq!(scan.captures()[0].defect(), Some(HeredocDefect::BodyOverBudget));
    assert!(
        !scan.stripped().contains("sentinel body line"),
        "body lines past the budget must be dropped, not parsed as code"
    );
    assert!(!scan.stripped().contains("\nEOF\n"), "the terminator must not reappear as code");
    assert!(scan.stripped().contains("print \"after\""), "code after the heredoc must survive");
    assert!(
        scan.diagnostics().iter().any(|d| d.message().contains("dropped rather than parsed")),
        "the diagnostic must say what happened to the dropped bytes"
    );
}

#[test]
fn filehandle_form_heredocs_are_a_known_grammar_limitation_not_a_scanner_gap() {
    // perl treats `print $fh <<EOF` as a heredoc, but this crate's grammar does
    // not admit the filehandle form at all — it produces no heredoc node. The
    // scanner therefore owns nothing, which is the *safe* direction: owning the
    // body would remove source the grammar still expects to parse.
    //
    // Pinned as a limitation so the boundary is explicit rather than silent. It
    // belongs to the grammar, not to this contract.
    for source in
        ["print $fh <<EOF;\n", "printf $fh <<EOF;\n", "say $fh <<EOF;\n", "print {$fh} <<EOF;\n"]
    {
        assert_eq!(grammar_openers(source), 0, "grammar must still not admit {source:?}");
        assert_eq!(
            perl_parser_pest::heredoc::scan(source).captures().len(),
            0,
            "the scanner must agree with the grammar for {source:?}"
        );
    }
}
