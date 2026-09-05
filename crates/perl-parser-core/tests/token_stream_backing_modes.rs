#![expect(clippy::expect_used, reason = "bounded fixture assertions for #3021")]
//! Discriminating tests for `TokenStream` backing modes (issue #8128).
//!
//! The live backing must apply parser-directed contextual operations by
//! restoring real captured boundary checkpoints (preserving every
//! prefix-established lexer state), while the buffered backing must refuse
//! classification-level operations with a typed fallback requirement instead
//! of silently clearing its lookahead cache. Parser outputs over live and
//! faithfully buffered streams must agree, and an unappliable buffered request
//! must be observable as an advisory diagnostic, never silence.

use perl_lexer::LexerMode;
use perl_parser_core::token_stream::{
    ContextualFallbackReason, ContextualOpResult, ContextualTokenOp, Token, TokenKind, TokenStream,
};
use perl_parser_core::{ParseError, Parser};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Collect the non-EOF parser tokens of a live stream pass.
fn collect_live_tokens(source: &str) -> Vec<Token> {
    let mut stream = TokenStream::new(source);
    let mut tokens = Vec::new();
    loop {
        let token = stream.next().expect("live token");
        if token.kind() == TokenKind::Eof {
            break;
        }
        tokens.push(token);
    }
    tokens
}

/// Advance the stream until the head lookahead token has the requested kind.
fn advance_until_kind(stream: &mut TokenStream<'_>, target: TokenKind) {
    loop {
        let kind = stream.peek().expect("peek during scan").kind();
        if kind == target {
            return;
        }
        assert_ne!(kind, TokenKind::Eof, "scanned past EOF looking for {target:?}");
        stream.next().expect("consume during scan");
    }
}

// ---------------------------------------------------------------------------
// Live backing — real captured boundary reclassification
// ---------------------------------------------------------------------------

/// Reclassification must rewrite the head lookahead token from a real captured
/// boundary: the `/` after a word is classified as division, and restoring the
/// complete pre-token checkpoint in `ExpectTerm` context must re-derive the
/// exact same span as a regex delimiter.
#[test]
fn live_reclassify_rewrites_head_token_from_real_boundary() {
    // After a closing block brace the lexer stays in ExpectOperator and emits
    // a division Slash for the statement-start `/`; the parser directs
    // term-context reclassification for exactly this shape.
    let source = "{ 1 }\n/re/;";
    let regex_span_start = source.find("/re/").expect("regex present");

    let mut stream = TokenStream::new(source);
    advance_until_kind(&mut stream, TokenKind::Slash);

    let slash = stream.peek().expect("head slash");
    assert_eq!(slash.start(), regex_span_start, "head token must be the ambiguous slash");

    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::ReclassifyFromBoundary {
            expected_context: LexerMode::ExpectTerm
        }),
        ContextualOpResult::AppliedLive,
        "live reclassification must report application"
    );

    // The reclassified token absorbs the whole `/re/` form at the same span.
    let reclassified = stream.peek().expect("reclassified head");
    assert_eq!(reclassified.kind(), TokenKind::Regex, "/ must become a regex delimiter");
    assert_eq!(reclassified.start(), regex_span_start, "span start must be preserved exactly");
    assert_eq!(reclassified.end(), regex_span_start + "/re/".len(), "span end must cover /re/");
    assert_eq!(&*reclassified.text, "/re/");
}

