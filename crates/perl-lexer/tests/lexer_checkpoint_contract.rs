//! Discriminating checkpoint contract for #8090.
//!
//! Live boundaries only; identity is explicit; restore is typed and atomic;
//! caller-selected positions and overlapping edits fail closed.

use perl_lexer::{
    CHECKPOINT_SCHEMA_VERSION, CheckpointRestoreError, Checkpointable, LexerCheckpoint,
    LexerConfig, LocalSymbolTable, PerlLexer, TokenType,
};
use perl_source_identity::{LogicalSourceId, ProjectId, SourceGeneration, WorkspaceRootId};

type R = Result<(), Box<dyn std::error::Error>>;

fn collect(lexer: &mut PerlLexer<'_>) -> Vec<(String, usize, usize)> {
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token() {
        let eof = matches!(token.token_type, TokenType::EOF);
        tokens.push((format!("{:?}", token.token_type), token.start, token.end));
        if eof {
            break;
        }
    }
    tokens
}

fn advance_to(lexer: &mut PerlLexer<'_>, needle: &str) {
    loop {
        let Some(token) = lexer.next_token() else {
            return;
        };
        if token.text.as_ref() == needle || matches!(token.token_type, TokenType::EOF) {
            return;
        }
    }
}

fn logical_source(path: &str) -> LogicalSourceId {
    let project = ProjectId::from_canonical_name("https://github.com/acme/widget");
    let root = WorkspaceRootId::from_project_and_root_key(&project, "root");
    LogicalSourceId::from_root_and_path(&root, path)
}

fn assert_suffix_matches(source: &str) {
    let mut lexer = PerlLexer::new(source);
    let _ = lexer.next_token();
    let checkpoint = lexer.checkpoint();
    let first = collect(&mut lexer);
    assert!(lexer.restore(&checkpoint).is_ok(), "live restore must succeed for {source:?}");
    let replayed = collect(&mut lexer);
    assert_eq!(first, replayed, "restored suffix must equal uninterrupted suffix for {source:?}");
}

#[test]
fn live_origin_round_trip_equals_uninterrupted_lexing() {
    for source in [
        "10 / 2; /pat/;\n",
        "$obj->m(); m/pat/;\n",
        "sub f($$) { 1 }\n",
        "qq{hello}; q{world};\n",
        "my $v = <<EOF;\nbody\nEOF\nprint $v;\n",
        "print <<A, <<B;\none\nA\ntwo\nB\n",
        "format STDOUT =\n@<<<\n$text\n.\n",
        "=pod\nignored\n=cut\nmy $x = 1;\n",
        "__DATA__\npayload\n",
        "my $café = 1;\n",
        "\u{feff}my $x = 1;\n",
        "my $x = 1;\r\nmy $y = 2;\r\n",
        "my $x = 1;\nmy $y = 2;\n",
        "my $x = 1;\rmy $y = 2;\r",
    ] {
        assert_suffix_matches(source);
        let origin = LexerCheckpoint::origin(source);
        let lexer = PerlLexer::new(source);
        assert!(lexer.can_restore(&origin), "origin must restore onto the same source");
    }
}

#[test]
fn caller_selected_position_is_not_a_restart_boundary() {
    let source = "my $value = 1;";
    let lexer = PerlLexer::new(source);
    #[expect(deprecated, reason = "proves at_position is not a live restore boundary")]
    let synthetic = LexerCheckpoint::at_position(3);
    assert_eq!(
        lexer.validate_restore(&synthetic),
        Err(CheckpointRestoreError::UnsupportedBoundary)
    );
}

#[test]
fn mid_codepoint_offset_fails_closed() {
    let source = "éx";
    let mut checkpoint = PerlLexer::new(source).checkpoint();
    checkpoint.__test_stamp_position(1);
    assert_eq!(
        PerlLexer::new(source).validate_restore(&checkpoint),
        Err(CheckpointRestoreError::InvalidUtf8Boundary)
    );
}

#[test]
fn same_length_different_source_fails_closed() {
    let mut left = PerlLexer::new("aaa / 2;");
    advance_to(&mut left, "/");
    let checkpoint = left.checkpoint();
    let right = PerlLexer::new("bbb / 2;");
    assert_ne!(
        checkpoint.identity().content(),
        PerlLexer::new("bbb / 2;").checkpoint().identity().content(),
        "same-length sources must not share a content digest"
    );
    assert_eq!(right.validate_restore(&checkpoint), Err(CheckpointRestoreError::WrongContent));
    assert_eq!(right.checkpoint().position(), 0);
}

#[test]
fn failed_restore_is_atomic() {
    let mut lexer = PerlLexer::new("my $x = 1; my $y = 2;");
    let _ = lexer.next_token();
    let before = lexer.checkpoint();
    let foreign = PerlLexer::new("zz $x = 1; my $y = 2;").checkpoint();
    assert_eq!(lexer.restore(&foreign), Err(CheckpointRestoreError::WrongContent));
    assert_eq!(lexer.checkpoint(), before);
}

