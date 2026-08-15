#![cfg(feature = "incremental")]
//! Public-contract tests for generation-bound stored lexer restart.

use perl_lexer::{PerlLexer, Token, TokenType};
use perl_parser::incremental::{LexRestartStrategy, MAX_STORED_LEX_CHECKPOINTS};
use perl_parser::{Edit, IncrementalState, apply_edits};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fresh_tokens(source: &str) -> Vec<Token> {
    let mut lexer = PerlLexer::new(source);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token() {
        if token.token_type == TokenType::EOF {
            break;
        }
        tokens.push(token);
    }
    tokens
}

fn assert_tokens_equal(actual: &[Token], expected: &[Token]) {
    assert_eq!(actual.len(), expected.len(), "token count diverged");
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(actual.token_type, expected.token_type, "token kind {index}");
        assert_eq!(actual.text, expected.text, "token payload {index}");
        assert_eq!(actual.start, expected.start, "token start {index}");
        assert_eq!(actual.end, expected.end, "token end {index}");
    }
}

fn replacing_edit(source: &str, needle: &str, replacement: &str) -> Result<Edit, std::io::Error> {
    let start = source
        .find(needle)
        .ok_or_else(|| std::io::Error::other(format!("fixture needle {needle:?} is missing")))?;
    Ok(Edit {
        start_byte: start,
        old_end_byte: start + needle.len(),
        new_end_byte: start + replacement.len(),
        new_text: replacement.to_string(),
    })
}

#[test]
fn empty_edit_batch_reports_unchanged_without_lexer_or_parser_work() -> TestResult {
    let source = "my $before = 1; my $after = 2;";
    let mut state = IncrementalState::new(source.to_string());
    let token_count = state.tokens().len();

    let result = apply_edits(&mut state, &[])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::Unchanged);
    assert_eq!(result.lex_restart.restart_byte, source.len());
    assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
    assert_eq!(result.lex_restart.relexed_bytes, 0);
    assert_eq!(result.lex_restart.reused_prefix_tokens, token_count);
    assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
    assert_eq!(result.reused_tokens, token_count);
    assert_eq!(result.reparsed_bytes, 0);
    assert!(result.lex_restart.stored_checkpoint_count > 0);
    assert!(result.changed_ranges.is_empty());
    assert_eq!(state.source(), source);
    assert_tokens_equal(state.tokens(), &fresh_tokens(source));
    Ok(())
}

#[test]
fn late_equal_width_edit_uses_stored_state_and_relexes_the_complete_suffix() -> TestResult {
    let source = "my $before = 1; my $target = 2; my $after = 3;";
    let start = source.find("= 2").ok_or("target literal is missing")? + 2;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start + 1,
        new_end_byte: start + 1,
        new_text: "9".to_string(),
    };
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
    assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
    assert!(result.lex_restart.restart_byte > 0);
    assert!(result.lex_restart.reused_prefix_tokens > 0);
    assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
    assert_eq!(
        result.lex_restart.relexed_bytes,
        state.source().len() - result.lex_restart.restart_byte
    );
    assert_eq!(result.reused_tokens, result.lex_restart.reused_tokens());
    assert_tokens_equal(state.tokens(), &fresh_tokens(state.source()));
    Ok(())
}

