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
    }
}

#[test]
fn custom_pod_command_after_statement_end_or_block_is_pod() {
    for prefix in ["my $x = 1;\n", "sub one { 1 }\n", "sub one { 1 }\n\n", "my $y\n\n"] {
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
