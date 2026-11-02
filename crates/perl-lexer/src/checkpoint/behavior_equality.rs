//! Versioned behavior-bearing state equality for exact suffix synchronization (#6986).
//!
//! Direct replay-state and construction-policy equality is authoritative. The
//! fingerprint exists only to accelerate candidate lookup and must never admit
//! reuse by itself.

use perl_source_identity::ContentDigest;
use thiserror::Error;

use super::{LexerCheckpoint, LexerPolicyIdentity, ReplayState};

/// Comparison contract version recorded in fingerprints and mismatch reports.
pub const BEHAVIOR_BEARING_STATE_COMPARE_VERSION: u32 = 1;

/// Domain-separated digest of behavior-bearing replay state and construction policy.
///
/// Two checkpoints with equal replay and policy produce equal fingerprints.
/// Fingerprint equality alone does not authorize suffix reuse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BehaviorBearingStateFingerprint(ContentDigest);

impl BehaviorBearingStateFingerprint {
    /// Access the underlying content digest bytes.
    #[must_use]
    pub fn digest(&self) -> &ContentDigest {
        &self.0
    }
}

/// Why two checkpoints differ in behavior-bearing state.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum BehaviorBearingStateMismatch {
    /// Construction-time lexer policy differs.
    #[error("construction policy differs")]
    Policy,
    /// Replay byte position differs.
    #[error("replay position differs")]
    Position,
    /// Lexer mode differs.
    #[error("lexer mode differs")]
    Mode,
    /// Delimiter stack differs.
    #[error("delimiter stack differs")]
    DelimiterStack,
    /// Prototype-tracking fields differ.
    #[error("prototype state differs")]
    PrototypeState,
    /// Parenthesis depth differs.
    #[error("parenthesis depth differs")]
    ParenDepth,
    /// `print`-list paren tracking differs.
    #[error("print-list paren tracking differs")]
    PrintListParens,
    /// Pending print-list paren flag differs.
    #[error("pending print-list paren flag differs")]
    PendingPrintListParen,
    /// Line/column snapshot differs.
    #[error("line/column snapshot differs")]
    CurrentPos,
    /// Newline-adjacent context differs.
    #[error("newline context differs")]
    NewlineContext,
    /// Queued heredoc replay state differs.
    #[error("pending heredoc queue differs")]
    PendingHeredocs,
    /// Physical line-start offset differs.
    #[error("line start offset differs")]
    LineStartOffset,
    /// In-progress quote-operator metadata differs.
    #[error("quote-operator state differs")]
    QuoteOperator,
    /// Terminal EOF emission flag differs.
    #[error("EOF emission state differs")]
    EofEmitted,
    /// Additional checkpoint context differs.
    #[error("checkpoint context differs")]
    Context,
}

impl LexerCheckpoint {
    /// Compare every behavior-bearing field that can change future tokenization.
    ///
    /// Checkpoint invalidation and live-boundary flags are intentionally excluded:
    /// callers must reject invalidated checkpoints before invoking suffix sync.
    pub fn behavior_bearing_state_equal(
        &self,
        other: &Self,
    ) -> Result<(), BehaviorBearingStateMismatch> {
        compare_policy(self.identity().policy(), other.identity().policy())?;
        compare_replay(self.replay(), other.replay())
    }

    /// Compute a deterministic fingerprint for candidate lookup.
    ///
    /// This must not be used as the sole admission oracle for suffix reuse.
    pub fn behavior_bearing_fingerprint(&self) -> BehaviorBearingStateFingerprint {
        BehaviorBearingStateFingerprint(fingerprint_material(
            self.identity().policy(),
            self.replay(),
        ))
    }
}

fn compare_policy(
    left: &LexerPolicyIdentity,
    right: &LexerPolicyIdentity,
) -> Result<(), BehaviorBearingStateMismatch> {
    if left == right { Ok(()) } else { Err(BehaviorBearingStateMismatch::Policy) }
}

