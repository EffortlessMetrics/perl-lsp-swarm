#![expect(clippy::expect_used, reason = "checked literal fixtures and live backing assertions")]
//! Private library proof for TokenStream checkpoint bookkeeping and refusal (#17846).
//!
//! Token kinds, text and byte spans below are literal oracles, not lexer-derived
//! expectations. Whole-checkpoint comparisons supplement those oracles. Missing
//! and foreign checkpoints are fault-injected branch guards, not user-facing
//! reproductions. This does not measure capture performance or prove a parser
//! fallback rebuild; the parser currently reports an advisory and continues.

use super::*;
use perl_lexer::CheckpointRestoreError;

const SOURCE: &str = "  #é\n11 + 22 * 33;";
const TOKENS: [(TokenKind, &str, usize, usize); 7] = [
    (TokenKind::Number, "11", 6, 8),
    (TokenKind::Plus, "+", 9, 10),
    (TokenKind::Number, "22", 11, 13),
    (TokenKind::Star, "*", 14, 15),
    (TokenKind::Number, "33", 16, 18),
    (TokenKind::Semicolon, ";", 18, 19),
    (TokenKind::Eof, "", 19, 19),
];

fn literal_tokens() -> Vec<Token> {
    TOKENS
        .iter()
        .map(|&(kind, text, start, end)| {
            Token::new_checked(kind, text, start, end).expect("ordered literal byte-span fixture")
        })
        .collect()
}

fn assert_token(token: &Token, expected: (TokenKind, &str, usize, usize)) {
    assert_eq!((token.kind(), &*token.text, token.start(), token.end()), expected);
}

/// The fixed oracle length bounds consumption; one extra read proves sticky EOF.
fn assert_drain(
    stream: &mut TokenStream<'_>,
    expected: &[(TokenKind, &str, usize, usize)],
) -> ParseResult<()> {
    for &token in expected {
        assert_token(&stream.next()?, token);
    }
    assert_token(&stream.next()?, *expected.last().expect("oracle ends in EOF"));
    Ok(())
}

fn cached_tokens(stream: &TokenStream<'_>) -> [Option<Token>; 3] {
    [stream.peeked.clone(), stream.peeked_second.clone(), stream.peeked_third.clone()]
}

fn cached_boundaries(stream: &TokenStream<'_>) -> [Option<LexerCheckpoint>; 3] {
    [
        stream.peek_boundary.clone(),
        stream.peek_second_boundary.clone(),
        stream.peek_third_boundary.clone(),
    ]
}

fn assert_clear(stream: &TokenStream<'_>) {
    assert_eq!(cached_tokens(stream), [None, None, None]);
    assert_eq!(cached_boundaries(stream), [None, None, None]);
}

fn checkpoint(stream: &TokenStream<'_>) -> LexerCheckpoint {
    let lexer = match &stream.inner {
        TokenStreamInner::Lexer(lexer) => Some(lexer),
        TokenStreamInner::Buffered(_) => None,
    };
    lexer.expect("live fixture backing").checkpoint()
}

fn reclassify(mode: LexerMode) -> ContextualTokenOp {
    ContextualTokenOp::ReclassifyFromBoundary { expected_context: mode }
}

