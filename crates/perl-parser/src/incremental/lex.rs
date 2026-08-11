use crate::incremental::LineIndex;
use crate::incremental::checkpoint::LexCheckpoint;
use anyhow::Result;
use perl_lexer::{
    Checkpointable, LexerCheckpoint as LiveLexerCheckpoint, PerlLexer, Token, TokenType,
};

pub(crate) struct LexedSource {
    pub(crate) tokens: Vec<Token>,
    pub(crate) checkpoints: Vec<LexCheckpoint>,
    #[cfg(test)]
    pub(crate) live_checkpoints: Vec<LiveLexerCheckpoint>,
}

fn summarize_checkpoint(
    checkpoint: &LiveLexerCheckpoint,
    line_index: &LineIndex,
) -> LexCheckpoint {
    let (line, column) = line_index.byte_to_position(checkpoint.position);
    LexCheckpoint { byte: checkpoint.position, mode: checkpoint.mode, line, column }
}

/// Lex one complete source and capture restart candidates from the lexer's
/// actual live state before every emitted token and before terminal EOF.
pub(crate) fn lex_source_with_checkpoints(source: &str, line_index: &LineIndex) -> LexedSource {
    let mut lexer = PerlLexer::new(source);
    let mut tokens = Vec::new();
    let mut checkpoints = Vec::new();
    #[cfg(test)]
    let mut live_checkpoints = Vec::new();

    loop {
        let live = lexer.checkpoint();
        checkpoints.push(summarize_checkpoint(&live, line_index));
        #[cfg(test)]
        live_checkpoints.push(live);

        let Some(token) = lexer.next_token() else {
            break;
        };
        if token.token_type == TokenType::EOF {
            break;
        }
        tokens.push(token);
    }

    LexedSource {
        tokens,
        checkpoints,
        #[cfg(test)]
        live_checkpoints,
    }
}

/// Replay the old source to one previously captured token boundary and return
/// the complete live lexer checkpoint for that exact boundary.
///
/// The public `LexCheckpoint` remains a compact compatibility summary. Restart
/// correctness is authorized only by this full `Checkpointable` state.
pub(crate) fn capture_live_checkpoint(
    source: &str,
    boundary: usize,
) -> Option<LiveLexerCheckpoint> {
    let mut lexer = PerlLexer::new(source);

    loop {
        let checkpoint = lexer.checkpoint();
        if checkpoint.position == boundary {
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

/// Restore a complete live checkpoint into the edited source and re-lex from
/// that boundary to EOF. No old suffix is reused in this correctness-first
/// strategy.
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

    let mut tokens = Vec::new();
    let mut checkpoints = Vec::new();
    #[cfg(test)]
    let mut live_checkpoints = Vec::new();
    let mut last_position = checkpoint.position;

    loop {
        let live = lexer.checkpoint();
        checkpoints.push(summarize_checkpoint(&live, line_index));
        #[cfg(test)]
        live_checkpoints.push(live);

        let Some(token) = lexer.next_token() else {
            break;
        };
        if token.token_type == TokenType::EOF {
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
        #[cfg(test)]
        live_checkpoints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let expected = &lexed.live_checkpoints[method_index];
        let replayed = capture_live_checkpoint(source, expected.position)
            .ok_or_else(|| anyhow::anyhow!("live checkpoint replay failed"))?;

        assert_eq!(&replayed, expected);
        Ok(())
    }
}
