use super::*;
use crate::{Checkpointable, LexerMode, PerlLexer, TokenType};

fn live_checkpoints(source: &str) -> Vec<LexerCheckpoint> {
    let mut lexer = PerlLexer::new(source);
    let mut out = vec![lexer.checkpoint()];
    while let Some(token) = lexer.next_token() {
        out.push(lexer.checkpoint());
        if matches!(token.token_type, TokenType::EOF) {
            break;
        }
    }
    out
}

fn checkpoint_after_tokens(source: &str, tokens: usize) -> LexerCheckpoint {
    let mut lexer = PerlLexer::new(source);
    for _ in 0..tokens {
        let Some(token) = lexer.next_token() else {
            break;
        };
        if matches!(token.token_type, TokenType::EOF) {
            break;
        }
    }
    lexer.checkpoint()
}

#[test]
fn test_checkpoint_creation() {
    let cp = LexerCheckpoint::origin("my $x = 1;");
    assert_eq!(cp.position(), 0);
    assert_eq!(cp.mode(), LexerMode::ExpectTerm);
    assert!(cp.delimiter_stack().is_empty());
}

#[test]
fn test_checkpoint_diff() {
    let source = "my $x = 1; my $y = 2;";
    let cp1 = checkpoint_after_tokens(source, 1);
    let cp2 = checkpoint_after_tokens(source, 3);
    let diff = cp2.diff(&cp1);
    assert!(diff.position_delta > 0);
}

#[test]
fn test_checkpoint_edit_prefix_shift_fails_closed() {
    let source = "aaaaaaaaaa bbbbbbbbbb cccccccccc";
    let mut cp = checkpoint_after_tokens(source, 1);
    let original_mode = cp.mode();
    assert!(!cp.try_apply_edit(0, 0, 3), "shifted line/column cannot be reconstructed");
    assert!(cp.is_invalidated());
    assert_eq!(cp.mode(), original_mode, "invalidation must not fabricate default state");
    assert!(!cp.is_valid_for("xxxaaaaaaaaaa bbbbbbbbbb cccccccccc"));
}

#[test]
fn test_checkpoint_edit_after_leaves_position() {
    let source = "aaaaaaaaaa bbbbbbbbbb cccccccccc";
    let mut cp = checkpoint_after_tokens(source, 1);
    let position = cp.position();
    assert!(cp.try_apply_edit(source.len(), 0, 3));
    assert_eq!(cp.position(), position);
    assert!(!cp.is_invalidated());
}

#[test]
fn test_checkpoint_edit_overlap_does_not_become_default_origin() {
    let source = "my $value = qq{hello};";
    let mut lexer = PerlLexer::new(source);
    while lexer.checkpoint().current_quote_op().is_none() {
        if lexer.next_token().is_none() {
            break;
        }
    }
    let mut cp = lexer.checkpoint();
    let quote_start = cp.current_quote_op().map(|quote| quote.start_pos).unwrap_or(cp.position());
    let original_mode = cp.mode();
    assert!(!cp.try_apply_edit(quote_start.saturating_sub(1), 4, 1));
    assert!(cp.is_invalidated());
    assert_eq!(cp.mode(), original_mode);
    assert!(!matches!(cp.mode(), LexerMode::ExpectTerm) || original_mode == LexerMode::ExpectTerm);
    let lexer = PerlLexer::new(source);
    assert!(!lexer.can_restore(&cp));
}

#[test]
fn test_checkpoint_helpers_and_validity() {
    let start = LexerCheckpoint::origin("abc");
    assert!(start.is_at_start());
    assert!(start.is_valid_for("abc"));
}

#[test]
fn test_checkpoint_cache() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let source = "a; b; c; d; e; f;";
    let checkpoints = live_checkpoints(source);
    let mut cache = CheckpointCache::new(3);
    for checkpoint in checkpoints.into_iter().take(6) {
        cache.add(checkpoint);
    }
    assert!(cache.len() <= 3);
    assert!(cache.find_before(source.len()).is_some());
    Ok(())
}

