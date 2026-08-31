//! Unified budget-stop recovery shape contract (#14158 adjudication, #6717).
//!
//! Every reachable per-token budget stop — regex scan steps, regex bytes, and
//! heredoc body bytes — must emit the same recovery shape at the lexer
//! boundary: an empty-text `UnknownRest` spanning the degraded remainder
//! `[token_start, input.len())`, immediately followed by terminal `EOF`, with
//! identical input producing identical tokens. Over-budget recovery must
//! never copy the unbounded source remainder; consumers that need the payload
//! reconstruct it from source they already hold (the parser `TokenStream`
//! conversion owns that step).
//!
//! Two budget stops are documented exceptions because their payloads stay
//! bounded; boundedness — not token kind — is the dividing line between the
//! shapes:
//!
//! - a heredoc that reaches EOF *inside* its budget keeps its body payload
//!   (`<= MAX_HEREDOC_BYTES`); pinned at the bottom of this file;
//! - `try_heredoc` at `MAX_HEREDOC_DEPTH` pending heredocs emits a
//!   payload-carrying `Error("Heredoc nesting too deep")` over the
//!   line-bounded header text (no remainder copy, no EOF jump); pinned by
//!   `tests/heredoc_security_tests.rs`.
//!
//! The `MAX_DELIM_NEST` arm of `budget_guard` is *not* part of this
//! contract: it has no public-API driver (its only caller passes
//! `depth = 0`), so it is pinned from inside the crate in `src/tests.rs`.
//! Wiring it through a real driver or removing it is tracked in #14389.

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

/// One of the two documented exceptions: EOF reached *inside* the heredoc
/// budget keeps its budget-bounded body payload.
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

/// The `<=` boundary of that exception: a body of exactly `MAX_HEREDOC_BYTES`
/// with no terminator sits *at* the budget when EOF arrives, so the full
/// payload must be retained. This discriminates the boundary — a regression
/// to a strict `<` comparison here, or an over-budget jump in this arm, would
/// swap the retained payload for the geometry-only shape pinned above.
#[test]
fn eof_at_exact_heredoc_budget_keeps_bounded_payload() -> R {
    let path = "EOF at exact heredoc budget";
    let source = format!("{HEREDOC_HEADER}{}", "x".repeat(MAX_HEREDOC_BYTES));
    assert_eq!(source.len() - HEREDOC_HEADER.len(), MAX_HEREDOC_BYTES);
    let body_start = HEREDOC_HEADER.len();

    let tokens = signatures(&source);
    let index = recovery_index(&tokens, path)?;
    let recovery = &tokens[index];

    assert_eq!(
        &*recovery.1,
        &source[body_start..],
        "{path}: the exactly-at-budget body payload is retained"
    );
    assert_eq!(
        recovery.1.len(),
        MAX_HEREDOC_BYTES,
        "{path}: the retained payload pins the <= MAX_HEREDOC_BYTES boundary"
    );
    assert_eq!((recovery.2, recovery.3), (body_start, source.len()));
    assert!(
        matches!(tokens.get(index + 1).map(|token| &token.0), Some(TokenType::EOF)),
        "{path}: terminal EOF must immediately follow the recovery token"
    );
    Ok(())
}