/// The negative control for the old `LexerCheckpoint::at_position` defect: a
/// heredoc queued before the reclassification point is prefix-established
/// lexer state that must survive the boundary restore. With a synthesized
/// default checkpoint the queue was wiped and the heredoc body was lexed as
/// ordinary code inside the body region.
#[test]
fn live_reclassify_preserves_prefix_established_heredoc_state() {
    // The `/` follows a closing block brace, so the live lexer emits a
    // division Slash. The heredoc queued at `<<END` is prefix-established
    // lexer state that must survive the boundary restore.
    let source = "print <<END, do { 1 } /re/;\nbody line\nEND\nmy $after;\n";
    let body_start = source.find("body line").expect("body present");
    let body_end = body_start + "body line".len();
    let after_offset = source.find("my $after").expect("following code present");
    let heredoc_start_offset = source.find("<<END").expect("heredoc introducer present");

    let mut stream = TokenStream::new(source);
    let mut scanned = Vec::new();
    loop {
        let kind = stream.peek().expect("peek during scan").kind();
        if kind == TokenKind::Slash {
            break;
        }
        assert_ne!(kind, TokenKind::Eof, "scanned past EOF looking for the slash");
        scanned.push(stream.next().expect("consume during scan"));
    }

    let slash = stream.peek().expect("head slash");
    let slash_start = slash.start();

    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::ReclassifyFromBoundary {
            expected_context: LexerMode::ExpectTerm
        }),
        ContextualOpResult::AppliedLive
    );
    let reclassified = stream.peek().expect("reclassified");
    assert_eq!(reclassified.kind(), TokenKind::Regex);
    assert_eq!(reclassified.start(), slash_start);
    assert_eq!(&*reclassified.text, "/re/");

    let mut drained = scanned;
    loop {
        let token = stream.next().expect("drain token");
        let eof = token.kind() == TokenKind::Eof;
        drained.push(token);
        if eof {
            break;
        }
    }

    // The heredoc introducer itself was lexed before the reclassification.
    assert!(
        drained
            .iter()
            .any(|t| t.kind() == TokenKind::HeredocStart && t.start() == heredoc_start_offset),
        "heredoc start token must be present"
    );

    // No ordinary code token may appear inside the heredoc body region: that
    // would mean the pending-heredoc queue was destroyed by the restore.
    let intruders: Vec<String> = drained
        .iter()
        .filter(|t| {
            t.start() >= body_start && t.end() <= body_end && t.kind() != TokenKind::HeredocBody
        })
        .map(|t| format!("{:?}@{}..{}", t.kind(), t.start(), t.end()))
        .collect();
    assert!(
        intruders.is_empty(),
        "prefix lexer state was lost during reclassification; tokens lexed inside the \
         heredoc body: {intruders:?}"
    );

    // The statement following the heredoc terminator must survive intact at
    // its exact source offset.
    assert!(
        drained.iter().any(|t| t.kind() == TokenKind::My && t.start() == after_offset),
        "code after the heredoc must still be tokenized at its exact offset"
    );
}

/// Statement-boundary reset applies on the live backing and leaves the stream
/// fully consumable.
#[test]
fn live_statement_boundary_reset_applies() {
    let source = "my $x = 1;\nmy $y = 2;";
    let mut stream = TokenStream::new(source);
    advance_until_kind(&mut stream, TokenKind::Semicolon);
    stream.next().expect("consume statement terminator");

    // Prime lookahead across the boundary so the reset has a window to clear.
    let _ = stream.peek_second().expect("second lookahead");

    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::StatementBoundaryReset),
        ContextualOpResult::AppliedLive
    );

    let mut drained = Vec::new();
    while !stream.is_eof() {
        drained.push(stream.next().expect("drain after reset"));
        assert!(drained.len() <= 20, "statement reset must not stall the stream");
    }
    assert_eq!(
        drained.iter().map(Token::kind).collect::<Vec<_>>(),
        vec![
            TokenKind::My,
            TokenKind::Identifier,
            TokenKind::Assign,
            TokenKind::Number,
            TokenKind::Semicolon,
        ],
        "reset must re-derive every prefetched token instead of skipping it"
    );
    assert_eq!(drained[0].start(), source.find("my $y").expect("second statement"));
    assert_eq!(drained[0].text.as_ref(), "my");
}

/// Format-body entry applies on the live backing: the next token produced is
/// the format body consumed in `InFormatBody` context.
#[test]
fn live_enter_format_body_applies_and_reclassifies_next_token() {
    let source = "format STD =\n0 1 2\n.\n";
    let mut stream = TokenStream::new(source);
    // Consume `format`, `STD`, `=`.
    advance_until_kind(&mut stream, TokenKind::Assign);
    stream.next().expect("consume assign");
    assert_eq!(stream.peek().expect("prefetch format body").kind(), TokenKind::Number);

    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::EnterFormatBody),
        ContextualOpResult::AppliedLive
    );
    assert_eq!(
        stream.peek().expect("format body token").kind(),
        TokenKind::FormatBody,
        "the next token after format entry must be the format body"
    );
}

