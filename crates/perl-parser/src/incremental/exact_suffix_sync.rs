//! Fail-closed exact suffix synchronization contract (#6986).
//!
//! This module defines the admission oracle and typed denial reasons. Production
//! lex restart still reports zero suffix reuse until a later slice wires this
//! oracle into the canonical incremental path.

use perl_lexer::{BehaviorBearingStateMismatch, LexerCheckpoint, Token, TokenType};
use thiserror::Error;

/// Contract version for the suffix-sync evaluation surface.
pub const EXACT_SUFFIX_SYNC_CONTRACT_VERSION: u32 = 1;

/// Correctness policy for direct token-window evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactSuffixSyncPolicy {
    /// Number of consecutive tokens that must agree after state equality.
    pub window_tokens: usize,
}

impl Default for ExactSuffixSyncPolicy {
    fn default() -> Self {
        Self { window_tokens: 3 }
    }
}

/// Work and retention counters reported by exact suffix synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExactSuffixSyncMetrics {
    pub new_bytes_lexed: usize,
    pub new_tokens_produced: usize,
    pub prefix_tokens_retained: usize,
    pub suffix_tokens_retained: usize,
    pub state_candidates_considered: usize,
    pub state_candidates_rejected: usize,
    pub window_tokens_compared: usize,
    pub validation_oracle_work: usize,
    pub full_parser_invocations: usize,
}

/// Why exact suffix reuse was denied.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ExactSuffixSyncDenial {
    #[error("complete behavior-bearing lexer state differs")]
    StateMismatch(BehaviorBearingStateMismatch),
    #[error("behavior-bearing state cannot be compared")]
    StateIncomparable,
    #[error("direct token window mismatch at index {index}")]
    WindowMismatch { index: usize },
    #[error("edit intersects the candidate suffix span")]
    EditIntersectsSuffix,
    #[error("source mapping is not one stable byte shift")]
    NonuniformShift,
    #[error("recovery or timeout-sensitive boundary is not admissible")]
    RecoveryBoundary,
    #[error("suffix generation or cache identity is stale")]
    StaleGeneration,
    #[error("parser or lexer configuration differs")]
    WrongConfiguration,
    #[error("no synchronization candidate was available")]
    NoCandidate,
    #[error("token-window evidence requires a non-zero window")]
    InsufficientWindowEvidence,
}

/// Inputs required to evaluate one suffix-sync candidate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExactSuffixSyncCandidate<'a> {
    pub old_checkpoint: &'a LexerCheckpoint,
    pub new_checkpoint: &'a LexerCheckpoint,
    pub byte_shift: isize,
    pub edit_start: usize,
    pub edit_old_end: usize,
    pub suffix_start_old: usize,
}

/// Evaluate one exact suffix synchronization candidate.
///
/// Direct state equality and token-window evidence are both required. A matching
/// fingerprint alone is insufficient and is not consulted here.
pub fn evaluate_exact_suffix_sync(
    policy: ExactSuffixSyncPolicy,
    new_source: &str,
    old_suffix: &[Token],
    new_suffix: &[Token],
    candidate: ExactSuffixSyncCandidate<'_>,
) -> Result<ExactSuffixSyncMetrics, ExactSuffixSyncDenial> {
    if candidate.edit_old_end > candidate.edit_start
        && candidate.suffix_start_old < candidate.edit_old_end
        && candidate.edit_start <= candidate.suffix_start_old
    {
        return Err(ExactSuffixSyncDenial::EditIntersectsSuffix);
    }
    if candidate.old_checkpoint.is_invalidated() || candidate.new_checkpoint.is_invalidated() {
        return Err(ExactSuffixSyncDenial::StateIncomparable);
    }
    if candidate.old_checkpoint.is_timeout_sensitive()
        || candidate.new_checkpoint.is_timeout_sensitive()
    {
        return Err(ExactSuffixSyncDenial::RecoveryBoundary);
    }
    if policy.window_tokens == 0 {
        return Err(ExactSuffixSyncDenial::InsufficientWindowEvidence);
    }

    candidate
        .old_checkpoint
        .behavior_bearing_state_equal_with_byte_shift(
            candidate.new_checkpoint,
            candidate.byte_shift,
        )
        .map_err(ExactSuffixSyncDenial::StateMismatch)?;

    if let (Some(old_first), Some(new_first)) = (old_suffix.first(), new_suffix.first()) {
        if old_first.start != candidate.suffix_start_old {
            return Err(ExactSuffixSyncDenial::WindowMismatch { index: 0 });
        }
        let mapped_start = apply_shift(old_first.start, candidate.byte_shift, new_source)?;
        if new_first.start != mapped_start {
            return Err(ExactSuffixSyncDenial::WindowMismatch { index: 0 });
        }
    }

    let compared = compare_direct_token_window(
        new_source,
        candidate.byte_shift,
        old_suffix,
        new_suffix,
        policy.window_tokens,
    )?;

    Ok(ExactSuffixSyncMetrics {
        suffix_tokens_retained: old_suffix.len(),
        window_tokens_compared: compared,
        state_candidates_considered: 1,
        ..ExactSuffixSyncMetrics::default()
    })
}