#[test]
fn captures_before_trivia_and_shifts_every_slot() -> ParseResult<()> {
    let mut stream = TokenStream::new(SOURCE);
    assert_token(stream.peek()?, TOKENS[0]);
    assert_eq!(stream.peek_boundary.as_ref().map(LexerCheckpoint::position), Some(0));
    assert!(stream.last_boundary.is_none(), "peek takes the produced boundary");
    assert_token(stream.peek_second()?, TOKENS[1]);
    assert_eq!(stream.peek_second_boundary.as_ref().map(LexerCheckpoint::position), Some(8));
    assert!(stream.last_boundary.is_none());
    assert_token(stream.peek_third()?, TOKENS[2]);
    assert_eq!(
        cached_boundaries(&stream).map(|boundary| boundary.map(|b| b.position())),
        [Some(0), Some(8), Some(10)]
    );
    assert!(stream.last_boundary.is_none());
    let tokens = cached_tokens(&stream);
    let boundaries = cached_boundaries(&stream);
    let lexer = checkpoint(&stream);
    assert_eq!(lexer.position(), 13);
    for _ in 0..3 {
        assert_token(stream.peek()?, TOKENS[0]);
        assert_token(stream.peek_second()?, TOKENS[1]);
        assert_token(stream.peek_third()?, TOKENS[2]);
        assert_eq!(cached_tokens(&stream), tokens);
        assert_eq!(cached_boundaries(&stream), boundaries);
        assert_eq!(checkpoint(&stream), lexer, "repeat peek must not advance the lexer");
    }

    assert_token(&stream.next()?, TOKENS[0]);
    assert_eq!(cached_tokens(&stream), [tokens[1].clone(), tokens[2].clone(), None]);
    assert_eq!(cached_boundaries(&stream), [boundaries[1].clone(), boundaries[2].clone(), None]);
    assert_token(stream.peek_third()?, TOKENS[3]);
    assert_eq!(stream.peek_third_boundary.as_ref().map(LexerCheckpoint::position), Some(13));
    assert!(stream.last_boundary.is_none());
    let tokens = cached_tokens(&stream);
    let boundaries = cached_boundaries(&stream);
    assert_token(&stream.next()?, TOKENS[1]);
    assert_eq!(cached_tokens(&stream), [tokens[1].clone(), tokens[2].clone(), None]);
    assert_eq!(cached_boundaries(&stream), [boundaries[1].clone(), boundaries[2].clone(), None]);
    assert_token(stream.peek_third()?, TOKENS[4]);
    assert_eq!(stream.peek_third_boundary.as_ref().map(LexerCheckpoint::position), Some(15));
    assert!(stream.last_boundary.is_none());
    assert_drain(&mut stream, &TOKENS[2..])
}

#[test]
fn capture_flag_and_fresh_next_clear_last_boundary() -> ParseResult<()> {
    let mut stream = TokenStream::new(SOURCE);
    assert_token(&stream.next_token(false)?, TOKENS[0]);
    assert!(stream.last_boundary.is_none(), "false must not capture");
    assert_token(&stream.next_token(true)?, TOKENS[1]);
    assert_eq!(stream.last_boundary.as_ref().map(LexerCheckpoint::position), Some(8));
    assert_clear(&stream);
    assert_token(&stream.next_token(false)?, TOKENS[2]);
    assert!(stream.last_boundary.is_none(), "false replaces earlier capture");
    assert_drain(&mut stream, &TOKENS[3..])?;

    let mut stream = TokenStream::new(SOURCE);
    assert_token(&stream.next_token(true)?, TOKENS[0]);
    assert!(stream.last_boundary.is_some(), "seed a real stale capture");
    assert_token(&stream.next()?, TOKENS[1]);
    assert!(stream.last_boundary.is_none(), "public fresh next clears bookkeeping");
    assert_clear(&stream);
    assert_token(stream.peek()?, TOKENS[2]);
    assert_eq!(stream.peek_boundary.as_ref().map(LexerCheckpoint::position), Some(10));
    assert!(stream.last_boundary.is_none());
    assert_drain(&mut stream, &TOKENS[2..])
}

#[test]
fn invalidation_clears_six_slots_without_rewinding() -> ParseResult<()> {
    for backing in 0..3 {
        let mut stream = match backing {
            0 => TokenStream::new(SOURCE),
            1 => TokenStream::from_vec(literal_tokens()),
            _ => TokenStream::from_vec_with_source(literal_tokens(), SOURCE),
        };
        assert_token(stream.peek_third()?, TOKENS[2]);
        let lexer = (backing == 0).then(|| checkpoint(&stream));
        if backing == 0 {
            assert!(cached_boundaries(&stream).iter().all(Option::is_some));
        }
        assert_eq!(
            stream.apply_contextual(ContextualTokenOp::InvalidateLookahead),
            if backing == 0 {
                ContextualOpResult::AppliedLive
            } else {
                ContextualOpResult::AppliedReplay
            }
        );
        assert_clear(&stream);
        assert!(stream.last_boundary.is_none());
        if let Some(lexer) = lexer {
            assert_eq!(checkpoint(&stream), lexer, "invalidation does not rewind");
        }
        assert_eq!(
            stream.apply_contextual(ContextualTokenOp::InvalidateLookahead),
            ContextualOpResult::NotRequired
        );
        assert_clear(&stream);
        assert_drain(&mut stream, &TOKENS[3..])?;
    }
    Ok(())
}