/// Reclassification with no lookahead in flight reports `not_required` rather
/// than pretending an application occurred.
#[test]
fn live_reclassify_without_lookahead_is_not_required() {
    let mut stream = TokenStream::new("my $x;");
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::ReclassifyFromBoundary {
            expected_context: LexerMode::ExpectTerm
        }),
        ContextualOpResult::NotRequired,
        "nothing is in flight to reclassify"
    );
}

/// Reclassification at a sticky EOF is `not_required`.
#[test]
fn live_reclassify_at_eof_is_not_required() {
    let mut stream = TokenStream::new("");
    assert_eq!(stream.peek().expect("eof peek").kind(), TokenKind::Eof);
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::ReclassifyFromBoundary {
            expected_context: LexerMode::ExpectTerm
        }),
        ContextualOpResult::NotRequired
    );
}

/// Reclassification into a body-consumption context is not a token
/// reclassification and must be reported as unsupported on both backings.
#[test]
fn reclassify_into_body_context_is_unsupported() {
    let mut live = TokenStream::new("my $x;");
    assert_eq!(
        live.apply_contextual(ContextualTokenOp::ReclassifyFromBoundary {
            expected_context: LexerMode::InFormatBody
        }),
        ContextualOpResult::Unsupported
    );

    let mut buffered = TokenStream::from_vec(vec![
        Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
    ]);
    assert_eq!(
        buffered.apply_contextual(ContextualTokenOp::ReclassifyFromBoundary {
            expected_context: LexerMode::InFormatBody
        }),
        ContextualOpResult::Unsupported
    );
}

/// Live lookahead invalidation applies when the cache holds tokens and is
/// `not_required` when it is already empty.
#[test]
fn live_invalidate_lookahead_reports_occurrence() {
    let mut stream = TokenStream::new("my $x;");
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::InvalidateLookahead),
        ContextualOpResult::NotRequired,
        "empty lookahead has nothing to invalidate"
    );

    let _ = stream.peek().expect("prime lookahead");
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::InvalidateLookahead),
        ContextualOpResult::AppliedLive
    );
}

// ---------------------------------------------------------------------------
// Buffered backing — replay-or-fallback contract
// ---------------------------------------------------------------------------

/// A source-less buffered stream refuses reclassification by naming the
/// missing authority, and its state is untouched by the refusal.
#[test]
fn buffered_reclassify_without_source_names_missing_source() {
    let tokens = vec![
        Token::new_checked(TokenKind::Identifier, "split", 0, 5).expect("valid token"),
        Token::new_checked(TokenKind::Slash, "/", 6, 7).expect("valid token"),
        Token::new_checked(TokenKind::Number, "1", 8, 9).expect("valid token"),
    ];
    let mut stream = TokenStream::from_vec(tokens);

    stream.next().expect("consume the leading identifier");
    assert_eq!(stream.peek().expect("head").kind(), TokenKind::Slash);
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::ReclassifyFromBoundary {
            expected_context: LexerMode::ExpectTerm
        }),
        ContextualOpResult::FallbackRequired { reason: ContextualFallbackReason::NoBufferedSource },
        "source-less buffered streams must expose their limitation explicitly"
    );
    assert_eq!(
        stream.peek().expect("head after refusal").kind(),
        TokenKind::Slash,
        "refusal must leave the stream state untouched"
    );
}

/// A source-backed buffered stream still refuses reclassification: source
/// identity alone is not replay authority — an exact complete checkpoint at
/// the boundary is required, and only a live pass captures it.
#[test]
fn buffered_reclassify_with_source_names_missing_checkpoint_authority() {
    let source = "split /x/, 1;";
    let tokens = vec![
        Token::new_checked(TokenKind::Identifier, "split", 0, 5).expect("valid token"),
        Token::new_checked(TokenKind::Slash, "/", 6, 7).expect("valid token"),
        Token::new_checked(TokenKind::Number, "1", 8, 9).expect("valid token"),
    ];
    let mut stream = TokenStream::from_vec_with_source(tokens, source);

    stream.next().expect("consume the leading identifier");
    assert_eq!(stream.peek().expect("head").kind(), TokenKind::Slash);
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::ReclassifyFromBoundary {
            expected_context: LexerMode::ExpectTerm
        }),
        ContextualOpResult::FallbackRequired {
            reason: ContextualFallbackReason::NoCheckpointAuthority
        },
        "source identity without a captured checkpoint must still refuse replay"
    );
}

