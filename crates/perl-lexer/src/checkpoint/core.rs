use crate::mode::LexerMode;
use crate::{LexerConfig, Position};
use perl_source_identity::{LogicalSourceId, SourceGeneration};
use std::fmt;

use super::identity::{CheckpointRestoreError, LexerCheckpointIdentity};

/// Replay-safe representation of one queued heredoc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingHeredocCheckpoint {
    /// Heredoc terminator label.
    pub label: String,
    /// Byte offset where the heredoc body begins.
    pub body_start: usize,
    /// Whether `<<~` indentation is allowed.
    pub allow_indent: bool,
    /// Whether the body interpolates (#8779). Part of the replay-safe state:
    /// a restored checkpoint must keep the body's interpolation disposition.
    pub interpolates: bool,
}

/// Replay-safe representation of an in-progress quote-like operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteOperatorCheckpoint {
    /// Operator name such as `q`, `qr`, `s`, or `tr`.
    pub operator: String,
    /// Opening delimiter.
    pub delimiter: char,
    /// Byte offset where the operator begins.
    pub start_pos: usize,
}

/// Additional context that may be needed for certain lexer states.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckpointContext {
    /// Normal lexing.
    Normal,
    /// Inside a heredoc.
    Heredoc {
        /// Terminator label, such as `END` in `<<END`.
        terminator: String,
        /// Whether the heredoc body is interpolated.
        is_interpolated: bool,
    },
    /// Inside a format body.
    Format {
        /// Byte offset where the format body begins.
        start_position: usize,
    },
    /// Inside a regex or substitution.
    Regex {
        /// Regex delimiter.
        delimiter: char,
        /// Byte offset where flags begin, if already scanned.
        flags_position: Option<usize>,
    },
    /// Inside a quote-like operator.
    QuoteLike {
        /// Operator name such as `q`, `qq`, or `qw`.
        operator: String,
        /// Opening delimiter.
        delimiter: char,
        /// Whether the delimiter is paired.
        is_paired: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplayState {
    pub(crate) position: usize,
    pub(crate) mode: LexerMode,
    pub(crate) delimiter_stack: Vec<char>,
    pub(crate) in_prototype: bool,
    pub(crate) prototype_depth: usize,
    pub(crate) after_sub: bool,
    pub(crate) after_arrow: bool,
    pub(crate) hash_brace_depth: usize,
    pub(crate) after_var_subscript: bool,
    pub(crate) paren_depth: usize,
    pub(crate) current_pos: Position,
    pub(crate) after_newline: bool,
    pub(crate) pending_heredocs: Vec<PendingHeredocCheckpoint>,
    pub(crate) line_start_offset: usize,
    pub(crate) current_quote_op: Option<QuoteOperatorCheckpoint>,
    pub(crate) eof_emitted: bool,
    pub(crate) context: CheckpointContext,
}

/// Opaque snapshot of live lexer restart state.
///
/// Production checkpoints are captured from a live lexer at a token boundary.
/// Fields are private so callers cannot synthesize an arbitrary-position
/// restart. Restore is fallible and leaves the lexer unchanged on failure.
#[derive(Debug, Clone, PartialEq)]
pub struct LexerCheckpoint {
    identity: LexerCheckpointIdentity,
    replay: ReplayState,
    invalidated: bool,
    live_boundary: bool,
}

impl LexerCheckpoint {
    pub(crate) fn from_live(identity: LexerCheckpointIdentity, replay: ReplayState) -> Self {
        Self { identity, replay, invalidated: false, live_boundary: true }
    }

    /// Semantically valid origin: the live start-of-input checkpoint for `source`.
    #[must_use]
    pub fn origin(source: &str) -> Self {
        Checkpointable::checkpoint(&crate::PerlLexer::new(source))
    }

    /// Semantically valid origin under an explicit configuration.
    #[must_use]
    pub fn origin_with_config(source: &str, config: LexerConfig) -> Self {
        Checkpointable::checkpoint(&crate::PerlLexer::with_config(source, config))
    }

    /// Compatibility origin for empty default-configured source.
    #[deprecated(
        since = "0.17.0",
        note = "bind origin to the real source: LexerCheckpoint::origin(source) or PerlLexer::checkpoint()"
    )]
    #[must_use]
    pub fn new() -> Self {
        Self::origin("")
    }

    /// Position label that is never a live restart boundary.
    ///
    /// Cache window tests may still use this as a sorted position key.
    /// Production restore rejects it.
    #[deprecated(
        since = "0.17.0",
        note = "not a live restart boundary; capture from PerlLexer::checkpoint()"
    )]
    #[must_use]
    pub fn at_position(position: usize) -> Self {
        let mut checkpoint = Self::origin("");
        checkpoint.replay.position = position;
        checkpoint.replay.current_pos.byte = position;
        checkpoint.live_boundary = false;
        checkpoint
    }

    /// Identity captured with this checkpoint.
    #[must_use]
    pub fn identity(&self) -> &LexerCheckpointIdentity {
        &self.identity
    }

    /// Current byte position.
    #[must_use]
    pub fn position(&self) -> usize {
        self.replay.position
    }

    /// Primary lexer mode.
    #[must_use]
    pub fn mode(&self) -> LexerMode {
        self.replay.mode
    }

    /// Nested delimiter stack.
    #[must_use]
    pub fn delimiter_stack(&self) -> &[char] {
        &self.replay.delimiter_stack
    }

    /// Whether prototype parentheses are active.
    #[must_use]
    pub fn in_prototype(&self) -> bool {
        self.replay.in_prototype
    }

    /// Prototype parenthesis depth.
    #[must_use]
    pub fn prototype_depth(&self) -> usize {
        self.replay.prototype_depth
    }

    /// Whether `sub` was just emitted.
    #[must_use]
    pub fn after_sub(&self) -> bool {
        self.replay.after_sub
    }

    /// Whether `->` was just emitted.
    #[must_use]
    pub fn after_arrow(&self) -> bool {
        self.replay.after_arrow
    }

    /// Hash-subscript brace depth.
    #[must_use]
    pub fn hash_brace_depth(&self) -> usize {
        self.replay.hash_brace_depth
    }

    /// Whether a complete variable was just emitted.
    #[must_use]
    pub fn after_var_subscript(&self) -> bool {
        self.replay.after_var_subscript
    }

    /// Open-parenthesis depth.
    #[must_use]
    pub fn paren_depth(&self) -> usize {
        self.replay.paren_depth
    }

    /// Line/column summary captured with this checkpoint.
    #[must_use]
    pub fn current_pos(&self) -> Position {
        self.replay.current_pos
    }

    /// Whether the previous consumed unit ended a line.
    #[must_use]
    pub fn after_newline(&self) -> bool {
        self.replay.after_newline
    }

    /// Queued heredoc replay state.
    #[must_use]
    pub fn pending_heredocs(&self) -> &[PendingHeredocCheckpoint] {
        &self.replay.pending_heredocs
    }

    /// Byte offset of the current physical line start.
    #[must_use]
    pub fn line_start_offset(&self) -> usize {
        self.replay.line_start_offset
    }

    /// In-progress quote-operator metadata, when present.
    #[must_use]
    pub fn current_quote_op(&self) -> Option<&QuoteOperatorCheckpoint> {
        self.replay.current_quote_op.as_ref()
    }

    /// Whether the terminal EOF token has already been emitted.
    #[must_use]
    pub fn eof_emitted(&self) -> bool {
        self.replay.eof_emitted
    }

    /// Additional context snapshot.
    #[must_use]
    pub fn context(&self) -> &CheckpointContext {
        &self.replay.context
    }

    /// Interpolation policy captured as identity, not as mutable replay state.
    #[must_use]
    pub fn parse_interpolation(&self) -> bool {
        self.identity.policy().interpolation_enabled()
    }

    /// Heredoc body-token policy captured as identity.
    #[must_use]
    pub fn emit_heredoc_body_tokens(&self) -> bool {
        self.identity.policy().emit_heredoc_body_tokens()
    }

    /// `qw` recovery policy captured as identity.
    #[must_use]
    pub fn qw_recovery_enabled(&self) -> bool {
        self.identity.policy().qw_recovery_enabled()
    }

    /// Whether this checkpoint was invalidated by an edit.
    #[must_use]
    pub fn is_invalidated(&self) -> bool {
        self.invalidated
    }

    /// Check whether this checkpoint is at the start of input.
    #[must_use]
    pub fn is_at_start(&self) -> bool {
        self.replay.position == 0
    }

    /// Whether restoring this checkpoint would re-enter a pending-heredoc
    /// timeout-sensitive path.
    #[must_use]
    pub fn is_timeout_sensitive(&self) -> bool {
        !self.replay.pending_heredocs.is_empty()
    }

    /// Whether behavior-bearing replay state differs from `other`.
    #[must_use]
    pub fn behavior_state_changed(&self, other: &Self) -> bool {
        self.replay != other.replay
    }

    pub(crate) fn replay(&self) -> &ReplayState {
        &self.replay
    }

    /// Calculate the difference between two checkpoints.
    #[must_use]
    pub fn diff(&self, other: &Self) -> super::CheckpointDiff {
        super::CheckpointDiff {
            position_delta: self.replay.position as isize - other.replay.position as isize,
            mode_changed: self.replay.mode != other.replay.mode,
            delimiter_stack_changed: self.replay.delimiter_stack != other.replay.delimiter_stack,
            prototype_state_changed: self.replay.in_prototype != other.replay.in_prototype
                || self.replay.prototype_depth != other.replay.prototype_depth
                || self.replay.after_sub != other.replay.after_sub
                || self.replay.after_arrow != other.replay.after_arrow
                || self.replay.hash_brace_depth != other.replay.hash_brace_depth
                || self.replay.after_var_subscript != other.replay.after_var_subscript
                || self.replay.paren_depth != other.replay.paren_depth,
            eof_state_changed: self.replay.eof_emitted != other.replay.eof_emitted,
            context_changed: self.replay.context != other.replay.context
                || self.replay.after_newline != other.replay.after_newline
                || self.replay.pending_heredocs != other.replay.pending_heredocs
                || self.replay.line_start_offset != other.replay.line_start_offset
                || self.replay.current_quote_op != other.replay.current_quote_op
                || self.identity.policy() != other.identity.policy(),
        }
    }

    /// Apply an edit to source-relative checkpoint offsets.
    ///
    /// Overlapping or shifted checkpoints are invalidated rather than rewritten
    /// as a default-state origin at the edit start. Call [`Self::try_apply_edit`]
    /// when the caller must branch on that result.
    pub fn apply_edit(&mut self, start: usize, old_len: usize, new_len: usize) {
        let _ = self.try_apply_edit(start, old_len, new_len);
    }

    /// Apply an edit and report whether the checkpoint remains a live restart.
    ///
    /// An edit overlapping a required state offset invalidates the checkpoint
    /// without fabricating default lexer state at the edit start. A shift of
    /// the replay position also fails closed because byte counts do not contain
    /// enough information to recompute line and column. An edit beginning
    /// exactly at an offset leaves it anchored so the new text is re-lexed.
    #[must_use]
    pub fn try_apply_edit(&mut self, start: usize, old_len: usize, new_len: usize) -> bool {
        if self.invalidated {
            return false;
        }
        let original_position = self.replay.position;
        let Some(position) = transform_offset(self.replay.position, start, old_len, new_len) else {
            self.invalidate();
            return false;
        };
        let Some(line_start_offset) =
            transform_offset(self.replay.line_start_offset, start, old_len, new_len)
        else {
            self.invalidate();
            return false;
        };

        let mut pending_heredocs = self.replay.pending_heredocs.clone();
        for pending in &mut pending_heredocs {
            let Some(body_start) = transform_offset(pending.body_start, start, old_len, new_len)
            else {
                self.invalidate();
                return false;
            };
            pending.body_start = body_start;
        }

        let mut current_quote_op = self.replay.current_quote_op.clone();
        if let Some(quote) = &mut current_quote_op {
            let Some(start_pos) = transform_offset(quote.start_pos, start, old_len, new_len) else {
                self.invalidate();
                return false;
            };
            quote.start_pos = start_pos;
        }

        let mut context = self.replay.context.clone();
        let context_valid = match &mut context {
            CheckpointContext::Format { start_position } => {
                transform_offset(*start_position, start, old_len, new_len)
                    .map(|shifted| *start_position = shifted)
                    .is_some()
            }
            CheckpointContext::Regex { flags_position, .. } => {
                flags_position.as_mut().is_none_or(|flags| {
                    transform_offset(*flags, start, old_len, new_len)
                        .map(|shifted| *flags = shifted)
                        .is_some()
                })
            }
            CheckpointContext::Normal
            | CheckpointContext::Heredoc { .. }
            | CheckpointContext::QuoteLike { .. } => true,
        };
        if !context_valid {
            self.invalidate();
            return false;
        }

        self.replay.position = position;
        self.replay.line_start_offset = line_start_offset;
        self.replay.pending_heredocs = pending_heredocs;
        self.replay.current_quote_op = current_quote_op;
        self.replay.context = context;
        self.replay.eof_emitted = false;
        if self.replay.position != original_position {
            // Prefix inserts/deletes shift the byte cursor but cannot rebuild
            // line/column or content identity. Keep transformed offsets for
            // inspection and refuse restore.
            self.invalidate();
            return false;
        }
        true
    }

    /// Rebind this prefix checkpoint to a new source generation after a
    /// validated edit that did not invalidate its consumed prefix.
    ///
    /// Offsets must already be valid for `source`. Policy identity is unchanged.
    pub fn rebind_to_source(
        &mut self,
        source: &str,
        generation: SourceGeneration,
    ) -> Result<(), CheckpointRestoreError> {
        if self.invalidated {
            return Err(CheckpointRestoreError::Invalidated);
        }
        self.ensure_complete()?;
        if !source.is_char_boundary(self.replay.position) {
            return Err(CheckpointRestoreError::InvalidUtf8Boundary);
        }
        if !self.offsets_valid_for(source) {
            return Err(CheckpointRestoreError::UnsupportedBoundary);
        }
        self.identity.retarget_content(source);
        self.identity.set_generation(generation);
        Ok(())
    }

    /// Replace only the logical-source component of identity.
    pub fn set_logical_source(&mut self, logical_source: Option<LogicalSourceId>) {
        self.identity.set_logical_source(logical_source);
    }

    /// Replace only the generation component of identity.
    pub fn set_generation(&mut self, generation: SourceGeneration) {
        self.identity.set_generation(generation);
    }

    /// Validate all source-relative checkpoint offsets for an input.
    #[must_use]
    pub fn is_valid_for(&self, input: &str) -> bool {
        !self.invalidated && self.offsets_valid_for(input)
    }

    fn offsets_valid_for(&self, input: &str) -> bool {
        offset_is_valid(input, self.replay.current_pos.byte)
            && offset_is_valid(input, self.replay.position)
            && offset_is_valid(input, self.replay.line_start_offset)
            && self.replay.line_start_offset <= self.replay.position
            && self
                .replay
                .pending_heredocs
                .iter()
                .all(|pending| offset_is_valid(input, pending.body_start))
            && self.replay.current_quote_op.as_ref().is_none_or(|quote| {
                offset_is_valid(input, quote.start_pos) && quote.start_pos <= self.replay.position
            })
            && match &self.replay.context {
                CheckpointContext::Format { start_position } => {
                    offset_is_valid(input, *start_position)
                }
                CheckpointContext::Regex { flags_position, .. } => {
                    flags_position.is_none_or(|position| offset_is_valid(input, position))
                }
                CheckpointContext::Normal
                | CheckpointContext::Heredoc { .. }
                | CheckpointContext::QuoteLike { .. } => true,
            }
    }

    pub(crate) fn ensure_complete(&self) -> Result<(), CheckpointRestoreError> {
        if self.invalidated {
            return Err(CheckpointRestoreError::Invalidated);
        }
        if !self.live_boundary {
            return Err(CheckpointRestoreError::UnsupportedBoundary);
        }
        if matches!(self.replay.mode, LexerMode::ExpectDelimiter)
            && self.replay.current_quote_op.is_none()
            && self.replay.delimiter_stack.is_empty()
        {
            return Err(CheckpointRestoreError::IncompleteState);
        }
        if let Some(quote) = &self.replay.current_quote_op
            && quote.start_pos > self.replay.position
        {
            return Err(CheckpointRestoreError::IncompleteState);
        }
        if matches!(self.replay.mode, LexerMode::InFormatBody)
            && !matches!(self.replay.context, CheckpointContext::Format { .. })
        {
            return Err(CheckpointRestoreError::IncompleteState);
        }
        Ok(())
    }

    fn invalidate(&mut self) {
        self.invalidated = true;
    }

    #[doc(hidden)]
    pub fn __test_stamp_position(&mut self, position: usize) {
        self.replay.position = position;
        self.replay.current_pos.byte = position;
    }

    #[doc(hidden)]
    pub fn __test_stamp_schema(&mut self, schema: u32) {
        self.identity.set_schema_for_test(schema);
    }

    #[doc(hidden)]
    pub fn __test_stamp_incomplete_quote(&mut self) {
        self.replay.mode = LexerMode::ExpectDelimiter;
        self.replay.current_quote_op = None;
        self.replay.delimiter_stack.clear();
    }

    #[doc(hidden)]
    pub fn __test_clear_quote_op_keep_delimiters(&mut self) {
        self.replay.current_quote_op = None;
        if self.replay.delimiter_stack.is_empty() {
            self.replay.delimiter_stack.push('{');
        }
        self.replay.mode = LexerMode::ExpectDelimiter;
    }
}

