use super::*;
use crate::Parser;
use anyhow::Result;
use perl_ast::{Node, NodeKind, SourceLocation};
use perl_lexer::Token;
use proptest::prelude::*;

#[derive(Clone, Debug)]
struct FuzzEdit {
    start: usize,
    delete_len: usize,
    insert_text: String,
}

fn apply_edit_to_ground_truth(source: &mut String, edit: &FuzzEdit) {
    let start = edit.start.min(source.len());
    let old_end = (start + edit.delete_len).min(source.len());
    source.replace_range(start..old_end, &edit.insert_text);
}

fn assert_token_streams_equal(actual: &[Token], expected: &[Token]) {
    assert_eq!(actual.len(), expected.len(), "token count diverged");
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(actual.token_type, expected.token_type, "token kind {index}");
        assert_eq!(actual.text, expected.text, "token payload {index}");
        assert_eq!(actual.start, expected.start, "token start {index}");
        assert_eq!(actual.end, expected.end, "token end {index}");
    }
}

fn assert_incremental_matches_fresh(state: &IncrementalState) {
    let mut fresh_parser = Parser::new(state.source());
    let fresh_output = fresh_parser.parse_with_recovery();
    let incremental_output = state.parse_output();

    assert_eq!(incremental_output.ast, fresh_output.ast, "AST diverged from fresh parse");
    assert_eq!(
        incremental_output.diagnostics, fresh_output.diagnostics,
        "diagnostics diverged from fresh parse"
    );
    assert_eq!(incremental_output.recovered_count, fresh_output.recovered_count);
    assert_eq!(incremental_output.terminated_early(), fresh_output.terminated_early());
    assert_eq!(incremental_output.stop_cause(), fresh_output.stop_cause());
    assert_eq!(
        state.snapshot().disposition(),
        ParseSnapshot::from_output(
            state.source(),
            state.generation(),
            ParseSnapshotStrategy::Fresh,
            fresh_output.clone(),
        )
        .disposition()
    );
    assert!(state.snapshot().validate_against(state.source()).is_ok());
}

fn assert_restored_token_stream_matches_fresh(source: &str) -> Result<()> {
    let line_index = LineIndex::new(source);
    let fresh_lexed = super::lex::lex_source_with_checkpoints(source, &line_index);
    let checkpoint = fresh_lexed
        .stored_checkpoints
        .iter()
        .find(|stored| {
            stored.live.position > 0
                && stored.live.position < source.len()
                && !stored.live.is_timeout_sensitive()
        })
        .ok_or_else(|| anyhow::anyhow!("fixture must retain a restorable checkpoint"))?;
    let prefix_len = fresh_lexed
        .tokens
        .iter()
        .position(|token| token.start >= checkpoint.live.position)
        .unwrap_or(fresh_lexed.tokens.len());
    let restored = super::lex::lex_from_live_checkpoint(source, &line_index, &checkpoint.live)?;

    assert_token_streams_equal(&restored.tokens, &fresh_lexed.tokens[prefix_len..]);
    assert_eq!(restored.terminal_checkpoint, fresh_lexed.terminal_checkpoint);

    let mut assembled = fresh_lexed.tokens[..prefix_len].to_vec();
    assembled.extend(restored.tokens);
    let parser_tokens = crate::TokenStream::lexer_tokens_to_parser_tokens(assembled);
    let mut resumed_parser = Parser::from_tokens(parser_tokens, source);
    let resumed_output = resumed_parser.parse_with_recovery();
    let mut fresh_parser = Parser::new(source);
    let fresh_output = fresh_parser.parse_with_recovery();

    assert_eq!(resumed_output.ast, fresh_output.ast, "restored token AST diverged");
    assert_eq!(resumed_output.diagnostics, fresh_output.diagnostics);
    assert_eq!(resumed_output.recovered_count, fresh_output.recovered_count);
    assert_eq!(resumed_output.terminated_early(), fresh_output.terminated_early());
    assert_eq!(resumed_output.stop_cause(), fresh_output.stop_cause());
    Ok(())
}

