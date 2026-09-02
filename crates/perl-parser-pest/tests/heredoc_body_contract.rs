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
fn when_shift_follows_a_method_call_then_following_source_survives() {
    // perl 5.38: with `sub val { 4 }`, `$o->val <<2` prints 16 — a left shift.
    // A method call takes no unparenthesized list, so the call is a completed
    // term. Reading the method name as a bareword list operator makes `<<2` an
    // opener, and the pre-pass then deletes every following line as body text.
    for source in [
        "my $y = $o->val <<2;\nmy $z = 3;\n",
        "my $y = $o -> val <<2;\nmy $z = 3;\n",
        "my $y = Foo->new <<2;\nmy $z = 3;\n",
        "my $y = $o->val <<EOF;\nmy $z = 3;\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "method-call shift must own no body: {source:?}");
        assert_eq!(scan.stripped(), source, "no source may be removed for: {source:?}");
        assert!(
            sexp(source).contains("(variable_declaration $z   = (number 3)"),
            "the statement after a method-call shift must still parse: {source:?}"
        );
    }
}

#[test]
fn when_a_multiline_quote_like_closes_with_modifiers_then_the_shift_owns_no_body() {
    // perl 5.38: `my $x = qr{\nfoo\n}ix <<2;` compiles and the next statement
    // runs, so `<<2` is a left shift. The same-line paths already consume
    // trailing modifiers, but a construct carried across lines closed at its
    // delimiter and left `ix` looking like a bareword — which made the scanner
    // take the shift for an opener and delete every following line.
    for source in [
        "my $x = qr{\nfoo\n}ix <<2;\nmy $z = 3;\n",
        "my $x = m{\nfoo\n}i <<2;\nmy $z = 3;\n",
        "my $x = s{\nfoo\n}{bar}g <<2;\nmy $z = 3;\n",
        "my $x = tr{\nabc\n}{xyz}r <<2;\nmy $z = 3;\n",
        // A bare regex left open across lines carries modifiers too.
        "my $x = /a\nb/i <<2;\nmy $z = 3;\n",
        "my $x = $t =~ /a\nb/gi <<2;\nmy $z = 3;\n",
        // The no-modifier form was already correct; keeping it here pins that
        // the fix consumes modifiers rather than blanket-skipping letters.
        "my $x = qr{\nfoo\n} <<2;\nmy $z = 3;\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "carried-construct shift owns no body: {source:?}");
        assert_eq!(scan.stripped(), source, "no source may be removed for: {source:?}");
    }
}

#[test]
fn when_a_closing_delimiter_is_followed_by_x_then_the_operator_decides() {
    // The sharpest case for *which* operators take modifiers, because the same
    // byte means opposite things. perl 5.38: `qq{a}x 3` is "aaa" — after `qq`,
    // `x` is the repetition operator, so it is code and `<<2` still opens a
    // heredoc. After `qr`, the same `x` is the /x regex modifier, completing the
    // term and making `<<2` a shift. Treating every quote-like operator as
    // modifier-taking swallows the repetition operator and loses the body.
    let owned = "my $x = qq{\na\n}x <<EOF;\nbody\nEOF\nmy $z = 3;\n";
    let scan = perl_parser_pest::heredoc::scan(owned);
    assert_eq!(scan.captures().len(), 1, "`x` after qq is repetition, not a modifier");
    assert_eq!(scan.captures().first().map(|capture| capture.content()), Some("body\n"));

    let shifted = "my $x = qr{\na\n}x <<2;\nmy $z = 3;\n";
    let scan = perl_parser_pest::heredoc::scan(shifted);
    assert!(scan.captures().is_empty(), "`x` after qr is a modifier, so `<<2` is a shift");
    assert_eq!(scan.stripped(), shifted, "no source may be removed for: {shifted:?}");
}

