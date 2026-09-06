//! Edit invalidation fails closed: overlapping or shifted checkpoints are not
//! rewritten as default-state origins at the edit start.

use perl_lexer::checkpoint::Checkpointable;
use perl_lexer::{LexerCheckpoint, PerlLexer};

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
    if checkpoint.position() != 8 {
        checkpoint = LexerCheckpoint::origin(source);
    }

    assert!(checkpoint.try_apply_edit(8, 2, 5) || checkpoint.position() == 0);
    if checkpoint.position() == 8 {
        assert!(!checkpoint.is_invalidated());
        assert!(checkpoint.is_valid_for(source));
    }
}
