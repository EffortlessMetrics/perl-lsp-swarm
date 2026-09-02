//! Validate the differential real-Perl oracle fixture manifest.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const MANIFEST_PATH: &str = "crates/perl-corpus/fixtures/differential_oracle/manifest.json";
const SCHEMA_PATH: &str = "schemas/oracle_fixture_manifest.v1.schema.json";
const SCHEMA_VERSION: &str = "oracle_fixture_manifest.v1";
const MANIFEST_NAME: &str = "differential-real-perl-oracle-fixtures";
const ORACLE_SPEC: &str = "docs/specs/PLSP-SPEC-0027-differential-real-perl-oracle.md";

const REQUIRED_COMPARISON_CLASSES: &[&str] = &[
    "PackageSubTable",
    "ImportExport",
    "IsaComposition",
    "ConstantPrototype",
    "FrameworkGeneratedMember",
    "CompileEffect",
];

const REQUIRED_RESULT_CLASSES: &[&str] = &[
    "oracle_agrees",
    "compiler_missing",
    "compiler_extra",
    "range_mismatch",
    "provenance_mismatch",
    "confidence_or_freshness_mismatch",
    "dynamic_or_unsupported",
    "oracle_ambient_unbounded",
    "stale_or_partial",
    "unknown",
];

/// Fact families each comparison class compares, per the "Required Comparison
/// Classes" table in `docs/specs/PLSP-SPEC-0027-differential-real-perl-oracle.md`.
///
/// This is the single fact-family vocabulary: manifest declarations and the
/// per-class receipt contract share it rather than maintaining a second ledger.
const CLASS_FACT_FAMILIES: &[(&str, &[&str])] = &[
    ("PackageSubTable", &["packages", "named_subs", "source_ranges", "stash_entries"]),
    ("ImportExport", &["import_specs", "export_sets", "visible_symbols"]),
    ("IsaComposition", &["isa_entries", "inheritance_facts", "role_composition_facts"]),
    ("ConstantPrototype", &["constants", "prototype_entries", "compile_effects"]),
    ("FrameworkGeneratedMember", &["generated_members"]),
    ("CompileEffect", &["compile_effects", "dynamic_boundaries"]),
];

const ALLOWED_CLASS_COVERAGE: &[&str] = &["declared", "pending_fixture"];

const REQUIRED_ENVIRONMENT_DENIALS: &[&str] = &["PERL5LIB", "PERL5OPT", "local::lib"];
const ALLOWED_PATH_CLASSES: &[&str] = &["public_test_fixture", "redacted_private_fixture"];
const ALLOWED_INCLUDE_PATH_AUTHORITIES: &[&str] =
    &["declared_fixture_root", "declared_module_roots", "ambient_reported"];
const REQUIRED_CLAIM_PHRASES: &[&str] = &[
    "no oracle runner",
    "Perl execution",
    "provider behavior",
    "support-tier promotion",
    "parser/corpus bucket movement",
];

#[derive(Debug, Deserialize)]
struct OracleFixtureManifest {
    schema_version: String,
    manifest: String,
    owner: String,
    status: String,
    updated: String,
    spec: String,
    runner: String,
    editor_runtime_dependency: bool,
    comparison_classes: Vec<String>,
    result_classes: Vec<String>,
    required_environment_denials: Vec<String>,
    default_claim_boundary: String,
    #[serde(default)]
    class_contracts: Vec<OracleClassContract>,
    #[serde(default)]
    fixtures: Vec<OracleFixture>,
}

