//! Validate the licensed command-line one-liner fixture corpus and publish coverage.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub const MANIFEST_PATH: &str = "crates/perl-corpus/fixtures/command_line_oneliners/manifest.json";
pub const COVERAGE_PATH: &str =
    "crates/perl-corpus/fixtures/command_line_oneliners/coverage_report.v1.json";
pub const SCHEMA_PATH: &str = "schemas/command_line_oneliner_corpus.v1.schema.json";
pub const PARSER_CORPUS_PATH: &str = "crates/perl-parser-core/tests/command_line_oneliners.rs";

const SCHEMA_VERSION: &str = "command_line_oneliner_corpus.v1";
const MANIFEST_NAME: &str = "licensed-command-line-oneliner-fixtures";
const COVERAGE_SCHEMA_VERSION: &str = "command_line_oneliner_coverage.v1";

#[derive(Debug, Deserialize)]
struct CorpusManifest {
    schema_version: String,
    manifest: String,
    runner: String,
    ratchet_generation: u32,
    falsifier_matrix: Vec<String>,
    coverage_dimensions: Vec<String>,
    fixtures: Vec<CorpusFixture>,
}

#[derive(Debug, Deserialize)]
struct CorpusFixture {
    id: String,
    proof_layer: String,
    fixture_kind: String,
    falsifier_tags: Vec<String>,
    expectation_tier: u32,
    parser_corpus_test: Option<String>,
    argv: CorpusArgv,
    execution_risk_class: String,
    allowed_evidence_modes: Vec<String>,
    oracle_disposition: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CorpusArgv {
    switch_tokens: Vec<String>,
}

pub fn run(check: bool) -> Result<()> {
    let root = project_root()?;
    let (stats, coverage) = evaluate(&root)?;

    if check {
        let expected = coverage.to_string();
        let on_disk = read_text(&root, COVERAGE_PATH)?;
        if on_disk.trim() != expected.trim() {
            bail!("{COVERAGE_PATH} is stale; run `cargo xtask command-line-oneliner-corpus`");
        }
    } else {
        fs::write(root.join(COVERAGE_PATH), coverage.to_string())
            .with_context(|| format!("failed to write {COVERAGE_PATH}"))?;
    }

    println!(
        "command-line one-liner corpus {} passed: {} fixtures, {} live parser links, {} falsifier tags covered",
        if check { "check" } else { "write" },
        stats.fixtures,
        stats.live_parser_links,
        stats.falsifier_tags_covered
    );
    Ok(())
}

#[derive(Debug)]
struct ValidationStats {
    fixtures: usize,
    live_parser_links: usize,
    falsifier_tags_covered: usize,
}

fn evaluate(root: &Path) -> Result<(ValidationStats, Value)> {
    validate_json_parse(root, SCHEMA_PATH)?;
    validate_json_parse(root, MANIFEST_PATH)?;

    let mut violations = Vec::new();
    validate_against_schema(root, &mut violations)?;

    let manifest = read_manifest(root)?;
    validate_manifest_semantics(root, &manifest, &mut violations);

    if !violations.is_empty() {
        eprintln!("command-line one-liner corpus violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!("command-line one-liner corpus check failed with {} violation(s)", violations.len());
    }

    let coverage = build_coverage_report(&manifest);
    let live_links =
        manifest.fixtures.iter().filter(|fixture| fixture.parser_corpus_test.is_some()).count();
    let falsifier_covered = manifest
        .falsifier_matrix
        .iter()
        .filter(|tag| manifest.fixtures.iter().any(|fixture| fixture.falsifier_tags.contains(tag)))
        .count();

    Ok((
        ValidationStats {
            fixtures: manifest.fixtures.len(),
            live_parser_links: live_links,
            falsifier_tags_covered: falsifier_covered,
        },
        coverage,
    ))
}

fn validate_against_schema(root: &Path, violations: &mut Vec<String>) -> Result<()> {
    let schema: Value = serde_json::from_str(&read_text(root, SCHEMA_PATH)?)
        .with_context(|| format!("failed to parse {SCHEMA_PATH} as JSON"))?;
    let manifest: Value = serde_json::from_str(&read_text(root, MANIFEST_PATH)?)
        .with_context(|| format!("failed to parse {MANIFEST_PATH} as JSON"))?;

    let validator = jsonschema::validator_for(&schema)
        .with_context(|| format!("failed to compile {SCHEMA_PATH}"))?;
    for error in validator.iter_errors(&manifest) {
        violations.push(format!(
            "{MANIFEST_PATH}: schema violation at {}: {error}",
            error.instance_path()
        ));
    }
    Ok(())
}

fn validate_manifest_semantics(
    root: &Path,
    manifest: &CorpusManifest,
    violations: &mut Vec<String>,
) {
    if manifest.schema_version != SCHEMA_VERSION {
        violations.push(format!(
            "{MANIFEST_PATH}: schema_version {:?}; expected {SCHEMA_VERSION}",
            manifest.schema_version
        ));
    }
    if manifest.manifest != MANIFEST_NAME {
        violations.push(format!(
            "{MANIFEST_PATH}: manifest {:?}; expected {MANIFEST_NAME}",
            manifest.manifest
        ));
    }
    if manifest.runner != "none" {
        violations.push(format!(
            "{MANIFEST_PATH}: runner must be `none` for hermetic corpus (got {:?})",
            manifest.runner
        ));
    }

    let corpus_tests = parser_corpus_test_names(root);
    let mut ids = BTreeSet::new();
    for fixture in &manifest.fixtures {
        if !ids.insert(fixture.id.clone()) {
            violations.push(format!("duplicate fixture id {:?}", fixture.id));
        }

        if fixture.execution_risk_class == "unbounded_execution" {
            violations.push(format!(
                "fixture {}: unbounded_execution is forbidden in the seed corpus",
                fixture.id
            ));
        }

        if fixture.allowed_evidence_modes.iter().any(|mode| mode == "bounded_perl_differential")
            && fixture.oracle_disposition.as_deref() != Some("opt_in_bounded")
        {
            violations.push(format!(
                "fixture {}: bounded_perl_differential requires oracle_disposition opt_in_bounded",
                fixture.id
            ));
        }

        if let Some(test_name) = &fixture.parser_corpus_test {
            if !corpus_tests.contains(test_name) {
                violations.push(format!(
                    "fixture {}: parser_corpus_test `{test_name}` missing from {PARSER_CORPUS_PATH}",
                    fixture.id
                ));
            }
            if fixture.proof_layer != "parser_result_diagnostics" {
                violations.push(format!(
                    "fixture {}: parser_corpus_test requires proof_layer parser_result_diagnostics",
                    fixture.id
                ));
            }
        }

        if fixture.expectation_tier < 1 {
            violations.push(format!("fixture {}: expectation_tier must be >= 1", fixture.id));
        }
    }

    for tag in &manifest.falsifier_matrix {
        let covered = manifest.fixtures.iter().any(|fixture| fixture.falsifier_tags.contains(tag));
        if !covered {
            violations
                .push(format!("falsifier_matrix tag `{tag}` has no fixture carrying that tag"));
        }
    }

    let required_switch_families =
        ["-e", "-E", "-n", "-p", "-a", "-F", "-I", "-M", "-m", "-l", "-0", "-0777", "--"];
    let mut switch_tokens: BTreeSet<String> = BTreeSet::new();
    for fixture in &manifest.fixtures {
        for token in &fixture.argv.switch_tokens {
            switch_tokens.insert(token.clone());
        }
    }
    for family in required_switch_families {
        if !switch_family_present(&switch_tokens, family) {
            violations.push(format!("seed corpus missing representative switch family `{family}`"));
        }
    }
}

fn switch_family_present(tokens: &BTreeSet<String>, family: &str) -> bool {
    if tokens.contains(family) {
        return true;
    }
    match family {
        "-e" => tokens
            .iter()
            .any(|token| token == "-e" || token.starts_with("-e") && !token.starts_with("-E")),
        "-E" => tokens.iter().any(|token| token.starts_with("-E")),
        "-n" => tokens.iter().any(|token| token.contains('n')),
        "-p" => tokens.iter().any(|token| token.contains('p')),
        "-a" => tokens.iter().any(|token| token.contains('a')),
        "-l" => tokens.iter().any(|token| token.contains('l')),
        "-0" => tokens.iter().any(|token| token == "-0" || token.starts_with("-0")),
        "-0777" => tokens.iter().any(|token| token.contains("0777")),
        "--" => tokens.iter().any(|token| token.contains("--")),
        "-F" | "-I" | "-M" | "-m" => tokens.iter().any(|token| token.starts_with(family)),
        _ => false,
    }
}

fn build_coverage_report(manifest: &CorpusManifest) -> Value {
    let mut by_switch: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_layer: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_falsifier: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_outcome: BTreeMap<String, u32> = BTreeMap::new();
    let mut live_links = 0_u32;

    for fixture in &manifest.fixtures {
        *by_layer.entry(fixture.proof_layer.clone()).or_default() += 1;
        *by_kind.entry(fixture.fixture_kind.clone()).or_default() += 1;
        for token in &fixture.argv.switch_tokens {
            *by_switch.entry(token.clone()).or_default() += 1;
        }
        for tag in &fixture.falsifier_tags {
            *by_falsifier.entry(tag.clone()).or_default() += 1;
        }
        if fixture.parser_corpus_test.is_some() {
            live_links += 1;
        }
    }

    let mut gaps = Vec::new();
    if live_links < 20 {
        gaps.push(format!(
            "parser_corpus_test links: {live_links} (target keeps parity with command_line_oneliners.rs)"
        ));
    }

    let mut report = Map::new();
    report.insert("schema_version".to_owned(), Value::String(COVERAGE_SCHEMA_VERSION.to_owned()));
    report.insert(
        "generated_by".to_owned(),
        Value::String("cargo xtask command-line-oneliner-corpus".to_owned()),
    );
    report.insert("manifest".to_owned(), Value::String(MANIFEST_PATH.to_owned()));
    report
        .insert("ratchet_generation".to_owned(), Value::Number(manifest.ratchet_generation.into()));
    report.insert("fixture_count".to_owned(), Value::Number(manifest.fixtures.len().into()));
    report.insert("live_parser_corpus_links".to_owned(), Value::Number(live_links.into()));
    report.insert(
        "dimensions".to_owned(),
        serde_json::json!({
            "switch": by_switch,
            "proof_layer": by_layer,
            "fixture_kind": by_kind,
            "falsifier_tag": by_falsifier,
            "parser_outcome": by_outcome,
        }),
    );
    report.insert(
        "coverage_dimensions".to_owned(),
        Value::Array(manifest.coverage_dimensions.iter().cloned().map(Value::String).collect()),
    );
    report.insert("gaps".to_owned(), Value::Array(gaps.into_iter().map(Value::String).collect()));
    Value::Object(report)
}

fn parser_corpus_test_names(root: &Path) -> BTreeSet<String> {
    let text = read_text(root, PARSER_CORPUS_PATH).unwrap_or_default();
    let mut names = BTreeSet::new();
    for captures in regex::Regex::new(r"command_line_oneliner!\(\s*([a-z][a-z0-9_]*)")
        .expect("valid regex")
        .captures_iter(&text)
    {
        names.insert(captures[1].to_owned());
    }
    names
}

fn read_manifest(root: &Path) -> Result<CorpusManifest> {
    let text = read_text(root, MANIFEST_PATH)?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {MANIFEST_PATH}"))
}

fn validate_json_parse(root: &Path, rel: &str) -> Result<()> {
    let text = read_text(root, rel)?;
    let _: Value =
        serde_json::from_str(&text).with_context(|| format!("failed to parse {rel} as JSON"))?;
    Ok(())
}

fn read_text(root: &Path, rel: &str) -> Result<String> {
    let path = root.join(rel);
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_manifest_passes_validator() -> Result<()> {
        let root = project_root()?;
        let (stats, _) = evaluate(&root)?;
        assert!(stats.fixtures >= 30, "seed corpus should cover switch families");
        assert!(stats.live_parser_links >= 19, "live parser links should mirror corpus");
        Ok(())
    }

    #[test]
    fn missing_parser_corpus_test_is_rejected() -> Result<()> {
        let root = project_root()?;
        let mut manifest = read_manifest(&root)?;
        manifest.fixtures[0].parser_corpus_test = Some("ghost_fixture_name".to_owned());
        let mut violations = Vec::new();
        validate_manifest_semantics(&root, &manifest, &mut violations);
        assert!(violations.iter().any(|v| v.contains("ghost_fixture_name")));
        Ok(())
    }
}