fn compare_replay(
    left: &ReplayState,
    right: &ReplayState,
) -> Result<(), BehaviorBearingStateMismatch> {
    if left.position != right.position {
        return Err(BehaviorBearingStateMismatch::Position);
    }
    if left.mode != right.mode {
        return Err(BehaviorBearingStateMismatch::Mode);
    }
    if left.delimiter_stack != right.delimiter_stack {
        return Err(BehaviorBearingStateMismatch::DelimiterStack);
    }
    if left.in_prototype != right.in_prototype
        || left.prototype_depth != right.prototype_depth
        || left.prototype_signature_seen != right.prototype_signature_seen
        || left.after_sub != right.after_sub
        || left.after_arrow != right.after_arrow
        || left.hash_brace_depth != right.hash_brace_depth
        || left.after_var_subscript != right.after_var_subscript
    {
        return Err(BehaviorBearingStateMismatch::PrototypeState);
    }
    if left.paren_depth != right.paren_depth {
        return Err(BehaviorBearingStateMismatch::ParenDepth);
    }
    if left.print_list_parens != right.print_list_parens {
        return Err(BehaviorBearingStateMismatch::PrintListParens);
    }
    if left.pending_print_list_paren != right.pending_print_list_paren {
        return Err(BehaviorBearingStateMismatch::PendingPrintListParen);
    }
    if left.current_pos != right.current_pos {
        return Err(BehaviorBearingStateMismatch::CurrentPos);
    }
    if left.after_newline != right.after_newline {
        return Err(BehaviorBearingStateMismatch::NewlineContext);
    }
    if left.pending_heredocs != right.pending_heredocs {
        return Err(BehaviorBearingStateMismatch::PendingHeredocs);
    }
    if left.line_start_offset != right.line_start_offset {
        return Err(BehaviorBearingStateMismatch::LineStartOffset);
    }
    if left.current_quote_op != right.current_quote_op {
        return Err(BehaviorBearingStateMismatch::QuoteOperator);
    }
    if left.eof_emitted != right.eof_emitted {
        return Err(BehaviorBearingStateMismatch::EofEmitted);
    }
    if left.context != right.context {
        return Err(BehaviorBearingStateMismatch::Context);
    }
    Ok(())
}

fn fingerprint_material(policy: &LexerPolicyIdentity, replay: &ReplayState) -> ContentDigest {
    let mut material = Vec::new();
    material.extend_from_slice(b"perl-lsp:behavior-bearing-state:v1\0");
    material.extend_from_slice(&BEHAVIOR_BEARING_STATE_COMPARE_VERSION.to_le_bytes());
    push_policy(&mut material, policy);
    push_replay(&mut material, replay);
    ContentDigest::of_bytes(&material)
}

fn push_policy(out: &mut Vec<u8>, policy: &LexerPolicyIdentity) {
    out.push(u8::from(policy.interpolation_enabled()));
    out.extend_from_slice(&policy.lookahead_limit().to_le_bytes());
    out.push(u8::from(policy.qw_recovery_enabled()));
    out.push(u8::from(policy.emit_heredoc_body_tokens()));
    out.push(u8::from(policy.has_symbol_table()));
}

fn push_replay(out: &mut Vec<u8>, replay: &ReplayState) {
    out.extend_from_slice(&replay.position.to_le_bytes());
    push_mode(out, replay.mode);
    out.extend_from_slice(&(replay.delimiter_stack.len() as u32).to_le_bytes());
    for delimiter in &replay.delimiter_stack {
        out.extend_from_slice(&(*delimiter as u32).to_le_bytes());
    }
    out.push(u8::from(replay.in_prototype));
    out.extend_from_slice(&replay.prototype_depth.to_le_bytes());
    out.push(u8::from(replay.prototype_signature_seen));
    out.push(u8::from(replay.after_sub));
    out.push(u8::from(replay.after_arrow));
    out.extend_from_slice(&replay.hash_brace_depth.to_le_bytes());
    out.push(u8::from(replay.after_var_subscript));
    out.extend_from_slice(&replay.paren_depth.to_le_bytes());
    out.extend_from_slice(&(replay.print_list_parens.len() as u32).to_le_bytes());
    for (depth, left, right) in &replay.print_list_parens {
        out.extend_from_slice(&depth.to_le_bytes());
        out.push(u8::from(*left));
        out.push(u8::from(*right));
    }
    out.push(u8::from(replay.pending_print_list_paren));
    out.extend_from_slice(&replay.current_pos.byte.to_le_bytes());
    out.extend_from_slice(&replay.current_pos.line.to_le_bytes());
    out.extend_from_slice(&replay.current_pos.column.to_le_bytes());
    out.push(u8::from(replay.after_newline));
    out.extend_from_slice(&(replay.pending_heredocs.len() as u32).to_le_bytes());
    for pending in &replay.pending_heredocs {
        out.extend_from_slice(pending.label.as_bytes());
        out.push(0);
        out.extend_from_slice(&pending.body_start.to_le_bytes());
        out.push(u8::from(pending.allow_indent));
        out.push(u8::from(pending.interpolates));
    }
    out.extend_from_slice(&replay.line_start_offset.to_le_bytes());
    if let Some(quote) = &replay.current_quote_op {
        out.push(1);
        out.extend_from_slice(&quote.start_pos.to_le_bytes());
        out.extend_from_slice(quote.operator.as_bytes());
        out.push(0);
        out.extend_from_slice(&(quote.delimiter as u32).to_le_bytes());
    } else {
        out.push(0);
    }
    out.push(u8::from(replay.eof_emitted));
    push_context(out, &replay.context);
}

