use crate::{LexerMode, Position};
use std::fmt;

/// Replay-safe representation of one queued heredoc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingHeredocCheckpoint {
    /// Heredoc terminator label.
    pub label: String,
    /// Byte offset where the heredoc body begins.
    pub body_start: usize,
    /// Whether `<<~` indentation is allowed.
    pub allow_indent: bool,
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

/// A checkpoint that captures all mutable lexer state needed for token replay.
///
/// Input references and the wall-clock timeout origin are deliberately not
/// persisted. Restore targets supply the edited input, retain their configured
/// lexer policy, and begin a fresh operation-local timeout budget.
#[derive(Debug, Clone, PartialEq)]
pub struct LexerCheckpoint {
    /// Current position in the input.
    pub position: usize,
    /// Current lexer mode (`ExpectTerm`, `ExpectOperator`, etc.).
    pub mode: LexerMode,
    /// Stack for nested delimiters in `s{}{} ` constructs.
    pub delimiter_stack: Vec<char>,
    /// Whether the lexer is inside prototype parentheses after `sub`.
    pub in_prototype: bool,
    /// Parenthesis depth used to detect the end of a prototype.
    pub prototype_depth: usize,
    /// Whether `sub` was just emitted and a prototype may follow.
    pub after_sub: bool,
    /// Whether `->` was just emitted, suppressing quote-op interpretation.
    pub after_arrow: bool,
    /// Depth of hash-subscript brace nesting.
    pub hash_brace_depth: usize,
    /// Whether the lexer just emitted a complete variable token.
    pub after_var_subscript: bool,
    /// Depth of open parentheses used by heredoc/bitshift disambiguation.
    pub paren_depth: usize,
    /// Current position with line/column tracking.
    pub current_pos: Position,
    /// Whether the previous consumed source unit ended a line.
    pub after_newline: bool,
    /// Ordered heredoc queue waiting for body consumption.
    pub pending_heredocs: Vec<PendingHeredocCheckpoint>,
    /// Byte offset of the current physical line start.
    pub line_start_offset: usize,
    /// Whether heredoc body tokens are emitted instead of consumed virtually.
    pub emit_heredoc_body_tokens: bool,
    /// In-progress quote-operator metadata, when present.
    pub current_quote_op: Option<QuoteOperatorCheckpoint>,
    /// Whether malformed `qw` constructs use the recovery path.
    pub qw_recovery_enabled: bool,
    /// Whether the terminal EOF token has already been emitted.
    pub eof_emitted: bool,
    /// Additional context for complex states.
    pub context: CheckpointContext,
}

/// Additional context that may be needed for certain lexer states.
#[derive(Debug, Clone, PartialEq)]
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