#[test]
fn test_incremental_state_small_edit_reuses_tokens_before_full_parse() -> Result<()> {
    let source = (0..30usize).map(|i| format!("my $var_{i} = {i};")).collect::<Vec<_>>().join("\n");
    let mut state = IncrementalState::new(source.clone());
    assert!(state.lex_checkpoints.len() > 1);
    let edit_start =
        source.find("10;").ok_or_else(|| anyhow::anyhow!("test source is missing edit target"))?;
    let edit = Edit {
        start_byte: edit_start,
        old_end_byte: edit_start + 2,
        new_end_byte: edit_start + 3,
        new_text: "999".to_string(),
    };

    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.reparsed_bytes, state.source().len());
    assert!(result.token_count > 0);
    assert!(result.reused_tokens > 0);
    assert!(result.reused_tokens <= result.token_count);
    Ok(())
}

proptest! {
    #[test]
    fn prop_incremental_apply_edits_matches_ground_truth(
        edits in prop::collection::vec(
            (0usize..240usize, 0usize..24usize, "[a-zA-Z0-9_ ]{0,24}"),
            1..20,
        )
    ) {
        let mut state = IncrementalState::new("my $seed = 0;\n".repeat(80));
        let mut expected = state.source().to_owned();

        for (start, delete_len, insert_text) in edits {
            let fuzz = FuzzEdit { start, delete_len, insert_text };
            let start_byte = fuzz.start.min(state.source().len());
            let old_end = (start_byte + fuzz.delete_len).min(state.source().len());
            apply_edit_to_ground_truth(&mut expected, &fuzz);
            let edit = Edit {
                start_byte,
                old_end_byte: old_end,
                new_end_byte: start_byte + fuzz.insert_text.len(),
                new_text: fuzz.insert_text,
            };
            prop_assert!(apply_edits(&mut state, &[edit]).is_ok());
            prop_assert_eq!(state.source(), &expected);

            assert_incremental_matches_fresh(&state);
        }
    }
}

#[test]
fn stateful_checkpoint_restart_matches_fresh_tokens_and_ast() -> Result<()> {
    let prefix = "my $seed = 0;\n".repeat(24);
    let source = format!(
        "{prefix}my $value = $input =~ /a\\/b/ ? s{{old}}{{new}} : qq{{value}};\nmy $tail = 1;\n"
    );
    let edit_start =
        source.find("a\\/b").ok_or_else(|| anyhow::anyhow!("regex fixture is missing"))?;
    let edit = Edit {
        start_byte: edit_start,
        old_end_byte: edit_start + "a\\/b".len(),
        new_end_byte: edit_start + "c\\/d".len(),
        new_text: "c\\/d".to_string(),
    };

    let mut state = IncrementalState::new(source);
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
    assert!(result.lex_restart.restart_byte > 0);
    assert_incremental_matches_fresh(&state);
    assert_restored_token_stream_matches_fresh(state.source())?;
    Ok(())
}

#[test]
fn unicode_crlf_checkpoint_restart_preserves_spans() -> Result<()> {
    let prefix = "my $seed = 0;\r\n".repeat(24);
    let source = format!("{prefix}my $value = 'café';\r\nmy $tail = 1;\r\n");
    let edit_start =
        source.find("café").ok_or_else(|| anyhow::anyhow!("unicode fixture is missing"))?;
    let edit = Edit {
        start_byte: edit_start,
        old_end_byte: edit_start + "café".len(),
        new_end_byte: edit_start + "茶".len(),
        new_text: "茶".to_string(),
    };

    let mut state = IncrementalState::new(source);
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
    assert!(result.lex_restart.restart_byte > 0);
    assert_incremental_matches_fresh(&state);
    assert_restored_token_stream_matches_fresh(state.source())?;
    Ok(())
}

