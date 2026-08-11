use super::*;
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
fn test_incremental_state_small_edit_uses_checkpoint() -> Result<()> {
    let source = (0..30usize).map(|i| format!("my $var_{i} = {i};")).collect::<Vec<_>>().join("\n");
    let doc_len = source.len();
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
    assert!(result.reparsed_bytes < doc_len);
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
        let mut expected = state.source.clone();

        for (start, delete_len, insert_text) in edits {
            let fuzz = FuzzEdit { start, delete_len, insert_text };
            let start_byte = fuzz.start.min(state.source.len());
            let old_end = (start_byte + fuzz.delete_len).min(state.source.len());
            apply_edit_to_ground_truth(&mut expected, &fuzz);
            let edit = Edit {
                start_byte,
                old_end_byte: old_end,
                new_end_byte: start_byte + fuzz.insert_text.len(),
                new_text: fuzz.insert_text,
            };
            prop_assert!(apply_edits(&mut state, &[edit]).is_ok());
            prop_assert_eq!(&state.source, &expected);
        }
    }
}

#[test]
fn incremental_state_records_lex_and_parse_restart_points() {
    let source =
        "package Example;\nmy ($scalar, @items);\nsub run { my $local = 1; }\n".to_string();
    let state = IncrementalState::new(source.clone());

    let first_lex = state
        .find_lex_checkpoint(0)
        .expect("the lexer always has an origin checkpoint");
    assert_eq!(first_lex.byte, 0);
    assert!(state.find_lex_checkpoint(source.len()).is_some());

    let package_start = source.find("package").expect("package declaration is present");
    let package_checkpoint = state
        .find_parse_checkpoint(package_start)
        .expect("package declarations create parse checkpoints");
    assert_eq!(package_checkpoint.scope_snapshot.package_name, "Example");

    let sub_start = source.find("sub run").expect("subroutine declaration is present");
    let sub_checkpoint = state
        .find_parse_checkpoint(sub_start)
        .expect("subroutine declarations create parse checkpoints");
    assert_eq!(sub_checkpoint.scope_snapshot.package_name, "Example");
    assert!(sub_checkpoint.node_id > 0);
}

#[test]
fn expression_only_trees_have_no_parse_restart_checkpoint() {
    let state = IncrementalState::new("1 + 2;".to_string());
    assert!(state.find_parse_checkpoint(0).is_none());
}

#[expect(
    deprecated,
    reason = "the test verifies the legacy AST error fallback used by IncrementalState::new"
)]
#[test]
fn incremental_state_retains_error_ast_when_initial_parse_fails() {
    let state = IncrementalState::new("sub {".to_string());
    assert!(matches!(state.ast.kind, NodeKind::Error { .. }));
}