impl LexerCheckpoint {
    /// Create a checkpoint with the default lexer state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            position: 0,
            mode: LexerMode::ExpectTerm,
            delimiter_stack: Vec::new(),
            in_prototype: false,
            prototype_depth: 0,
            after_sub: false,
            after_arrow: false,
            hash_brace_depth: 0,
            after_var_subscript: false,
            paren_depth: 0,
            current_pos: Position::start(),
            after_newline: true,
            pending_heredocs: Vec::new(),
            line_start_offset: 0,
            emit_heredoc_body_tokens: false,
            current_quote_op: None,
            qw_recovery_enabled: true,
            eof_emitted: false,
            context: CheckpointContext::Normal,
        }
    }

    /// Create a default-state checkpoint at a specific position.
    #[must_use]
    pub fn at_position(position: usize) -> Self {
        Self { position, ..Self::new() }
    }

    /// Check whether this checkpoint is at the start of input.
    #[must_use]
    pub fn is_at_start(&self) -> bool {
        self.position == 0
    }

    /// Calculate the difference between two checkpoints.
    #[must_use]
    pub fn diff(&self, other: &Self) -> super::CheckpointDiff {
        super::CheckpointDiff {
            position_delta: self.position as isize - other.position as isize,
            mode_changed: self.mode != other.mode,
            delimiter_stack_changed: self.delimiter_stack != other.delimiter_stack,
            prototype_state_changed: self.in_prototype != other.in_prototype
                || self.prototype_depth != other.prototype_depth
                || self.after_sub != other.after_sub
                || self.after_arrow != other.after_arrow
                || self.hash_brace_depth != other.hash_brace_depth
                || self.after_var_subscript != other.after_var_subscript
                || self.paren_depth != other.paren_depth,
            eof_state_changed: self.eof_emitted != other.eof_emitted,
            context_changed: self.context != other.context
                || self.after_newline != other.after_newline
                || self.pending_heredocs != other.pending_heredocs
                || self.line_start_offset != other.line_start_offset
                || self.emit_heredoc_body_tokens != other.emit_heredoc_body_tokens
                || self.current_quote_op != other.current_quote_op
                || self.qw_recovery_enabled != other.qw_recovery_enabled,
        }
    }

    /// Apply an edit to source-relative checkpoint offsets.
    ///
    /// Invalidated checkpoints retain the historical behavior of rewinding to
    /// `start`. Call [`Self::try_apply_edit`] when the caller must distinguish a
    /// transformed checkpoint from a conservative reset.
    pub fn apply_edit(&mut self, start: usize, old_len: usize, new_len: usize) {
        let _ = self.try_apply_edit(start, old_len, new_len);
    }

    /// Apply an edit and report whether all required replay state survived.
    ///
    /// An edit overlapping the replay position or another required state offset
    /// invalidates the checkpoint and rewinds it to `start`, returning `false`.
    /// Offsets after the replaced range are shifted. An edit beginning exactly
    /// at an offset leaves it anchored before the replacement so the new text is
    /// re-lexed.
    #[must_use]
    pub fn try_apply_edit(&mut self, start: usize, old_len: usize, new_len: usize) -> bool {
        let original_position = self.position;
        let Some(position) = transform_offset(self.position, start, old_len, new_len) else {
            self.invalidate_at(start);
            return false;
        };
        let Some(line_start_offset) =
            transform_offset(self.line_start_offset, start, old_len, new_len)
        else {
            self.invalidate_at(start);
            return false;
        };

        let mut pending_heredocs = self.pending_heredocs.clone();
        for pending in &mut pending_heredocs {
            let Some(body_start) = transform_offset(pending.body_start, start, old_len, new_len)
            else {
                self.invalidate_at(start);
                return false;
            };
            pending.body_start = body_start;
        }

        let mut current_quote_op = self.current_quote_op.clone();
        if let Some(quote) = &mut current_quote_op {
            let Some(start_pos) = transform_offset(quote.start_pos, start, old_len, new_len) else {
                self.invalidate_at(start);
                return false;
            };
            quote.start_pos = start_pos;
        }

        let mut context = self.context.clone();
        let context_valid = match &mut context {
            CheckpointContext::Format { start_position } => {
                transform_offset(*start_position, start, old_len, new_len)
                    .map(|shifted| *start_position = shifted)
                    .is_some()
            }
            CheckpointContext::Regex { flags_position, .. } => flags_position.as_mut().is_none_or(
                |flags| {
                    transform_offset(*flags, start, old_len, new_len)
                        .map(|shifted| *flags = shifted)
                        .is_some()
                },
            ),
            CheckpointContext::Normal
            | CheckpointContext::Heredoc { .. }
            | CheckpointContext::QuoteLike { .. } => true,
        };
        if !context_valid {
            self.invalidate_at(start);
            return false;
        }

        self.position = position;
        self.line_start_offset = line_start_offset;
        self.pending_heredocs = pending_heredocs;
        self.current_quote_op = current_quote_op;
        self.context = context;
        self.eof_emitted = false;
        if self.position != original_position {
            self.current_pos = Position::start();
        }
        true
    }

    /// Validate all source-relative checkpoint offsets for an input.
    #[must_use]
    pub fn is_valid_for(&self, input: &str) -> bool {
        offset_is_valid(input, self.position)
            && offset_is_valid(input, self.line_start_offset)
            && self.line_start_offset <= self.position
            && self
                .pending_heredocs
                .iter()
                .all(|pending| offset_is_valid(input, pending.body_start))
            && self.current_quote_op.as_ref().is_none_or(|quote| {
                offset_is_valid(input, quote.start_pos) && quote.start_pos <= self.position
            })
            && match &self.context {
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

    fn invalidate_at(&mut self, start: usize) {
        let mut reset = Self::new();
        reset.position = start;
        *self = reset;
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
        Self::new()
    }
}

impl fmt::Display for LexerCheckpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Checkpoint@{} mode={:?} delims={} proto={} after_sub={} heredocs={}",
            self.position,
            self.mode,
            self.delimiter_stack.len(),
            self.in_prototype,
            self.after_sub,
            self.pending_heredocs.len()
        )
    }
}

/// Trait for lexers that support state checkpointing.
pub trait Checkpointable {
    /// Capture all mutable state required to replay tokenization.
    fn checkpoint(&self) -> LexerCheckpoint;

    /// Restore mutable replay state into a lexer for the target input.
    ///
    /// The target lexer retains its configured policy and fresh timeout origin.
    fn restore(&mut self, checkpoint: &LexerCheckpoint);

    /// Check whether every source-relative checkpoint offset is valid.
    fn can_restore(&self, checkpoint: &LexerCheckpoint) -> bool;
}