#[test]
fn recovery_checkpoint_restart_matches_fresh_diagnostics() -> Result<()> {
    let prefix = "my $seed = 0;\n".repeat(24);
    let source = format!("{prefix}my $value = ;\nmy $tail = 1;\n");
    let edit_start =
        source.find("1;").ok_or_else(|| anyhow::anyhow!("recovery fixture is missing"))?;
    let edit = Edit {
        start_byte: edit_start,
        old_end_byte: edit_start + "1".len(),
        new_end_byte: edit_start + "2".len(),
        new_text: "2".to_string(),
    };

    let mut state = IncrementalState::new(source);
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
    assert!(result.lex_restart.restart_byte > 0);
    assert!(!state.parse_output().diagnostics.is_empty());
    assert_incremental_matches_fresh(&state);
    assert_restored_token_stream_matches_fresh(state.source())?;
    Ok(())
}

#[test]
fn incremental_state_records_lex_and_parse_restart_points() -> Result<()> {
    let source =
        "package Example;\nmy ($scalar, @items);\nsub run { my $local = 1; }\n".to_string();
    let state = IncrementalState::new(source.clone());

    let first_lex = state
        .find_lex_checkpoint(0)
        .ok_or_else(|| anyhow::anyhow!("the lexer always has an origin checkpoint"))?;
    anyhow::ensure!(first_lex.byte == 0, "the origin checkpoint must start at byte 0");
    anyhow::ensure!(
        state.find_lex_checkpoint(source.len()).is_some(),
        "the lexer must retain a checkpoint at the document end"
    );

    let package_start = source
        .find("package")
        .ok_or_else(|| anyhow::anyhow!("package declaration not found in source"))?;
    let package_checkpoint = state
        .find_parse_checkpoint(package_start)
        .ok_or_else(|| anyhow::anyhow!("package declarations create parse checkpoints"))?;
    anyhow::ensure!(
        package_checkpoint.scope_snapshot.package_name == "Example",
        "package checkpoint must retain the package scope"
    );

    let sub_start = source
        .find("sub run")
        .ok_or_else(|| anyhow::anyhow!("subroutine declaration not found in source"))?;
    let sub_checkpoint = state
        .find_parse_checkpoint(sub_start)
        .ok_or_else(|| anyhow::anyhow!("subroutine declarations create parse checkpoints"))?;
    anyhow::ensure!(
        sub_checkpoint.scope_snapshot.package_name == "Example",
        "subroutine checkpoint must retain the package scope"
    );
    anyhow::ensure!(sub_checkpoint.node_id > 0, "subroutine checkpoint must retain a node id");
    Ok(())
}

#[test]
fn parse_checkpoints_capture_scalar_and_list_locals_before_nested_blocks() -> Result<()> {
    // The block under test is a top-level statement: `walk_ast_for_checkpoints`
    // recurses only through `Program`/`Block` statement lists and never enters
    // `Subroutine.body`, so a subroutine-body block is not a reachable fixture.
    let source = "package Example;\nmy $scalar;\nmy ($first, @items);\nsub run { my $local = 1; }\n{ my $block_local = 1; }\n";
    let state = IncrementalState::new(source.to_string());

    let sub_start = source
        .find("sub run")
        .ok_or_else(|| anyhow::anyhow!("subroutine declaration not found in source"))?;
    let sub_checkpoint = state
        .find_parse_checkpoint(sub_start)
        .ok_or_else(|| anyhow::anyhow!("subroutine declaration must create a checkpoint"))?;
    assert_eq!(
        sub_checkpoint.scope_snapshot.locals,
        vec!["$scalar".to_string(), "$first".to_string(), "@items".to_string()],
        "scalar and list declarations before the subroutine must be retained"
    );

    let block_start = source
        .find("{ my $block_local")
        .ok_or_else(|| anyhow::anyhow!("top-level block not found in source"))?;
    let block_checkpoint = state
        .find_parse_checkpoint(block_start)
        .ok_or_else(|| anyhow::anyhow!("nested blocks must create a checkpoint"))?;
    // `find_parse_checkpoint` returns the nearest checkpoint at or before the
    // byte; require the block's own checkpoint so a missing block emission
    // cannot be masked by the preceding subroutine checkpoint.
    assert_eq!(
        block_checkpoint.byte, block_start,
        "the lookup must return the block's own checkpoint, not a predecessor"
    );
    assert_eq!(
        block_checkpoint.scope_snapshot.locals,
        vec!["$scalar".to_string(), "$first".to_string(), "@items".to_string()],
        "the block checkpoint must retain the enclosing lexical scope"
    );
    assert!(
        !block_checkpoint.scope_snapshot.locals.contains(&"$block_local".to_string()),
        "a block's entry checkpoint must not include declarations inside the block"
    );
    Ok(())
}

