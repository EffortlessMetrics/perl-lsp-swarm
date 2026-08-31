//! Unified budget-stop recovery shape contract (#14158 adjudication, #6717).
//!
//! Every per-token budget stop — regex scan steps, regex bytes, delimiter
//! nesting depth, heredoc body bytes — must emit the same recovery shape at
//! the lexer boundary: an empty-text `UnknownRest` spanning the degraded
//! remainder `[token_start, input.len())`, immediately followed by terminal
//! `EOF`, with identical input producing identical tokens. Over-budget
//! recovery must never copy the unbounded source remainder; consumers that
//! need the payload reconstruct it from source they already hold (the parser
//! `TokenStream` conversion owns that step).
//!
//! The one documented exception, pinned at the bottom, is a heredoc that
//! reaches EOF *inside* its budget: its body is bounded by the budget itself,
//! so retaining the payload is safe. Boundedness — not token kind — is the
//! dividing line between the two shapes.
//!
//! The delimiter-depth arm of `budget_guard` has no public-API driver (its
//! only caller passes `depth = 0`), so it is pinned from inside the crate in
//! `src/tests.rs` instead.

use std::sync::Arc;

use perl_lexer::{MAX_REGEX_PARSE_STEPS, PerlLexer, TokenType};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;
type TokenSignature = (TokenType, Arc<str>, usize, usize);

// Mirrors the crate-private budget constants (`src/limits.rs`). The fixtures
// below are sized so exactly one arm can fire per scenario.
const MAX_REGEX_BYTES: usize = 64 * 1024;
const MAX_HEREDOC_BYTES: usize = 256 * 1024;

const REGEX_PREFIX: &str = "my $x = ";
const HEREDOC_HEADER: &str = "my $text = <<END;\n";

fn signatures(source: &str) -> Vec<TokenSignature> {
    let mut lexer = PerlLexer::with_body_tokens(source);
    lexer
        .collect_tokens()
        .into_iter()
        .map(|token| (token.token_type, token.text, token.start, token.end))
        .collect()
}

fn recovery_index(tokens: &[TokenSignature], path: &str) -> R<usize> {
    tokens.iter().position(|token| matches!(token.0, TokenType::UnknownRest)).ok_or_else(|| {
        std::io::Error::other(format!("{path}: over-budget input must emit UnknownRest")).into()
    })
}

/// Assert the unified over-budget recovery shape for one budget-stop path.
fn assert_geometry_only_over_budget_recovery(source: &str, recovery_start: usize, path: &str) -> R {
    let tokens = signatures(source);
    let index = recovery_index(&tokens, path)?;
    let recovery = &tokens[index];

    assert!(
        recovery.0.is_recovery_token(),
        "{path}: budget recovery must classify as a recovery token"
    );
    assert!(
        recovery.1.is_empty(),
        "{path}: over-budget recovery must not copy the unbounded source remainder"
    );
    assert_eq!(
        (recovery.2, recovery.3),
        (recovery_start, source.len()),
        "{path}: recovery span must cover the degraded remainder through EOF"
    );
    assert!(
        matches!(tokens.get(index + 1).map(|token| &token.0), Some(TokenType::EOF)),
        "{path}: terminal EOF must immediately follow the recovery token"
    );
    assert_eq!(tokens.len(), index + 2, "{path}: no token may follow terminal EOF");
    assert_eq!(
        signatures(source),
        tokens,
        "{path}: budget outcome must be repeatable for identical input"
    );
    Ok(())
}

/// Regex scan-step budget: enough plain pattern characters to exhaust
/// `MAX_REGEX_PARSE_STEPS` while staying far below the byte budget, so
/// exactly the step arm fires.
#[test]
fn regex_step_budget_stop_is_geometry_only() -> R {
    let path = "regex step budget";
    let source = format!("{REGEX_PREFIX}/{}", "a".repeat(MAX_REGEX_PARSE_STEPS + 1024));
    assert!(source.len() < MAX_REGEX_BYTES, "fixture must exhaust steps before bytes");
    assert_geometry_only_over_budget_recovery(&source, REGEX_PREFIX.len(), path)
}

/// Regex byte budget: `\é` pairs grow the span three bytes per scan step, so
/// `MAX_REGEX_BYTES` is exceeded before the step limit can fire.
#[test]
fn regex_byte_budget_stop_is_geometry_only() -> R {
    let path = "regex byte budget";
    let source = format!("{REGEX_PREFIX}/{}", "\\é".repeat(24_000));
    assert!(
        source.len() > REGEX_PREFIX.len() + MAX_REGEX_BYTES,
        "fixture must exceed the byte budget"
    );
    assert_geometry_only_over_budget_recovery(&source, REGEX_PREFIX.len(), path)
}

/// Heredoc body budget: the geometry-only shape #6717 pins at the adjacent
/// byte boundary, restated here so the unified enumeration covers every
/// budget stop in one place.
#[test]
fn heredoc_byte_budget_stop_is_geometry_only() -> R {
    let path = "heredoc byte budget";
    let source = format!("{HEREDOC_HEADER}{}\nEND\n", "x".repeat(MAX_HEREDOC_BYTES + 1));
    assert_geometry_only_over_budget_recovery(&source, HEREDOC_HEADER.len(), path)
}

/// The single documented exception: EOF reached *inside* the heredoc budget
/// keeps its budget-bounded body payload.
#[test]
fn eof_inside_heredoc_budget_keeps_bounded_payload() -> R {
    let path = "EOF inside heredoc budget";
    let source = format!("{HEREDOC_HEADER}abc");
    let body_start = HEREDOC_HEADER.len();

    let tokens = signatures(&source);
    let index = recovery_index(&tokens, path)?;
    let recovery = &tokens[index];

    assert_eq!(
        &*recovery.1,
        &source[body_start..],
        "{path}: the in-budget body payload is retained"
    );
    assert!(
        recovery.1.len() <= MAX_HEREDOC_BYTES,
        "{path}: the retained payload stays budget-bounded"
    );
    assert_eq!((recovery.2, recovery.3), (body_start, source.len()));
    assert!(
        matches!(tokens.get(index + 1).map(|token| &token.0), Some(TokenType::EOF)),
        "{path}: terminal EOF must immediately follow the recovery token"
    );
    Ok(())
}
