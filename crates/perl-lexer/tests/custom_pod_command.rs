//! Non-standard POD commands (e.g. Pod::Weaver's `=construct`) start POD at a line start.
use perl_lexer::{PerlLexer, TokenType};

fn idents(src: &str) -> Vec<String> {
    let mut lx = PerlLexer::new(src);
    let mut out = Vec::new();
    while let Some(t) = lx.next_token() {
        if matches!(t.token_type, TokenType::EOF) {
            break;
        }
        out.push(t.text.to_string());
    }
    out
}

#[test]
fn custom_pod_command_is_skipped_until_cut() {
    let src =
        "sub one { 1 }\n\n=construct path\n\nsome 'unbalanced { text\n\n=cut\n\nsub two { 2 }\n";
    let toks = idents(src);
    assert!(toks.iter().any(|t| t == "one"), "{toks:?}");
    assert!(toks.iter().any(|t| t == "two"), "{toks:?}");
    assert!(!toks.iter().any(|t| t == "construct" || t == "unbalanced"), "{toks:?}");
}

#[test]
fn equals_followed_by_non_letter_is_not_pod() {
    let toks = idents("my $x\n= 1;\nsub two { 2 }\n");
    assert!(toks.iter().any(|t| t == "="), "{toks:?}");
}

#[test]
fn word_after_term_on_continuation_line_is_not_pod() {
    for word in ["shift", "lc", "uc", "map", "join", "sort", "scalar"] {
        let src = format!("my $x\n={word};\nsub two {{ 2 }}\n");
        let toks = idents(&src);
        assert!(toks.iter().any(|t| t == word), "{word}: {toks:?}");
        assert!(toks.iter().any(|t| t == "two"), "{word}: {toks:?}");
        // Without a later `=cut` and with one: the word must stay a token.
        let src = format!("my $x\n={word};\n=cut\nsub two {{ 2 }}\n");
        let toks = idents(&src);
        assert!(toks.iter().any(|t| t == word), "{word} +cut: {toks:?}");
        // The later `=cut` follows a statement end, so it opens POD and the text
        // after it is POD, which is how perl treats it.
        assert!(!toks.iter().any(|t| t == "two"), "{word} +cut: {toks:?}");
    }
}

#[test]
fn stray_cut_opens_pod_and_swallows_the_rest() {
    // `perl -c` accepts `my $x = 1;\n=cut\nsub two { 2 }`: a line-initial `=cut`
    // outside POD opens a block that runs to the next `=cut`, or to EOF. So `two`
    // is POD text, not a subroutine.
    let toks = idents("my $x = 1;\n=cut\nsub two { 2 }\n");
    assert!(!toks.iter().any(|t| t == "two"), "{toks:?}");
}

#[test]
fn whitespace_only_line_counts_as_blank() {
    let src = "sub one { 1 }\n \t\n=construct path\nunbalanced { text\n=cut\nsub two { 2 }\n";
    let toks = idents(src);
    assert!(toks.iter().any(|t| t == "two"), "{toks:?}");
    assert!(!toks.iter().any(|t| t == "unbalanced"), "{toks:?}");
}

#[test]
fn semicolon_in_comment_is_not_statement_end() {
    let toks = idents("my $x # note;\n=shift;\n=cut\nsub two { 2 }\n");
    assert!(toks.iter().any(|t| t == "shift"), "{toks:?}");
    // The `=cut` follows a statement end, so it opens POD and `two` is POD text.
    assert!(!toks.iter().any(|t| t == "two"), "{toks:?}");
}

#[test]
fn many_candidates_without_cut_is_linear() {
    let mut src = String::from("my $x;\n");
    for _ in 0..20000 {
        src.push_str("my $y\n=foo;\n");
    }
    let start = std::time::Instant::now();
    let toks = idents(&src);
    assert!(toks.iter().filter(|t| *t == "foo").count() == 20000);
    assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
}

#[test]
fn custom_pod_command_after_statement_end_or_block_is_pod() {
    for prefix in ["my $x = 1;\n", "sub one { 1 }\n\n", "my $y\n\n"] {
        let src = format!("{prefix}=construct path\nunbalanced {{ text\n=cut\nsub two {{ 2 }}\n");
        let toks = idents(&src);
        assert!(toks.iter().any(|t| t == "two"), "{prefix:?}: {toks:?}");
        assert!(!toks.iter().any(|t| t == "unbalanced"), "{prefix:?}: {toks:?}");
    }
}