#[test]
fn stateful_and_source_boundary_edits_match_fresh_lexing() -> TestResult {
    let fixtures = [
        ("division", "my $x = 10 / 2; my $after = 1;", "/ 2", "/ 3"),
        ("regex", "my $ok = /foo/; my $after = 1;", "foo", "bar"),
        ("quote-single", "my $x = q{foo}; my $after = 1;", "foo", "bar"),
        ("quote-double", "my $x = qq{foo}; my $after = 1;", "foo", "bar"),
        ("quote-words", "my @x = qw(foo bar); my $after = 1;", "foo", "baz"),
        ("quote-command", "my $x = qx{echo foo}; my $after = 1;", "foo", "bar"),
        ("substitution", "$x =~ s/foo/bar/; my $after = 1;", "foo", "baz"),
        ("transliteration", "$x =~ tr/a-z/A-Z/; my $after = 1;", "a-z", "b-z"),
        ("prototype", "sub f($$) { return 1; } my $after = 1;", "return 1", "return 2"),
        ("unicode", "my $x = \"café\"; my $after = 1;", "é", "ø"),
        (
            "crlf",
            "my $x = 1;
my $y = 2;
",
            "= 2",
            "= 3",
        ),
        (
            "heredoc-body",
            "my $value = <<EOF;
body
EOF
print $value;
",
            "body",
            "changed",
        ),
    ];

    for (name, source, needle, replacement) in fixtures {
        let edit = replacing_edit(source, needle, replacement)?;
        let mut state = IncrementalState::new(source.to_string());
        let result = apply_edits(&mut state, &[edit])?;

        assert_eq!(
            result.lex_restart.strategy,
            LexRestartStrategy::StoredCheckpointToEof,
            "{name} unexpectedly abandoned the stored-checkpoint path"
        );
        assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0, "{name}");
        assert_eq!(result.lex_restart.reused_suffix_tokens, 0, "{name}");
        assert_tokens_equal(state.tokens(), &fresh_tokens(state.source()));
    }
    Ok(())
}

#[test]
fn method_context_edit_matches_fresh_lexing_after_stored_state_restore() -> TestResult {
    let source = "my $value = $object->method(); my $after = 1;";
    let start = source.find("method").ok_or("method name is missing")?;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start + "method".len(),
        new_end_byte: start + "member".len(),
        new_text: "member".to_string(),
    };
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
    assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
    assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
    assert_tokens_equal(state.tokens(), &fresh_tokens(state.source()));
    Ok(())
}

#[test]
fn large_edit_reports_full_relex_instead_of_checkpoint_reuse() -> TestResult {
    let source = "my $value = 1;";
    let replacement = "my $value = 2;\n".repeat(80);
    let edit = Edit {
        start_byte: 0,
        old_end_byte: source.len(),
        new_end_byte: replacement.len(),
        new_text: replacement,
    };
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::FullRelex);
    assert_eq!(result.lex_restart.restart_byte, 0);
    assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
    assert_eq!(result.lex_restart.reused_prefix_tokens, 0);
    assert_eq!(result.lex_restart.reused_suffix_tokens, 0);
    assert_eq!(result.lex_restart.relexed_bytes, state.source().len());
    assert!(result.lex_restart.stored_checkpoint_count <= MAX_STORED_LEX_CHECKPOINTS);
    assert_tokens_equal(state.tokens(), &fresh_tokens(state.source()));
    Ok(())
}

#[test]
fn timeout_sensitive_heredoc_state_selects_an_earlier_safe_checkpoint() -> TestResult {
    let source = "my $value = <<EOF;\nbody\nEOF\nmy $after = 1;\n";
    let edit = replacing_edit(source, "body", "changed")?;
    let mut state = IncrementalState::new(source.to_string());

    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
    assert_eq!(result.lex_restart.old_prefix_bytes_replayed, 0);
    assert_eq!(result.lex_restart.reused_suffix_tokens, 0);

    let expected = fresh_tokens(state.source());
    assert_tokens_equal(state.tokens(), &expected);
    let downstream_start =
        state.source().find("my $after").ok_or("downstream fixture is missing")?;
    let actual_downstream =
        state.tokens().iter().filter(|token| token.start >= downstream_start).collect::<Vec<_>>();
    let expected_downstream =
        expected.iter().filter(|token| token.start >= downstream_start).collect::<Vec<_>>();
    assert_eq!(actual_downstream.len(), expected_downstream.len());
    for (index, (actual, expected)) in actual_downstream.iter().zip(expected_downstream).enumerate()
    {
        assert_eq!(actual.token_type, expected.token_type, "downstream token kind {index}");
        assert_eq!(actual.text, expected.text, "downstream token payload {index}");
        assert_eq!(actual.start, expected.start, "downstream token start {index}");
        assert_eq!(actual.end, expected.end, "downstream token end {index}");
    }
    Ok(())
}
