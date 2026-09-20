#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use criterion::{Criterion, criterion_group, criterion_main};
#[path = "support/perf_scorecard.rs"]
mod perf_scorecard;
use perl_lexer::{Token as LexerToken, TokenType};
use perl_parser_core::tokens::token_stream::TokenStream;
use perl_token::{Token, TokenKind, TokenRef};
use std::hint::black_box;

const SHORT_TOKEN_TEXT: &str = "my";
const LONG_TOKEN_TEXT: &str =
    "ThisIsALongerTokenPayloadToMeasureArcAllocationAndCopyCostsAcrossParserBoundaries";
const START: usize = 12;
const END: usize = 18;

fn run_scorecard() {
    perf_scorecard::record_metric(
        "token_new_short",
        perf_scorecard::sample_metric(5000, || {
            let token = Token::new_checked(
                TokenKind::Identifier,
                SHORT_TOKEN_TEXT,
                0,
                SHORT_TOKEN_TEXT.len(),
            )
            .expect("valid token");
            black_box(token);
        }),
    );

    perf_scorecard::record_metric(
        "token_new_long",
        perf_scorecard::sample_metric(5000, || {
            let token = Token::new_checked(
                TokenKind::Identifier,
                LONG_TOKEN_TEXT,
                0,
                LONG_TOKEN_TEXT.len(),
            )
            .expect("valid token");
            black_box(token);
        }),
    );

    let base_token =
        Token::new_checked(TokenKind::Identifier, LONG_TOKEN_TEXT, 4, 4 + LONG_TOKEN_TEXT.len())
            .expect("valid token");
    perf_scorecard::record_metric(
        "token_clone",
        perf_scorecard::sample_metric(5000, || {
            let token = base_token.clone();
            black_box(token);
        }),
    );

    let lhs = Token::new_checked(TokenKind::Identifier, "same", 10, 14).expect("valid token");
    let rhs = Token::new_checked(TokenKind::Identifier, "same", 10, 14).expect("valid token");
    perf_scorecard::record_metric(
        "token_equality",
        perf_scorecard::sample_metric(5000, || {
            black_box(lhs == rhs);
        }),
    );

    perf_scorecard::record_metric(
        "token_kind_display_name",
        perf_scorecard::sample_metric(5000, || {
            black_box(TokenKind::LeftBrace.display_name());
            black_box(TokenKind::Identifier.display_name());
            black_box(TokenKind::Eof.display_name());
        }),
    );

    perf_scorecard::record_metric(
        "token_kind_category_predicates",
        perf_scorecard::sample_metric(5000, || {
            black_box(TokenKind::If.is_keyword());
            black_box(TokenKind::Plus.is_operator());
            black_box(TokenKind::String.is_literal());
        }),
    );

    let lexer_tokens = vec![
        LexerToken::new(TokenType::Keyword("my".into()), "my", 0, 2),
        LexerToken::new(TokenType::Identifier("$x".into()), "$x", 3, 5),
        LexerToken::new(TokenType::Operator("=".into()), "=", 6, 7),
        LexerToken::new(TokenType::Number("42".into()), "42", 8, 10),
        LexerToken::new(TokenType::Semicolon, ";", 10, 11),
    ];
    perf_scorecard::record_metric(
        "lexer_to_parser_token_conversion",
        perf_scorecard::sample_metric(3000, || {
            let parser_tokens = TokenStream::lexer_tokens_to_parser_tokens(lexer_tokens.clone());
            black_box(parser_tokens);
        }),
    );

    perf_scorecard::record_metric(
        "eof_synthetic_token_construction",
        perf_scorecard::sample_metric(5000, || {
            let eof = Token::new_checked(TokenKind::Eof, "", 0, 0).expect("valid token");
            let synthetic = Token::new_checked(TokenKind::Unknown, "?", 0, 1).expect("valid token");
            black_box((eof, synthetic));
        }),
    );
}

fn benchmark_token_scorecard(c: &mut Criterion) {
    run_scorecard();

    c.bench_function("token_scorecard_recorded", |b| {
        b.iter(|| {
            black_box(0usize);
        });
    });
}

fn bench_borrowed_token_construction(c: &mut Criterion) {
    c.bench_function("token/borrowed_construction", |b| {
        b.iter(|| {
            black_box(
                TokenRef::new_checked(
                    TokenKind::Identifier,
                    black_box("foobar"),
                    black_box(START),
                    black_box(END),
                )
                .expect("valid token"),
            )
        });
    });
}

fn bench_owned_token_construction(c: &mut Criterion) {
    c.bench_function("token/owned_construction", |b| {
        b.iter(|| {
            black_box(
                Token::new_checked(
                    TokenKind::Identifier,
                    black_box("foobar"),
                    black_box(START),
                    black_box(END),
                )
                .expect("valid token"),
            )
        });
    });
}

fn bench_borrowed_to_owned_conversion(c: &mut Criterion) {
    let borrowed =
        TokenRef::new_checked(TokenKind::Identifier, "foobar", START, END).expect("valid token");
    c.bench_function("token/borrowed_to_owned_conversion", |b| {
        b.iter(|| black_box(borrowed).to_owned_token());
    });
}

criterion_group!(
    benches,
    benchmark_token_scorecard,
    bench_borrowed_token_construction,
    bench_owned_token_construction,
    bench_borrowed_to_owned_conversion
);
criterion_main!(benches);
