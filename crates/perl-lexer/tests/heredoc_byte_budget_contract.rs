//! Exact deterministic threshold contract for the lexer heredoc byte budget (#6717).
//!
//! Identical source and configuration must produce identical source-derived
//! tokens. The accepted and rejected cases therefore pin the adjacent byte
//! boundary, payload, range, terminal recovery shape, Unicode prefix handling,
//! and both single-line and line-oriented bodies.

use std::sync::Arc;

use perl_lexer::{PerlLexer, TokenType};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;
type TokenSignature = (TokenType, Arc<str>, usize, usize);

const MAX_HEREDOC_BYTES: usize = 256 * 1024;
const HEADER: &str = "my $text = <<END;\n";
const TERMINATOR: &str = "END\n";

fn missing(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}

fn heredoc_with_single_line_body(body_bytes: usize) -> String {
    assert!(body_bytes > 0, "single-line fixture requires its terminating newline");

    let mut source = String::with_capacity(HEADER.len() + body_bytes + TERMINATOR.len());
    source.push_str(HEADER);
    source.push_str(&"x".repeat(body_bytes - 1));
    source.push('\n');
    source.push_str(TERMINATOR);
    source
}

fn heredoc_with_multiline_body(body_bytes: usize) -> String {
    assert_eq!(body_bytes % 2, 0, "multiline fixture uses two-byte `x\\n` lines");

    let mut source = String::with_capacity(HEADER.len() + body_bytes + TERMINATOR.len());
    source.push_str(HEADER);
    source.push_str(&"x\n".repeat(body_bytes / 2));
    source.push_str(TERMINATOR);
    source
}

fn signatures(source: &str) -> Vec<TokenSignature> {
    let mut lexer = PerlLexer::with_body_tokens(source);
    lexer
        .collect_tokens()
        .into_iter()
        .map(|token| (token.token_type, token.text, token.start, token.end))
        .collect()
}

fn accepted_body<'a>(source: &'a str, tokens: &'a [TokenSignature]) -> R<&'a TokenSignature> {
    let bodies = tokens
        .iter()
        .filter(|token| matches!(&token.0, TokenType::HeredocBody(_)))
        .collect::<Vec<_>>();
    if bodies.len() != 1 {
        return Err(missing(format!("expected one accepted heredoc body, got {}", bodies.len())));
    }
    let body = bodies[0];
    assert!(body.1.is_empty(), "body events carry geometry, not duplicate text");
    assert!(source.is_char_boundary(body.2));
    assert!(source.is_char_boundary(body.3));
    Ok(body)
}

#[test]
fn byte_budget_accepts_max_minus_one_and_the_exact_boundary() -> R {
    for body_bytes in [MAX_HEREDOC_BYTES - 1, MAX_HEREDOC_BYTES] {
        let source = heredoc_with_single_line_body(body_bytes);
        let tokens = signatures(&source);
        let body = accepted_body(&source, &tokens)?;
        let expected_start = HEADER.len();
        let expected_end = expected_start + body_bytes;

        assert!(matches!(&body.0, TokenType::HeredocBody(payload) if payload.is_empty()));
        assert_eq!((body.2, body.3), (expected_start, expected_end));
        assert_eq!(source.get(body.2..body.3), source.get(expected_start..expected_end));
        assert_eq!(source.get(body.2..body.3).map(str::len), Some(body_bytes));
        assert!(tokens.iter().all(|token| !token.0.is_recovery_token()));
        assert!(matches!(tokens.last().map(|token| &token.0), Some(TokenType::EOF)));
    }
    Ok(())
}

#[test]
fn byte_budget_rejects_the_first_byte_above_the_boundary() -> R {
    let source = heredoc_with_single_line_body(MAX_HEREDOC_BYTES + 1);
    let tokens = signatures(&source);
    let body_start = HEADER.len();

    assert!(!tokens.iter().any(|token| matches!(&token.0, TokenType::HeredocBody(_))));
    let recovery_index = tokens
        .iter()
        .position(|token| matches!(&token.0, TokenType::UnknownRest))
        .ok_or_else(|| missing("body one byte above the budget must emit UnknownRest"))?;
    let recovery = &tokens[recovery_index];

    assert!(
        recovery.1.is_empty(),
        "over-budget recovery must not copy the unbounded source remainder"
    );
    assert_eq!((recovery.2, recovery.3), (body_start, source.len()));
    assert!(matches!(tokens.get(recovery_index + 1).map(|token| &token.0), Some(TokenType::EOF)));
    assert_eq!(tokens.len(), recovery_index + 2, "no token may follow terminal EOF");
    Ok(())
}

#[test]
fn multiline_scanning_uses_the_same_exact_byte_boundary() -> R {
    for body_bytes in [MAX_HEREDOC_BYTES, MAX_HEREDOC_BYTES + 2] {
        let source = heredoc_with_multiline_body(body_bytes);
        let tokens = signatures(&source);
        if body_bytes == MAX_HEREDOC_BYTES {
            let body = accepted_body(&source, &tokens)?;
            assert_eq!((body.2, body.3), (HEADER.len(), HEADER.len() + body_bytes));
            assert!(tokens.iter().all(|token| !token.0.is_recovery_token()));
        } else {
            assert!(tokens.iter().any(|token| matches!(&token.0, TokenType::UnknownRest)));
            assert!(!tokens.iter().any(|token| matches!(&token.0, TokenType::HeredocBody(_))));
        }
    }
    Ok(())
}