#[test]
fn when_a_quote_like_spelling_names_a_declaration_then_its_heredocs_are_owned() {
    // perl 5.38 accepts `package s { ... }` and `sub s { ... }`: after those
    // keywords the word is a name, not a substitution. Reading it as `s{...}{...}`
    // consumed to a bogus delimiter, and every opener inside the block was then
    // missed — the body stayed in the text to be misparsed as code.
    for source in [
        "package s {\n  sub hi { my $x = <<EOF;\nbody\nEOF\n  return $x; }\n}\n",
        "package q {\n  sub hi { my $x = <<EOF;\nbody\nEOF\n  return $x; }\n}\n",
        "sub s { 1 }\nmy $x = <<EOF;\nbody\nEOF\n",
        "sub y { 1 }\nmy $x = <<EOF;\nbody\nEOF\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(scan.captures().len(), 1, "declaration name is not an operator: {source:?}");
        assert_eq!(
            scan.captures().first().map(|capture| capture.content()),
            Some("body\n"),
            "the body must be owned for: {source:?}"
        );
    }

    // perl also lets the name start the next line, so the keyword has to carry
    // across the line break.
    for source in [
        "package\ns {\n  sub hi { my $x = <<EOF;\nbody\nEOF\n  return $x; }\n}\n",
        "sub\ns { 1 }\nmy $x = <<EOF;\nbody\nEOF\n",
        // Blank lines between the keyword and the name are insignificant to
        // Perl, so they must not clear the context either.
        "package\n\ns {\n  sub hi { my $x = <<EOF;\nbody\nEOF\n  return $x; }\n}\n",
        "package\n   \n\ns {\n  sub hi { my $x = <<EOF;\nbody\nEOF\n  return $x; }\n}\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(scan.captures().len(), 1, "the keyword must carry across lines: {source:?}");
        assert_eq!(
            scan.captures().first().map(|capture| capture.content()),
            Some("body\n"),
            "the body must be owned for: {source:?}"
        );
    }

    // The guard must not disarm the operator itself: `s/a/b/` is still a
    // substitution and owns nothing, and no source is removed.
    for source in ["my $y = s/a/b/;\nmy $z = 3;\n", "my $y = s{a}{b};\nmy $z = 3;\n"] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "substitution still owns nothing: {source:?}");
        assert_eq!(scan.stripped(), source, "no source may be removed for: {source:?}");
    }

    // The carried keyword is read from the walk, not the raw text, so a word in
    // a comment or a string cannot arm it — and it must be a whole word. Each
    // of these leaves the next line's `s{a}{b}` an operator.
    for source in [
        "my $p = 1; # package\ns{a}{b};\nmy $z = 3;\n",
        "my $t = \"package\";\ns{a}{b};\nmy $z = 3;\n",
        "my $mypackage = 1;\ns{a}{b};\nmy $z = 3;\n",
        // Carrying across a blank line must not resurrect a stale keyword: an
        // ordinary line before the gap still leaves the operator armed.
        "my $q = 1;\n\ns{a}{b};\nmy $z = 3;\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "a non-keyword must not arm the guard: {source:?}");
        assert_eq!(scan.stripped(), source, "no source may be removed for: {source:?}");
    }
}

#[test]
fn when_a_replacement_stands_off_from_its_pattern_then_a_later_heredoc_is_owned() {
    // perl 5.38: `s{a} {b}` substitutes normally and a heredoc after it still
    // attaches. The same-line path took the gap itself as the second section's
    // delimiter, so the run swallowed following lines; `continue_construct`
    // already skipped that whitespace, so the two paths also disagreed.
    for source in [
        "$t =~ s{a} {b};\nmy $x = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        "$t =~ s{a}\t{b};\nmy $x = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        "$t =~ tr{a} {b};\nmy $x = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        // The gapless form must keep working.
        "$t =~ s{a}{b};\nmy $x = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        // The replacement may start on a later line, and a comment may sit in
        // the gap. perl draws the line by adjacency: `s{a} #b#` is a fatal
        // "Substitution replacement not terminated" because the space makes the
        // `#` a comment, so the replacement is on the next line.
        "$t =~ s{a} # comment\n{b};\nmy $x = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        "$t =~ s{a}\n{b};\nmy $x = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(scan.captures().len(), 1, "the heredoc after it must be seen: {source:?}");
        assert_eq!(
            scan.captures().first().map(|capture| capture.content()),
            Some("body\n"),
            "the body must be owned for: {source:?}"
        );
        assert!(
            scan.stripped().contains("my $z = 3;"),
            "code after the terminator must survive: {source:?}"
        );
    }

    // The other side of that adjacency rule: `s{a}#b#` *is* valid perl with `#`
    // as the replacement delimiter (it substitutes, where the spaced form is a
    // compile error), so an adjacent `#` must stay a delimiter and the
    // substitution must still own nothing.
    // The heredoc after it on the same line is what discriminates: if the `#`
    // were read as a comment instead, everything after it — including the
    // opener — would be swallowed. perl runs this and yields `x=body`.
    let source = "$t =~ s{a}#b#; my $x = <<EOF;\nbody\nEOF\nmy $z = 3;\n";
    let scan = perl_parser_pest::heredoc::scan(source);
    assert_eq!(scan.captures().len(), 1, "an adjacent `#` is a delimiter, not a comment");
    assert_eq!(
        scan.captures().first().map(|capture| capture.content()),
        Some("body\n"),
        "the heredoc after the substitution must still be owned"
    );
}

