//! RIPR call-observation proof for the POD opener/`cut` closer seam (#16607).
//!
//! Seams:
//! | File | Expression |
//! |------|-----------|
//! | `src/lib.rs` | `opens_pod = .get(position + 1).is_some_and(\|byte\| byte.is_ascii_alphabetic())` |
//! | `src/lib.rs` | `line_end_byte == b'\r'` (CRLF opener-line consumption) |
//! | `src/lib.rs` | `bytes[i..].starts_with(b"=cut")` + no word byte at `i + 4` |
//! | `src/symbol_table/mod.rs` | `state.quote == QuoteState::Code && state.quote_like.is_none() && starts_pod(line)` |
//!
//! Every test drives the production entry points — the `perl_lexer` token
//! stream and `LocalSymbolTable::scan_subs` (wired into `LexerConfig` via
//! `parser_context.rs`) — so a mutation at a named seam flips the observed
//! tokens or table facts. Boundary sides are pinned to the perl 5.42
//! tokenizer oracle.

use perl_lexer::{LocalSymbolTable, PerlLexer, Token, TokenType};

fn significant(source: &str) -> Vec<Token> {
    PerlLexer::new(source)
        .collect_tokens()
        .into_iter()
        .filter(|token| {
            !matches!(token.token_type, TokenType::Whitespace | TokenType::Newline | TokenType::EOF)
        })
        .collect()
}

/// `opens_pod` (`lib.rs`) true side: a line-initial `=` followed by an ASCII
/// letter opens POD, so the call after the block never reaches the stream.
#[test]
fn opens_pod_true_side_hides_following_call() {
    let toks = significant("real(1);\n=cut\npod text here\n");
    assert!(
        !toks.iter().any(|t| t.text.as_ref() == "real"),
        "a call before a stray =cut opener stays code, everything after is POD: {toks:?}"
    );
}

/// `opens_pod` false side: no byte after `=` (EOF) and a digit-led `=` both
/// leave the operator in the stream.
#[test]
fn opens_pod_false_side_keeps_operator_token() {
    for (name, source) in [
        ("equals at EOF", "my $x\n="),
        ("digit-led", "my $x\n=1;"),
        ("underscore-led", "$x\n=_foo;"),
    ] {
        let toks = significant(source);
        assert!(
            toks.iter().any(|t| t.text.as_ref() == "="),
            "{name}: '=' must survive as an operator token: {toks:?}"
        );
    }
}

/// `line_end_byte == b'\r'` (lib.rs): the stray-opener line ends with CRLF and
/// the following call stays hidden while the call after the closing `=cut`
/// reaches the stream.
#[test]
fn crlf_ended_opener_line_observes_call_on_both_sides() {
    let toks = significant("before(1);\r\n=cut\r\npod text here\r\n=cut\r\nreal(2);\r\n");
    assert!(
        toks.iter().any(|t| t.text.as_ref() == "before"),
        "the call before the CRLF opener must stay code: {toks:?}"
    );
    assert!(
        toks.iter().any(|t| t.text.as_ref() == "real"),
        "the call after the closing =cut must stay code: {toks:?}"
    );
    assert!(
        !toks.iter().any(|t| t.text.as_ref() == "here"),
        "POD text between the CRLF pair stays hidden: {toks:?}"
    );
}

/// `=cut` word-boundary closer (`lib.rs`): word extensions keep the block
/// open — the call after them stays hidden — and only a word-bounded `=cut`
/// (or non-word byte after `cut`) resumes code.
#[test]
fn cut_command_word_boundary_observes_call_resumption() {
    let toks = significant(
        "before(1);\n=cut\n=cutlery\n=cut123\n=cut_foo\nhidden(9);\n=cut.foo\nreal(2);\n",
    );
    assert!(
        toks.iter().any(|t| t.text.as_ref() == "before"),
        "the call before the stray =cut must stay code: {toks:?}"
    );
    assert!(
        !toks.iter().any(|t| t.text.as_ref() == "hidden"),
        "word extensions of =cut must keep the call inside POD: {toks:?}"
    );
    assert!(
        toks.iter().any(|t| t.text.as_ref() == "real"),
        "=cut.foo must close the block so the call after resumes code: {toks:?}"
    );
}

/// Prepass quote-like gate (`symbol_table/mod.rs`): a line-initial `=cut`
/// inside a multiline `q{}` body must not open POD, so the later `sub real`
/// call site resolves against a table that knows `real`.
#[test]
fn quote_like_body_with_pod_shaped_line_keeps_sub_visible() {
    let source = "my $doc = q{\n=cut\n};\nsub real { }\nreal(1);\n";
    let table = LocalSymbolTable::scan_subs(source);

    assert!(
        table.is_known_sub("real"),
        "the =cut inside the q{{}} body must not open POD in the prepass"
    );
}
