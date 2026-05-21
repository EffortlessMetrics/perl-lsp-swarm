use crate::{LexerMode, PerlLexer, checkpoint::CheckpointContext};

pub(super) fn build_checkpoint_context(lexer: &PerlLexer<'_>) -> CheckpointContext {
    if matches!(lexer.mode, LexerMode::InFormatBody) {
        return CheckpointContext::Format { start_position: lexer.position.saturating_sub(100) };
    }

    if let Some(delimiter) = lexer.delimiter_stack.last().copied() {
        return CheckpointContext::QuoteLike {
            operator: String::new(),
            delimiter,
            is_paired: true,
        };
    }

    CheckpointContext::Normal
}