fn push_mode(out: &mut Vec<u8>, mode: crate::LexerMode) {
    use crate::LexerMode;
    let tag = match mode {
        LexerMode::ExpectTerm => 0,
        LexerMode::ExpectOperator => 1,
        LexerMode::ExpectDelimiter => 2,
        LexerMode::InFormatBody => 3,
        LexerMode::InDataSection => 4,
    };
    out.push(tag);
}

fn push_context(out: &mut Vec<u8>, context: &super::CheckpointContext) {
    use super::CheckpointContext;
    match context {
        CheckpointContext::Normal => out.push(0),
        CheckpointContext::Heredoc { terminator, is_interpolated } => {
            out.push(1);
            out.extend_from_slice(terminator.as_bytes());
            out.push(0);
            out.push(u8::from(*is_interpolated));
        }
        CheckpointContext::Format { start_position } => {
            out.push(2);
            out.extend_from_slice(&start_position.to_le_bytes());
        }
        CheckpointContext::Regex { delimiter, flags_position } => {
            out.push(3);
            out.extend_from_slice(&(*delimiter as u32).to_le_bytes());
            match flags_position {
                Some(position) => {
                    out.push(1);
                    out.extend_from_slice(&position.to_le_bytes());
                }
                None => out.push(0),
            }
        }
        CheckpointContext::QuoteLike { operator, delimiter, is_paired } => {
            out.push(4);
            out.extend_from_slice(operator.as_bytes());
            out.push(0);
            out.extend_from_slice(&(*delimiter as u32).to_le_bytes());
            out.push(u8::from(*is_paired));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Checkpointable, LexerConfig, PerlLexer};

    #[test]
    fn equal_live_checkpoints_pass_direct_equality_and_match_fingerprints() {
        let source = "my $x = 1; /pat/;\n";
        let mut lexer = PerlLexer::new(source);
        let _ = lexer.next_token();
        let left = lexer.checkpoint();
        let _ = lexer.next_token();
        let right = lexer.checkpoint();
        assert_ne!(
            left.behavior_bearing_fingerprint(),
            right.behavior_bearing_fingerprint(),
            "distinct boundaries should usually differ"
        );
        assert_eq!(
            left.behavior_bearing_state_equal(&left),
            Ok(()),
            "checkpoint must equal itself"
        );
        assert_eq!(left.behavior_bearing_fingerprint(), left.behavior_bearing_fingerprint());
    }

    #[test]
    fn restored_checkpoint_matches_boundary_after_live_restore() {
        let source = "10 / 2; m/pat/;\n";
        let mut lexer = PerlLexer::new(source);
        let _ = lexer.next_token();
        let boundary = lexer.checkpoint();
        let _ = lexer.next_token();
        assert!(lexer.restore(&boundary).is_ok());
        let replayed_boundary = lexer.checkpoint();
        assert_eq!(
            boundary.behavior_bearing_state_equal(&replayed_boundary),
            Ok(()),
            "live restore must reproduce behavior-bearing state at the boundary"
        );
        assert_eq!(
            boundary.behavior_bearing_fingerprint(),
            replayed_boundary.behavior_bearing_fingerprint()
        );
    }

    #[test]
    fn different_interpolation_policy_is_a_policy_mismatch() {
        let source = "qq{hello};";
        let enabled = PerlLexer::with_config(source, LexerConfig::default()).checkpoint();
        let disabled = PerlLexer::with_config(
            source,
            LexerConfig { parse_interpolation: false, ..LexerConfig::default() },
        )
        .checkpoint();
        assert_eq!(
            enabled.behavior_bearing_state_equal(&disabled),
            Err(BehaviorBearingStateMismatch::Policy)
        );
        assert_ne!(enabled.behavior_bearing_fingerprint(), disabled.behavior_bearing_fingerprint());
    }

    #[test]
    fn fingerprint_collision_does_not_imply_equality() {
        let source = "my $a = 1;\nmy $b = 2;\n";
        let mut lexer = PerlLexer::new(source);
        let first = lexer.checkpoint();
        let _ = lexer.next_token();
        let second = lexer.checkpoint();
        assert_ne!(first.behavior_bearing_fingerprint(), second.behavior_bearing_fingerprint());
        assert_ne!(first.behavior_bearing_state_equal(&second), Ok(()));
    }
}
