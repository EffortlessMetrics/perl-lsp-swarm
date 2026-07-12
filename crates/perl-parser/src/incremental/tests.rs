use super::*;
use anyhow::Result;
use perl_parser_core::parser::Parser;
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

#[test]
fn safe_single_edits_use_the_shared_core_kernel() -> Result<()> {
    let source = "my $value = 1;\n".repeat(80);
    let start = source
        .find('1')
        .ok_or_else(|| anyhow::anyhow!("test source is missing the edit target"))?;
    let new_source = source.replacen('1', "22", 1);
    let edit = Edit {
        start_byte: start,
        old_end_byte: start + 1,
        new_end_byte: start + 2,
        new_text: "22".to_owned(),
    };

    let mut state = IncrementalState::new(source);
    let result = apply_edits(&mut state, &[edit])?;
    let mut fresh_parser = Parser::new(&new_source);
    let fresh =
        fresh_parser.parse().map_err(|error| anyhow::anyhow!("fresh parse failed: {error}"))?;

    if state.core_state.is_none() {
        return Err(anyhow::anyhow!("safe edit did not retain the shared core state"));
    }
    if result.reused_tokens == 0 {
        return Err(anyhow::anyhow!("safe edit reported no reused tokens"));
    }
    if result.reparsed_bytes >= new_source.len() {
        return Err(anyhow::anyhow!("safe edit reparsed the complete source"));
    }
    if state.ast.to_sexp() != fresh.to_sexp() {
        return Err(anyhow::anyhow!("core-backed AST differs from a fresh parse"));
    }
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
