use crate::{LexerCheckpoint, LexerMode, PerlLexer, checkpoint::CheckpointContext};

pub(super) fn restore_format_mode_context(lexer: &mut PerlLexer<'_>, checkpoint: &LexerCheckpoint) {
    if let CheckpointContext::Format { .. } = &checkpoint.context
        && !matches!(lexer.mode, LexerMode::InFormatBody)
    {
        lexer.mode = LexerMode::InFormatBody;
    }
}
