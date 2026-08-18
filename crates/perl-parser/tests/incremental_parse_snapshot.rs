use perl_parser::incremental::{
    Edit, IncrementalState, ParseGeneration, ParseTerminalDisposition, apply_edits,
};

#[test]
fn committed_edits_advance_one_generation_and_bind_exact_source() -> anyhow::Result<()> {
    let mut state = IncrementalState::new("my $x = 1;".to_string());
    assert_eq!(state.generation(), ParseGeneration::INITIAL);
    assert_eq!(state.snapshot().disposition(), ParseTerminalDisposition::Clean);
    state.snapshot().validate_against(state.source())?;

    let result = apply_edits(
        &mut state,
        &[Edit { start_byte: 8, old_end_byte: 9, new_end_byte: 8, new_text: String::new() }],
    )?;

    assert_eq!(state.generation().get(), 1);
    assert_eq!(result.snapshot.generation(), state.generation());
    assert_eq!(state.snapshot().disposition(), ParseTerminalDisposition::Recovered);
    assert_eq!(
        result.snapshot.parse_output().diagnostics.len(),
        result.parse_output().diagnostics.len()
    );
    state.snapshot().validate_against(state.source())?;
    Ok(())
}

#[test]
fn recovery_to_clean_publishes_a_new_clean_generation() -> anyhow::Result<()> {
    let mut state = IncrementalState::new("my $x = ;".to_string());
    assert_eq!(state.snapshot().disposition(), ParseTerminalDisposition::Recovered);

    apply_edits(
        &mut state,
        &[Edit { start_byte: 8, old_end_byte: 8, new_end_byte: 9, new_text: "1".to_string() }],
    )?;

    assert_eq!(state.generation().get(), 1);
    assert_eq!(state.snapshot().disposition(), ParseTerminalDisposition::Clean);
    state.snapshot().validate_against(state.source())?;
    Ok(())
}

#[test]
fn invalid_transaction_preserves_the_previous_snapshot_exactly() {
    let mut state = IncrementalState::new("my $x = 1;".to_string());
    let generation = state.generation();
    let fingerprint = state.snapshot().content_digest().clone();
    let source = state.source().to_string();

    let result = apply_edits(
        &mut state,
        &[
            Edit { start_byte: 0, old_end_byte: 4, new_end_byte: 1, new_text: "x".to_string() },
            Edit { start_byte: 2, old_end_byte: 5, new_end_byte: 3, new_text: "y".to_string() },
        ],
    );

    assert!(result.is_err());
    assert_eq!(state.source(), source);
    assert_eq!(state.generation(), generation);
    assert_eq!(state.snapshot().content_digest(), &fingerprint);
    assert!(state.snapshot().validate_against(state.source()).is_ok());
}

#[test]
fn empty_edit_batch_is_generation_neutral() -> anyhow::Result<()> {
    let mut state = IncrementalState::new("my $x = 1;".to_string());
    let generation = state.generation();
    let fingerprint = state.snapshot().content_digest().clone();

    let result = apply_edits(&mut state, &[])?;

    assert_eq!(state.generation(), generation);
    assert_eq!(result.snapshot.generation(), generation);
    assert_eq!(state.snapshot().content_digest(), &fingerprint);
    assert!(result.changed_ranges.is_empty());
    Ok(())
}

#[test]
fn a_stale_generation_snapshot_is_rejected_against_the_committed_source() -> anyhow::Result<()> {
    let mut state = IncrementalState::new("my $x = 1;".to_string());
    let stale = state.snapshot().clone();
    assert_eq!(stale.generation(), ParseGeneration::INITIAL);

    apply_edits(
        &mut state,
        &[Edit { start_byte: 8, old_end_byte: 9, new_end_byte: 8, new_text: String::new() }],
    )?;

    // The old generation's snapshot must not validate against the new source,
    // and its generation must lag the committed state.
    assert_eq!(state.generation().get(), 1);
    assert!(stale.generation() < state.generation());
    assert!(stale.validate_against(state.source()).is_err());
    // The committed snapshot still validates.
    state.snapshot().validate_against(state.source())?;
    Ok(())
}