/// A buffered stream cannot apply a statement-boundary reset: clearing the
/// peek cache while leaving classification fixed is not an accepted
/// contextual operation, so the request is refused wholesale.
#[test]
fn buffered_statement_boundary_reset_is_refused() {
    let tokens = vec![
        Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "x", 3, 4).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 4, 5).expect("valid token"),
    ];
    let mut stream = TokenStream::from_vec_with_source(tokens, "my x;");

    let _ = stream.peek().expect("head");
    let _ = stream.peek_second().expect("second");
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::StatementBoundaryReset),
        ContextualOpResult::FallbackRequired {
            reason: ContextualFallbackReason::NoCheckpointAuthority
        }
    );
    // The refusal is observable: the lookahead window survives.
    assert_eq!(stream.peek().expect("head").kind(), TokenKind::My);
    assert_eq!(stream.peek_second().expect("second").kind(), TokenKind::Identifier);
}

/// A buffered stream refuses format-body entry when the buffered head token
/// is not already a format body: it cannot re-classify the following token.
#[test]
fn buffered_enter_format_body_is_refused() {
    let tokens = vec![Token::new_checked(TokenKind::Assign, "=", 10, 11).expect("valid token")];
    let mut stream = TokenStream::from_vec(tokens);

    let _ = stream.peek().expect("head");
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::EnterFormatBody),
        ContextualOpResult::FallbackRequired { reason: ContextualFallbackReason::NoBufferedSource }
    );
}

/// When the buffered head token is already a format body, the requested
/// context verifiably holds and format entry is `not_required` on the
/// buffered backing.
#[test]
fn buffered_enter_format_body_already_satisfied_is_not_required() {
    let tokens =
        vec![Token::new_checked(TokenKind::FormatBody, "0 1 2\n", 13, 19).expect("valid token")];
    let mut stream = TokenStream::from_vec_with_source(tokens, "format STD =\n0 1 2\n.\n");

    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::EnterFormatBody),
        ContextualOpResult::NotRequired,
        "the buffered head already is the format body"
    );
    assert_eq!(stream.peek().expect("head").kind(), TokenKind::FormatBody);
}

/// Lookahead invalidation cannot change classification, so it applies on the
/// buffered backing and the cursor resumes at the first un-fetched token.
#[test]
fn buffered_invalidate_lookahead_applies() {
    let tokens = vec![
        Token::new_checked(TokenKind::My, "my", 0, 2).expect("valid token"),
        Token::new_checked(TokenKind::Identifier, "x", 3, 4).expect("valid token"),
        Token::new_checked(TokenKind::Semicolon, ";", 4, 5).expect("valid token"),
    ];
    let mut stream = TokenStream::from_vec(tokens);

    let _ = stream.peek().expect("head");
    let _ = stream.peek_second().expect("second");
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::InvalidateLookahead),
        ContextualOpResult::AppliedReplay,
        "classification-preserving invalidation applies on the buffered backing"
    );
    assert_eq!(
        stream.peek().expect("refetched head").kind(),
        TokenKind::Semicolon,
        "invalidation drops the fetched window; the cursor resumes at the first un-fetched token"
    );
}

/// Backing introspection exposes the mode and source retention so the
/// incremental layer can decide whether a live rebuild is available.
#[test]
fn backing_reports_mode_and_source_retention() {
    use perl_parser_core::token_stream::TokenStreamBacking;

    assert_eq!(TokenStream::new("my $x;").backing(), TokenStreamBacking::Live);
    assert_eq!(
        TokenStream::from_vec(vec![]).backing(),
        TokenStreamBacking::Buffered { source_retained: false }
    );
    assert_eq!(
        TokenStream::from_vec_with_source(vec![], "my $x;").backing(),
        TokenStreamBacking::Buffered { source_retained: true }
    );
}