/// One declared comparison class: its contract version, its single active owner
/// issue, whether a fixture already declares it, and the fact families it compares.
#[derive(Debug, Deserialize)]
struct OracleClassContract {
    comparison_class: String,
    contract_version: String,
    owner: String,
    coverage: String,
    #[serde(default)]
    fact_families: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OracleFixture {
    id: String,
    source: String,
    path_class: String,
    perl_version_constraint: String,
    include_path_authority: String,
    module_roots: Vec<String>,
    environment_denials: Vec<String>,
    comparison_classes: Vec<String>,
    #[serde(default)]
    dynamic_boundaries: Vec<String>,
    #[serde(default)]
    unsupported_effects: Vec<String>,
    #[serde(default)]
    framework_adapters: Vec<String>,
    claim_boundary: String,
    #[serde(default)]
    expected_fact_families: Vec<String>,
    owner: String,
    /// Additional load files beyond `source` that this fixture's module graph
    /// needs. Empty for single-file fixtures.
    #[serde(default)]
    module_files: Vec<String>,
}

#[derive(Debug)]
struct ValidationStats {
    fixtures: usize,
    comparison_classes: usize,
    result_classes: usize,
}

pub fn run() -> Result<()> {
    let root = project_root()?;
    let stats = validate(&root)?;
    println!(
        "oracle fixture manifest check passed: {} fixtures, {} comparison classes, {} result classes",
        stats.fixtures, stats.comparison_classes, stats.result_classes
    );
    Ok(())
}

fn validate(root: &Path) -> Result<ValidationStats> {
    validate_json_parse(root, SCHEMA_PATH)?;
    let manifest = read_manifest(root, MANIFEST_PATH)?;
    let mut violations = Vec::new();

    validate_manifest_shape(root, &manifest, &mut violations);
    validate_fixtures(root, &manifest, &mut violations);
    validate_class_contracts(&manifest, &mut violations);

    if !violations.is_empty() {
        eprintln!("oracle fixture manifest violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!("oracle fixture manifest check failed with {} violation(s)", violations.len());
    }

    Ok(ValidationStats {
        fixtures: manifest.fixtures.len(),
        comparison_classes: manifest.comparison_classes.len(),
        result_classes: manifest.result_classes.len(),
    })
}

fn validate_json_parse(root: &Path, rel: &str) -> Result<()> {
    let text = read_text(root, rel)?;
    let _: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("failed to parse {rel} as JSON"))?;
    Ok(())
}

fn read_manifest(root: &Path, rel: &str) -> Result<OracleFixtureManifest> {
    let text = read_text(root, rel)?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {rel}"))
}

fn read_text(root: &Path, rel: &str) -> Result<String> {
    let path = root.join(rel);
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

fn validate_manifest_shape(
    root: &Path,
    manifest: &OracleFixtureManifest,
    violations: &mut Vec<String>,
) {
    if manifest.schema_version != SCHEMA_VERSION {
        violations.push(format!(
            "{MANIFEST_PATH}: schema_version is {:?}; expected {:?}",
            manifest.schema_version, SCHEMA_VERSION
        ));
    }
    if manifest.manifest != MANIFEST_NAME {
        violations.push(format!(
            "{MANIFEST_PATH}: manifest is {:?}; expected {:?}",
            manifest.manifest, MANIFEST_NAME
        ));
    }
    require_non_empty(MANIFEST_PATH, "owner", &manifest.owner, violations);
    require_non_empty(MANIFEST_PATH, "updated", &manifest.updated, violations);
    if manifest.status != "declaration-only" {
        violations.push(format!(
            "{MANIFEST_PATH}: status is {:?}; expected \"declaration-only\"",
            manifest.status
        ));
    }
    if manifest.spec != ORACLE_SPEC {
        violations.push(format!(
            "{MANIFEST_PATH}: spec is {:?}; expected {:?}",
            manifest.spec, ORACLE_SPEC
        ));
    }
    validate_relative_existing_path(root, MANIFEST_PATH, "spec", &manifest.spec, violations);
    validate_relative_existing_path(root, MANIFEST_PATH, "schema", SCHEMA_PATH, violations);
    if manifest.runner != "none" {
        violations.push(format!(
            "{MANIFEST_PATH}: runner is {:?}; expected \"none\" for declaration-only manifest",
            manifest.runner
        ));
    }
    if manifest.editor_runtime_dependency {
        violations.push(format!(
            "{MANIFEST_PATH}: editor_runtime_dependency must be false for oracle fixtures"
        ));
    }

    require_exact_set(
        MANIFEST_PATH,
        "comparison_classes",
        &manifest.comparison_classes,
        REQUIRED_COMPARISON_CLASSES,
        violations,
    );
    require_exact_set(
        MANIFEST_PATH,
        "result_classes",
        &manifest.result_classes,
        REQUIRED_RESULT_CLASSES,
        violations,
    );
    require_exact_set(
        MANIFEST_PATH,
        "required_environment_denials",
        &manifest.required_environment_denials,
        REQUIRED_ENVIRONMENT_DENIALS,
        violations,
    );
    validate_claim_boundary(
        MANIFEST_PATH,
        "default_claim_boundary",
        &manifest.default_claim_boundary,
        violations,
    );

    if manifest.fixtures.is_empty() {
        violations.push(format!("{MANIFEST_PATH}: fixtures list must not be empty"));
    }
}

fn validate_fixtures(root: &Path, manifest: &OracleFixtureManifest, violations: &mut Vec<String>) {
    let comparison_class_set =
        manifest.comparison_classes.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();

    for fixture in &manifest.fixtures {
        let doc = format!("{MANIFEST_PATH}: fixture {}", fixture.id);
        require_non_empty(&doc, "id", &fixture.id, violations);
        if !seen.insert(fixture.id.clone()) {
            violations.push(format!("{MANIFEST_PATH}: duplicate fixture id {:?}", fixture.id));
        }

        validate_relative_existing_path(root, &doc, "source", &fixture.source, violations);
        validate_allowed(&doc, "path_class", &fixture.path_class, ALLOWED_PATH_CLASSES, violations);
        require_non_empty(
            &doc,
            "perl_version_constraint",
            &fixture.perl_version_constraint,
            violations,
        );
        validate_allowed(
            &doc,
            "include_path_authority",
            &fixture.include_path_authority,
            ALLOWED_INCLUDE_PATH_AUTHORITIES,
            violations,
        );
        validate_non_empty_path_list(root, &doc, "module_roots", &fixture.module_roots, violations);
        require_contains_all(
            &doc,
            "environment_denials",
            &fixture.environment_denials,
            REQUIRED_ENVIRONMENT_DENIALS,
            violations,
        );
        validate_fixture_comparison_classes(
            &doc,
            &comparison_class_set,
            &fixture.comparison_classes,
            violations,
        );
        validate_string_list(&doc, "dynamic_boundaries", &fixture.dynamic_boundaries, violations);
        validate_string_list(&doc, "unsupported_effects", &fixture.unsupported_effects, violations);
        validate_string_list(&doc, "framework_adapters", &fixture.framework_adapters, violations);
        validate_framework_adapter_requirement(&doc, fixture, violations);
        validate_claim_boundary(&doc, "claim_boundary", &fixture.claim_boundary, violations);
        validate_expected_fact_families(&doc, fixture, violations);
        validate_owner_issue(&doc, "owner", &fixture.owner, violations);
        validate_declared_module_topology(root, &doc, fixture, violations);
    }
}

/// Every declared comparison class must carry exactly one contract: one version,
/// one active owner issue, the spec's fact families, and a coverage state that
/// agrees with whether any fixture actually declares the class.
///
/// This is what stops the manifest from naming six classes while silently
/// leaving one with no fixture and no owner.
fn validate_class_contracts(manifest: &OracleFixtureManifest, violations: &mut Vec<String>) {
    let declared = manifest.comparison_classes.iter().map(String::as_str).collect::<BTreeSet<_>>();

    let mut covered_by_fixture = BTreeSet::new();
    for fixture in &manifest.fixtures {
        for class in &fixture.comparison_classes {
            covered_by_fixture.insert(class.as_str());
        }
    }

    let mut seen = BTreeSet::new();
    for contract in &manifest.class_contracts {
        let class = contract.comparison_class.as_str();
        let doc = format!("{MANIFEST_PATH}: class_contract {class}");

        if !declared.contains(class) {
            violations.push(format!(
                "{MANIFEST_PATH}: class_contracts declares unknown comparison class {class:?}"
            ));
            continue;
        }
        if !seen.insert(class) {
            violations.push(format!(
                "{MANIFEST_PATH}: duplicate class_contract for comparison class {class:?}"
            ));
            continue;
        }

        validate_contract_version(&doc, &contract.contract_version, violations);
        validate_owner_issue(&doc, "owner", &contract.owner, violations);
        validate_allowed(&doc, "coverage", &contract.coverage, ALLOWED_CLASS_COVERAGE, violations);

        match class_fact_families(class) {
            Some(expected) => require_exact_set(
                &doc,
                "fact_families",
                &contract.fact_families,
                expected,
                violations,
            ),
            None => violations
                .push(format!("{doc}: no specification fact families are defined for this class")),
        }

        let has_fixture = covered_by_fixture.contains(class);
        match contract.coverage.as_str() {
            "declared" if !has_fixture => violations.push(format!(
                "{doc}: coverage is \"declared\" but no fixture declares this comparison class"
            )),
            "pending_fixture" if has_fixture => violations.push(format!(
                "{doc}: coverage is \"pending_fixture\" but a fixture already declares this comparison class"
            )),
            _ => {}
        }
    }

    for class in declared.difference(&seen) {
        violations
            .push(format!("{MANIFEST_PATH}: comparison class {class:?} has no class_contract"));
    }
}

fn class_fact_families(class: &str) -> Option<&'static [&'static str]> {
    CLASS_FACT_FAMILIES.iter().find(|(name, _)| *name == class).map(|(_, families)| *families)
}

/// A fixture may only expect fact families that its own comparison classes
/// actually compare. Declaring `constants` on an `ImportExport` fixture is a
/// declaration that no class contract could ever satisfy.
fn validate_expected_fact_families(
    doc: &str,
    fixture: &OracleFixture,
    violations: &mut Vec<String>,
) {
    if fixture.expected_fact_families.is_empty() {
        violations.push(format!("{doc}: expected_fact_families must not be empty"));
        return;
    }
    validate_string_list(
        doc,
        "expected_fact_families",
        &fixture.expected_fact_families,
        violations,
    );

    let mut admissible = BTreeSet::new();
    for class in &fixture.comparison_classes {
        if let Some(families) = class_fact_families(class) {
            admissible.extend(families.iter().copied());
        }
    }

    let mut seen = BTreeSet::new();
    for family in &fixture.expected_fact_families {
        if !seen.insert(family.as_str()) {
            violations
                .push(format!("{doc}: expected_fact_families repeats entry {:?}", family.as_str()));
        }
        if !admissible.contains(family.as_str()) {
            violations.push(format!(
                "{doc}: expected_fact_families entry {:?} is not compared by any of this fixture's comparison classes",
                family.as_str()
            ));
        }
    }
}

/// Every declared load file must sit inside one of the fixture's own declared
/// module roots. A fixture may not quietly depend on a module outside the roots
/// the runner is told to use.
fn validate_declared_module_topology(
    root: &Path,
    doc: &str,
    fixture: &OracleFixture,
    violations: &mut Vec<String>,
) {
    let mut seen = BTreeSet::new();
    for module_file in &fixture.module_files {
        if !seen.insert(module_file.as_str()) {
            violations
                .push(format!("{doc}: module_files repeats entry {:?}", module_file.as_str()));
        }
        validate_relative_existing_path(root, doc, "module_files", module_file, violations);
    }

    for (field, value) in std::iter::once(("source", &fixture.source))
        .chain(fixture.module_files.iter().map(|file| ("module_files", file)))
    {
        if !fixture.module_roots.iter().any(|module_root| is_contained_by(value, module_root)) {
            violations.push(format!(
                "{doc}: {field} {value:?} is not contained by any declared module_root"
            ));
        }
    }
}

/// Purely lexical containment on repo-relative slash paths. Existence and
/// repo-root escape are checked separately by `validate_relative_existing_path`.
fn is_contained_by(path: &str, module_root: &str) -> bool {
    let root = module_root.trim_end_matches('/');
    if root.is_empty() {
        return false;
    }
    path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

fn validate_owner_issue(doc: &str, field: &str, value: &str, violations: &mut Vec<String>) {
    if !is_issue_reference(value) {
        violations.push(format!(
            "{doc}: {field} {value:?} must be an issue reference such as \"#13622\""
        ));
    }
}

fn is_issue_reference(value: &str) -> bool {
    let Some(digits) = value.strip_prefix('#') else {
        return false;
    };
    !digits.is_empty()
        && !digits.starts_with('0')
        && digits.chars().all(|character| character.is_ascii_digit())
}

fn validate_contract_version(doc: &str, value: &str, violations: &mut Vec<String>) {
    let valid = value.strip_prefix('v').is_some_and(|d| {
        !d.is_empty() && !d.starts_with('0') && d.chars().all(|c| c.is_ascii_digit())
    });
    if !valid {
        violations.push(format!(
            "{doc}: contract_version {value:?} must look like \"v1\" (exactly one version per class)"
        ));
    }
}

fn validate_fixture_comparison_classes(
    doc: &str,
    allowed: &BTreeSet<&str>,
    values: &[String],
    violations: &mut Vec<String>,
) {
    if values.is_empty() {
        violations.push(format!("{doc}: comparison_classes must not be empty"));
        return;
    }
    for value in values {
        if value.trim().is_empty() {
            violations.push(format!("{doc}: comparison_classes contains an empty item"));
            continue;
        }
        if !allowed.contains(value.as_str()) {
            violations
                .push(format!("{doc}: comparison_classes contains unknown class {:?}", value));
        }
    }
}

fn validate_framework_adapter_requirement(
    doc: &str,
    fixture: &OracleFixture,
    violations: &mut Vec<String>,
) {
    if fixture.comparison_classes.iter().any(|class| class == "FrameworkGeneratedMember")
        && fixture.framework_adapters.is_empty()
    {
        violations.push(format!(
            "{doc}: FrameworkGeneratedMember fixtures must declare at least one framework_adapter"
        ));
    }
}

fn validate_relative_existing_path(
    root: &Path,
    doc: &str,
    field: &str,
    value: &str,
    violations: &mut Vec<String>,
) {
    if value.trim().is_empty() {
        violations.push(format!("{doc}: {field} must not be empty"));
        return;
    }
    if Path::new(value).is_absolute() || value.contains(':') || value.contains('\\') {
        violations.push(format!("{doc}: {field} must be a repo-relative slash path: {value}"));
        return;
    }
    let path = root.join(value);
    if !path.exists() {
        violations.push(format!("{doc}: {field} points to missing path {value}"));
        return;
    }

    let Ok(root) = root.canonicalize() else {
        violations.push(format!("{doc}: could not canonicalize repo root {}", root.display()));
        return;
    };
    let Ok(path) = path.canonicalize() else {
        violations.push(format!("{doc}: could not canonicalize {field} path {value}"));
        return;
    };
    if !path.starts_with(&root) {
        violations.push(format!("{doc}: {field} escapes repo root: {value}"));
    }
}

fn validate_non_empty_path_list(
    root: &Path,
    doc: &str,
    field: &str,
    values: &[String],
    violations: &mut Vec<String>,
) {
    if values.is_empty() {
        violations.push(format!("{doc}: {field} must not be empty"));
        return;
    }
    for value in values {
        validate_relative_existing_path(root, doc, field, value, violations);
    }
}

fn validate_string_list(doc: &str, field: &str, values: &[String], violations: &mut Vec<String>) {
    for value in values {
        if value.trim().is_empty() {
            violations.push(format!("{doc}: {field} contains an empty item"));
        }
    }
}

fn validate_allowed(
    doc: &str,
    field: &str,
    value: &str,
    allowed: &[&str],
    violations: &mut Vec<String>,
) {
    if !allowed.contains(&value) {
        violations.push(format!("{doc}: {field} {:?} is not allowed", value));
    }
}

fn validate_claim_boundary(doc: &str, field: &str, value: &str, violations: &mut Vec<String>) {
    require_non_empty(doc, field, value, violations);
    for phrase in REQUIRED_CLAIM_PHRASES {
        if !value.contains(phrase) {
            violations.push(format!("{doc}: {field} must include phrase {phrase:?}"));
        }
    }
}

fn require_exact_set(
    doc: &str,
    field: &str,
    actual: &[String],
    expected: &[&str],
    violations: &mut Vec<String>,
) {
    let actual_set = actual.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let expected_set = expected.iter().copied().collect::<BTreeSet<_>>();

    for missing in expected_set.difference(&actual_set) {
        violations.push(format!("{doc}: {field} missing required entry {missing:?}"));
    }
    for unexpected in actual_set.difference(&expected_set) {
        violations.push(format!("{doc}: {field} contains unsupported entry {unexpected:?}"));
    }
}

fn require_contains_all(
    doc: &str,
    field: &str,
    actual: &[String],
    expected: &[&str],
    violations: &mut Vec<String>,
) {
    if actual.is_empty() {
        violations.push(format!("{doc}: {field} must not be empty"));
        return;
    }
    let actual_set = actual.iter().map(String::as_str).collect::<BTreeSet<_>>();
    for missing in expected {
        if !actual_set.contains(missing) {
            violations.push(format!("{doc}: {field} missing required entry {missing:?}"));
        }
    }
    for unexpected in actual_set.difference(&expected.iter().copied().collect::<BTreeSet<_>>()) {
        violations.push(format!("{doc}: {field} contains unsupported entry {unexpected:?}"));
    }
    validate_string_list(doc, field, actual, violations);
}

fn require_non_empty(doc: &str, field: &str, value: &str, violations: &mut Vec<String>) {
    if value.trim().is_empty() {
        violations.push(format!("{doc}: {field} must not be empty"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T>;

    #[test]
    fn accepts_minimal_valid_manifest() -> TestResult {
        let tempdir = valid_manifest_workspace()?;

        let stats = validate(tempdir.path())?;

        assert_eq!(stats.fixtures, 1);
        assert_eq!(stats.comparison_classes, REQUIRED_COMPARISON_CLASSES.len());
        assert_eq!(stats.result_classes, REQUIRED_RESULT_CLASSES.len());
        Ok(())
    }

    #[test]
    fn rejects_missing_required_environment_denial() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""environment_denials": ["PERL5LIB", "PERL5OPT", "local::lib"]"#,
                r#""environment_denials": ["PERL5LIB", "PERL5OPT"]"#,
            ),
        )?;

        let err = validate(tempdir.path()).expect_err("missing local::lib denial should fail");

        assert!(
            err.to_string().contains("oracle fixture manifest check failed"),
            "unexpected error: {err:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_unsupported_environment_denial() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""environment_denials": ["PERL5LIB", "PERL5OPT", "local::lib"]"#,
                r#""environment_denials": ["PERL5LIB", "PERL5OPT", "local::lib", "PATH"]"#,
            ),
        )?;

        let err = validate(tempdir.path()).expect_err("unsupported PATH denial should fail");

        assert!(
            err.to_string().contains("oracle fixture manifest check failed"),
            "unexpected error: {err:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_framework_generated_member_without_adapter() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(r#""framework_adapters": ["Moo"]"#, r#""framework_adapters": []"#),
        )?;

        let err = validate(tempdir.path()).expect_err("missing framework adapter should fail");

        assert!(
            err.to_string().contains("oracle fixture manifest check failed"),
            "unexpected error: {err:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_absolute_source_path() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""source": "fixtures/package_basic.pl""#,
                r#""source": "C:/tmp/package_basic.pl""#,
            ),
        )?;

        let err = validate(tempdir.path()).expect_err("absolute source path should fail");

        assert!(
            err.to_string().contains("oracle fixture manifest check failed"),
            "unexpected error: {err:?}"
        );
        Ok(())
    }