#[test]
fn reclassification_restores_the_shifted_slash_boundary() -> ParseResult<()> {
    let mut stream = TokenStream::new("{ 1 }\n/re/;");
    assert_token(&stream.next()?, (TokenKind::LeftBrace, "{", 0, 1));
    assert_token(&stream.next()?, (TokenKind::Number, "1", 2, 3));
    assert_token(stream.peek()?, (TokenKind::RightBrace, "}", 4, 5));
    assert_token(stream.peek_second()?, (TokenKind::Slash, "/", 6, 7));
    assert_token(stream.peek_third()?, (TokenKind::Identifier, "re", 7, 9));
    let slash_boundary = stream.peek_second_boundary.clone();
    assert_eq!(slash_boundary.as_ref().map(LexerCheckpoint::position), Some(5));
    assert_token(&stream.next()?, (TokenKind::RightBrace, "}", 4, 5));
    assert_eq!(stream.peek_boundary, slash_boundary);
    assert_eq!(
        stream.apply_contextual(reclassify(LexerMode::ExpectTerm)),
        ContextualOpResult::AppliedLive
    );
    assert_clear(&stream);
    assert!(stream.last_boundary.is_none());
    assert_eq!(checkpoint(&stream).position(), 5, "rewind before newline trivia");
    assert_drain(
        &mut stream,
        &[
            (TokenKind::Regex, "/re/", 6, 10),
            (TokenKind::Semicolon, ";", 10, 11),
            (TokenKind::Eof, "", 11, 11),
        ],
    )
}

#[test]
fn missing_reclassification_boundary_refuses_without_mutation() -> ParseResult<()> {
    let mut stream = TokenStream::new(SOURCE);
    assert_token(stream.peek_third()?, TOKENS[2]);
    // Only reclassification requires an existing boundary. The reset/format
    // helper deliberately accepts absence; do not assert a different contract.
    stream.peek_boundary = None;
    let tokens = cached_tokens(&stream);
    let boundaries = cached_boundaries(&stream);
    let last = stream.last_boundary.clone();
    let lexer = checkpoint(&stream);
    assert_eq!(
        stream.apply_contextual(reclassify(LexerMode::ExpectTerm)),
        ContextualOpResult::FallbackRequired {
            reason: ContextualFallbackReason::NoCheckpointAuthority
        }
    );
    assert_eq!(cached_tokens(&stream), tokens);
    assert_eq!(cached_boundaries(&stream), boundaries);
    assert_eq!(stream.last_boundary, last);
    assert_eq!(checkpoint(&stream), lexer);
    assert_drain(&mut stream, &TOKENS)
}

#[test]
fn foreign_checkpoint_refuses_each_live_classification_operation() -> ParseResult<()> {
    let foreign = LexerCheckpoint::origin("99 + 88 * 77;");
    let mut lexer = PerlLexer::new(SOURCE);
    let before = lexer.checkpoint();
    assert_eq!(lexer.restore(&foreign), Err(CheckpointRestoreError::WrongContent));
    assert_eq!(lexer.checkpoint(), before, "restore itself is fail-closed");

    for operation in [
        ContextualTokenOp::StatementBoundaryReset,
        reclassify(LexerMode::ExpectTerm),
        ContextualTokenOp::EnterFormatBody,
    ] {
        let mut stream = TokenStream::new(SOURCE);
        assert_token(stream.peek_third()?, TOKENS[2]);
        stream.peek_boundary = Some(foreign.clone());
        let tokens = cached_tokens(&stream);
        let boundaries = cached_boundaries(&stream);
        let last = stream.last_boundary.clone();
        let lexer = checkpoint(&stream);
        assert_eq!(
            stream.apply_contextual(operation),
            ContextualOpResult::FallbackRequired {
                reason: ContextualFallbackReason::NoCheckpointAuthority
            },
            "{operation:?} must refuse foreign authority"
        );
        assert_eq!(cached_tokens(&stream), tokens);
        assert_eq!(cached_boundaries(&stream), boundaries);
        assert_eq!(stream.last_boundary, last);
        assert_eq!(checkpoint(&stream), lexer);
        assert_drain(&mut stream, &TOKENS)?;
    }
    Ok(())
}

