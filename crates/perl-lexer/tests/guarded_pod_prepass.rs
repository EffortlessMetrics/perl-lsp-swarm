//! #17440: a refused POD opener is pattern text, not declaration text.

use perl_lexer::{LexerConfig, LocalSymbolTable, PerlLexer, Token, TokenType};

const DIRECTIVES: [&str; 11] =
    ["pod", "over", "item", "back", "begin", "end", "for", "encoding", "head", "head1", "head12"];
const LINE_ENDINGS: [&str; 3] = ["\n", "\r\n", "\r"];

fn guarded_patterns() -> Vec<String> {
    let mut sources = Vec::new();
    for directive in DIRECTIVES {
        for trailing in ["", " \t", " # delim"] {
            for newline in LINE_ENDINGS {
                sources.push(format!(
                    "my $r = /{trailing}{newline}={directive} sub phantom{newline}=cut{newline}/;{newline}sub real {{ }}{newline}"
                ));
            }
        }
    }
    sources
}

fn tokens_with_prepass(source: &str) -> Vec<Token> {
    let config = LexerConfig {
        symbol_table: Some(LocalSymbolTable::scan_subs(source)),
        ..LexerConfig::default()
    };
    PerlLexer::with_config(source, config).collect_tokens()
}

#[test]
fn guarded_pod_directive_text_does_not_declare_pattern_subs() {
    for source in guarded_patterns() {
        let table = LocalSymbolTable::scan_subs(&source);
        assert!(!table.is_known_sub("phantom"), "pattern text declared phantom in {source:?}");
        assert!(table.is_known_sub("real"), "lost the real declaration in {source:?}");
    }
}

#[test]
fn guarded_pod_pattern_name_keeps_exact_later_division_spans() {
    for prefix in guarded_patterns() {
        // The first row is the issue fixture verbatim. Its later slashes are
        // [56, 57) and [58, 59); the earlier bare regex is legitimate.
        let source = format!("{prefix}phantom /x/;");
        let slash_start = prefix.len() + "phantom ".len();
        let tokens = tokens_with_prepass(&source);
        assert!(
            tokens.iter().any(|token| {
                token.start == "my $r = ".len()
                    && token.end <= prefix.len()
                    && matches!(token.token_type, TokenType::RegexMatch)
            }),
            "the initial regex must remain a regex: {source:?}; {tokens:?}"
        );
        for start in [slash_start, slash_start + 2] {
            assert!(
                tokens.iter().any(|token| {
                    token.start == start
                        && token.end == start + 1
                        && matches!(token.token_type, TokenType::Division)
                }),
                "expected division at [{start}, {}): {source:?}; {tokens:?}",
                start + 1
            );
        }
    }
}

#[test]
fn real_known_sub_after_guarded_pod_pattern_keeps_regex_path() {
    // No =cut: opening POD here would hide the real declaration to EOF.
    let prefix = "my $r = /\n=pod sub phantom\n/;\nsub real { }\n";
    let source = format!("{prefix}real /x/;");
    let table = LocalSymbolTable::scan_subs(&source);
    assert!(table.is_known_sub("real"));
    let slash_start = prefix.len() + "real ".len();
    let tokens = tokens_with_prepass(&source);
    assert!(
        tokens.iter().any(|token| {
            token.start == slash_start
                && token.end == slash_start + "/x/".len()
                && matches!(token.token_type, TokenType::RegexMatch)
        }),
        "the real call must retain its own regex argument: {tokens:?}"
    );
}

#[test]
fn guarded_pattern_same_line_closer_keeps_real_code_suffix() {
    for directive in DIRECTIVES {
        for newline in LINE_ENDINGS {
            for pattern in ["sub phantom", r"escaped \/ sub phantom"] {
                let prefix =
                    format!("my $r = /{newline}={directive} {pattern}/; sub real {{ }}{newline}");
                let table = LocalSymbolTable::scan_subs(&prefix);
                assert!(!table.is_known_sub("phantom"), "pattern text leaked: {prefix:?}");
                assert!(table.is_known_sub("real"), "same-line real declaration lost: {prefix:?}");

                let unknown = format!("{prefix}phantom /x/;");
                let slash_start = prefix.len() + "phantom ".len();
                let tokens = tokens_with_prepass(&unknown);
                for start in [slash_start, slash_start + 2] {
                    assert!(
                        tokens.iter().any(|token| {
                            token.start == start
                                && token.end == start + 1
                                && matches!(token.token_type, TokenType::Division)
                        }),
                        "expected division at {start}: {unknown:?}; {tokens:?}"
                    );
                }

                let known = format!("{prefix}real /x/;");
                let slash_start = prefix.len() + "real ".len();
                let tokens = tokens_with_prepass(&known);
                assert!(
                    tokens.iter().any(|token| {
                        token.start == slash_start
                            && token.end == slash_start + "/x/".len()
                            && matches!(token.token_type, TokenType::RegexMatch)
                    }),
                    "same-line real call lost its regex argument: {known:?}; {tokens:?}"
                );
            }
        }
    }
}

#[test]
fn ordinary_pod_still_hides_directive_and_body_sub_text() {
    for directive in DIRECTIVES {
        for newline in LINE_ENDINGS {
            let source = format!(
                "={directive} sub phantom{newline}sub prose {{ }}{newline}=cut{newline}sub real {{ }}{newline}"
            );
            let table = LocalSymbolTable::scan_subs(&source);
            assert!(!table.is_known_sub("phantom"), "directive text leaked: {source:?}");
            assert!(!table.is_known_sub("prose"), "POD body leaked: {source:?}");
            assert!(table.is_known_sub("real"), "POD did not close: {source:?}");
        }
    }
}

#[test]
fn guarded_directive_consumption_clears_the_one_line_guard() {
    for newline in LINE_ENDINGS {
        let source = format!(
            "my $r = /{newline}=pod sub phantom{newline}=pod{newline}sub second_phantom {{ }}{newline}=cut{newline}/;{newline}sub real {{ }}{newline}"
        );
        let table = LocalSymbolTable::scan_subs(&source);
        assert!(!table.is_known_sub("phantom"), "first directive leaked: {source:?}");
        assert!(!table.is_known_sub("second_phantom"), "guard survived its one line: {source:?}");
        assert!(table.is_known_sub("real"), "lost the real declaration: {source:?}");
    }
}