#[test]
fn equals_word_inside_heredoc_and_string_is_untouched() {
    let src = "my $h = <<'EOT';\n=construct path\nEOT\nmy $s = \"a\n=foo\nb\";\nsub two { 2 }\n";
    let toks = idents(src);
    assert!(toks.iter().any(|t| t == "two"), "{toks:?}");
}

#[test]
fn closing_brace_term_then_equals_word_is_not_pod() {
    for (head, word) in [
        ("my %h; $h{a}\n=lc 'X';\n", "lc"),
        ("my $x = do { 1 }\n=shift;\n", "shift"),
        ("my $x = { a=>1 }\n=shift;\n", "shift"),
        ("$h->{a}\n=shift;\n", "shift"),
        ("@{$x}\n=shift;\n", "shift"),
        ("my $v = $c ? {a=>1} : {}\n=lc;\n", "lc"),
    ] {
        // A closing brace ends a term, so the `=word` after it stays code.
        let src = format!("{head}sub tail{{1}}\n");
        let toks = idents(&src);
        assert!(toks.iter().any(|t| t == word), "{head:?}: {toks:?}");
        assert!(toks.iter().any(|t| t == "tail"), "{head:?}: {toks:?}");
        // A later `=cut` does not make the `=word` a POD opener. The `=cut` itself
        // follows a statement end, so it opens POD and the text after it is POD,
        // which is how perl treats it.
        let src = format!("{head}=cut\nsub tail{{1}}\n");
        let toks = idents(&src);
        assert!(toks.iter().any(|t| t == word), "{head:?}: {toks:?}");
        assert!(!toks.iter().any(|t| t == "tail"), "{head:?}: {toks:?}");
    }
}

#[test]
fn generic_word_without_later_cut_is_not_swallowed() {
    let toks = idents("my $x = 1;\n\n=construct path\nsub two { 2 }\n");
    assert!(toks.iter().any(|t| t == "two"), "{toks:?}");
}

#[test]
fn operator_or_bracket_continuation_is_not_pod() {
    for (head, word) in [
        ("my $x =\n", "lc"),
        ("return\n", "foo"),
        ("my $v = $c ? 1 :\n", "shift"),
        ("my $v = $a and\n", "shift"),
        ("my $v = $a ||\n", "shift"),
        ("my $v = $a +\n", "shift"),
        ("my @a = (1,\n", "shift"),
        ("foo(\n", "shift"),
        ("$x = [\n", "shift"),
        ("$x = 1 if\n", "shift"),
    ] {
        let src = format!("{head}={word};\nsub tail {{ 1 }}\n=cut\n");
        let toks = idents(&src);
        assert!(toks.iter().any(|t| t == word), "{head:?}: {toks:?}");
        assert!(toks.iter().any(|t| t == "tail"), "{head:?}: {toks:?}");
    }
}

#[test]
fn hash_inside_string_or_regex_is_not_a_comment() {
    for head in ["my $s = \"foo; # x\"\n", "my $s = qr/a; #b/\n", "my $s = q{a; #b}\n"] {
        let src = format!("{head}=shift;\nsub tail {{ 1 }}\n=cut\n");
        let toks = idents(&src);
        assert!(toks.iter().any(|t| t == "shift"), "{head:?}: {toks:?}");
        assert!(toks.iter().any(|t| t == "tail"), "{head:?}: {toks:?}");
    }
}

#[test]
fn real_comments_keep_their_behavior() {
    let toks = idents("my $x # note;\n=shift;\nsub tail { 1 }\n=cut\n");
    assert!(toks.iter().any(|t| t == "shift") && toks.iter().any(|t| t == "tail"), "{toks:?}");
    let toks = idents("$x = 1; # c\n=construct path\nunbalanced { text\n=cut\nsub tail { 1 }\n");
    assert!(toks.iter().any(|t| t == "tail"), "{toks:?}");
    assert!(!toks.iter().any(|t| t == "unbalanced"), "{toks:?}");
}

#[test]
fn ambiguous_comment_line_is_not_a_statement_end() {
    for head in ["my $x = 'a' # see foo;\n", "$a / $b # x;\n", "my $x = 'a' # see {\n"] {
        let src = format!("{head}=shift;\nsub tail {{ 1 }}\n=cut\n");
        let toks = idents(&src);
        assert!(toks.iter().any(|t| t == "shift"), "{head:?}: {toks:?}");
        assert!(toks.iter().any(|t| t == "tail"), "{head:?}: {toks:?}");
    }
}
