//! Executable contract slice for #6986 state equality and token-window denial.

use perl_lexer::{BehaviorBearingStateMismatch, Checkpointable, PerlLexer, Token, TokenType};
use perl_parser::incremental::{
    ExactSuffixSyncCandidate, ExactSuffixSyncDenial, ExactSuffixSyncPolicy,
    compare_direct_token_window, evaluate_exact_suffix_sync,
};
use std::sync::Arc;

type R = Result<(), Box<dyn std::error::Error>>;

#[test]
fn behavior_bearing_state_equality_is_field_authoritative() -> R {
    let source = "my $x = 1;\n";
    let mut lexer = PerlLexer::new(source);
    let _ = lexer.next_token();
    let first = lexer.checkpoint();
    let _ = lexer.next_token();
    let second = lexer.checkpoint();
    assert_ne!(
        first.behavior_bearing_state_equal(&second),
        Ok(()),
        "position/mode changes must be observable"
    );
    assert!(matches!(
        first.behavior_bearing_state_equal(&second),
        Err(BehaviorBearingStateMismatch::Position)
    ));
    Ok(())
}

#[test]
fn regex_context_change_after_one_equal_operator_is_denied() -> R {
    let old_suffix = vec![
        Token::new(TokenType::Division, "/", 10, 11),
        Token::new(TokenType::Number(Arc::from("2")), "2", 12, 13),
    ];
    let new_suffix = vec![
        Token::new(TokenType::RegexMatch, "/", 10, 11),
        Token::new(TokenType::Identifier(Arc::from("pat")), "pat", 11, 14),
    ];
    let new_source = "my $x = /pat;\n";
    let err = compare_direct_token_window(new_source, 0, &old_suffix, &new_suffix, 1)
        .expect_err("regex/division divergence must fail the window");
    assert!(matches!(err, ExactSuffixSyncDenial::WindowMismatch { index: 0 }));
    Ok(())
}

#[test]
fn evaluate_requires_equal_behavior_bearing_state() -> R {
    let source = "my $prefix = 1;\nmy $suffix = 2;\n";
    let mut old_lexer = PerlLexer::new(source);
    for _ in 0..8 {
        let _ = old_lexer.next_token();
    }
    let old_cp = old_lexer.checkpoint();
    let old_suffix: Vec<Token> = old_lexer.collect_tokens();

    let mut new_lexer = PerlLexer::new(source);
    for _ in 0..7 {
        let _ = new_lexer.next_token();
    }
    let new_cp = new_lexer.checkpoint();

    let candidate = ExactSuffixSyncCandidate {
        old_checkpoint: &old_cp,
        new_checkpoint: &new_cp,
        byte_shift: 0,
        edit_start: 14,
        edit_old_end: 15,
        suffix_start_old: 16,
    };
    let err = evaluate_exact_suffix_sync(
        ExactSuffixSyncPolicy { window_tokens: 1 },
        source,
        &old_suffix,
        &old_suffix,
        candidate,
    )
    .expect_err("misaligned checkpoints must not authorize reuse");
    assert!(matches!(
        err,
        ExactSuffixSyncDenial::StateMismatch(BehaviorBearingStateMismatch::Position)
    ));
    Ok(())
}
