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

/// Classify the first significant token so each boundary side has an exact,
/// comparable value (`op:=` operator, `num:1` number, `none` skipped input).
fn first_kind(source: &str) -> String {
    match significant(source).into_iter().next() {
        Some(token) => match token.token_type {
            TokenType::Identifier(text) => format!("id:{text}"),
            TokenType::Number(text) => format!("num:{text}"),
            TokenType::Keyword(text) => format!("kw:{text}"),
            TokenType::Operator(text) => format!("op:{text}"),
            other => format!("{other:?}"),
        },
        None => "none".to_string(),
    }
}

/// Exact-value boundary discriminator for the `opens_pod` predicate
/// (`src/lib.rs`, `if opens_pod {`): every `assert_eq!` observes the exact
/// first surviving token for one input on one side of a boundary, so flipping
/// the predicate or any boundary comparison flips an observed value.
///
/// Boundary sides (perl 5.42 tokenizer oracle where noted):
/// - `=` + ASCII letter takes the `opens_pod` branch and the block is skipped.
/// - `=` + digit/underscore, or `=` with no following byte, leaves the
///   operator in the stream.
/// - `=cut` followed by a word byte stays open; a non-word byte closes.
#[test]
fn skip_whitespace_and_comments_boundary_discriminator() {
    // opens_pod true: `=pod` at byte 0 takes the branch; the block runs to
    // the closing `=cut`.
    assert_eq!(first_kind("=pod\nx\n=cut\n1"), "num:1", "=pod must open POD and =cut close it");
    // opens_pod true via a stray `=cut` opener: the opener consumes its own
    // line, so it closes only at the next `=cut`.
    assert_eq!(
        first_kind("=cut\n=cut\n1"),
        "num:1",
        "a stray =cut opens POD and the next =cut closes it"
    );
    // opens_pod false: a digit after `=` misses `is_ascii_alphabetic`.
    assert_eq!(first_kind("\n=1;"), "op:=", "a digit-led line-initial = must stay the operator");
    // opens_pod false: `=` as the final byte — `.get(position + 1)` is None.
    assert_eq!(first_kind("="), "op:=", "a trailing = must stay the operator");
    // opens_pod false: underscore after `=` (oracle: `=_foo` is code).
    assert_eq!(first_kind("=_foo;"), "op:=", "an underscore-led = must stay the operator");
    // `=cut` closer word boundary: letter extension is a distinct command.
    assert_eq!(
        first_kind("=pod\nx\n=cutlery\n=cut\n1"),
        "num:1",
        "=cutlery must keep POD open until the word-bounded =cut"
    );
    // Digit extension is a distinct command (oracle: `=cut123` keeps POD open).
    assert_eq!(
        first_kind("=pod\nx\n=cut123\n=cut\n1"),
        "num:1",
        "=cut123 must keep POD open until the word-bounded =cut"
    );
    // Underscore extension is a distinct command (oracle: `=cut_foo` keeps POD open).
    assert_eq!(
        first_kind("=pod\nx\n=cut_foo\n=cut\n1"),
        "num:1",
        "=cut_foo must keep POD open until the word-bounded =cut"
    );
    // The first non-word byte after `cut` ends the command and closes the block.
    assert_eq!(first_kind("=pod\nx\n=cut.foo\n1"), "num:1", "=cut.foo must close POD");
}

/// Call-presence observer for the two ASCII classification calls in
/// `skip_whitespace_and_comments` — `byte.is_ascii_alphabetic()` on the
/// opener and `byte.is_ascii_alphanumeric() || *byte == b'_'` on the `=cut`
/// closer. Each input drives one call and asserts the exact observed
/// outcome, so deleting a call flips an asserted value.
#[test]
fn skip_whitespace_and_comments_call_presence_observer() {
    // Opener call reached with a letter: POD opens and runs to EOF.
    assert_eq!(
        first_kind("=p\n1"),
        "none",
        "a letter after = must reach is_ascii_alphabetic and open POD"
    );
    // Opener call reached with a digit: the classification is false.
    assert_eq!(
        first_kind("=9"),
        "op:=",
        "a digit after = must fail is_ascii_alphabetic and stay code"
    );
    // Opener call true, then the closer call reached with `\n` after `=cut`.
    assert_eq!(
        first_kind("=p\n=cut\n1"),
        "num:1",
        "=cut at a word boundary must reach is_ascii_alphanumeric and close POD"
    );
    // Closer call reached with a digit after `=cut`: still a word, stays open.
    assert_eq!(
        first_kind("=pod\nx\n=cut7\n=cut\n1"),
        "num:1",
        "=cut7 must reach is_ascii_alphanumeric and keep POD open"
    );
}

/// `opens_pod` (`lib.rs`) true side: a line-initial `=` followed by an ASCII
/// letter opens POD, so the call before the opener stays code and the text
/// after it never reaches the stream.
#[test]
fn opens_pod_true_side_hides_following_call() {
    let toks = significant("real(1);\n=cut\npod text here\n");
    assert!(
        toks.iter().any(|t| t.text.as_ref() == "real"),
        "the call before a stray =cut opener stays code: {toks:?}"
    );
    assert!(
        !toks.iter().any(|t| t.text.as_ref() == "here"),
        "text after the stray =cut opener stays POD: {toks:?}"
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