    /// `CLASS_FACT_FAMILIES` keys and `REQUIRED_COMPARISON_CLASSES` are two
    /// spellings of the same class set. Pin them together so a class added to one
    /// cannot silently miss the other and lose its fact-family contract.
    #[test]
    fn fact_family_table_covers_exactly_the_required_comparison_classes() {
        let tabled =
            CLASS_FACT_FAMILIES.iter().map(|(class, _)| *class).collect::<BTreeSet<&str>>();
        let required = REQUIRED_COMPARISON_CLASSES.iter().copied().collect::<BTreeSet<&str>>();

        assert_eq!(tabled, required, "fact-family table and comparison-class list disagree");
        assert!(
            CLASS_FACT_FAMILIES.iter().all(|(_, families)| !families.is_empty()),
            "every comparison class must compare at least one fact family"
        );
    }

    /// The checked-in manifest is the artifact this contract governs. Without
    /// this the tempdir tests could all pass while the real manifest violated
    /// every rule, because nothing else validates it under `cargo test`.
    #[test]
    fn repository_manifest_satisfies_the_contract() -> TestResult {
        let root = project_root()?;

        let stats = validate(&root)?;

        assert_eq!(stats.comparison_classes, REQUIRED_COMPARISON_CLASSES.len());
        assert!(stats.fixtures > 0, "repository manifest declares no fixtures");
        Ok(())
    }

