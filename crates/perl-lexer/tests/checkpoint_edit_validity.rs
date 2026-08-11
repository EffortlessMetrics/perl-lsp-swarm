use perl_lexer::checkpoint::{Checkpointable, LexerCheckpoint, QuoteOperatorCheckpoint};
use perl_lexer::{PerlLexer, Position};

#[test]
fn shifted_replay_position_fails_closed_without_target_source_coordinates() {
    let mut checkpoint = LexerCheckpoint::at_position(8);
    checkpoint.current_pos = Position::new(8, 2, 3);

    let preserved = checkpoint.try_apply_edit(0, 0, 3);

    assert!(!preserved, "byte lengths cannot reconstruct shifted line and column state");
    assert_eq!(checkpoint.position, 11, "compatibility inspection keeps the shifted byte");
    assert_ne!(checkpoint.current_pos.byte, checkpoint.position);

    let lexer = PerlLexer::new("xxxmy $value = 1;\n");
    assert!(!checkpoint.is_valid_for("xxxmy $value = 1;\n"));
    assert!(!lexer.can_restore(&checkpoint));
}

#[test]
fn compatibility_edit_cannot_turn_overlapped_quote_state_into_a_restorable_default() {
    let mut checkpoint = LexerCheckpoint::at_position(12);
    checkpoint.current_quote_op = Some(QuoteOperatorCheckpoint {
        operator: "qq".to_string(),
        delimiter: '{',
        start_pos: 9,
    });

    checkpoint.apply_edit(8, 5, 1);

    assert_eq!(checkpoint.position, 8, "compatibility inspection retains the edit boundary");
    assert!(!checkpoint.is_valid_for("xxxxxxxxrest"));

    let lexer = PerlLexer::new("xxxxxxxxrest");
    assert!(!lexer.can_restore(&checkpoint));
}

#[test]
fn exact_boundary_edit_keeps_a_live_checkpoint_replayable() {
    let mut checkpoint = LexerCheckpoint::at_position(8);
    checkpoint.current_pos = Position::new(8, 1, 9);

    assert!(checkpoint.try_apply_edit(8, 2, 5));
    assert_eq!(checkpoint.position, 8);
    assert_eq!(checkpoint.current_pos, Position::new(8, 1, 9));
    assert!(checkpoint.is_valid_for("12345678replacement"));
}
