use perl_corpus::fixture_expectations::{self, FixtureExpectation};
use perl_corpus::sidecar::{
    ConceptRegistry, ExpectationMode, FixtureExpectationSidecar, SidecarValidationContext,
    parse_sidecar_str, validate_sidecar,
};
use std::any::TypeId;
use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::path::Path;

fn minimal(id: &str, mode: &str) -> String {
    format!(
        r#"
[concept]
id = "{id}"
tier = "pr"

[expect]
panic = false
timeout = false
mode = "{mode}"
"#
    )
}

fn write_pair(root: &Path, name: &str, raw: &str) -> Result<(), Box<dyn Error>> {
    fs::write(root.join(format!("{name}.meta.toml")), raw)?;
    fs::write(root.join(format!("{name}.pl")), "1;")?;
    Ok(())
}

#[test]
fn schema_and_compatibility_mode_identities_remain_explicit() -> Result<(), Box<dyn Error>> {
    assert_eq!(FixtureExpectationSidecar::SCHEMA, "fixture_expectation.v1");
    assert_ne!(
        TypeId::of::<ExpectationMode>(),
        TypeId::of::<fixture_expectations::ExpectationMode>()
    );

    let cases = [
        ("parse_clean", ExpectationMode::ParseClean),
        ("recover_without_panic", ExpectationMode::RecoverWithoutPanic),
        ("expected_error", ExpectationMode::ExpectedError),
        ("token_only", ExpectationMode::TokenOnly),
        ("span_only", ExpectationMode::SpanOnly),
    ];
    for (token, expected) in cases {
        let parsed = parse_sidecar_str(&minimal("parser.example", token))?;
        assert_eq!(parsed.expect.mode, expected);
    }
    Ok(())
}

#[test]
fn canonical_and_compatibility_models_reject_the_same_invalid_documents() {
    let unknown_mode = minimal("parser.example", "unknown");
    let unknown_field = format!("{}\nextra = true\n", minimal("parser.example", "parse_clean"));
    let missing_concept = r#"
[expect]
panic = false
timeout = false
mode = "parse_clean"
"#;
    let partial_snapshots =
        format!("{}\n[snapshots]\nast = true\n", minimal("parser.example", "parse_clean"));

    for raw in
        [unknown_mode.as_str(), unknown_field.as_str(), missing_concept, partial_snapshots.as_str()]
    {
        assert!(toml::from_str::<FixtureExpectationSidecar>(raw).is_err());
        assert!(toml::from_str::<FixtureExpectation>(raw).is_err());
    }
}

#[test]
fn registry_semantics_match_across_both_public_paths() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    write_pair(root.path(), "case", &minimal("parser.example", "parse_clean"))?;
    let context = SidecarValidationContext::discover(root.path())?;
    let parsed = perl_corpus::sidecar::parse_sidecar(&context, Path::new("case.meta.toml"))?;

    let pending = validate_sidecar(&context, Path::new("case.meta.toml"), &parsed, None);
    assert!(pending.is_ok());
    assert!(pending.warnings.iter().any(|warning| warning.contains("resolution pending")));

    let canonical_registry = ConceptRegistry::from_ids(["parser.other".to_string()]);
    let canonical =
        validate_sidecar(&context, Path::new("case.meta.toml"), &parsed, Some(&canonical_registry));
    let compatibility_registry = HashSet::from(["parser.other".to_string()]);
    let compatibility = fixture_expectations::validate_sidecar(
        &context,
        Path::new("case.meta.toml"),
        Some(&compatibility_registry),
    );
    assert_eq!(compatibility.errors, canonical.errors);
    assert_eq!(compatibility.warnings, canonical.warnings);
    Ok(())
}
