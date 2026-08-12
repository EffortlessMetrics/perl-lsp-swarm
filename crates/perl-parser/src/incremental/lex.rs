use crate::incremental::LineIndex;
use crate::incremental::checkpoint::LexCheckpoint;
use crate::incremental::edit::Edit;
use anyhow::Result;
use perl_lexer::{
    Checkpointable, LexerCheckpoint as LiveLexerCheckpoint, PerlLexer, Token, TokenType,
};

/// Hard cap for complete behavior-bearing checkpoints retained in one state.
pub const MAX_STORED_LEX_CHECKPOINTS: usize = 4096;
const INITIAL_CHECKPOINT_SPACING: usize = 128;

/// One complete checkpoint bound to the exact source generation that produced it.
#[derive(Clone)]
pub(crate) struct StoredLexCheckpoint {
    pub(crate) summary: LexCheckpoint,
    pub(crate) live: LiveLexerCheckpoint,
    source_fingerprint: u64,
    prefix_fingerprint: u64,
}

impl StoredLexCheckpoint {
    fn capture(source: &str, live: LiveLexerCheckpoint, line_index: &LineIndex) -> Self {
        let summary = summarize_checkpoint(&live, line_index);
        let prefix = source.get(..live.position).unwrap_or_default();
        Self {
            summary,
            live,
            source_fingerprint: fingerprint(source.as_bytes()),
            prefix_fingerprint: fingerprint(prefix.as_bytes()),
        }
    }

    fn belongs_to_source(&self, source: &str) -> bool {
        if self.live.position > source.len() || !source.is_char_boundary(self.live.position) {
            return false;
        }
        let Some(prefix) = source.get(..self.live.position) else {
            return false;
        };
        self.source_fingerprint == fingerprint(source.as_bytes())
            && self.prefix_fingerprint == fingerprint(prefix.as_bytes())
            && self.live.is_valid_for(source)
    }

    /// Clone and transform the complete state for one old-generation edit.
    pub(crate) fn prepare_for_edit(
        &self,
        old_source: &str,
        edit: &Edit,
    ) -> Option<LiveLexerCheckpoint> {
        if !self.belongs_to_source(old_source)
            || self.summary.byte > edit.start_byte
            || self.live.is_timeout_sensitive()
        {
            return None;
        }
        let old_len = edit.old_end_byte.checked_sub(edit.start_byte)?;
        let mut live = self.live.clone();
        live.try_apply_edit(edit.start_byte, old_len, edit.new_text.len()).then_some(live)
    }

    /// Carry an old prefix checkpoint into the edited generation when all
    /// behavior-bearing offsets and prefix bytes remain valid.
    pub(crate) fn transform_for_generation(
        &self,
        old_source: &str,
        new_source: &str,
        edit: &Edit,
    ) -> Option<Self> {
        if !self.belongs_to_source(old_source) || self.summary.byte >= edit.start_byte {
            return None;
        }
        let old_len = edit.old_end_byte.checked_sub(edit.start_byte)?;
        let mut live = self.live.clone();
        if !live.try_apply_edit(edit.start_byte, old_len, edit.new_text.len())
            || !live.is_valid_for(new_source)
        {
            return None;
        }
        let prefix = new_source.get(..live.position)?;
        if fingerprint(prefix.as_bytes()) != self.prefix_fingerprint {
            return None;
        }
        Some(Self {
            summary: self.summary,
            live,
            source_fingerprint: fingerprint(new_source.as_bytes()),
            prefix_fingerprint: self.prefix_fingerprint,
        })
    }
}

pub(crate) struct LexedSource {
    pub(crate) tokens: Vec<Token>,
    pub(crate) checkpoints: Vec<LexCheckpoint>,
    pub(crate) stored_checkpoints: Vec<StoredLexCheckpoint>,
    #[cfg(test)]
    pub(crate) live_checkpoints: Vec<LiveLexerCheckpoint>,
}

fn fingerprint(bytes: &[u8]) -> u64 {
    let mut value = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x0000_0100_0000_01b3);
    }
    value
}

fn summarize_checkpoint(checkpoint: &LiveLexerCheckpoint, line_index: &LineIndex) -> LexCheckpoint {
    let (line, column) = line_index.byte_to_position(checkpoint.position);
    LexCheckpoint { byte: checkpoint.position, mode: checkpoint.mode, line, column }
}

