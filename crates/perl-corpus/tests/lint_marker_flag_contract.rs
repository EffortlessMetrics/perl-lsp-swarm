use anyhow::{Result, ensure};
use perl_corpus::lint::{KNOWN_FLAGS, LintConfig, check_sections};
use perl_corpus::metadata::{IdSource, Section};

fn section_with_flags(flags: &[&str]) -> Section {
    Section {
        id: "marker.flags".to_string(),
        id_source: IdSource::Explicit,
        explicit_id: Some("marker.flags".to_string()),
        generated_id: None,
        title: "Marker flags".to_string(),
        file: "marker_flags.txt".to_string(),
        tags: Vec::new(),
        perl: None,
        flags: flags.iter().map(|flag| (*flag).to_string()).collect(),
        body: "die \"expected failure\";".to_string(),
        expected: None,
        line: Some(1),
    }
}

#[test]
fn canonical_marker_flags_are_known_and_silent() -> Result<()> {
    let canonical_flags = ["expected-error", "todo", "wip"];
    for flag in canonical_flags {
        ensure!(KNOWN_FLAGS.contains(&flag), "canonical marker flag {flag:?} is missing");
    }

    let result = check_sections(&[section_with_flags(&canonical_flags)], &LintConfig::default());
    ensure!(result.is_ok(), "canonical marker flags produced lint errors: {:?}", result.errors);
    ensure!(
        result.warnings.is_empty(),
        "canonical marker flags produced warnings: {:?}",
        result.warnings
    );
    Ok(())
}

#[test]
fn unknown_marker_neighbour_still_warns() -> Result<()> {
    let unknown_flag = "expected-errors";
    let result = check_sections(&[section_with_flags(&[unknown_flag])], &LintConfig::default());
    let expected_warning =
        format!("Unknown flag '{unknown_flag}' in marker_flags.txt: marker.flags");

    ensure!(result.is_ok(), "unknown flags should remain warnings, not errors");
    ensure!(
        result.warnings == vec![expected_warning],
        "unknown-marker control produced unexpected warnings: {:?}",
        result.warnings
    );
    Ok(())
}
