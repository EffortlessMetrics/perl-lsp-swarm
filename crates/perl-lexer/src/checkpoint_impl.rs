use crate::checkpoint::{Checkpointable, PendingHeredocCheckpoint, QuoteOperatorCheckpoint};
use crate::heredoc::HeredocSpec;
use crate::quote_handler::QuoteOperatorInfo;
use crate::{LexerCheckpoint, LexerMode, PerlLexer, checkpoint};
use std::sync::Arc;

impl Checkpointable for PerlLexer<'_> {
    fn checkpoint(&self) -> LexerCheckpoint {
        use checkpoint::CheckpointContext;

        let context = if matches!(self.mode, LexerMode::InFormatBody) {
            CheckpointContext::Format {
                // Format bodies are consumed atomically by `next_token`, so a
                // checkpoint can observe this mode only where body parsing begins.
                start_position: self.position,
            }
        } else if !self.delimiter_stack.is_empty() {
            CheckpointContext::QuoteLike {
                operator: self
                    .current_quote_op
                    .as_ref()
                    .map_or_else(String::new, |quote| quote.operator.clone()),
                delimiter: self.delimiter_stack.last().copied().unwrap_or('\0'),
                is_paired: true,
            }
        } else {
            CheckpointContext::Normal
        };

        LexerCheckpoint {
            position: self.position,
            mode: self.mode,
            delimiter_stack: self.delimiter_stack.clone(),
            in_prototype: self.in_prototype,
            prototype_depth: self.prototype_depth,
            after_sub: self.after_sub,
            after_arrow: self.after_arrow,
            hash_brace_depth: self.hash_brace_depth,
            after_var_subscript: self.after_var_subscript,
            paren_depth: self.paren_depth,
            current_pos: self.current_pos,
            after_newline: self.after_newline,
            pending_heredocs: self
                .pending_heredocs
                .iter()
                .map(|pending| PendingHeredocCheckpoint {
                    label: pending.label.to_string(),
                    body_start: pending.body_start,
                    allow_indent: pending.allow_indent,
                })
                .collect(),
            line_start_offset: self.line_start_offset,
            emit_heredoc_body_tokens: self.emit_heredoc_body_tokens,
            current_quote_op: self.current_quote_op.as_ref().map(|quote| QuoteOperatorCheckpoint {
                operator: quote.operator.clone(),
                delimiter: quote.delimiter,
                start_pos: quote.start_pos,
            }),
            qw_recovery_enabled: self.qw_recovery_enabled,
            eof_emitted: self.eof_emitted,
            context,
        }
    }

    fn restore(&mut self, checkpoint: &LexerCheckpoint) {
        self.position = checkpoint.position;
        self.mode = checkpoint.mode;
        self.delimiter_stack.clone_from(&checkpoint.delimiter_stack);
        self.in_prototype = checkpoint.in_prototype;
        self.prototype_depth = checkpoint.prototype_depth;
        self.after_sub = checkpoint.after_sub;
        self.after_arrow = checkpoint.after_arrow;
        self.hash_brace_depth = checkpoint.hash_brace_depth;
        self.after_var_subscript = checkpoint.after_var_subscript;
        self.paren_depth = checkpoint.paren_depth;
        self.current_pos = checkpoint.current_pos;
        self.after_newline = checkpoint.after_newline;
        self.pending_heredocs = checkpoint
            .pending_heredocs
            .iter()
            .map(|pending| HeredocSpec {
                label: Arc::from(pending.label.as_str()),
                body_start: pending.body_start,
                allow_indent: pending.allow_indent,
            })
            .collect();
        self.line_start_offset = checkpoint.line_start_offset;
        self.emit_heredoc_body_tokens = checkpoint.emit_heredoc_body_tokens;
        self.current_quote_op =
            checkpoint.current_quote_op.as_ref().map(|quote| QuoteOperatorInfo {
                operator: quote.operator.clone(),
                delimiter: quote.delimiter,
                start_pos: quote.start_pos,
            });
        self.qw_recovery_enabled = checkpoint.qw_recovery_enabled;
        self.eof_emitted = checkpoint.eof_emitted;

        use checkpoint::CheckpointContext;
        if matches!(checkpoint.context, CheckpointContext::Format { .. }) {
            self.mode = LexerMode::InFormatBody;
        }
    }

    fn can_restore(&self, checkpoint: &LexerCheckpoint) -> bool {
        checkpoint.is_valid_for(self.input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Position;
    use crate::checkpoint::CheckpointContext;

    type TestResult = std::result::Result<(), String>;

    fn format_start(checkpoint: &LexerCheckpoint) -> std::result::Result<usize, String> {
        match &checkpoint.context {
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
        lexer.restore(&format_checkpoint);
        let restored = lexer.checkpoint();
        let actual = format_start(&restored)?;
        if actual != prefix.len() {
            return Err(format!("restored format start {actual} did not match {}", prefix.len()));
        }

        lexer.set_mode(LexerMode::ExpectTerm);
        if !matches!(lexer.checkpoint().context, CheckpointContext::Normal) {
            return Err("non-format mode retained format checkpoint context".to_string());
        }
        Ok(())
    }

    #[test]
    fn restore_round_trip_preserves_every_mutable_replay_field() {
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
        lexer.pending_heredocs =
            vec![HeredocSpec { label: Arc::from("END"), body_start: 48, allow_indent: true }];
        lexer.line_start_offset = 24;
        lexer.emit_heredoc_body_tokens = true;
        lexer.current_quote_op =
            Some(QuoteOperatorInfo { operator: "s".to_string(), delimiter: '{', start_pos: 28 });
        lexer.qw_recovery_enabled = false;
        lexer.eof_emitted = true;

        let expected = lexer.checkpoint();
        let mut restored = PerlLexer::new(&input);
        restored.restore(&expected);

        assert_eq!(restored.checkpoint(), expected);
    }
}