#[test]
fn budget_outcome_is_repeatable_for_identical_input() {
    for body_bytes in [MAX_HEREDOC_BYTES - 1, MAX_HEREDOC_BYTES, MAX_HEREDOC_BYTES + 1] {
        let source = heredoc_with_single_line_body(body_bytes);
        assert_eq!(signatures(&source), signatures(&source), "body bytes: {body_bytes}");
    }
}

#[test]
fn unicode_prefix_preserves_the_exact_body_payload_and_threshold() -> R {
    let prefix = "my $café = 1;\n";
    let source = format!("{prefix}{}", heredoc_with_single_line_body(MAX_HEREDOC_BYTES));
    let tokens = signatures(&source);
    let body = accepted_body(&source, &tokens)?;
    let expected_start = prefix.len() + HEADER.len();
    let expected_end = expected_start + MAX_HEREDOC_BYTES;

    assert_eq!((body.2, body.3), (expected_start, expected_end));
    assert_eq!(source.get(body.2..body.3).map(str::len), Some(MAX_HEREDOC_BYTES));
    assert!(tokens.iter().all(|token| !token.0.is_recovery_token()));
    assert!(matches!(tokens.last().map(|token| &token.0), Some(TokenType::EOF)));
    Ok(())
}

#[test]
fn interleaved_lexers_produce_the_same_deterministic_stream() {
    let source = heredoc_with_single_line_body(MAX_HEREDOC_BYTES);
    let mut left = PerlLexer::with_body_tokens(&source);
    let mut right = PerlLexer::with_body_tokens(&source);
    let mut left_tokens = Vec::new();
    let mut right_tokens = Vec::new();

    loop {
        let left_token = left.next_token();
        std::thread::yield_now();
        let right_token = right.next_token();
        let left_done =
            left_token.as_ref().is_some_and(|token| matches!(&token.token_type, TokenType::EOF));
        let right_done =
            right_token.as_ref().is_some_and(|token| matches!(&token.token_type, TokenType::EOF));
        left_tokens
            .extend(left_token.map(|token| (token.token_type, token.text, token.start, token.end)));
        right_tokens.extend(
            right_token.map(|token| (token.token_type, token.text, token.start, token.end)),
        );
        assert_eq!(left_done, right_done);
        if left_done {
            break;
        }
    }

    assert_eq!(left_tokens, right_tokens);
    assert!(left.next_token().is_none());
    assert!(right.next_token().is_none());
}

#[test]
fn oversized_single_physical_line_has_bounded_scan_and_empty_recovery_payload() -> R {
    const BODY_BYTES: usize = 4 * 1024 * 1024;
    let mut source = String::with_capacity(HEADER.len() + BODY_BYTES);
    source.push_str(HEADER);
    source.push_str(&"x".repeat(BODY_BYTES));

    let tokens = signatures(&source);
    let recovery_index = tokens
        .iter()
        .position(|token| matches!(&token.0, TokenType::UnknownRest))
        .ok_or_else(|| missing("oversized physical line must emit UnknownRest"))?;
    let recovery = &tokens[recovery_index];

    assert!(recovery.1.is_empty());
    assert_eq!((recovery.2, recovery.3), (HEADER.len(), source.len()));
    assert!(matches!(tokens.get(recovery_index + 1).map(|token| &token.0), Some(TokenType::EOF)));
    assert_eq!(tokens.len(), recovery_index + 2);

    const LIB: &str = include_str!("../src/lib.rs");
    let normalized: String = LIB.chars().filter(|ch| !ch.is_whitespace()).collect();
    assert!(normalized.contains("fnheredoc_budget_recovery"));
    assert!(normalized.contains(
        "letbody_scan_end=body_start.saturating_add(MAX_HEREDOC_BYTES+1).min(self.input.len());"
    ));
    assert!(normalized.contains("&self.input_bytes[..body_scan_end]"));
    assert!(normalized.contains(
        "letscan_end=line_start.saturating_add(MAX_HEREDOC_BYTES).min(self.input_bytes.len());"
    ));
    Ok(())
}

#[test]
fn production_heredoc_scanning_contains_no_wall_clock_cutoff() {
    const LIB: &str = include_str!("../src/lib.rs");
    const LIMITS: &str = include_str!("../src/limits.rs");
    const STATE: &str = include_str!("../src/lexer/state.rs");
    const DRIVER: &str = include_str!("../src/lexer/driver.rs");

    for source in [LIB, LIMITS, STATE, DRIVER] {
        assert!(!source.contains("HEREDOC_TIMEOUT_MS"));
        assert!(!source.contains("Heredoc parsing timeout"));
        assert!(!source.contains("start_time"));
        assert!(!source.contains("Instant::now"));
        assert!(!source.contains(".elapsed().as_millis()"));
    }
}