#[test]
fn when_a_control_character_variable_precedes_a_shift_then_no_body_is_owned() {
    // perl 5.38: with `$^W` set to 1, `$^W <<2` is 4 — a left shift. The
    // punctuation-variable rule only inspects two bytes and cannot see these
    // three-byte names, so `<<2` was taken for an opener and the rest of the
    // file was consumed as body text.
    //
    // This crate's grammar *does* admit a heredoc here, so the scanner and the
    // grammar deliberately disagree, exactly as they do for `=cutlass` POD
    // prose. That disagreement is reported by `parse_heredoc_outcome` rather
    // than silently resolved, and refusing to delete real source is the safe
    // side of it — which is why these rows are not in the agreement matrix.
    for source in ["my $y = $^W <<2;\nmy $z = 3;\n", "my $y = $^H <<2;\nmy $z = 3;\n"] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "control-char variable is a value: {source:?}");
        assert_eq!(scan.stripped(), source, "no source may be removed for: {source:?}");
    }
}

#[test]
fn when_opener_follows_defined_or_then_the_body_is_owned() {
    // perl 5.38: `my $x = $u // <<EOF;` assigns the heredoc body when `$u` is
    // undef, so `//` here is the defined-or *operator* and `<<EOF` starts a
    // term. Treating `//` as a completed value (an empty pattern) makes the
    // scanner miss the opener, leaving the body to be misparsed as code.
    for source in [
        "my $x = $u // <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        "my $x = $u // $v // <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        "my $x = /a/ // <<EOF;\nbody\nEOF\nmy $z = 3;\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(scan.captures().len(), 1, "defined-or must leave term position: {source:?}");
        assert_eq!(
            scan.captures().first().map(|capture| capture.content()),
            Some("body\n"),
            "the body must be owned for: {source:?}"
        );
        assert!(
            !scan.stripped().contains("body"),
            "the body must leave the text handed to Pest for: {source:?}"
        );
        assert!(
            sexp(source).contains("(variable_declaration $z   = (number 3)"),
            "code after the terminator must still parse: {source:?}"
        );
    }
}

