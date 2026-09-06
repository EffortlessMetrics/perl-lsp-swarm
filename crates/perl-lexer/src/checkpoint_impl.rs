use crate::checkpoint::{
    CheckpointContext, CheckpointRestoreError, Checkpointable, LexerCheckpointIdentity,
    PendingHeredocCheckpoint, QuoteOperatorCheckpoint, ReplayState,
};
use crate::heredoc::HeredocSpec;
use crate::quote_handler::QuoteOperatorInfo;
use crate::{LexerCheckpoint, LexerMode, PerlLexer};
use std::sync::Arc;

impl Checkpointable for PerlLexer<'_> {
    fn checkpoint(&self) -> LexerCheckpoint {
        // Exhaustive projection: adding a PerlLexer field without a disposition
        // here fails to compile. Identity, immutable input, and operation-local
        // work are named rather than restored as mutable replay state.
        let Self {
            input,
            input_bytes,
            position,
            mode,
            config,
            delimiter_stack,
            in_prototype,
            prototype_depth,
            after_sub,
            after_arrow,
            hash_brace_depth,
            after_var_subscript,
            paren_depth,
            current_pos,
            after_newline,
            pending_heredocs,
            line_start_offset,
            emit_heredoc_body_tokens,
            current_quote_op,
            qw_recovery_enabled,
            eof_emitted,
            scan_limit,
            logical_source,
            generation,
        } = self;
        let _ = input_bytes;
        let _ = scan_limit;

        let context = if matches!(mode, LexerMode::InFormatBody) {
            CheckpointContext::Format {
                // Format bodies are consumed atomically by `next_token`, so a
                // checkpoint can observe this mode only where body parsing begins.
                start_position: *position,
            }
        } else if !delimiter_stack.is_empty() {
            CheckpointContext::QuoteLike {
                operator: current_quote_op
                    .as_ref()
                    .map_or_else(String::new, |quote| quote.operator.clone()),
                delimiter: delimiter_stack.last().copied().unwrap_or('\0'),
                is_paired: true,
            }
        } else {
            CheckpointContext::Normal
        };

        let identity = LexerCheckpointIdentity::capture(
            input,
            config,
            *qw_recovery_enabled,
            *emit_heredoc_body_tokens,
            logical_source.clone(),
            generation.clone(),
        );
        let replay = ReplayState {
            position: *position,
            mode: *mode,
            delimiter_stack: delimiter_stack.clone(),
            in_prototype: *in_prototype,
            prototype_depth: *prototype_depth,
            after_sub: *after_sub,
            after_arrow: *after_arrow,
            hash_brace_depth: *hash_brace_depth,
            after_var_subscript: *after_var_subscript,
            paren_depth: *paren_depth,
            current_pos: *current_pos,
            after_newline: *after_newline,
            pending_heredocs: pending_heredocs
                .iter()
                .map(|pending| PendingHeredocCheckpoint {
                    label: pending.label.to_string(),
                    body_start: pending.body_start,
                    allow_indent: pending.allow_indent,
                    interpolates: pending.interpolates,
                })
                .collect(),
            line_start_offset: *line_start_offset,
            current_quote_op: current_quote_op.as_ref().map(|quote| QuoteOperatorCheckpoint {
                operator: quote.operator.clone(),
                delimiter: quote.delimiter,
                start_pos: quote.start_pos,
            }),
            eof_emitted: *eof_emitted,
            context,
        };
        LexerCheckpoint::from_live(identity, replay)
    }

    fn validate_restore(&self, checkpoint: &LexerCheckpoint) -> Result<(), CheckpointRestoreError> {
        checkpoint.ensure_complete()?;
        checkpoint.identity().matches_target(
            self.input,
            &self.config,
            self.qw_recovery_enabled,
            self.emit_heredoc_body_tokens,
            self.logical_source.as_ref(),
            &self.generation,
        )?;
        if !self.input.is_char_boundary(checkpoint.position()) {
            return Err(CheckpointRestoreError::InvalidUtf8Boundary);
        }
        if !checkpoint.is_valid_for(self.input) {
            return Err(CheckpointRestoreError::UnsupportedBoundary);
        }
        Ok(())
    }

    fn restore(&mut self, checkpoint: &LexerCheckpoint) -> Result<(), CheckpointRestoreError> {
        self.validate_restore(checkpoint)?;
        let replay = checkpoint.replay();
        self.position = replay.position;
        self.mode = replay.mode;
        self.delimiter_stack.clone_from(&replay.delimiter_stack);
        self.in_prototype = replay.in_prototype;
        self.prototype_depth = replay.prototype_depth;
        self.after_sub = replay.after_sub;
        self.after_arrow = replay.after_arrow;
        self.hash_brace_depth = replay.hash_brace_depth;
        self.after_var_subscript = replay.after_var_subscript;
        self.paren_depth = replay.paren_depth;
        self.current_pos = replay.current_pos;
        self.after_newline = replay.after_newline;
        self.pending_heredocs = replay
            .pending_heredocs
            .iter()
            .map(|pending| HeredocSpec {
                label: Arc::from(pending.label.as_str()),
                body_start: pending.body_start,
                allow_indent: pending.allow_indent,
                interpolates: pending.interpolates,
            })
            .collect();
        self.line_start_offset = replay.line_start_offset;
        self.current_quote_op = replay.current_quote_op.as_ref().map(|quote| QuoteOperatorInfo {
            operator: quote.operator.clone(),
            delimiter: quote.delimiter,
            start_pos: quote.start_pos,
        });
        self.eof_emitted = replay.eof_emitted;
        self.scan_limit = None;
        if matches!(replay.context, CheckpointContext::Format { .. }) {
            self.mode = LexerMode::InFormatBody;
        }
        Ok(())
    }
}