#[test]
fn changed_interpolation_policy_is_wrong_configuration() {
    let source = "qq{hello};";
    let enabled = PerlLexer::with_config(source, LexerConfig::default()).checkpoint();
    let disabled = PerlLexer::with_config(
        source,
        LexerConfig { parse_interpolation: false, ..LexerConfig::default() },
    );
    assert_eq!(
        disabled.validate_restore(&enabled),
        Err(CheckpointRestoreError::WrongConfiguration)
    );
}

#[test]
fn body_token_policy_mismatch_is_wrong_configuration() {
    let source = "my $v = <<EOF;\nbody\nEOF\n";
    let ordinary = PerlLexer::new(source).checkpoint();
    let body_tokens = PerlLexer::with_body_tokens(source);
    assert_eq!(
        body_tokens.validate_restore(&ordinary),
        Err(CheckpointRestoreError::WrongConfiguration)
    );
}

#[test]
fn track_positions_noop_does_not_split_identity() {
    #[expect(deprecated, reason = "track_positions is a documented compatibility no-op")]
    let untracked = LexerConfig { track_positions: false, ..LexerConfig::default() };
    let source = "my $x = 1;";
    let a = PerlLexer::with_config(source, LexerConfig::default()).checkpoint();
    let b = PerlLexer::with_config(source, untracked);
    assert!(b.can_restore(&a));
}

#[test]
fn stale_generation_fails_closed() {
    let source = "my $x = 1;";
    let mut lexer = PerlLexer::new(source);
    lexer.bind_generation(SourceGeneration::known("1"));
    let checkpoint = lexer.checkpoint();
    lexer.bind_generation(SourceGeneration::known("2"));
    assert_eq!(lexer.validate_restore(&checkpoint), Err(CheckpointRestoreError::WrongGeneration));
}

#[test]
fn different_logical_sources_fail_closed() {
    let source = "my $x = 1;";
    let mut left = PerlLexer::new(source);
    left.bind_logical_source(logical_source("lib/A.pm"));
    let checkpoint = left.checkpoint();
    let mut right = PerlLexer::new(source);
    right.bind_logical_source(logical_source("lib/B.pm"));
    assert_eq!(right.validate_restore(&checkpoint), Err(CheckpointRestoreError::WrongSource));
}

#[test]
fn unbound_source_vs_bound_source_fails_closed() {
    let source = "my $x = 1;";
    let unbound = PerlLexer::new(source).checkpoint();
    let mut bound = PerlLexer::new(source);
    bound.bind_logical_source(logical_source("lib/A.pm"));
    assert_eq!(bound.validate_restore(&unbound), Err(CheckpointRestoreError::WrongSource));
}

#[test]
fn unknown_schema_fails_closed() {
    let source = "my $x = 1;";
    let mut checkpoint = PerlLexer::new(source).checkpoint();
    checkpoint.__test_stamp_schema(CHECKPOINT_SCHEMA_VERSION + 7);
    assert_eq!(
        PerlLexer::new(source).validate_restore(&checkpoint),
        Err(CheckpointRestoreError::UnknownSchema)
    );
}

#[test]
fn incomplete_quote_state_fails_closed() {
    let source = "qq{hello};";
    let mut checkpoint = PerlLexer::new(source).checkpoint();
    checkpoint.__test_stamp_incomplete_quote();
    assert_eq!(
        PerlLexer::new(source).validate_restore(&checkpoint),
        Err(CheckpointRestoreError::IncompleteState)
    );
}

#[test]
fn overlapping_edit_does_not_become_default_origin() -> R {
    let source = "sub f($$) { return 1; }";
    let mut lexer = PerlLexer::new(source);
    while !lexer.checkpoint().in_prototype() {
        if lexer.next_token().is_none() {
            break;
        }
    }
    let mut checkpoint = lexer.checkpoint();
    assert!(checkpoint.in_prototype());
    let _ = checkpoint.try_apply_edit(checkpoint.position().saturating_sub(1), 4, 1);
    assert!(checkpoint.is_invalidated());
    assert!(checkpoint.in_prototype());
    assert_eq!(
        PerlLexer::new(source).validate_restore(&checkpoint),
        Err(CheckpointRestoreError::Invalidated)
    );
    Ok(())
}

#[test]
fn symbol_table_mismatch_is_wrong_configuration() {
    let source = "builder /pattern/; sub builder { 1 }";
    let table = LocalSymbolTable::scan_subs(source);
    let with_table = PerlLexer::with_config(
        source,
        LexerConfig { symbol_table: Some(table), ..LexerConfig::default() },
    )
    .checkpoint();
    let without = PerlLexer::new(source);
    assert_eq!(
        without.validate_restore(&with_table),
        Err(CheckpointRestoreError::WrongConfiguration)
    );
}

#[test]
fn schema_is_explicit_on_live_checkpoints() {
    let checkpoint = PerlLexer::new("1").checkpoint();
    assert_eq!(checkpoint.identity().schema(), CHECKPOINT_SCHEMA_VERSION);
    assert_eq!(
        checkpoint.identity().newline_policy(),
        perl_lexer::CheckpointNewlinePolicy::SourceExact
    );
}
