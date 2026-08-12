#![cfg(feature = "incremental")]
//! Differential recovery-transition proof for generation-bound parse snapshots.

use perl_parser::incremental::{
    ContentFingerprint, Edit, IncrementalState, ParseSnapshot, ParseSnapshotStrategy,
    ParseTerminalDisposition, apply_edits,
};
use perl_parser::{ParseOutput, Parser};

fn fresh_output(source: &str) -> ParseOutput {
    Parser::new(source).parse_with_recovery()
}

fn replacement(source: &str, needle: &str, replacement: &str) -> Edit {
    let start = source.find(needle).unwrap_or_else(|| panic!("fixture needle {needle:?} missing"));
    Edit {
        start_byte: start,
        old_end_byte: start + needle.len(),
        new_end_byte: start + replacement.len(),
        new_text: replacement.to_string(),
    }
}

fn insertion(source: &str, byte: usize, text: &str) -> Edit {
    assert!(byte <= source.len());
    assert!(source.is_char_boundary(byte));
    Edit {
        start_byte: byte,
        old_end_byte: byte,
        new_end_byte: byte + text.len(),
        new_text: text.to_string(),
    }
}

fn apply_reference(source: &str, edits: &[Edit]) -> String {
    let mut final_source = source.to_string();
    let mut ordered = edits.to_vec();
    ordered.sort_by_key(|edit| edit.start_byte);
    ordered.reverse();
    for edit in ordered {
        final_source.replace_range(edit.start_byte..edit.old_end_byte, &edit.new_text);
    }
    final_source
}

fn assert_budget_equal(actual: &ParseOutput, expected: &ParseOutput) {
    assert_eq!(actual.budget_usage.errors_emitted, expected.budget_usage.errors_emitted);
    assert_eq!(actual.budget_usage.current_depth, expected.budget_usage.current_depth);
    assert_eq!(actual.budget_usage.max_depth_reached, expected.budget_usage.max_depth_reached);
    assert_eq!(actual.budget_usage.tokens_skipped, expected.budget_usage.tokens_skipped);
    assert_eq!(
        actual.budget_usage.recoveries_attempted,
        expected.budget_usage.recoveries_attempted
    );
}

fn assert_parse_output_equal(actual: &ParseOutput, expected: &ParseOutput) {
    assert_eq!(actual.ast, expected.ast);
    assert_eq!(actual.diagnostics, expected.diagnostics);
    assert_eq!(actual.terminated_early, expected.terminated_early);
    assert_eq!(actual.recovered_count, expected.recovered_count);
    assert_budget_equal(actual, expected);
}

fn assert_snapshot_equal(actual: &ParseSnapshot, expected: &ParseSnapshot) {
    assert_eq!(actual.generation, expected.generation);
    assert_eq!(actual.content_fingerprint, expected.content_fingerprint);
    assert_eq!(actual.source_len, expected.source_len);
    assert_eq!(actual.disposition, expected.disposition);
    assert_eq!(actual.strategy, expected.strategy);
    assert_parse_output_equal(&actual.parse_output, &expected.parse_output);
}

fn assert_fresh_parity(
    initial: &str,
    edits: &[Edit],
    expected_disposition: ParseTerminalDisposition,
) -> anyhow::Result<IncrementalState> {
    let final_source = apply_reference(initial, edits);
    let expected = fresh_output(&final_source);
    let mut state = IncrementalState::new(initial.to_string());
    let result = apply_edits(&mut state, edits)?;

    assert_eq!(state.source(), final_source);
    assert_eq!(state.generation().get(), 1);
    assert_eq!(result.snapshot.generation, state.generation());
    assert_eq!(result.snapshot.content_fingerprint, ContentFingerprint::from_source(&final_source));
    assert_eq!(result.snapshot.source_len, final_source.len());
    assert_eq!(result.snapshot.disposition, expected_disposition);
    assert_ne!(result.snapshot.strategy, ParseSnapshotStrategy::Fresh);
    assert_parse_output_equal(&result.snapshot.parse_output, &expected);
    assert_parse_output_equal(&result.parse_output, &result.snapshot.parse_output);
    assert_snapshot_equal(&result.snapshot, state.snapshot());
    result.snapshot.validate_against(state.source())?;
    state.snapshot().validate_against(state.source())?;
    Ok(state)
}