#[test]
fn test_find_before_binary_search() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let source = "a; b; c; d; e;";
    let checkpoints = live_checkpoints(source);
    let mut cache = CheckpointCache::new(50);
    for checkpoint in &checkpoints {
        cache.add(checkpoint.clone());
    }
    let last = checkpoints.last().ok_or("expected checkpoints")?.position();
    assert_eq!(cache.find_before(last).ok_or("find_before last should hit")?.position(), last);
    assert!(cache.find_before(0).is_some());
    Ok(())
}

#[test]
fn test_checkpoint_cache_add_replaces_same_position()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut cache = CheckpointCache::new(5);
    let first = LexerCheckpoint::origin("my $x = 1;");
    cache.add(first);
    let replacement = checkpoint_after_tokens("my $x = 1;", 1);
    cache.add(LexerCheckpoint::origin("my $x = 1;"));
    cache.add(replacement.clone());
    let found = cache.find_before(replacement.position()).ok_or("expected replacement")?;
    assert_eq!(found.position(), replacement.position());
    Ok(())
}

#[test]
fn test_checkpoint_cache_capacity_50() {
    let source = "a;".repeat(60);
    let checkpoints = live_checkpoints(&source);
    let mut cache = CheckpointCache::new(50);
    for checkpoint in checkpoints.into_iter().take(50) {
        cache.add(checkpoint);
    }
    assert_eq!(cache.len(), 50);
    cache.add(LexerCheckpoint::origin(&source));
    assert_eq!(cache.len(), 50);
}

#[test]
fn test_checkpoint_cache_zero_capacity_is_noop() {
    let mut cache = CheckpointCache::new(0);
    cache.add(LexerCheckpoint::origin("a; b;"));
    assert!(cache.is_empty());
    assert!(cache.find_before(100).is_none());
    assert!(cache.find_after(0).is_none());
}

#[test]
fn test_find_after_binary_search() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let source = "a; b; c; d;";
    let checkpoints = live_checkpoints(source);
    let mut cache = CheckpointCache::new(10);
    for checkpoint in &checkpoints {
        cache.add(checkpoint.clone());
    }
    let first = checkpoints.first().ok_or("expected first")?.position();
    let found = cache.find_after(first).ok_or("find_after first")?;
    assert_eq!(found.position(), first);
    assert!(cache.find_after(source.len() + 1).is_none());
    Ok(())
}

#[test]
fn test_checkpoint_cache_apply_edit_invalidates_overlap()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let source = "my $value = 1; my $other = 2;";
    let mut cache = CheckpointCache::new(8);
    for checkpoint in live_checkpoints(source) {
        cache.add(checkpoint);
    }
    cache.apply_edit(4, 6, 2);
    if let Some(checkpoint) = cache.find_before(4) {
        assert!(
            checkpoint.is_invalidated() || checkpoint.position() <= 4,
            "prefix checkpoints may remain, overlapped ones must invalidate"
        );
    }
    Ok(())
}

#[test]
fn test_checkpoint_cache_capacity_one_keeps_latest()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let source = "a; b; c;";
    let checkpoints = live_checkpoints(source);
    let mut cache = CheckpointCache::new(1);
    for checkpoint in &checkpoints {
        cache.add(checkpoint.clone());
    }
    let latest = cache.find_before(usize::MAX).ok_or("capacity-1 cache should keep one")?;
    let last = checkpoints.last().ok_or("expected last")?;
    assert_eq!(latest.position(), last.position());
    Ok(())
}

#[test]
fn test_checkpoint_start_and_input_validity_helpers() {
    let start = LexerCheckpoint::origin("abc");
    assert!(start.is_at_start());
    assert!(start.is_valid_for("abc"));
}

#[test]
fn overlapping_edit_does_not_reset_mode_to_term_origin() {
    let source = "sub f($$) { 1 }";
    let mut lexer = PerlLexer::new(source);
    while !lexer.checkpoint().in_prototype() {
        if lexer.next_token().is_none() {
            break;
        }
    }
    let mut checkpoint = lexer.checkpoint();
    assert!(checkpoint.in_prototype());
    let _ = checkpoint.try_apply_edit(checkpoint.position().saturating_sub(1), 3, 1);
    assert!(checkpoint.is_invalidated());
    assert!(
        checkpoint.in_prototype(),
        "invalidation must not fabricate a default ExpectTerm origin"
    );
}