#[test]
fn buffered_classification_refusal_preserves_window_and_suffix() -> ParseResult<()> {
    for source_retained in [false, true] {
        for operation in [
            ContextualTokenOp::StatementBoundaryReset,
            reclassify(LexerMode::ExpectTerm),
            ContextualTokenOp::EnterFormatBody,
        ] {
            let mut stream = if source_retained {
                TokenStream::from_vec_with_source(literal_tokens(), SOURCE)
            } else {
                TokenStream::from_vec(literal_tokens())
            };
            assert_token(stream.peek_third()?, TOKENS[2]);
            let tokens = cached_tokens(&stream);
            let boundaries = cached_boundaries(&stream);
            let last = stream.last_boundary.clone();
            let eof_pos = stream.buffered_eof_pos;
            let suffix = match &stream.inner {
                TokenStreamInner::Buffered(buffer) => Some(buffer.tokens.clone()),
                TokenStreamInner::Lexer(_) => None,
            };
            assert!(suffix.is_some());
            assert_eq!(boundaries, [None, None, None]);
            assert_eq!(
                stream.apply_contextual(operation),
                ContextualOpResult::FallbackRequired {
                    reason: if source_retained {
                        ContextualFallbackReason::NoCheckpointAuthority
                    } else {
                        ContextualFallbackReason::NoBufferedSource
                    }
                }
            );
            assert_eq!(cached_tokens(&stream), tokens);
            assert_eq!(cached_boundaries(&stream), boundaries);
            assert_eq!(stream.last_boundary, last);
            assert_eq!(stream.buffered_eof_pos, eof_pos);
            if let TokenStreamInner::Buffered(buffer) = &stream.inner {
                assert_eq!(Some(buffer.tokens.clone()), suffix);
                assert_eq!(buffer.source, source_retained.then_some(SOURCE));
            }
            assert_drain(&mut stream, &TOKENS)?;
        }
    }
    Ok(())
}

#[test]
fn unsupported_and_no_target_reclassification_are_non_mutating() -> ParseResult<()> {
    for backing in 0..3 {
        let mut stream = match backing {
            0 => TokenStream::new(SOURCE),
            1 => TokenStream::from_vec(literal_tokens()),
            _ => TokenStream::from_vec_with_source(literal_tokens(), SOURCE),
        };
        let lexer = (backing == 0).then(|| checkpoint(&stream));
        assert_eq!(
            stream.apply_contextual(reclassify(LexerMode::ExpectTerm)),
            ContextualOpResult::NotRequired,
            "supported request with no head target"
        );
        assert_clear(&stream);
        if let Some(lexer) = lexer {
            assert_eq!(checkpoint(&stream), lexer);
        }
        assert_token(stream.peek_third()?, TOKENS[2]);
        let tokens = cached_tokens(&stream);
        let boundaries = cached_boundaries(&stream);
        let last = stream.last_boundary.clone();
        let lexer = (backing == 0).then(|| checkpoint(&stream));
        assert_eq!(
            stream.apply_contextual(reclassify(LexerMode::InFormatBody)),
            ContextualOpResult::Unsupported
        );
        assert_eq!(cached_tokens(&stream), tokens);
        assert_eq!(cached_boundaries(&stream), boundaries);
        assert_eq!(stream.last_boundary, last);
        if let Some(lexer) = lexer {
            assert_eq!(checkpoint(&stream), lexer);
        }
        assert_drain(&mut stream, &TOKENS)?;
        for mode in [LexerMode::ExpectTerm, LexerMode::ExpectOperator, LexerMode::ExpectDelimiter] {
            let tokens = cached_tokens(&stream);
            let boundaries = cached_boundaries(&stream);
            let lexer = (backing == 0).then(|| checkpoint(&stream));
            assert_eq!(
                stream.apply_contextual(reclassify(mode)),
                ContextualOpResult::NotRequired,
                "sticky EOF has no reclassification target"
            );
            assert_eq!(cached_tokens(&stream), tokens);
            assert_eq!(cached_boundaries(&stream), boundaries);
            if let Some(lexer) = lexer {
                assert_eq!(checkpoint(&stream), lexer);
            }
            assert_token(&stream.next()?, TOKENS[6]);
        }
    }
    Ok(())
}