/// Compare a bounded direct token window under one stable byte shift.
pub fn compare_direct_token_window(
    new_source: &str,
    byte_shift: isize,
    old_suffix: &[Token],
    new_suffix: &[Token],
    window: usize,
) -> Result<usize, ExactSuffixSyncDenial> {
    if window == 0 {
        return Err(ExactSuffixSyncDenial::InsufficientWindowEvidence);
    }
    if old_suffix.len() < window || new_suffix.len() < window {
        return Err(ExactSuffixSyncDenial::InsufficientWindowEvidence);
    }

    for index in 0..window {
        let old = &old_suffix[index];
        let new = &new_suffix[index];
        if token_blocks_exact_sync(old) || token_blocks_exact_sync(new) {
            return Err(ExactSuffixSyncDenial::RecoveryBoundary);
        }
        let mapped_start = apply_shift(old.start, byte_shift, new_source)?;
        let mapped_end = apply_shift(old.end, byte_shift, new_source)?;
        if new.start != mapped_start || new.end != mapped_end {
            return Err(ExactSuffixSyncDenial::WindowMismatch { index });
        }
        if !token_types_equivalent(&old.token_type, &new.token_type) {
            return Err(ExactSuffixSyncDenial::WindowMismatch { index });
        }
        if old.text.as_ref() != new.text.as_ref() {
            return Err(ExactSuffixSyncDenial::WindowMismatch { index });
        }
        if !old.text.is_empty() {
            let mapped_slice = new_source
                .get(mapped_start..mapped_end)
                .ok_or(ExactSuffixSyncDenial::NonuniformShift)?;
            if mapped_slice != old.text.as_ref() {
                return Err(ExactSuffixSyncDenial::WindowMismatch { index });
            }
        }
    }

    Ok(window)
}

fn apply_shift(
    offset: usize,
    shift: isize,
    new_source: &str,
) -> Result<usize, ExactSuffixSyncDenial> {
    let shifted = offset as isize + shift;
    if shifted < 0 {
        return Err(ExactSuffixSyncDenial::NonuniformShift);
    }
    let shifted = shifted as usize;
    if shifted > new_source.len() || !new_source.is_char_boundary(shifted) {
        return Err(ExactSuffixSyncDenial::NonuniformShift);
    }
    Ok(shifted)
}

fn token_blocks_exact_sync(token: &Token) -> bool {
    token.token_type.is_recovery_token()
}

fn token_types_equivalent(left: &TokenType, right: &TokenType) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_lexer::{Checkpointable, PerlLexer};

    type R = Result<(), Box<dyn std::error::Error>>;

    fn collect_suffix(source: &str, tokens_before: usize) -> Vec<Token> {
        let mut lexer = PerlLexer::new(source);
        for _ in 0..tokens_before {
            let _ = lexer.next_token();
        }
        let mut out = Vec::new();
        while let Some(token) = lexer.next_token() {
            let done = matches!(token.token_type, TokenType::EOF);
            out.push(token);
            if done {
                break;
            }
        }
        out
    }

    #[test]
    fn same_kind_same_width_different_payload_is_denied() -> R {
        use std::sync::Arc;
        let old = Token::new(TokenType::Identifier(Arc::from("aaa")), "aaa", 0, 3);
        let new = Token::new(TokenType::Identifier(Arc::from("bbb")), "bbb", 0, 3);
        let new_source = "bbb; my $y = 1;\n";
        let err = compare_direct_token_window(
            new_source,
            0,
            std::slice::from_ref(&old),
            std::slice::from_ref(&new),
            1,
        )
        .expect_err("payload mismatch must deny reuse");
        assert!(matches!(err, ExactSuffixSyncDenial::WindowMismatch { index: 0 }));
        Ok(())
    }

    #[test]
    fn converged_suffix_with_equal_state_and_window_is_admitted() -> R {
        let old_source = "my $prefix = 1;\nmy $suffix = 2;\n";
        let new_source = "my $prefix = 9;\nmy $suffix = 2;\n";
        let old_suffix = collect_suffix(old_source, 8);
        let new_suffix = collect_suffix(new_source, 8);
        let old_cp = {
            let mut lexer = PerlLexer::new(old_source);
            for _ in 0..8 {
                let _ = lexer.next_token();
            }
            lexer.checkpoint()
        };
        let new_cp = {
            let mut lexer = PerlLexer::new(new_source);
            for _ in 0..8 {
                let _ = lexer.next_token();
            }
            lexer.checkpoint()
        };
        let suffix_start_old = old_suffix.first().map_or(0, |token| token.start);
        let candidate = ExactSuffixSyncCandidate {
            old_checkpoint: &old_cp,
            new_checkpoint: &new_cp,
            byte_shift: 0,
            edit_start: 14,
            edit_old_end: 15,
            suffix_start_old,
        };
        let metrics = evaluate_exact_suffix_sync(
            ExactSuffixSyncPolicy { window_tokens: 3 },
            new_source,
            &old_suffix,
            &new_suffix,
            candidate,
        )?;
        assert_eq!(metrics.suffix_tokens_retained, old_suffix.len());
        assert_eq!(metrics.window_tokens_compared, 3);
        Ok(())
    }

    #[test]
    fn fingerprint_difference_implies_state_mismatch() -> R {
        let source = "my $a = 1;\nmy $b = 2;\n";
        let mut lexer = PerlLexer::new(source);
        let first = lexer.checkpoint();
        let _ = lexer.next_token();
        let second = lexer.checkpoint();
        assert_ne!(first.behavior_bearing_fingerprint(), second.behavior_bearing_fingerprint());
        assert_ne!(first.behavior_bearing_state_equal(&second), Ok(()));
        Ok(())
    }
}
