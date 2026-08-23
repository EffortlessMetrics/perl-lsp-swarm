use perl_incremental_parsing::incremental as compatibility;
use perl_parser::incremental as canonical;

#[test]
fn compatibility_types_are_the_canonical_types() -> Result<(), Box<dyn std::error::Error>> {
    let mut state: canonical::IncrementalState =
        compatibility::IncrementalState::new("my $x = 1;".to_string());
    let edit: canonical::Edit = compatibility::Edit {
        start_byte: 8,
        old_end_byte: 9,
        new_end_byte: 9,
        new_text: "2".to_string(),
    };

    let result: canonical::ReparseResult = compatibility::apply_edits(&mut state, &[edit])?;

    assert_eq!(state.source(), "my $x = 2;");
    assert_eq!(result.parse_output().ast, state.parse_output().ast);
    Ok(())
}

#[test]
fn canonical_values_flow_through_the_compatibility_entry_point()
-> Result<(), Box<dyn std::error::Error>> {
    let mut state: compatibility::IncrementalState =
        canonical::IncrementalState::new("my $x = 1;".to_string());
    let edit = canonical::Edit {
        start_byte: 8,
        old_end_byte: 9,
        new_end_byte: 9,
        new_text: "3".to_string(),
    };

    let result: compatibility::ReparseResult = compatibility::apply_edits(&mut state, &[edit])?;

    assert_eq!(state.source(), "my $x = 3;");
    assert_eq!(result.parse_output().ast, state.parse_output().ast);
    Ok(())
}