#[test]
fn valid_statement_reset_restores_and_reclassifies_full_window() -> ParseResult<()> {
    let mut stream = TokenStream::new("{ 1 }\n/re/;");
    assert_token(&stream.next()?, (TokenKind::LeftBrace, "{", 0, 1));
    assert_token(&stream.next()?, (TokenKind::Number, "1", 2, 3));
    assert_token(&stream.next()?, (TokenKind::RightBrace, "}", 4, 5));
    assert_token(stream.peek()?, (TokenKind::Slash, "/", 6, 7));
    assert_token(stream.peek_second()?, (TokenKind::Identifier, "re", 7, 9));
    assert_token(stream.peek_third()?, (TokenKind::Slash, "/", 9, 10));
    assert!(cached_tokens(&stream).iter().all(Option::is_some));
    assert_eq!(
        cached_boundaries(&stream).map(|boundary| boundary.map(|b| b.position())),
        [Some(5), Some(7), Some(9)]
    );
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::StatementBoundaryReset),
        ContextualOpResult::AppliedLive
    );
    assert_clear(&stream);
    assert!(stream.last_boundary.is_none());
    assert_eq!(checkpoint(&stream).position(), 5, "restore before newline trivia");
    assert_drain(
        &mut stream,
        &[
            (TokenKind::Regex, "/re/", 6, 10),
            (TokenKind::Semicolon, ";", 10, 11),
            (TokenKind::Eof, "", 11, 11),
        ],
    )
}

#[test]
fn valid_format_entry_restores_and_reclassifies_full_window() -> ParseResult<()> {
    let source = "format NAME =\né\n.\n42;";
    assert_eq!(&source[13..17], "\né\n");
    assert_eq!(&source[17..19], ".\n");
    assert_eq!(source.len(), 22);
    let mut stream = TokenStream::new(source);
    assert_token(&stream.next()?, (TokenKind::Format, "format", 0, 6));
    assert_token(&stream.next()?, (TokenKind::Identifier, "NAME", 7, 11));
    assert_token(&stream.next()?, (TokenKind::Assign, "=", 12, 13));
    assert_token(stream.peek()?, (TokenKind::Identifier, "é", 14, 16));
    assert_token(stream.peek_second()?, (TokenKind::Dot, ".", 17, 18));
    assert_token(stream.peek_third()?, (TokenKind::Number, "42", 19, 21));
    assert!(cached_tokens(&stream).iter().all(Option::is_some));
    assert_eq!(
        cached_boundaries(&stream).map(|boundary| boundary.map(|b| b.position())),
        [Some(13), Some(16), Some(18)]
    );
    assert_eq!(
        stream.apply_contextual(ContextualTokenOp::EnterFormatBody),
        ContextualOpResult::AppliedLive
    );
    assert_clear(&stream);
    assert!(stream.last_boundary.is_none());
    assert_eq!(checkpoint(&stream).position(), 13, "restore before body-leading newline");
    // The lexer consumes through byte 19, including the terminator. Parser
    // Token geometry is payload-aligned: the four-byte "\né\n" spans 13..17.
    assert_drain(
        &mut stream,
        &[
            (TokenKind::FormatBody, "\né\n", 13, 17),
            (TokenKind::Number, "42", 19, 21),
            (TokenKind::Semicolon, ";", 21, 22),
            (TokenKind::Eof, "", 22, 22),
        ],
    )
}
