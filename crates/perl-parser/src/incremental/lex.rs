use crate::incremental::LineIndex;
use crate::incremental::checkpoint::LexCheckpoint;
use crate::incremental::edit::Edit;
use anyhow::Result;
use perl_lexer::{
    Checkpointable, LexerCheckpoint as LiveLexerCheckpoint, PerlLexer, Token, TokenType,
};
use perl_source_identity::ContentDigest;
use std::sync::Arc;

/// Hard cap for complete behavior-bearing checkpoints retained in one state.
pub const MAX_STORED_LEX_CHECKPOINTS: usize = 4096;
const INITIAL_CHECKPOINT_SPACING: usize = 128;

/// One complete checkpoint bound to the canonical identity of the exact source
/// generation that produced it.
///
/// Restoration is authorized only by the domain-separated SHA-256
/// [`ContentDigest`] of the whole capture source plus structural validity of
/// every source-relative offset. Unchanged prefix bytes are proven by edit
/// coordinates — a validated non-overlapping edit never touches bytes before
/// `edit.start_byte`, so any checkpoint positioned at or before the edit start
/// consumed only unchanged bytes. No short fingerprint participates in that
/// decision, and the generation digest is computed once per generation rather
/// than once per checkpoint.
#[derive(Clone)]
pub(crate) struct StoredLexCheckpoint {
    pub(crate) summary: LexCheckpoint,
    pub(crate) live: LiveLexerCheckpoint,
    source_digest: Arc<ContentDigest>,
}

impl StoredLexCheckpoint {
    fn capture(
        source_digest: &Arc<ContentDigest>,
        live: LiveLexerCheckpoint,
        line_index: &LineIndex,
    ) -> Self {
        let summary = summarize_checkpoint(&live, line_index);
        Self { summary, live, source_digest: Arc::clone(source_digest) }
    }

    /// Whether this checkpoint still belongs to `source`.
    ///
    /// `observed` must be the canonical digest of the whole `source`, computed
    /// once by the caller. Position and offset structure reject misaligned or
    /// corrupted state; the canonical digest rejects every other generation.
    fn belongs_to_source(&self, source: &str, observed: &ContentDigest) -> bool {
        if self.live.position > source.len() || !source.is_char_boundary(self.live.position) {
            return false;
        }
        self.source_digest.as_ref() == observed && self.live.is_valid_for(source)
    }

    /// Clone and transform the complete state for one old-generation edit.
    ///
    /// The checkpoint may sit exactly at the edit start: bytes before that
    /// boundary are unchanged by a validated edit, so restoring here re-lexes
    /// only the replacement onward.
    pub(crate) fn prepare_for_edit(
        &self,
        old_source: &str,
        observed: &ContentDigest,
        edit: &Edit,
    ) -> Option<LiveLexerCheckpoint> {
        if self.summary.byte > edit.start_byte || self.live.is_timeout_sensitive() {
            return None;
        }
        if !self.belongs_to_source(old_source, observed) {
            return None;
        }
        let old_len = edit.old_end_byte.checked_sub(edit.start_byte)?;
        let mut live = self.live.clone();
        live.try_apply_edit(edit.start_byte, old_len, edit.new_text.len()).then_some(live)
    }

    /// Carry an old prefix checkpoint into the edited generation when all
    /// behavior-bearing offsets remain valid.
    ///
    /// Carry-forward requires the checkpoint strictly before the edit start.
    /// Consumed prefix bytes are therefore unchanged by construction, and
    /// `try_apply_edit` either leaves every remaining behavior-bearing offset
    /// before the edit, shifts it uniformly with the suffix, or fails closed.
    pub(crate) fn transform_for_generation(
        &self,
        old_source: &str,
        new_source: &str,
        observed: &ContentDigest,
        new_digest: &ContentDigest,
        edit: &Edit,
    ) -> Option<Self> {
        if self.summary.byte >= edit.start_byte || !self.belongs_to_source(old_source, observed) {
            return None;
        }
        let old_len = edit.old_end_byte.checked_sub(edit.start_byte)?;
        let mut live = self.live.clone();
        if !live.try_apply_edit(edit.start_byte, old_len, edit.new_text.len())
            || !live.is_valid_for(new_source)
        {
            return None;
        }
        Some(Self { summary: self.summary, live, source_digest: Arc::new(new_digest.clone()) })
    }
}