#[test]
fn parse_checkpoints_restore_block_locals_before_later_sibling() -> Result<()> {
    // Direct-AST discriminator for scope-exit: after `{ my $inner; }`, a later
    // sibling block must not see `$inner`. A walker that mutates one shared
    // snapshot and never restores it fails this assertion.
    let location = |start, end| SourceLocation::new(start, end);
    let inner = Node::new(
        NodeKind::VariableDeclaration {
            declarator: "my".to_string(),
            variable: Box::new(Node::new(
                NodeKind::Variable { sigil: "$".to_string(), name: "inner".to_string() },
                location(1, 10),
            )),
            attributes: vec![],
            initializer: None,
        },
        location(1, 10),
    );
    let first_block = Node::new(NodeKind::Block { statements: vec![inner] }, location(0, 12));
    let later_block = Node::new(NodeKind::Block { statements: vec![] }, location(20, 22));
    let program = Node::new(
        NodeKind::Program { statements: vec![first_block, later_block] },
        location(0, 22),
    );
    let checkpoints = IncrementalState::create_parse_checkpoints(&program);
    let checkpoint = checkpoints
        .iter()
        .find(|checkpoint| checkpoint.byte == 20)
        .ok_or_else(|| anyhow::anyhow!("later sibling block must create a checkpoint"))?;
    assert_eq!(
        checkpoint.scope_snapshot.locals,
        Vec::<String>::new(),
        "a later sibling checkpoint must not retain locals from an earlier closed block"
    );
    Ok(())
}

#[test]
fn parse_checkpoints_parsed_source_block_local_does_not_leak_into_later_sub() -> Result<()> {
    // Parsed-source repro from #12466: `{ my $block_local = 1; }` is a
    // walker-reachable top-level block before `sub run`. The subroutine
    // checkpoint must keep package-scope accumulation and must not see the
    // closed block local.
    let source = "package Example;\nmy $scalar;\nmy ($first, @items);\n{ my $block_local = 1; }\nsub run { my $local = 1; }\n";
    let state = IncrementalState::new(source.to_string());

    let block_start = source
        .find("{ my $block_local")
        .ok_or_else(|| anyhow::anyhow!("top-level block not found in source"))?;
    let block_checkpoint = state
        .find_parse_checkpoint(block_start)
        .ok_or_else(|| anyhow::anyhow!("nested blocks must create a checkpoint"))?;
    assert_eq!(
        block_checkpoint.byte, block_start,
        "the lookup must return the block's own checkpoint, not a predecessor"
    );
    assert_eq!(
        block_checkpoint.scope_snapshot.locals,
        vec!["$scalar".to_string(), "$first".to_string(), "@items".to_string()],
        "the block entry checkpoint must retain enclosing lexical scope"
    );

    let sub_start = source
        .find("sub run")
        .ok_or_else(|| anyhow::anyhow!("subroutine declaration not found in source"))?;
    let sub_checkpoint = state
        .find_parse_checkpoint(sub_start)
        .ok_or_else(|| anyhow::anyhow!("subroutine declaration must create a checkpoint"))?;
    assert_eq!(
        sub_checkpoint.byte, sub_start,
        "the lookup must return the subroutine's own checkpoint, not a predecessor"
    );
    assert_eq!(
        sub_checkpoint.scope_snapshot.locals,
        vec!["$scalar".to_string(), "$first".to_string(), "@items".to_string()],
        "package-level declarations must still accumulate across earlier siblings"
    );
    assert!(
        !sub_checkpoint.scope_snapshot.locals.contains(&"$block_local".to_string()),
        "a later subroutine checkpoint must not retain a local from a closed block"
    );
    Ok(())
}