#[test]
fn clean_recovered_and_repaired_transitions_match_fresh_parsing() -> anyhow::Result<()> {
    let clean = "my $x = 1;";
    let clean_to_clean = replacement(clean, "1", "2");
    let state = assert_fresh_parity(clean, &[clean_to_clean], ParseTerminalDisposition::Clean)?;
    assert_eq!(state.snapshot().disposition, ParseTerminalDisposition::Clean);

    let clean_to_recovered = replacement(clean, "1", "");
    let state =
        assert_fresh_parity(clean, &[clean_to_recovered], ParseTerminalDisposition::Recovered)?;
    assert_eq!(state.snapshot().disposition, ParseTerminalDisposition::Recovered);

    let recovered = "my $x = ;";
    let recovered_to_clean =
        insertion(recovered, recovered.find(';').expect("semicolon missing"), "1");
    let state =
        assert_fresh_parity(recovered, &[recovered_to_clean], ParseTerminalDisposition::Clean)?;
    assert_eq!(state.snapshot().disposition, ParseTerminalDisposition::Clean);
    Ok(())
}

#[test]
fn recovered_diagnostics_shift_and_change_family_exactly_like_fresh_output() -> anyhow::Result<()> {
    let source = "my $x = ;\nmy $y = 2;\n";
    let old_diagnostics = fresh_output(source).diagnostics;
    assert!(!old_diagnostics.is_empty());

    let prefix = insertion(source, 0, "# café\r\n");
    let shifted = assert_fresh_parity(source, &[prefix], ParseTerminalDisposition::Recovered)?;
    assert_eq!(shifted.snapshot().disposition, ParseTerminalDisposition::Recovered);
    assert_ne!(shifted.snapshot().parse_output.diagnostics, old_diagnostics);

    let changed_family = replacement(source, "my $x = ;", "my $x = (");
    let changed =
        assert_fresh_parity(source, &[changed_family], ParseTerminalDisposition::Recovered)?;
    assert_eq!(changed.snapshot().disposition, ParseTerminalDisposition::Recovered);
    assert_ne!(changed.snapshot().parse_output.diagnostics, old_diagnostics);
    Ok(())
}

#[test]
fn multi_edit_transactions_publish_one_final_recovery_snapshot() -> anyhow::Result<()> {
    let source = "my $x = ;\nmy $y = ;\n";
    let first = insertion(source, source.find(';').expect("first semicolon missing"), "1");
    let second_start = source.rfind(';').expect("second semicolon missing");
    let second = insertion(source, second_start, "2");

    let state = assert_fresh_parity(source, &[first, second], ParseTerminalDisposition::Clean)?;

    assert_eq!(state.generation().get(), 1);
    assert_eq!(state.snapshot().disposition, ParseTerminalDisposition::Clean);
    assert!(state.snapshot().parse_output.diagnostics.is_empty());
    Ok(())
}

#[test]
fn stateful_unicode_and_newline_transition_matrix_has_exact_fresh_parity() -> anyhow::Result<()> {
    let cases = [
        ("my $text = qq{café};\n", "café", "cafø", ParseTerminalDisposition::Clean),
        ("my $ok = /foo/;\r\n", "foo", "bar", ParseTerminalDisposition::Clean),
        ("my $value = <<EOF;\nbody\nEOF\n", "body", "changed", ParseTerminalDisposition::Clean),
        ("my $x = 1;\rmy $y = 2;\r", "2", "3", ParseTerminalDisposition::Clean),
        ("\u{feff}my $x = 1;\n", "1", "2", ParseTerminalDisposition::Clean),
        ("", "", "my $x = ;", ParseTerminalDisposition::Recovered),
        ("   \n", "   ", "my $x = ;", ParseTerminalDisposition::Recovered),
    ];

    for (source, needle, replacement_text, expected_disposition) in cases {
        let edit = if source.is_empty() {
            insertion(source, 0, replacement_text)
        } else {
            replacement(source, needle, replacement_text)
        };
        let _ = assert_fresh_parity(source, &[edit], expected_disposition)?;
    }
    Ok(())
}

#[test]
fn invalid_transaction_preserves_the_complete_previous_snapshot() {
    let source = "my $x = ;";
    let mut state = IncrementalState::new(source.to_string());
    let generation = state.generation();
    let fingerprint = state.snapshot().content_fingerprint;
    let disposition = state.snapshot().disposition;
    let diagnostics = state.snapshot().parse_output.diagnostics.clone();
    let ast = state.snapshot().parse_output.ast.clone();

    let result = apply_edits(
        &mut state,
        &[
            Edit { start_byte: 0, old_end_byte: 5, new_end_byte: 1, new_text: "x".to_string() },
            Edit { start_byte: 2, old_end_byte: 7, new_end_byte: 3, new_text: "y".to_string() },
        ],
    );

    assert!(result.is_err());
    assert_eq!(state.source(), source);
    assert_eq!(state.generation(), generation);
    assert_eq!(state.snapshot().content_fingerprint, fingerprint);
    assert_eq!(state.snapshot().disposition, disposition);
    assert_eq!(state.snapshot().parse_output.diagnostics, diagnostics);
    assert_eq!(state.snapshot().parse_output.ast, ast);
    assert!(state.snapshot().validate_against(state.source()).is_ok());
}
