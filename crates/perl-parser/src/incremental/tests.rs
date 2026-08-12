use super::*;
use crate::Parser;
use anyhow::Result;
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

#[test]
fn test_incremental_state_small_edit_reuses_tokens_before_full_parse() -> Result<()> {
    let source =
        (0..30usize).map(|i| format!("my $var_{i} = {i};")).collect::<Vec<_>>().join("\n");
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

            let mut cold_parser = Parser::new(&expected);
            let cold_ast = cold_parser.parse().map_err(|err| {
                TestCaseError::fail(format!("cold parse failed after edit: {err}"))
            })?;
            prop_assert_eq!(
                state.parse_output().ast.to_sexp(),
                cold_ast.to_sexp(),
                "incremental AST diverged from a fresh cold parse"
            );
        }
    }
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