    /// "Exactly one class-contract version and one active owner" — a second
    /// contract for the same class must fail rather than let two versions coexist.
    #[test]
    fn rejects_duplicate_class_contract() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r##"    {"comparison_class": "CompileEffect", "contract_version": "v1", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"]}"##,
                r##"    {"comparison_class": "CompileEffect", "contract_version": "v1", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"]},
    {"comparison_class": "CompileEffect", "contract_version": "v2", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"]}"##,
            ),
        )?;

        let err = validate(tempdir.path()).expect_err("duplicate class contract must fail");

        assert!(err.to_string().contains("oracle fixture manifest check failed"), "{err:?}");
        Ok(())
    }

    /// Every declared class needs an owner. Dropping a contract must fail rather
    /// than leave a declared class silently unowned.
    #[test]
    fn rejects_declared_class_without_class_contract() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r##",
    {"comparison_class": "CompileEffect", "contract_version": "v1", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"]}"##,
                "",
            ),
        )?;

        let err = validate(tempdir.path()).expect_err("missing class contract must fail");

        assert!(err.to_string().contains("oracle fixture manifest check failed"), "{err:?}");
        Ok(())
    }

    /// A class claiming `declared` coverage with no fixture behind it is the
    /// exact silent hole this contract exists to close.
    #[test]
    fn rejects_declared_coverage_without_a_fixture() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r##""comparison_class": "PackageSubTable", "contract_version": "v1", "owner": "#13645", "coverage": "pending_fixture""##,
                r##""comparison_class": "PackageSubTable", "contract_version": "v1", "owner": "#13645", "coverage": "declared""##,
            ),
        )?;

        let err =
            validate(tempdir.path()).expect_err("declared coverage without fixture must fail");

        assert!(err.to_string().contains("oracle fixture manifest check failed"), "{err:?}");
        Ok(())
    }

    /// The opposite direction: a class a fixture really does declare may not be
    /// reported as still pending.
    #[test]
    fn rejects_pending_coverage_for_a_covered_class() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r##""comparison_class": "FrameworkGeneratedMember", "contract_version": "v1", "owner": "#4766", "coverage": "declared""##,
                r##""comparison_class": "FrameworkGeneratedMember", "contract_version": "v1", "owner": "#4766", "coverage": "pending_fixture""##,
            ),
        )?;

        let err =
            validate(tempdir.path()).expect_err("pending coverage for a covered class must fail");

        assert!(err.to_string().contains("oracle fixture manifest check failed"), "{err:?}");
        Ok(())
    }

    /// A fixture may not expect a fact family none of its comparison classes
    /// compares — that declaration could never be satisfied by any receipt.
    #[test]
    fn rejects_fact_family_outside_the_fixture_comparison_classes() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""expected_fact_families": ["generated_members"]"#,
                r#""expected_fact_families": ["generated_members", "prototype_entries"]"#,
            ),
        )?;

        let err = validate(tempdir.path()).expect_err("cross-class fact family must fail");

        assert!(err.to_string().contains("oracle fixture manifest check failed"), "{err:?}");
        Ok(())
    }

    #[test]
    fn rejects_fixture_owner_that_is_not_an_issue_reference() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            // Scoped to the fixture row: the class contracts also carry "#4766",
            // and a blanket replace would be caught by contract validation instead.
            text.replace(
                "\"owner\": \"#4766\",\n      \"module_files\"",
                "\"owner\": \"perl-lsp maintainers\",\n      \"module_files\"",
            ),
        )?;

        let err = validate(tempdir.path()).expect_err("non-issue fixture owner must fail");

        assert!(err.to_string().contains("oracle fixture manifest check failed"), "{err:?}");
        Ok(())
    }

    /// #13622 falsifier 6: a fixture may not rely on a module outside the roots
    /// the runner is told to use.
    #[test]
    fn rejects_module_file_outside_declared_module_roots() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        fs::create_dir_all(tempdir.path().join("outside"))?;
        fs::write(tempdir.path().join("outside/Helper.pm"), "package Helper; 1;\n")?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(r#""module_files": []"#, r#""module_files": ["outside/Helper.pm"]"#),
        )?;

        let err = validate(tempdir.path()).expect_err("undeclared module root must fail");

        assert!(err.to_string().contains("oracle fixture manifest check failed"), "{err:?}");
        Ok(())
    }

    /// A declared load file that does not exist must fail, not be skipped.
    #[test]
    fn rejects_missing_declared_module_file() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(r#""module_files": []"#, r#""module_files": ["fixtures/Absent.pm"]"#),
        )?;

        let err = validate(tempdir.path()).expect_err("missing module file must fail");

        assert!(err.to_string().contains("oracle fixture manifest check failed"), "{err:?}");
        Ok(())
    }

    fn valid_manifest_workspace() -> TestResult<tempfile::TempDir> {
        let tempdir = tempfile::tempdir()?;
        fs::create_dir_all(tempdir.path().join("schemas"))?;
        fs::create_dir_all(tempdir.path().join("docs/specs"))?;
        fs::create_dir_all(tempdir.path().join("crates/perl-corpus/fixtures/differential_oracle"))?;
        fs::create_dir_all(tempdir.path().join("fixtures"))?;
        fs::write(tempdir.path().join(SCHEMA_PATH), "{}\n")?;
        fs::write(tempdir.path().join(ORACLE_SPEC), "# oracle spec\n")?;
        fs::write(tempdir.path().join("fixtures/package_basic.pl"), "package Demo; 1;\n")?;
        fs::write(tempdir.path().join(MANIFEST_PATH), valid_manifest_text())?;
        Ok(tempdir)
    }

    fn valid_manifest_text() -> String {
        format!(
            r##"{{
  "schema_version": "{SCHEMA_VERSION}",
  "manifest": "{MANIFEST_NAME}",
  "owner": "perl-lsp maintainers",
  "status": "declaration-only",
  "updated": "2026-05-22",
  "spec": "{ORACLE_SPEC}",
  "runner": "none",
  "editor_runtime_dependency": false,
  "comparison_classes": [{comparison_classes}],
  "result_classes": [{result_classes}],
  "required_environment_denials": ["PERL5LIB", "PERL5OPT", "local::lib"],
  "default_claim_boundary": "Fixture declaration only; no oracle runner, Perl execution, provider behavior, support-tier promotion, or parser/corpus bucket movement.",
  "class_contracts": [
    {{"comparison_class": "PackageSubTable", "contract_version": "v1", "owner": "#13645", "coverage": "pending_fixture", "fact_families": ["packages", "named_subs", "source_ranges", "stash_entries"]}},
    {{"comparison_class": "ImportExport", "contract_version": "v1", "owner": "#13624", "coverage": "pending_fixture", "fact_families": ["import_specs", "export_sets", "visible_symbols"]}},
    {{"comparison_class": "IsaComposition", "contract_version": "v1", "owner": "#13626", "coverage": "pending_fixture", "fact_families": ["isa_entries", "inheritance_facts", "role_composition_facts"]}},
    {{"comparison_class": "ConstantPrototype", "contract_version": "v1", "owner": "#13629", "coverage": "pending_fixture", "fact_families": ["constants", "prototype_entries", "compile_effects"]}},
    {{"comparison_class": "FrameworkGeneratedMember", "contract_version": "v1", "owner": "#4766", "coverage": "declared", "fact_families": ["generated_members"]}},
    {{"comparison_class": "CompileEffect", "contract_version": "v1", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"]}}
  ],
  "fixtures": [
    {{
      "id": "package_basic",
      "source": "fixtures/package_basic.pl",
      "path_class": "public_test_fixture",
      "perl_version_constraint": "any-supported-real-perl",
      "include_path_authority": "declared_fixture_root",
      "module_roots": ["fixtures"],
      "environment_denials": ["PERL5LIB", "PERL5OPT", "local::lib"],
      "comparison_classes": ["FrameworkGeneratedMember"],
      "dynamic_boundaries": [],
      "unsupported_effects": [],
      "framework_adapters": ["Moo"],
      "claim_boundary": "Fixture declaration only; no oracle runner, Perl execution, provider behavior, support-tier promotion, or parser/corpus bucket movement.",
      "expected_fact_families": ["generated_members"],
      "owner": "#4766",
      "module_files": []
    }}
  ]
}}
"##,
            comparison_classes = quoted_list(REQUIRED_COMPARISON_CLASSES),
            result_classes = quoted_list(REQUIRED_RESULT_CLASSES),
        )
    }

    fn quoted_list(values: &[&str]) -> String {
        values.iter().map(|value| format!("\"{value}\"")).collect::<Vec<_>>().join(", ")
    }
}