#[test]
fn when_empty_pattern_is_in_term_position_then_it_completes_a_term() {
    // The other half of the `//` split: in term position `//` is an empty
    // pattern, a value, so a `<<` after it is a shift and owns no body.
    for source in ["my $y = $x =~ // <<2;\nmy $z = 3;\n", "my $y = $x =~ //i <<2;\nmy $z = 3;\n"] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert!(scan.captures().is_empty(), "empty pattern is a value: {source:?}");
        assert_eq!(scan.stripped(), source, "no source may be removed for: {source:?}");
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
    let rows: [&str; 35] = [
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
        // Defined-or is an operator, so it leaves term position open.
        "my $y = $u // <<EOF;\n",
        "my $y = $u // $v // <<EOF;\n",
        "my $y = /a/ // <<EOF;\n",
        // Operator position — a completed term makes `<<` a left shift.
        "my $y = $x <<2;\n",
        "my $y = 1 <<2;\n",
        "my $y = f() <<2;\n",
        "my $y = $a[0] <<2;\n",
        "my $y = $i++ <<2;\n",
        "my $y = $i-- <<2;\n",
        // A method call takes no unparenthesized list, so it completes a term.
        "my $y = $o->val <<2;\n",
        "my $y = $o -> val <<2;\n",
        "my $y = Foo->new <<2;\n",
        // A carried quote-like construct completes only after its modifiers.
        "my $y = qr{\nfoo\n}ix <<2;\n",
        "my $y = m{\nfoo\n}i <<2;\n",
        "my $y = s{\nfoo\n}{bar}g <<2;\n",
        // The same byte, opposite meanings: `x` is the repetition operator after
        // `qq` (so `<<2` still opens) and the /x modifier after `qr` (so it does
        // not). Only an operator-aware rule agrees with the grammar on both.
        "my $y = qq{\na\n}x <<2;\n",
        "my $y = qr{\na\n}x <<2;\n",
        // Completed terms the preceding byte alone cannot recognize: a regex
        // ends in `/` or a flag letter, a special variable in punctuation, a
        // hex literal in a letter, a qualified name in a word byte.
        "my $y = /a/ <<2;\n",
        "my $y = /a/i <<2;\n",
        "my $y = $! <<2;\n",
        "my $y = $? <<2;\n",
        "my $y = $@ <<2;\n",
        "my $y = Foo::BAR <<2;\n",
        "my $y = 0xff <<2;\n",
        "my $y = \"s\" <<2;\n",
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
fn when_the_scanner_and_grammar_disagree_then_the_outcome_reports_it() -> Result<(), String> {
    // POD prose reading `=cutlass` is a genuine disagreement: the grammar's own
    // POD handling ends the block on any `=cut` prefix and reports an opener in
    // the prose, while the scanner (correctly) keeps the block closed and owns
    // nothing. A missed opener creates no capture, so no per-capture defect can
    // fire — this diagnostic is the only thing that can see it, and the test
    // fails if that block is removed.
    let source =
        "my $x=1;\n=pod\n\n=cutlass\n\nmy $y = <<EOF;\nhi\nEOF\n\n=cut\nprint \"after\";\n";
    let captured = perl_parser_pest::heredoc::scan(source).captures().len();
    assert!(
        grammar_openers(source) > captured,
        "fixture must actually make the grammar see more openers than the scanner"
    );

    let mut parser = PureRustPerlParser::new();
    let ParseAttempt::Outcome(outcome) = parser.parse_heredoc_outcome(source) else {
        return Err("expected a parser-domain outcome".to_string());
    };
    assert_eq!(outcome.completeness(), ParseCompleteness::Recovered);
    assert!(
        outcome.diagnostics().iter().any(|d| d.message().contains("scanner owned")),
        "the disagreement must be reported; got {:?}",
        outcome.diagnostics()
    );
    assert!(!outcome.recovery_ranges().is_empty(), "the disagreement must carry a range");
    Ok(())
}

#[test]
fn when_the_scanner_owns_a_user_sub_opener_then_the_outcome_is_complete() {
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

// --- Review round 3 ---------------------------------------------------------
//
// Every row here is a *completed term* followed by `<<`, which Perl and the
// grammar both read as a left shift. The preceding byte alone cannot tell them
// apart — `/a/i` ends in a word byte, `$!` in punctuation, `0xff` in a letter —
// so each one was a false positive that deleted following source.

#[test]
fn when_a_completed_term_precedes_the_shift_then_no_body_is_owned() {
    let rows: [(&str, &str); 8] = [
        ("completed bare regex", "my $y = /a/ <<2;\nmy $z = 3;\n"),
        ("regex with flags", "my $y = /a/i <<2;\nmy $z = 3;\n"),
        ("special variable $!", "my $y = $! <<2;\nmy $z = 3;\n"),
        ("special variable $?", "my $y = $? <<2;\nmy $z = 3;\n"),
        ("special variable $@", "my $y = $@ <<2;\nmy $z = 3;\n"),
        ("qualified name", "my $y = Foo::BAR <<2;\nmy $z = 3;\n"),
        ("hexadecimal literal", "my $y = 0xff <<2;\nmy $z = 3;\n"),
        ("string literal", "my $y = \"s\" <<2;\nmy $z = 3;\n"),
    ];
    for (label, source) in rows {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(
            scan.captures().len(),
            grammar_openers(source),
            "{label}: scanner and grammar must agree for {source:?}"
        );
        assert!(scan.captures().is_empty(), "{label}: a shift owns no body");
        assert_eq!(scan.stripped(), source, "{label}: no source may be removed");
    }
}

#[test]
fn when_a_name_merely_starts_with_a_quote_like_letter_then_it_is_not_an_operator() {
    // `$s->trim()` is not an `s///`. Treating it as one consumes to a bogus
    // delimiter and desynchronizes the scan, which surfaces as a *later*
    // heredoc silently losing its body — the subtlest failure in this family.
    let source = "my $t = $s->trim();\nmy $x = <<EOF;\nbody\nEOF\nmy $z = 3;\n";
    assert_eq!(
        heredoc_contents(source),
        vec![("EOF".to_string(), "body\n".to_string())],
        "a later heredoc must still own its body"
    );
    assert_eq!(perl_parser_pest::heredoc::scan(source).captures().len(), grammar_openers(source));
}

#[test]
fn when_source_uses_bare_cr_line_endings_then_bodies_are_still_owned() {
    // Recognizing only LF would make a bare-CR file one enormous line, so no
    // terminator could ever be found and the body would stay in the parsed text.
    let source = "my $x = <<EOF;\rbody\rEOF\rmy $z = 3;\r";
    let scan = perl_parser_pest::heredoc::scan(source);
    assert_eq!(scan.captures().len(), 1);
    assert_eq!(scan.captures()[0].content(), "body\r");
    assert!(scan.captures()[0].terminated());
    assert!(!scan.stripped().contains("body"), "the body must leave the parsed text");
    assert!(scan.stripped().contains("my $z = 3;"), "following code must survive");
}

#[test]
fn when_openers_exceed_the_depth_budget_then_the_excess_is_recorded_not_dropped() {
    // Silently truncating left those openers with no capture at all, so nothing
    // explained their empty content.
    let mut line = String::from("my @x = (");
    for index in 0..=MAX_HEREDOC_DEPTH {
        line.push_str(&format!("<<M{index}, "));
    }
    line.push_str(");\n");

    let scan = perl_parser_pest::heredoc::scan(&line);
    assert_eq!(scan.completeness(), ParseCompleteness::Unsupported);
    assert!(
        scan.captures().iter().any(|c| c.defect() == Some(HeredocDefect::DepthOverBudget)),
        "openers past the budget must be recorded with the depth defect"
    );
    assert!(
        scan.captures()
            .iter()
            .filter(|c| c.defect() == Some(HeredocDefect::DepthOverBudget))
            .all(|c| !c.terminated() && c.content().is_empty()),
        "an over-depth opener owns no body and claims no terminator"
    );
}

// --- Review round 4 ---------------------------------------------------------

#[test]
fn when_defined_or_precedes_a_heredoc_then_the_body_is_still_owned() {
    // `//` is one token. Letting its second slash open a regex scan left an
    // unterminated construct that carried to the next line and swallowed the
    // heredoc below it — a false *negative* reached through a false positive.
    for source in [
        "my $x = $a // 7;\nmy $h = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        "my $x = undef // 7;\nmy $h = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        "my $x = f() // 7;\nmy $h = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(
            scan.captures().len(),
            grammar_openers(source),
            "scanner and grammar must agree for {source:?}"
        );
        assert_eq!(
            heredoc_contents(source),
            vec![("EOF".to_string(), "body\n".to_string())],
            "the heredoc after a `//` must own its body: {source:?}"
        );
    }
}

#[test]
fn when_a_substitution_spans_lines_then_its_replacement_is_not_scanned_as_code() {
    // `s{...}\n{...}` has two sections. Forgetting the second across a line
    // break scanned the replacement as code, so `<<MARKER` inside it would be
    // taken as an opener and delete following source.
    // The discriminating shape is a *first* section that spans lines: the
    // carried state must remember that a replacement section still follows, or
    // the scanner resumes inside it and reads `<<EOF` as an opener.
    for source in [
        "$t =~ s{aa\n bb}{<<EOF};\nmy $h = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
        "$t =~ tr{aa\n bb}{<<EOF};\nmy $h = <<EOF;\nbody\nEOF\nmy $z = 3;\n",
    ] {
        let scan = perl_parser_pest::heredoc::scan(source);
        assert_eq!(scan.captures().len(), 1, "only the real heredoc may be owned: {source:?}");
        assert_eq!(scan.captures()[0].content(), "body\n", "for {source:?}");
        assert!(
            scan.stripped().contains("{<<EOF};"),
            "the replacement section must survive in the parsed text: {source:?}"
        );
    }
}

#[test]
fn when_the_grammar_over_reports_inside_a_multiline_substitution_then_it_is_not_complete()
-> Result<(), String> {
    // The grammar has no multiline `s///` state either, so it *also* reads the
    // replacement's marker as an opener — two openers against the scanner's
    // one. The body then attaches to the wrong node. The scanner is right and
    // the grammar is wrong here, which is exactly the case the outcome-level
    // disagreement check exists to surface rather than pass off as clean.
    let source = "$t =~ s{aaa}\n{<<EOF};\nmy $h = <<EOF;\nbody\nEOF\nmy $z = 3;\n";
    let mut parser = PureRustPerlParser::new();
    let ParseAttempt::Outcome(outcome) = parser.parse_heredoc_outcome(source) else {
        return Err("expected a parser-domain outcome".to_string());
    };
    assert_ne!(
        outcome.completeness(),
        ParseCompleteness::Complete,
        "a scanner/grammar disagreement must never report Complete"
    );
    assert!(
        outcome.diagnostics().iter().any(|d| d.message().contains("scanner owned")),
        "the disagreement must be named; got {:?}",
        outcome.diagnostics()
    );
    Ok(())
}

#[test]
fn when_an_indented_bare_cr_body_is_stripped_then_every_line_loses_its_indent() {
    // `split_inclusive_lines` must use the same line model as `physical_lines`.
    // Splitting only on LF left a bare-CR body as one line, so `<<~` stripped
    // the first line's indentation and left the rest over-indented.
    let scan = perl_parser_pest::heredoc::scan("my $x = <<~EOF;\r    a\r    b\r    EOF\r");
    assert_eq!(
        scan.captures()[0].content(),
        "a\rb\r",
        "indentation must be removed from every line, not just the first"
    );
}