impl PerlLexer<'_> {
    /// Bind a logical source identity used by subsequent checkpoints.
    pub fn bind_logical_source(&mut self, logical_source: perl_source_identity::LogicalSourceId) {
        self.logical_source = Some(logical_source);
    }

    /// Bind a source generation used by subsequent checkpoints.
    pub fn bind_generation(&mut self, generation: perl_source_identity::SourceGeneration) {
        self.generation = generation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Position;
    use crate::checkpoint::CheckpointContext;
    use perl_source_identity::SourceGeneration;

    type TestResult = std::result::Result<(), String>;

    fn format_start(checkpoint: &LexerCheckpoint) -> std::result::Result<usize, String> {
        match checkpoint.context() {
            CheckpointContext::Format { start_position } => Ok(*start_position),
            context => Err(format!("expected format checkpoint context, got {context:?}")),
        }
    }

    #[test]
    fn format_checkpoint_records_exact_short_prefix_start() -> TestResult {
        let prefix = "name: ";
        let mut lexer = PerlLexer::new("name: body\n.\n");
        lexer.position = prefix.len();
        lexer.enter_format_mode();

        let checkpoint = lexer.checkpoint();
        let actual = format_start(&checkpoint)?;
        if actual != prefix.len() {
            return Err(format!(
                "format start {actual} did not match prefix length {}",
                prefix.len()
            ));
        }
        Ok(())
    }

    #[test]
    fn format_checkpoint_records_exact_start_after_long_prefix() -> TestResult {
        let prefix = "x".repeat(160);
        let input = format!("{prefix}body\n.\n");
        let mut lexer = PerlLexer::new(&input);
        lexer.position = prefix.len();
        lexer.enter_format_mode();

        let checkpoint = lexer.checkpoint();
        let actual = format_start(&checkpoint)?;
        if actual != prefix.len() {
            return Err(format!(
                "format start {actual} did not match prefix length {}",
                prefix.len()
            ));
        }
        Ok(())
    }

    #[test]
    fn restore_preserves_format_start_and_non_format_context() -> TestResult {
        let prefix = "before format\n";
        let mut lexer = PerlLexer::new("before format\nbody\n.\n");
        lexer.position = prefix.len();
        lexer.enter_format_mode();
        let format_checkpoint = lexer.checkpoint();

        lexer.set_mode(LexerMode::ExpectTerm);
        lexer.position = 0;
        lexer.restore(&format_checkpoint).map_err(|error| error.to_string())?;
        let restored = lexer.checkpoint();
        let actual = format_start(&restored)?;
        if actual != prefix.len() {
            return Err(format!("restored format start {actual} did not match {}", prefix.len()));
        }

        lexer.set_mode(LexerMode::ExpectTerm);
        if !matches!(lexer.checkpoint().context(), CheckpointContext::Normal) {
            return Err("non-format mode retained format checkpoint context".to_string());
        }
        Ok(())
    }

    #[test]
    fn restore_round_trip_preserves_every_mutable_replay_field() -> TestResult {
        let input = "x".repeat(96);
        let mut lexer = PerlLexer::new(&input);
        lexer.position = 32;
        lexer.mode = LexerMode::ExpectOperator;
        lexer.delimiter_stack = vec!['{', '('];
        lexer.in_prototype = true;
        lexer.prototype_depth = 2;
        lexer.after_sub = true;
        lexer.after_arrow = true;
        lexer.hash_brace_depth = 3;
        lexer.after_var_subscript = true;
        lexer.paren_depth = 4;
        lexer.current_pos = Position { byte: 32, line: 3, column: 5 };
        lexer.after_newline = false;
        lexer.pending_heredocs = vec![HeredocSpec {
            label: Arc::from("END"),
            body_start: 48,
            allow_indent: true,
            interpolates: true,
        }];
        lexer.line_start_offset = 24;
        lexer.emit_heredoc_body_tokens = true;
        lexer.current_quote_op =
            Some(QuoteOperatorInfo { operator: "s".to_string(), delimiter: '{', start_pos: 28 });
        lexer.qw_recovery_enabled = false;
        lexer.eof_emitted = true;

        let expected = lexer.checkpoint();
        let mut restored = PerlLexer::new(&input);
        restored.emit_heredoc_body_tokens = true;
        restored.qw_recovery_enabled = false;
        restored.restore(&expected).map_err(|error| error.to_string())?;
        if restored.checkpoint() != expected {
            return Err("restored checkpoint diverged from captured replay state".to_string());
        }
        Ok(())
    }

    #[test]
    fn failed_restore_leaves_lexer_unchanged() -> TestResult {
        let mut lexer = PerlLexer::new("my $x = 1;");
        let _ = lexer.next_token();
        let before = lexer.checkpoint();
        let mut other = PerlLexer::new("my $y = 2;");
        match other.restore(&before) {
            Err(CheckpointRestoreError::WrongContent) => {}
            other => {
                return Err(format!(
                    "different content of the same length must fail closed, got {other:?}"
                ));
            }
        }
        if other.position != 0 || other.checkpoint().position() != 0 {
            return Err("failed restore mutated the target lexer".to_string());
        }
        Ok(())
    }

    #[test]
    fn stale_generation_fails_closed() {
        let source = "my $x = 1;";
        let mut lexer = PerlLexer::new(source);
        lexer.bind_generation(SourceGeneration::known("1"));
        let checkpoint = lexer.checkpoint();
        lexer.bind_generation(SourceGeneration::known("2"));
        assert_eq!(
            lexer.validate_restore(&checkpoint),
            Err(CheckpointRestoreError::WrongGeneration)
        );
    }
}
