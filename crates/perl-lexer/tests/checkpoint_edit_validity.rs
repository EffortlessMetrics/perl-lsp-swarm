//! Edit invalidation fails closed: overlapping or shifted checkpoints are not
//! rewritten as default-state origins at the edit start.

use perl_lexer::PerlLexer;
use perl_lexer::checkpoint::Checkpointable;
use perl_lexer::{CheckpointRestoreError, TokenType};

#[test]
fn same_length_consumed_prefix_edit_invalidates_lexical_state() {
    let original = "$x /foo/;";
    let edited = "if /foo/;";
    let mut lexer = PerlLexer::new(original);
    let _ = lexer.next_token();
    let mut checkpoint = lexer.checkpoint();
    assert_eq!(checkpoint.position(), 2);

    // Equal byte geometry does not imply equal lexical state: the unchanged
    // slash is division after $x, but begins a regex after the replacement if.
    let mut fresh = PerlLexer::new(edited);
    let _ = fresh.next_token();
    assert_ne!(checkpoint.mode(), fresh.checkpoint().mode());
    assert!(matches!(lexer.next_token().map(|t| t.token_type), Some(TokenType::Division)));
    assert!(matches!(fresh.next_token().map(|t| t.token_type), Some(TokenType::RegexMatch)));

    assert!(!checkpoint.try_apply_edit(0, 2, 2));
    assert!(checkpoint.is_invalidated());
    let generation = checkpoint.identity().generation().clone();
    assert_eq!(
        checkpoint.rebind_to_source(edited, generation),
        Err(CheckpointRestoreError::Invalidated)
    );
    let mut target = PerlLexer::new(edited);
    let before = target.checkpoint();
    assert_eq!(target.restore(&checkpoint), Err(CheckpointRestoreError::Invalidated));
    assert_eq!(target.checkpoint(), before, "refusal must leave the target unchanged");
}

#[test]
fn empty_edit_preserves_the_complete_eof_checkpoint() {
    let source = "$x / 2;";
    let mut lexer = PerlLexer::new(source);
    while lexer.next_token().is_some() {}
    let mut checkpoint = lexer.checkpoint();
    assert!(checkpoint.eof_emitted());
    let before = checkpoint.clone();
    assert!(checkpoint.try_apply_edit(0, 0, 0));
    assert_eq!(checkpoint, before, "a no-op must not clear EOF or alter replay state");
    assert!(PerlLexer::new(source).can_restore(&checkpoint));
}

#[test]
fn replacement_at_capture_boundary_rebinds_and_replays() {
    let source = "$x /foo/;";
    let edited = "$x + 2;";
    let mut lexer = PerlLexer::new(source);
    let _ = lexer.next_token();
    let mut checkpoint = lexer.checkpoint();
    assert_eq!(checkpoint.position(), 2);
    assert!(checkpoint.try_apply_edit(2, 7, 5));
    let generation = checkpoint.identity().generation().clone();
    assert!(checkpoint.rebind_to_source(edited, generation).is_ok());
    let mut resumed = PerlLexer::new(edited);
    assert!(resumed.restore(&checkpoint).is_ok());
    let mut fresh = PerlLexer::new(edited);
    let _ = fresh.next_token();
    loop {
        let expected = fresh.next_token().map(|t| (t.token_type, t.text, t.start, t.end));
        let actual = resumed.next_token().map(|t| (t.token_type, t.text, t.start, t.end));
        assert_eq!(actual, expected);
        if expected.is_none() {
            break;
        }
    }
}

#[test]
fn shifted_replay_position_fails_closed_without_target_source_coordinates() {
    let source = "my $value = 1;\n";
    let mut lexer = PerlLexer::new(source);
    let _ = lexer.next_token();
    let mut checkpoint = lexer.checkpoint();
    let original_mode = checkpoint.mode();

    let preserved = checkpoint.try_apply_edit(0, 0, 3);

    assert!(!preserved, "byte lengths cannot reconstruct shifted line and column state");
    assert_eq!(checkpoint.mode(), original_mode, "invalidation must not fabricate default state");
    assert!(checkpoint.is_invalidated());

    let edited = PerlLexer::new("xxxmy $value = 1;\n");
    assert!(
        !edited.can_restore(&checkpoint),
        "lexer must reject a shifted checkpoint without valid source coordinates"
    );
}

#[test]
fn overlapping_quote_state_is_not_turned_into_a_restorable_default() {
    let source = "qq{hello world};";
    let mut lexer = PerlLexer::new(source);
    while lexer.checkpoint().current_quote_op().is_none() {
        if lexer.next_token().is_none() {
            break;
        }
    }
    let mut checkpoint = lexer.checkpoint();
    let quote = lexer
        .checkpoint()
        .current_quote_op()
        .map(|quote| quote.start_pos)
        .unwrap_or(checkpoint.position());
    let original_mode = checkpoint.mode();

    checkpoint.apply_edit(quote.saturating_sub(1), 5, 1);

    assert!(checkpoint.is_invalidated());
    assert_eq!(checkpoint.mode(), original_mode);
    let lexer = PerlLexer::new("xrest");
    assert!(!lexer.can_restore(&checkpoint), "lexer must reject an invalidated quote checkpoint");
}

#[test]
fn exact_boundary_edit_keeps_a_live_checkpoint_replayable() {
    let source = "12345678replacement";
    let mut lexer = PerlLexer::new(source);
    while lexer.checkpoint().position() < 8 {
        if lexer.next_token().is_none() {
            break;
        }
    }
    let mut checkpoint = lexer.checkpoint();
    assert_eq!(checkpoint.position(), 8, "source must produce a token boundary at byte 8");

    assert!(
        checkpoint.try_apply_edit(8, 2, 5),
        "edit beginning exactly at a live boundary must remain a restart"
    );
    assert!(!checkpoint.is_invalidated());
    assert!(checkpoint.is_valid_for(source));
}
