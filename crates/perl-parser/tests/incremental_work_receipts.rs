#![cfg(feature = "incremental")]
//! Public proof for canonical incremental strategy/work receipts.

use perl_parser::incremental::{IncrementalStrategy, LexRestartStrategy};
use perl_parser::{Edit, IncrementalState, apply_edits};

#[test]
fn unchanged_receipt_reports_zero_fresh_work() -> anyhow::Result<()> {
    let source = "my $x = 1;";
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[])?;

    assert_eq!(result.work.strategy, IncrementalStrategy::Unchanged);
    assert_eq!(result.work.full_parser_invocations, 0);
    assert_eq!(result.work.recovery_parser_invocations, 0);
    assert_eq!(result.work.validation_parser_invocations, 0);
    assert_eq!(result.work.old_prefix_bytes_replayed, 0);
    assert_eq!(result.work.fresh_bytes_lexed, 0);
    assert_eq!(result.work.fresh_tokens_emitted, 0);
    assert_eq!(result.work.prefix_tokens_retained, state.tokens().len());
    assert_eq!(result.work.suffix_tokens_retained, 0);
    assert_eq!(result.work.nodes_constructed, 0);
    assert_eq!(result.work.final_source_bytes, source.len());
    assert_eq!(result.work.final_token_count, state.tokens().len());
    assert_eq!(result.work.final_node_count, state.parse_output().ast.count_nodes());
    result.work.validate()?;
    Ok(())
}

#[test]
fn stored_checkpoint_path_admits_lexer_reuse_but_not_parser_reuse() -> anyhow::Result<()> {
    let source = "my $before = 1; my $target = 2; my $after = 3;";
    let start = source.find("= 2").expect("target literal missing") + 2;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start + 1,
        new_end_byte: start + 1,
        new_text: "9".to_string(),
    };
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.lex_restart.strategy, LexRestartStrategy::StoredCheckpointToEof);
    assert_eq!(result.work.strategy, IncrementalStrategy::CheckpointToEofThenFullParse);
    assert_eq!(result.work.full_parser_invocations, 1);
    assert_eq!(result.work.recovery_parser_invocations, 1);
    assert_eq!(result.work.validation_parser_invocations, 0);
    assert_eq!(result.work.old_prefix_bytes_replayed, 0);
    assert_eq!(result.work.fresh_bytes_lexed, result.lex_restart.relexed_bytes);
    assert_eq!(result.work.prefix_tokens_retained, result.lex_restart.reused_prefix_tokens);
    assert_eq!(result.work.suffix_tokens_retained, 0);
    assert_eq!(result.work.nodes_retained_by_identity, 0);
    assert_eq!(result.work.nodes_cloned, 0);
    assert_eq!(result.work.nodes_patched, 0);
    assert_eq!(result.work.nodes_compared_only, 0);
    assert_eq!(result.work.checkpoints_restored, 1);
    assert_eq!(result.work.final_token_count, state.tokens().len());
    assert_eq!(result.work.final_node_count, state.parse_output().ast.count_nodes());
    assert_eq!(result.work.nodes_constructed, result.work.final_node_count);
    result.work.validate()?;
    Ok(())
}

#[test]
fn oversized_edit_reports_full_fallback_without_retained_work() -> anyhow::Result<()> {
    let source = "my $x = 1;";
    let replacement = "my $value = 2;\n".repeat(80);
    let edit = Edit {
        start_byte: 0,
        old_end_byte: source.len(),
        new_end_byte: replacement.len(),
        new_text: replacement,
    };
    let mut state = IncrementalState::new(source.to_string());
    let result = apply_edits(&mut state, &[edit])?;

    assert_eq!(result.work.strategy, IncrementalStrategy::FullFallback);
    assert_eq!(result.work.full_parser_invocations, 1);
    assert_eq!(result.work.recovery_parser_invocations, 1);
    assert_eq!(result.work.prefix_tokens_retained, 0);
    assert_eq!(result.work.suffix_tokens_retained, 0);
    assert_eq!(result.work.nodes_retained_by_identity, 0);
    assert_eq!(result.work.fresh_bytes_lexed, state.source().len());
    assert_eq!(result.work.final_source_bytes, state.source().len());
    assert_eq!(result.work.nodes_constructed, result.work.final_node_count);
    result.work.validate()?;
    Ok(())
}