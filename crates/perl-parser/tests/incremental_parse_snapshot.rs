use perl_parser::incremental::{
    Edit, IncrementalState, ParseGeneration, ParseTerminalDisposition,
    SourceGeometryAttachmentState, SourceGeometryUnavailableReason, apply_edits,
};

fn assert_geometry_unavailable_for_current_snapshot(state: &IncrementalState) {
    let attachment = state.snapshot().source_geometry();
    assert_eq!(attachment.subject().generation(), state.generation());
    assert_eq!(attachment.subject().content_digest(), state.snapshot().content_digest());
    assert_eq!(attachment.subject().source_len(), state.source().len());
    assert_eq!(attachment.subject().disposition(), state.snapshot().disposition());
    assert_eq!(attachment.subject().strategy(), state.snapshot().strategy());
    assert!(matches!(
        attachment.state(),
        SourceGeometryAttachmentState::Unavailable {
            reason: SourceGeometryUnavailableReason::ProducerNotRun
        }
    ));
}

#[test]
fn independently_created_identical_states_have_distinct_geometry_instances() {
    let first = IncrementalState::new("my $x = 1;".to_string());
    let reopened = IncrementalState::new("my $x = 1;".to_string());

    assert_eq!(first.generation(), reopened.generation());
    assert_eq!(first.snapshot().content_digest(), reopened.snapshot().content_digest());
    assert!(
        !first
            .snapshot()
            .source_geometry()
            .subject()
            .same_instance_as(reopened.snapshot().source_geometry().subject())
    );
    assert_ne!(
        first.snapshot().source_geometry().subject(),
        reopened.snapshot().source_geometry().subject()
    );
}

#[test]
fn committed_edits_advance_one_generation_and_bind_exact_source() -> anyhow::Result<()> {
    let mut state = IncrementalState::new("my $x = 1;".to_string());
    assert_eq!(state.generation(), ParseGeneration::INITIAL);
    assert_eq!(state.snapshot().disposition(), ParseTerminalDisposition::Clean);
    state.snapshot().validate_against(state.source())?;
    assert_geometry_unavailable_for_current_snapshot(&state);
    let initial_subject = state.snapshot().source_geometry().subject().clone();

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
    assert!(initial_subject.same_instance_as(state.snapshot().source_geometry().subject()));
    assert_ne!(&initial_subject, state.snapshot().source_geometry().subject());
    state.snapshot().validate_against(state.source())?;
    assert_geometry_unavailable_for_current_snapshot(&state);
    Ok(())
}

#[test]
fn recovery_to_clean_publishes_a_new_clean_generation() -> anyhow::Result<()> {
    let mut state = IncrementalState::new("my $x = ;".to_string());
    assert_eq!(state.snapshot().disposition(), ParseTerminalDisposition::Recovered);
    assert_geometry_unavailable_for_current_snapshot(&state);

    apply_edits(
        &mut state,
        &[Edit { start_byte: 8, old_end_byte: 8, new_end_byte: 9, new_text: "1".to_string() }],
    )?;

    assert_eq!(state.generation().get(), 1);
    assert_eq!(state.snapshot().disposition(), ParseTerminalDisposition::Clean);
    state.snapshot().validate_against(state.source())?;
    assert_geometry_unavailable_for_current_snapshot(&state);
    Ok(())
}

#[test]
fn invalid_transaction_preserves_the_previous_snapshot_exactly() {
    let mut state = IncrementalState::new("my $x = 1;".to_string());
    let generation = state.generation();
    let fingerprint = state.snapshot().content_digest().clone();
    let geometry = state.snapshot().source_geometry().clone();
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
    assert_eq!(state.snapshot().source_geometry(), &geometry);
    assert!(state.snapshot().validate_against(state.source()).is_ok());
}

#[test]
fn empty_edit_batch_is_generation_neutral() -> anyhow::Result<()> {
    let mut state = IncrementalState::new("my $x = 1;".to_string());
    let generation = state.generation();
    let fingerprint = state.snapshot().content_digest().clone();
    let geometry = state.snapshot().source_geometry().clone();

    let result = apply_edits(&mut state, &[])?;

    assert_eq!(state.generation(), generation);
    assert_eq!(result.snapshot.generation(), generation);
    assert_eq!(state.snapshot().content_digest(), &fingerprint);
    assert_eq!(state.snapshot().source_geometry(), &geometry);
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
    assert!(
        stale
            .source_geometry()
            .subject()
            .same_instance_as(state.snapshot().source_geometry().subject())
    );
    assert_ne!(
        stale.source_geometry().subject().generation(),
        state.snapshot().source_geometry().subject().generation()
    );
    // The committed snapshot still validates.
    state.snapshot().validate_against(state.source())?;
    assert_geometry_unavailable_for_current_snapshot(&state);
    Ok(())
}