fn push_summary(
    summaries: &mut Vec<LexCheckpoint>,
    checkpoint: &LiveLexerCheckpoint,
    line_index: &LineIndex,
) {
    if summaries.last().is_none_or(|summary| summary.byte != checkpoint.position) {
        summaries.push(summarize_checkpoint(checkpoint, line_index));
    }
}

fn behavior_state_changed(
    previous: &LiveLexerCheckpoint,
    current: &LiveLexerCheckpoint,
) -> bool {
    previous.mode != current.mode
        || previous.delimiter_stack != current.delimiter_stack
        || previous.in_prototype != current.in_prototype
        || previous.prototype_depth != current.prototype_depth
        || previous.after_sub != current.after_sub
        || previous.after_arrow != current.after_arrow
        || previous.hash_brace_depth != current.hash_brace_depth
        || previous.after_var_subscript != current.after_var_subscript
        || previous.paren_depth != current.paren_depth
        || previous.after_newline != current.after_newline
        || previous.pending_heredocs != current.pending_heredocs
        || previous.line_start_offset != current.line_start_offset
        || previous.emit_heredoc_body_tokens != current.emit_heredoc_body_tokens
        || previous.current_quote_op != current.current_quote_op
        || previous.qw_recovery_enabled != current.qw_recovery_enabled
        || previous.eof_emitted != current.eof_emitted
        || previous.context != current.context
}

fn compact_stored_checkpoints(checkpoints: &mut Vec<StoredLexCheckpoint>) {
    let original = std::mem::take(checkpoints);
    let original_len = original.len();
    checkpoints.extend(original.into_iter().enumerate().filter_map(|(index, checkpoint)| {
        (index == 0 || index % 2 == 0 || index + 1 == original_len).then_some(checkpoint)
    }));
}

fn push_stored_checkpoint(
    source: &str,
    line_index: &LineIndex,
    checkpoints: &mut Vec<StoredLexCheckpoint>,
    spacing: &mut usize,
    live: &LiveLexerCheckpoint,
) {
    if checkpoints.last().is_some_and(|stored| stored.live.position == live.position) {
        return;
    }

    let retain = checkpoints.last().is_none_or(|previous| {
        live.position.saturating_sub(previous.live.position) >= *spacing
            || behavior_state_changed(&previous.live, live)
    });
    if !retain {
        return;
    }

    if checkpoints.len() >= MAX_STORED_LEX_CHECKPOINTS {
        compact_stored_checkpoints(checkpoints);
        *spacing = spacing.saturating_mul(2).max(1);
    }
    checkpoints.push(StoredLexCheckpoint::capture(source, live.clone(), line_index));
}

/// Lex one complete source and capture complete, generation-bound restart state.
pub(crate) fn lex_source_with_checkpoints(source: &str, line_index: &LineIndex) -> LexedSource {
    let mut lexer = PerlLexer::new(source);
    let mut tokens = Vec::new();
    let mut checkpoints = Vec::new();
    let mut stored_checkpoints = Vec::new();
    let mut checkpoint_spacing = INITIAL_CHECKPOINT_SPACING;
    #[cfg(test)]
    let mut live_checkpoints = Vec::new();

    loop {
        let live = lexer.checkpoint();
        push_summary(&mut checkpoints, &live, line_index);
        push_stored_checkpoint(
            source,
            line_index,
            &mut stored_checkpoints,
            &mut checkpoint_spacing,
            &live,
        );
        #[cfg(test)]
        live_checkpoints.push(live);

        let Some(token) = lexer.next_token() else {
            break;
        };
        if token.token_type == TokenType::EOF {
            let terminal = lexer.checkpoint();
            push_summary(&mut checkpoints, &terminal, line_index);
            push_stored_checkpoint(
                source,
                line_index,
                &mut stored_checkpoints,
                &mut checkpoint_spacing,
                &terminal,
            );
            break;
        }
        tokens.push(token);
    }

    LexedSource {
        tokens,
        checkpoints,
        stored_checkpoints,
        #[cfg(test)]
        live_checkpoints,
    }
}

