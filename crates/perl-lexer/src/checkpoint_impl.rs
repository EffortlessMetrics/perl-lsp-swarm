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
            content_digest: _,
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

        let content =
            self.content_digest.get_or_init(|| crate::checkpoint::compute_content_digest(input));
        let identity = LexerCheckpointIdentity::capture(
            content,
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
        // UTF-8 is a property of the target buffer vs the claimed byte, and must
        // remain distinguishable from "not a live boundary" even when a test
        // hook has already revoked live-boundary authority.
        if !self.input.is_char_boundary(checkpoint.position()) {
            return Err(CheckpointRestoreError::InvalidUtf8Boundary);
        }
        checkpoint.ensure_complete()?;
        checkpoint.identity().matches_target(
            || {
                self.content_digest
                    .get_or_init(|| crate::checkpoint::compute_content_digest(self.input))
            },
            &self.config,
            self.qw_recovery_enabled,
            self.emit_heredoc_body_tokens,
            self.logical_source.as_ref(),
            &self.generation,
        )?;
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
    fn stale_generation_fails_closed() -> Result<(), String> {
        let source = "my $x = 1;";
        let mut checkpoint_source = PerlLexer::new(source);
        checkpoint_source.bind_generation(SourceGeneration::known("1"));
        let checkpoint = checkpoint_source.checkpoint();
        let mut target = PerlLexer::new(source);
        target.bind_generation(SourceGeneration::known("2"));
        crate::checkpoint::reset_diagnostic_digest_counter();
        if target.validate_restore(&checkpoint) != Err(CheckpointRestoreError::WrongGeneration) {
            return Err("stale generation did not fail closed".to_string());
        }
        let (bytes, calls) = crate::checkpoint::diagnostic_digest_counter();
        if (bytes, calls) != (0, 0) {
            return Err(format!(
                "stale generation computed target digest: calls={calls} bytes={bytes}"
            ));
        }
        Ok(())
    }

    #[test]
    fn restore_reuses_target_content_digest() -> Result<(), String> {
        let source = "package RestoreCache;\nmy $value = 1;\n";
        let mut source_lexer = PerlLexer::new(source);
        source_lexer.next_token().ok_or_else(|| "source ended before checkpoint".to_string())?;
        source_lexer.bind_generation(SourceGeneration::known("restore-generation"));
        let checkpoint = source_lexer.checkpoint();

        crate::checkpoint::reset_diagnostic_digest_counter();
        let mut target = PerlLexer::new(source);
        target.bind_generation(SourceGeneration::known("restore-generation"));
        target.restore(&checkpoint).map_err(|error| error.to_string())?;
        target.restore(&checkpoint).map_err(|error| error.to_string())?;

        let (bytes, calls) = crate::checkpoint::diagnostic_digest_counter();
        if calls != 1 || bytes != source.len() {
            return Err(format!(
                "expected one target digest over {} bytes, observed calls={calls} bytes={bytes}",
                source.len()
            ));
        }
        Ok(())
    }

    #[test]
    fn checkpoint_identity_digest_is_cached_per_lexer() -> Result<(), String> {
        let mut previous_captures = None;
        for statements in [64, 128] {
            crate::checkpoint::reset_diagnostic_digest_counter();
            let source = "package Cached;\n".to_string() + &"my $value = 1;\n".repeat(statements);
            let mut lexer = PerlLexer::new(&source);
            lexer.next_token().ok_or_else(|| "source ended before first token".to_string())?;
            let (bytes, calls) = crate::checkpoint::diagnostic_digest_counter();
            if calls != 0 || bytes != 0 {
                return Err(format!(
                    "digest was computed during lexing without checkpoint: calls={calls} bytes={bytes}"
                ));
            }
            let mut captures: usize = 0;
            while lexer.next_token().is_some() {
                let _ = lexer.checkpoint();
                captures = captures.saturating_add(1);
            }
            lexer.bind_generation(SourceGeneration::known("fresh-generation"));
            let refreshed = lexer.checkpoint();
            if refreshed.identity().generation() != &SourceGeneration::known("fresh-generation") {
                return Err("checkpoint did not refresh generation metadata".to_string());
            }
            let (bytes, calls) = crate::checkpoint::diagnostic_digest_counter();
            if calls != 1 || bytes != source.len() {
                return Err(format!(
                    "expected one digest over {} bytes for {statements} statements, observed calls={calls} bytes={bytes} captures={captures}",
                    source.len()
                ));
            }
            if captures <= statements {
                return Err(format!(
                    "expected token-boundary captures above {statements}, got {captures}"
                ));
            }
            if let Some(previous) = previous_captures
                && captures <= previous
            {
                return Err(format!(
                    "doubled source family did not add captures: previous={previous} current={captures}"
                ));
            }
            previous_captures = Some(captures);
        }
        Ok(())
    }
}