pub(crate) struct LexedSource {
    pub(crate) tokens: Vec<Token>,
    pub(crate) checkpoints: Vec<LexCheckpoint>,
    pub(crate) stored_checkpoints: Vec<StoredLexCheckpoint>,
    #[cfg(test)]
    pub(crate) live_checkpoints: Vec<LiveLexerCheckpoint>,
    #[cfg(test)]
    pub(crate) terminal_checkpoint: LiveLexerCheckpoint,
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
    // A queued/virtual lexer event may expose several internal states at one
    // byte. The public summary is one replayable boundary per byte and maps to
    // the first complete live state reproduced by `capture_live_checkpoint`.
    if summaries.last().is_none_or(|summary| summary.byte != checkpoint.position) {
        summaries.push(summarize_checkpoint(checkpoint, line_index));
    }
}

fn behavior_state_changed(previous: &LiveLexerCheckpoint, current: &LiveLexerCheckpoint) -> bool {
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
    source_digest: &Arc<ContentDigest>,
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
    checkpoints.push(StoredLexCheckpoint::capture(source_digest, live.clone(), line_index));
}

/// Lex one complete source and capture restart candidates from the lexer's
/// actual mutable state before emitted tokens and terminal EOF.
///
/// The live checkpoint includes the ordered heredoc queue, newline/line-start
/// context, quote-operator state, body-emission policy, and recovery policy, so
/// heredoc boundaries no longer need a parser-side suppression approximation.
///
/// Complete generation-bound restart state is additionally persisted as
/// [`StoredLexCheckpoint`] records sharing one canonical source digest computed
/// a single time for the whole generation.
pub(crate) fn lex_source_with_checkpoints(source: &str, line_index: &LineIndex) -> LexedSource {
    let source_digest = Arc::new(ContentDigest::of_bytes(source.as_bytes()));
    let mut lexer = PerlLexer::new(source);
    let mut tokens = Vec::new();
    let mut checkpoints = Vec::new();
    let mut stored_checkpoints = Vec::new();
    let mut checkpoint_spacing = INITIAL_CHECKPOINT_SPACING;
    #[cfg(test)]
    let mut live_checkpoints = Vec::new();
    #[cfg(test)]
    let mut terminal_checkpoint = None;

    loop {
        let live = lexer.checkpoint();
        push_summary(&mut checkpoints, &live, line_index);
        push_stored_checkpoint(
            &source_digest,
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
            #[cfg(test)]
            {
                terminal_checkpoint = Some(terminal.clone());
            }
            push_summary(&mut checkpoints, &terminal, line_index);
            push_stored_checkpoint(
                &source_digest,
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
        #[cfg(test)]
        terminal_checkpoint: terminal_checkpoint.expect("lexer must capture terminal checkpoint"),
    }
}

/// Replay the old source to one previously captured token boundary and return
/// the complete current `Checkpointable` state for that exact boundary.
///
/// This is the bounded fallback for generations without a qualifying stored
/// checkpoint; the canonical path restores persisted state without replaying
/// old bytes. Restart correctness is authorized only by the full state
/// returned here, never by the compact summary.
pub(crate) fn capture_live_checkpoint(
    source: &str,
    boundary: usize,
) -> Option<LiveLexerCheckpoint> {
    let mut lexer = PerlLexer::new(source);

    loop {
        let checkpoint = lexer.checkpoint();
        if checkpoint.position == boundary {
            if checkpoint.is_timeout_sensitive() {
                return None;
            }
            return Some(checkpoint);
        }
        if checkpoint.position > boundary {
            return None;
        }

        match lexer.next_token() {
            Some(token) if token.token_type != TokenType::EOF => {}
            _ => return None,
        }
    }
}

/// Restore the complete mutable checkpoint contract into the edited source and
/// re-lex from that boundary to EOF. No old suffix is reused in this
/// correctness-first strategy.
///
/// The restored segment regenerates complete generation-bound checkpoints for
/// the edited source; those records share one canonical digest computed once
/// for this call.
pub(crate) fn lex_from_live_checkpoint(
    source: &str,
    line_index: &LineIndex,
    checkpoint: &LiveLexerCheckpoint,
) -> Result<LexedSource> {
    let mut lexer = PerlLexer::new(source);
    if !lexer.can_restore(checkpoint) {
        anyhow::bail!("live lexer checkpoint is not valid for the edited source");
    }
    lexer.restore(checkpoint);

    let source_digest = Arc::new(ContentDigest::of_bytes(source.as_bytes()));
    let mut tokens = Vec::new();
    let mut checkpoints = Vec::new();
    let mut stored_checkpoints = Vec::new();
    let mut checkpoint_spacing = INITIAL_CHECKPOINT_SPACING;
    #[cfg(test)]
    let mut live_checkpoints = Vec::new();
    #[cfg(test)]
    let mut terminal_checkpoint = None;
    let mut last_position = checkpoint.position;

    loop {
        let live = lexer.checkpoint();
        push_summary(&mut checkpoints, &live, line_index);
        push_stored_checkpoint(
            &source_digest,
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
            #[cfg(test)]
            {
                terminal_checkpoint = Some(terminal.clone());
            }
            push_summary(&mut checkpoints, &terminal, line_index);
            push_stored_checkpoint(
                &source_digest,
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
        #[cfg(test)]
        terminal_checkpoint: terminal_checkpoint.expect("lexer must capture terminal checkpoint"),
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

        let other = "my $value = 2;";
        let observed = ContentDigest::of_bytes(other.as_bytes());
        assert!(stored.prepare_for_edit(other, &observed, &edit).is_none());

        let observed = ContentDigest::of_bytes(source.as_bytes());
        assert!(stored.prepare_for_edit(source, &observed, &edit).is_some());
        Ok(())
    }

    #[test]
    fn stored_checkpoint_set_is_bounded_and_retains_a_late_boundary() -> Result<()> {
        let source =
            (0..20_000).map(|index| format!("my $v{index} = {index};\n")).collect::<String>();
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
    fn stored_checkpoints_share_one_canonical_digest_per_generation() {
        let source = "my $value = 1;\nmy $other = 2;\n";
        let line_index = LineIndex::new(source);
        let lexed = lex_source_with_checkpoints(source, &line_index);

        assert!(lexed.stored_checkpoints.len() > 1, "fixture must store several checkpoints");
        let expected = ContentDigest::of_bytes(source.as_bytes());
        for checkpoint in &lexed.stored_checkpoints {
            assert_eq!(
                checkpoint.source_digest.as_ref(),
                &expected,
                "every record binds to the same canonical generation digest"
            );
        }
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
        let method_start = lexed.tokens[method_index].start;
        let checkpoint = lexed
            .live_checkpoints
            .iter()
            .find(|checkpoint| checkpoint.position == method_start)
            .ok_or_else(|| anyhow::anyhow!("method checkpoint is missing"))?;

        assert_eq!(checkpoint.position, method_start);
        assert!(checkpoint.after_arrow, "method restart must preserve after_arrow");
        Ok(())
    }

    #[test]
    fn live_checkpoint_preserves_prototype_and_nesting_state() {
        let source = "sub f($$) { return 1; }";
        let line_index = LineIndex::new(source);
        let lexed = lex_source_with_checkpoints(source, &line_index);

        assert!(
            lexed.live_checkpoints.iter().any(|checkpoint| checkpoint.in_prototype),
            "prototype fixture must expose an in_prototype checkpoint"
        );
        assert!(
            lexed.live_checkpoints.iter().any(|checkpoint| checkpoint.paren_depth > 0),
            "prototype fixture must expose parenthesis depth"
        );
    }

    #[test]
    fn replay_captures_the_same_complete_checkpoint() -> Result<()> {
        let source = "$object->method();";
        let line_index = LineIndex::new(source);
        let lexed = lex_source_with_checkpoints(source, &line_index);
        let method_index = lexed
            .tokens
            .iter()
            .position(|token| token.text.as_ref() == "method")
            .ok_or_else(|| anyhow::anyhow!("method token is missing"))?;
        let method_start = lexed.tokens[method_index].start;
        let expected = lexed
            .live_checkpoints
            .iter()
            .find(|checkpoint| checkpoint.position == method_start)
            .ok_or_else(|| anyhow::anyhow!("method checkpoint is missing"))?;
        let replayed = capture_live_checkpoint(source, expected.position)
            .ok_or_else(|| anyhow::anyhow!("live checkpoint replay failed"))?;

        assert_eq!(replayed, *expected);
        Ok(())
    }

    #[test]
    fn heredoc_queue_is_captured_and_restart_boundaries_resume() -> Result<()> {
        let source = "my $value = <<EOF;\nbody\nEOF\nprint $value;\n";
        let line_index = LineIndex::new(source);
        let lexed = lex_source_with_checkpoints(source, &line_index);
        let start_index = lexed
            .tokens
            .iter()
            .position(|token| token.token_type == TokenType::HeredocStart)
            .ok_or_else(|| anyhow::anyhow!("heredoc start token is missing"))?;
        let queued = lexed
            .live_checkpoints
            .get(start_index + 1)
            .ok_or_else(|| anyhow::anyhow!("checkpoint after heredoc start is missing"))?;

        assert_eq!(queued.pending_heredocs.len(), 1);
        assert_eq!(queued.pending_heredocs[0].label, "EOF");
        assert!(
            lexed.checkpoints.iter().any(|summary| summary.byte == queued.position),
            "queued-heredoc state must remain a replayable boundary"
        );
        assert!(
            capture_live_checkpoint(source, queued.position).is_none(),
            "queued-heredoc checkpoints must fall back before the deterministic heredoc budget path"
        );
        assert!(
            lexed.stored_checkpoints.iter().any(|stored| stored.live.is_timeout_sensitive()),
            "timeout-sensitive states are persisted but never selected for restart"
        );

        let resumed = lexed
            .live_checkpoints
            .iter()
            .find(|checkpoint| {
                checkpoint.position > queued.position && checkpoint.pending_heredocs.is_empty()
            })
            .ok_or_else(|| anyhow::anyhow!("checkpoint after heredoc completion is missing"))?;
        assert!(
            lexed.checkpoints.iter().any(|summary| summary.byte == resumed.position),
            "restart summaries must resume after the heredoc queue drains"
        );
        Ok(())
    }

    #[test]
    fn stored_checkpoint_replay_reaches_the_deterministic_heredoc_budget() -> Result<()> {
        let body = "x".repeat(256 * 1024 + 1);
        let source = format!("my $value = <<EOF;\n{body}\nEOF\n");
        let line_index = LineIndex::new(&source);
        let fresh = lex_source_with_checkpoints(&source, &line_index);
        let checkpoint = fresh
            .stored_checkpoints
            .first()
            .ok_or_else(|| anyhow::anyhow!("start checkpoint should be stored"))?
            .live
            .clone();

        assert!(!checkpoint.is_timeout_sensitive());
        assert!(
            fresh.tokens.iter().any(|token| token.token_type == TokenType::UnknownRest),
            "fresh lex must take deterministic heredoc budget recovery"
        );

        let replayed = lex_from_live_checkpoint(&source, &line_index, &checkpoint)?;
        assert_eq!(replayed.tokens.len(), fresh.tokens.len());
        for (index, (actual, expected)) in replayed.tokens.iter().zip(&fresh.tokens).enumerate() {
            assert_eq!(actual.token_type, expected.token_type, "token kind {index}");
            assert_eq!(actual.text, expected.text, "token payload {index}");
            assert_eq!(actual.start, expected.start, "token start {index}");
            assert_eq!(actual.end, expected.end, "token end {index}");
        }
        assert_eq!(replayed.checkpoints.len(), fresh.checkpoints.len());
        for (index, (actual, expected)) in
            replayed.checkpoints.iter().zip(&fresh.checkpoints).enumerate()
        {
            assert_eq!(actual.byte, expected.byte, "checkpoint byte {index}");
            assert_eq!(actual.mode, expected.mode, "checkpoint mode {index}");
            assert_eq!(actual.line, expected.line, "checkpoint line {index}");
            assert_eq!(actual.column, expected.column, "checkpoint column {index}");
        }
        Ok(())
    }

    #[test]
    fn persisted_restart_after_idle_matches_a_fresh_lex_without_time_dependent_state() -> Result<()>
    {
        // Delayed-restart control: nothing in a persisted checkpoint carries
        // process-local or wall-clock identity, so an arbitrarily long idle
        // gap between capture and restore cannot change the outcome. The
        // control restores into a freshly constructed lexer/state pair built
        // after the "idle" period and demands exact parity with a fresh lex.
        let source = "$object->call(); my $x = q{quote}; my @list = (1, 2);";
        let line_index = LineIndex::new(source);
        let persisted = lex_source_with_checkpoints(source, &line_index);
        let origin = persisted
            .stored_checkpoints
            .first()
            .ok_or_else(|| anyhow::anyhow!("origin checkpoint is missing"))?;
        assert!(!origin.live.is_timeout_sensitive());

        let fresh = lex_source_with_checkpoints(source, &line_index);
        let restored = lex_from_live_checkpoint(source, &line_index, &origin.live)
            .map_err(|_| anyhow::anyhow!("persisted origin checkpoint must restore"))?;
        assert_eq!(restored.tokens.len(), fresh.tokens.len());
        for (index, (actual, expected)) in restored.tokens.iter().zip(&fresh.tokens).enumerate() {
            assert_eq!(actual.token_type, expected.token_type, "token kind {index}");
            assert_eq!(actual.text, expected.text, "token payload {index}");
            assert_eq!(actual.start, expected.start, "token start {index}");
            assert_eq!(actual.end, expected.end, "token end {index}");
        }
        assert_eq!(
            origin.source_digest.as_ref(),
            &ContentDigest::of_bytes(source.as_bytes()),
            "persistence binds to canonical source identity, not process-local state"
        );
        Ok(())
    }
}