// ---------------------------------------------------------------------------
// Parser-level backing-mode agreement
// ---------------------------------------------------------------------------

/// Collect tokens the way a parser-directed live pass classifies them for the
/// format fixture: the lexer only produces the `FormatBody` classification
/// after parser-directed format entry (issue #8128).
fn collect_parser_directed_format_tokens(source: &str) -> Vec<Token> {
    use perl_lexer::{PerlLexer, TokenType};

    let mut lexer = PerlLexer::new(source);
    let mut raw = Vec::new();
    let mut format_entered = false;
    while let Some(token) = lexer.next_token() {
        if matches!(token.token_type, TokenType::EOF) {
            break;
        }
        let is_format_assign = !format_entered
            && matches!(token.token_type, TokenType::Operator(_))
            && token.text.as_ref() == "=";
        if !matches!(token.token_type, TokenType::Whitespace | TokenType::Newline)
            && !matches!(token.token_type, TokenType::Comment(_))
        {
            raw.push(token);
        }
        if is_format_assign {
            lexer.enter_format_mode();
            format_entered = true;
        }
    }
    TokenStream::lexer_tokens_to_parser_tokens(raw)
}

/// Parser outputs over a live stream and a faithfully buffered stream must
/// agree for the admitted ambiguity matrix: division vs regex after builtins,
/// formats, heredocs, and hash keys that resemble quote operators.
#[test]
fn live_and_faithfully_buffered_parser_outputs_agree() {
    let sources = [
        "my @r = split /x/, 1;",
        "my $q = { q => 1 };\nmy $s = q{not a hash key};\n",
        "print <<END, 1;\nbody line\nEND\nmy $after;\n",
    ];

    for source in sources {
        let live_ast = Parser::new(source).parse().expect("live parse must succeed");

        let buffered_ast = {
            let tokens = collect_live_tokens(source);
            Parser::from_tokens(tokens, source).parse().expect("buffered parse must succeed")
        };

        assert_eq!(
            live_ast.to_sexp(),
            buffered_ast.to_sexp(),
            "live and faithfully buffered parser outputs must agree for {source:?}"
        );
    }

    // The format fixture needs the parser-directed classification pass: the
    // format body token only exists after format entry.
    let format_source = "format STD =\n0 1 2\n.\n";
    let live_ast = Parser::new(format_source).parse().expect("live format parse");
    let buffered_ast =
        Parser::from_tokens(collect_parser_directed_format_tokens(format_source), format_source)
            .parse()
            .expect("buffered format parse");
    assert_eq!(
        live_ast.to_sexp(),
        buffered_ast.to_sexp(),
        "live and faithfully buffered parser outputs must agree for the format fixture"
    );
}

/// A misaligned buffered cache that cannot honor a reclassification request
/// must surface the refusal as an advisory diagnostic — never as silence and
/// never as a synthetic success.
#[test]
fn misaligned_buffered_reclassify_records_advisory_not_silence() {
    let source = "my @r = split /x/, 1;";
    let mut tokens = collect_live_tokens(source);

    // Corrupt the cache into a misaligned one: the regex delimiter is
    // presented as a division slash at the exact span where the parser will
    // request ExpectTerm reclassification.
    let mut corrupted = false;
    for token in tokens.iter_mut() {
        if token.kind() == TokenKind::Regex && token.text.starts_with('/') {
            *token = Token::new_checked(
                TokenKind::Slash,
                token.text.clone(),
                token.start(),
                token.end(),
            )
            .expect("valid replacement token");
            corrupted = true;
        }
    }
    assert!(corrupted, "fixture must contain the regex token to corrupt");

    let output = Parser::from_tokens(tokens, source).parse_with_recovery();

    assert!(
        output.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            ParseError::Advisory { message, .. }
                if message.contains("reclassify_from_boundary")
                    && message.contains("live lexer")
        )),
        "the refused contextual operation must be observable as an advisory; got {:?}",
        output.diagnostics
    );
}