fn transform_offset(offset: usize, start: usize, old_len: usize, new_len: usize) -> Option<usize> {
    let old_end = start.saturating_add(old_len);
    if offset <= start {
        Some(offset)
    } else if offset >= old_end {
        Some(offset.saturating_sub(old_len).saturating_add(new_len))
    } else {
        None
    }
}

fn offset_is_valid(input: &str, offset: usize) -> bool {
    offset <= input.len() && input.is_char_boundary(offset)
}

impl Default for LexerCheckpoint {
    fn default() -> Self {
        Self::origin("")
    }
}

impl fmt::Display for LexerCheckpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Checkpoint@{} mode={:?} delims={} proto={} after_sub={} heredocs={}",
            self.replay.position,
            self.replay.mode,
            self.replay.delimiter_stack.len(),
            self.replay.in_prototype,
            self.replay.after_sub,
            self.replay.pending_heredocs.len()
        )
    }
}

/// Trait for lexers that support state checkpointing.
pub trait Checkpointable {
    /// Capture all mutable state required to replay tokenization from a live boundary.
    fn checkpoint(&self) -> LexerCheckpoint;

    /// Restore mutable replay state into a lexer for the target input.
    ///
    /// Failed restoration leaves the lexer unchanged.
    fn restore(&mut self, checkpoint: &LexerCheckpoint) -> Result<(), CheckpointRestoreError>;

    /// Validate restore identity and offsets without mutating the lexer.
    fn validate_restore(&self, checkpoint: &LexerCheckpoint) -> Result<(), CheckpointRestoreError>;

    /// Check whether every source-relative checkpoint offset is valid and identity matches.
    fn can_restore(&self, checkpoint: &LexerCheckpoint) -> bool {
        self.validate_restore(checkpoint).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::transform_offset;

    #[test]
    fn transform_offset_boundaries_and_overlap() {
        assert_eq!(transform_offset(9, 10, 5, 8), Some(9));
        assert_eq!(transform_offset(10, 10, 5, 8), Some(10));
        assert_eq!(transform_offset(11, 10, 5, 8), None);
        assert_eq!(transform_offset(14, 10, 5, 8), None);
        assert_eq!(transform_offset(15, 10, 5, 8), Some(18));
        assert_eq!(transform_offset(20, 10, 5, 8), Some(23));
        assert_eq!(transform_offset(0, 0, 0, 3), Some(0));
        assert_eq!(transform_offset(5, 0, 0, 3), Some(8));
        assert_eq!(transform_offset(2, 0, 5, 0), None);
        assert_eq!(transform_offset(5, 0, 5, 0), Some(0));
    }
}