#[test]
fn parse_checkpoints_restore_inner_block_without_dropping_enclosing_locals() -> Result<()> {
    // Opposite-direction control for restore: clearing the whole snapshot on
    // block exit would drop `$outer` as well as `$inner`. A later sibling
    // inside the same enclosing block must still see the enclosing local.
    let location = |start, end| SourceLocation::new(start, end);
    let outer = Node::new(
        NodeKind::VariableDeclaration {
            declarator: "my".to_string(),
            variable: Box::new(Node::new(
                NodeKind::Variable { sigil: "$".to_string(), name: "outer".to_string() },
                location(1, 10),
            )),
            attributes: vec![],
            initializer: None,
        },
        location(1, 10),
    );
    let inner = Node::new(
        NodeKind::VariableDeclaration {
            declarator: "my".to_string(),
            variable: Box::new(Node::new(
                NodeKind::Variable { sigil: "$".to_string(), name: "inner".to_string() },
                location(12, 21),
            )),
            attributes: vec![],
            initializer: None,
        },
        location(12, 21),
    );
    let inner_block = Node::new(NodeKind::Block { statements: vec![inner] }, location(11, 23));
    let later_block = Node::new(NodeKind::Block { statements: vec![] }, location(24, 26));
    let enclosing = Node::new(
        NodeKind::Block { statements: vec![outer, inner_block, later_block] },
        location(0, 27),
    );
    let checkpoints = IncrementalState::create_parse_checkpoints(&enclosing);
    let checkpoint = checkpoints
        .iter()
        .find(|checkpoint| checkpoint.byte == 24)
        .ok_or_else(|| anyhow::anyhow!("later sibling block must create a checkpoint"))?;
    assert_eq!(
        checkpoint.scope_snapshot.locals,
        vec!["$outer".to_string()],
        "restoring a nested block must keep enclosing locals and drop only the inner ones"
    );
    Ok(())
}

#[test]
fn parse_checkpoints_accumulate_nested_scalar_locals_in_source_order() -> Result<()> {
    let location = |start, end| SourceLocation::new(start, end);
    let first = Node::new(
        NodeKind::VariableDeclaration {
            declarator: "my".to_string(),
            variable: Box::new(Node::new(
                NodeKind::Variable { sigil: "$".to_string(), name: "first".to_string() },
                location(1, 7),
            )),
            attributes: vec![],
            initializer: None,
        },
        location(1, 7),
    );
    let nested_block = Node::new(NodeKind::Block { statements: vec![] }, location(20, 22));
    let outer_block =
        Node::new(NodeKind::Block { statements: vec![first, nested_block] }, location(0, 23));
    let checkpoints = IncrementalState::create_parse_checkpoints(&outer_block);
    let checkpoint = checkpoints
        .iter()
        .find(|checkpoint| checkpoint.byte == 20)
        .ok_or_else(|| anyhow::anyhow!("nested block must create a checkpoint"))?;
    assert_eq!(
        checkpoint.scope_snapshot.locals,
        vec!["$first".to_string()],
        "a nested block checkpoint must see locals declared before it"
    );
    Ok(())
}

#[test]
fn expression_only_trees_have_no_parse_restart_checkpoint() -> Result<()> {
    let state = IncrementalState::new("1 + 2;".to_string());
    anyhow::ensure!(
        state.find_parse_checkpoint(0).is_none(),
        "expression-only trees must not create parse restart checkpoints"
    );
    Ok(())
}

#[test]
fn incremental_state_retains_recovery_output_when_initial_parse_fails() -> Result<()> {
    let source = "(".repeat(256);
    let state = IncrementalState::new(source.clone());
    let mut parser = Parser::new(&source);
    let fresh = parser.parse_with_recovery();

    anyhow::ensure!(
        !fresh.diagnostics.is_empty(),
        "the malformed source must exercise recovery diagnostics"
    );
    anyhow::ensure!(
        state.parse_output().ast == fresh.ast,
        "initial state must retain the recovery-aware AST"
    );
    anyhow::ensure!(
        state.parse_output().diagnostics == fresh.diagnostics,
        "initial state must retain the recovery diagnostics"
    );
    Ok(())
}
