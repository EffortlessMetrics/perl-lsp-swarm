//! Deterministic lexer parse budgets for graceful degradation on pathological input.
//!
//! Source-derived tokenization is governed by byte, step, and depth budgets.
//! Wall-clock cancellation belongs to the caller or process supervisor and must
//! not change the token stream for identical source and configuration.

// When these limits are exceeded, the lexer emits UnknownRest, preserving all
// previously parsed symbols while making the unlexed remainder explicit.
/// Maximum source bytes consumed by one regex literal.
pub(crate) const MAX_REGEX_BYTES: usize = 64 * 1024;

/// Maximum source bytes consumed by one heredoc body.
pub(crate) const MAX_HEREDOC_BYTES: usize = 256 * 1024;

/// Maximum delimiter nesting depth within one token.
pub(crate) const MAX_DELIM_NEST: usize = 128;

/// Maximum number of pending heredocs queued by one statement.
pub(crate) const MAX_HEREDOC_DEPTH: usize = 100;

/// Maximum scan iterations for a single regex literal.
/// This is a lexer parse budget, not regex-engine backtracking detection.
///
/// When the lexer encounters a regex literal that requires more than this
/// number of loop iterations, it
/// will emit an UnknownRest token for graceful degradation rather than
/// potentially hanging on pathological input.
///
/// The limit intentionally stays below `MAX_REGEX_BYTES` so this guard remains
/// reachable before the byte budget for very large but still bounded literals.
pub const MAX_REGEX_PARSE_STEPS: usize = 32 * 1024;