/// Restore complete mutable state into the edited source and re-lex to EOF.
pub(crate) fn lex_from_live_checkpoint(
    source: &str,
    line_index: &LineIndex,
    checkpoint: &LiveLexerCheckpoint,
) -> Result<LexedSource> {
    let mut lexer = PerlLexer::new(source);
    if !lexer.can_restore(checkpoint) {
        anyhow::bail!("stored lexer checkpoint is not valid for the edited source");
    }
    lexer.restore(checkpoint);

    let mut tokens = Vec::new();
    let mut checkpoints = Vec::new();
    let mut stored_checkpoints = Vec::new();
    let mut checkpoint_spacing = INITIAL_CHECKPOINT_SPACING;
    #[cfg(test)]
    let mut live_checkpoints = Vec::new();
    let mut last_position = checkpoint.position;

    loop {
        let live = lexer.checkpoint();
        push_summary(&mut checkpoints, &live, line_index);
        push_stored_checkpoint(
            source,
            line_index,
            &mut stored_checkpoints,
            &mut checkpoint_spacing,
            &live,
        );
        #[cfg(test)]
        live_checkpoints.push(live);

        let Some(token) = lexer.next_token() else {
            break;
        };
        if token.token_type == TokenType::EOF {
            let terminal = lexer.checkpoint();
            push_summary(&mut checkpoints, &terminal, line_index);
            push_stored_checkpoint(
                source,
                line_index,
                &mut stored_checkpoints,
                &mut checkpoint_spacing,
                &terminal,
            );
            break;
        }
        if token.end <= last_position {
            anyhow::bail!("incremental lexer did not advance at byte {}", token.start);
        }
        last_position = token.end;
        tokens.push(token);
    }

    Ok(LexedSource {
        tokens,
        checkpoints,
        stored_checkpoints,
        #[cfg(test)]
        live_checkpoints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_checkpoint_rejects_a_different_source_generation() -> Result<()> {
        let source = "my $value = 1;";
        let line_index = LineIndex::new(source);
        let lexed = lex_source_with_checkpoints(source, &line_index);
        let stored = lexed
            .stored_checkpoints
            .first()
            .ok_or_else(|| anyhow::anyhow!("origin checkpoint is missing"))?;
        let edit = Edit {
            start_byte: source.len(),
            old_end_byte: source.len(),
            new_end_byte: source.len(),
            new_text: String::new(),
        };

        assert!(stored.prepare_for_edit("my $value = 2;", &edit).is_none());
        Ok(())
    }

    #[test]
    fn stored_checkpoint_set_is_bounded_and_retains_a_late_boundary() -> Result<()> {
        let source = (0..20_000).map(|index| format!("my $v{index} = {index};\n")).collect::<String>();
        let line_index = LineIndex::new(&source);
        let lexed = lex_source_with_checkpoints(&source, &line_index);

        assert!(!lexed.stored_checkpoints.is_empty());
        assert!(lexed.stored_checkpoints.len() <= MAX_STORED_LEX_CHECKPOINTS);
        let last = lexed
            .stored_checkpoints
            .last()
            .ok_or_else(|| anyhow::anyhow!("last checkpoint is missing"))?;
        assert!(last.summary.byte > source.len() / 2);
        Ok(())
    }

    #[test]
    fn live_checkpoint_preserves_after_arrow_state() -> Result<()> {
        let source = "$object->method();";
        let line_index = LineIndex::new(source);
        let lexed = lex_source_with_checkpoints(source, &line_index);
        let method_index = lexed
            .tokens
            .iter()
            .position(|token| token.text.as_ref() == "method")
            .ok_or_else(|| anyhow::anyhow!("method token is missing"))?;
        let checkpoint = &lexed.live_checkpoints[method_index];

        assert_eq!(checkpoint.position, lexed.tokens[method_index].start);
        assert!(checkpoint.after_arrow, "method restart must preserve after_arrow");
        Ok(())
    }

    #[test]
    fn live_checkpoint_preserves_prototype_and_nesting_state() {
        let source = "sub f($$) { return 1; }";
        let line_index = LineIndex::new(source);
        let lexed = lex_source_with_checkpoints(source, &line_index);

        assert!(lexed.live_checkpoints.iter().any(|checkpoint| checkpoint.in_prototype));
        assert!(lexed.live_checkpoints.iter().any(|checkpoint| checkpoint.paren_depth > 0));
    }

    #[test]
    fn heredoc_queue_is_captured_but_timeout_sensitive_state_is_not_selected() -> Result<()> {
        let source = "my $value = <<EOF;\nbody\nEOF\nprint $value;\n";
        let line_index = LineIndex::new(source);
        let lexed = lex_source_with_checkpoints(source, &line_index);

        assert!(lexed.live_checkpoints.iter().any(|checkpoint| {
            !checkpoint.pending_heredocs.is_empty() && checkpoint.is_timeout_sensitive()
        }));
        assert!(lexed.stored_checkpoints.iter().any(|checkpoint| {
            !checkpoint.live.pending_heredocs.is_empty()
        }));
        Ok(())
    }
}