//! Legacy parser-accuracy metamorphic population freeze for #8099.
//!
//! This test reproduces the current trailing-whitespace admission rule only to
//! retain its investigation denominator during migration. The rule is not a
//! trusted Perl-region classifier and must not become typed-oracle authority.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

const MANIFEST_PATH: &str = "crates/perl-corpus/fixtures/parser_accuracy/manifest.json";
const LEGACY_APPLIED_CASE_COUNT: usize = 47;

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    id: String,
    source_path: String,
}

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn legacy_whitespace_case_applies(source: &str) -> bool {
    if source.contains("<<") || source.contains("__DATA__") || source.contains("__END__") {
        return false;
    }

    source.split_inclusive('\n').any(|segment| {
        let without_lf = segment.strip_suffix('\n').unwrap_or(segment);
        let body = without_lf.strip_suffix('\r').unwrap_or(without_lf);
        !body.trim().is_empty()
    })
}

#[test]
fn legacy_whitespace_population_retains_every_live_manifest_fixture() -> TestResult {
    let root = project_root();
    let manifest: Manifest = serde_json::from_str(&fs::read_to_string(root.join(MANIFEST_PATH))?)?;

    assert_eq!(manifest.schema_version, 1, "unexpected parser-accuracy manifest schema");
    assert!(!manifest.fixtures.is_empty(), "parser-accuracy manifest must not be empty");

    let mut fixture_ids = BTreeSet::new();
    let mut applied_case_ids = BTreeSet::new();
    let mut omitted_case_ids = BTreeSet::new();

    for fixture in &manifest.fixtures {
        assert!(
            fixture_ids.insert(fixture.id.clone()),
            "duplicate parser-accuracy fixture id `{}`",
            fixture.id
        );

        let source = fs::read_to_string(root.join(&fixture.source_path))?;
        let target = if legacy_whitespace_case_applies(&source) {
            &mut applied_case_ids
        } else {
            &mut omitted_case_ids
        };
        assert!(target.insert(fixture.id.clone()));
    }

    assert_eq!(
        applied_case_ids.len(),
        LEGACY_APPLIED_CASE_COUNT,
        "legacy hash population changed; retain the new case identities explicitly before changing the oracle"
    );
    assert_eq!(
        applied_case_ids.len() + omitted_case_ids.len(),
        manifest.fixtures.len(),
        "every live fixture must retain an applied or omitted legacy disposition"
    );
    assert_eq!(fixture_ids.len(), manifest.fixtures.len());
    assert!(
        !omitted_case_ids.is_empty(),
        "legacy whole-file filtering must remain visible until typed applicability replaces it"
    );

    Ok(())
}
