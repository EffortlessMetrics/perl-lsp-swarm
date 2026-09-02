//! Validate the differential real-Perl oracle fixture manifest.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
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

/// The single active owner issue for each comparison class, per the
/// "Entry conditions" table in issue #4767.
///
/// Pinning these makes the owner field mean something: a well-formed but wrong
/// issue number is caught offline, without making validation depend on live
/// GitHub state.
const CLASS_OWNERS: &[(&str, &str)] = &[
    ("PackageSubTable", "#13645"),
    ("ImportExport", "#13624"),
    ("IsaComposition", "#13626"),
    ("ConstantPrototype", "#13629"),
    ("FrameworkGeneratedMember", "#4766"),
    ("CompileEffect", "#13632"),
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
    #[serde(default)]
    pending_fact_families: Vec<String>,
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

/// Run every rule and return the stats alongside the exact violations.
///
/// Kept separate from [`validate`] so tests can assert *which* rule fired rather
/// than only that something failed — a rejection test that accepts any error can
/// pass on an unrelated violation.
fn evaluate(root: &Path) -> Result<(ValidationStats, Vec<String>)> {
    validate_json_parse(root, SCHEMA_PATH)?;
    let manifest = read_manifest(root, MANIFEST_PATH)?;
    let mut violations = Vec::new();

    validate_against_schema(root, &mut violations)?;
    validate_manifest_shape(root, &manifest, &mut violations);
    validate_fixtures(root, &manifest, &mut violations);
    validate_class_contracts(&manifest, &mut violations);

    let stats = ValidationStats {
        fixtures: manifest.fixtures.len(),
        comparison_classes: manifest.comparison_classes.len(),
        result_classes: manifest.result_classes.len(),
    };
    Ok((stats, violations))
}

fn validate(root: &Path) -> Result<ValidationStats> {
    let (stats, violations) = evaluate(root)?;

    if !violations.is_empty() {
        eprintln!("oracle fixture manifest violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!("oracle fixture manifest check failed with {} violation(s)", violations.len());
    }

    Ok(stats)
}

/// Apply the JSON Schema to the manifest.
///
/// Without this the schema file is documentation only: its `additionalProperties:
/// false` and `required` lists gate nothing, because the Rust structs ignore
/// unknown keys and default several fields. Compiling and applying it is the
/// same pattern `ux_scorecard` uses for the CI receipt schema.
fn validate_against_schema(root: &Path, violations: &mut Vec<String>) -> Result<()> {
    let schema: serde_json::Value = serde_json::from_str(&read_text(root, SCHEMA_PATH)?)
        .with_context(|| format!("failed to parse {SCHEMA_PATH} as JSON"))?;
    let manifest: serde_json::Value = serde_json::from_str(&read_text(root, MANIFEST_PATH)?)
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
    let mut attested_families: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for fixture in &manifest.fixtures {
        for class in &fixture.comparison_classes {
            covered_by_fixture.insert(class.as_str());
            // A fixture attests a family only for the classes it itself declares.
            let entry = attested_families.entry(class.as_str()).or_default();
            for family in &fixture.expected_fact_families {
                if class_fact_families(class).is_some_and(|f| f.contains(&family.as_str())) {
                    entry.insert(family.as_str());
                }
            }
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
        if let Some((_, expected)) = CLASS_OWNERS.iter().find(|(name, _)| *name == class)
            && contract.owner != *expected
        {
            violations.push(format!(
                "{doc}: owner is {:?}; the authoritative owner for this class is {expected:?}",
                contract.owner
            ));
        }
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

        validate_pending_fact_families(
            &doc,
            contract,
            attested_families.get(class).unwrap_or(&BTreeSet::new()),
            violations,
        );
    }

    for class in declared.difference(&seen) {
        violations
            .push(format!("{MANIFEST_PATH}: comparison class {class:?} has no class_contract"));
    }
}

/// `coverage` is class-granular, so "declared" alone can hide a fact family the
/// class compares but no fixture exercises. Every family in the class contract
/// must therefore be either attested by a fixture or listed as pending — and a
/// family cannot be both.
fn validate_pending_fact_families(
    doc: &str,
    contract: &OracleClassContract,
    attested: &BTreeSet<&str>,
    violations: &mut Vec<String>,
) {
    let declared_families =
        contract.fact_families.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let pending =
        contract.pending_fact_families.iter().map(String::as_str).collect::<BTreeSet<_>>();

    for family in pending.difference(&declared_families) {
        violations.push(format!(
            "{doc}: pending_fact_families entry {family:?} is not one of this class's fact_families"
        ));
    }
    for family in pending.intersection(attested) {
        violations.push(format!(
            "{doc}: fact family {family:?} is listed as pending but a fixture already attests it"
        ));
    }
    for family in &declared_families {
        if !attested.contains(family) && !pending.contains(family) {
            violations.push(format!(
                "{doc}: fact family {family:?} is neither attested by a fixture nor listed in pending_fact_families"
            ));
        }
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
        require_regular_file(root, doc, "module_files", module_file, violations);
    }
    require_regular_file(root, doc, "source", &fixture.source, violations);
    require_declared_module_graph(root, doc, fixture, violations);

    for (field, value) in std::iter::once(("source", &fixture.source))
        .chain(fixture.module_files.iter().map(|file| ("module_files", file)))
    {
        // A `..` segment would satisfy the lexical prefix test below while
        // actually resolving outside the declared root, and the repo-root check
        // in `validate_relative_existing_path` would still pass. Declared fixture
        // assets never need traversal, so refuse it outright.
        if has_traversal_segment(value) {
            violations
                .push(format!("{doc}: {field} {value:?} must not contain a \"..\" path segment"));
            continue;
        }
        if !fixture.module_roots.iter().any(|module_root| is_contained_by(value, module_root)) {
            violations.push(format!(
                "{doc}: {field} {value:?} is not contained by any declared module_root"
            ));
            continue;
        }
        // The lexical test above only proves the declared *path* sits under a
        // root. A symlink written below a root can still resolve to a file
        // elsewhere in the repository, which `validate_relative_existing_path`
        // accepts because the target is repo-internal. Compare resolved paths too.
        if !resolves_within_a_module_root(root, fixture, value) {
            violations.push(format!(
                "{doc}: {field} {value:?} resolves outside every declared module_root"
            ));
        }
    }
}

/// Containment after symlink resolution. A path whose declared form is inside a
/// root but whose target is not must not pass.
fn resolves_within_a_module_root(root: &Path, fixture: &OracleFixture, value: &str) -> bool {
    let Ok(asset) = root.join(value).canonicalize() else {
        return true; // Missing paths are reported by the existence check.
    };
    fixture.module_roots.iter().any(|module_root| {
        root.join(module_root).canonicalize().is_ok_and(|resolved| asset.starts_with(&resolved))
    })
}

/// `module_files` is only a promise unless something can tell when a required
/// entry is missing. Read the fixture source, resolve each `use`/`require` of a
/// package-style module against the fixture's own declared roots, and require
/// every one that actually resolves to a file to be declared.
///
/// Core and CPAN modules (`strict`, `Exporter`, ...) do not resolve inside a
/// fixture root, so they are not required here — the check names exactly the
/// files that make this fixture a multi-file graph.
fn require_declared_module_graph(
    root: &Path,
    doc: &str,
    fixture: &OracleFixture,
    violations: &mut Vec<String>,
) {
    let declared = fixture.module_files.iter().map(String::as_str).collect::<BTreeSet<_>>();

    // Walk the source *and* every declared load file: a declared module can pull
    // in a further module, and that transitive file is just as required.
    for (field, asset) in std::iter::once(("source", fixture.source.as_str()))
        .chain(fixture.module_files.iter().map(|file| ("module_files", file.as_str())))
    {
        let path = root.join(asset);
        if !path.is_file() {
            continue; // Reported by the existence / regular-file checks.
        }
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                // Existence and file type are not readability. An unreadable
                // asset cannot be loaded, so it must not pass silently.
                violations.push(format!("{doc}: {field} {asset:?} could not be read: {error}"));
                continue;
            }
        };

        for module in used_module_names(&text) {
            let relative = format!("{}.pm", module.replace("::", "/"));
            for module_root in &fixture.module_roots {
                let candidate = format!("{}/{relative}", module_root.trim_end_matches('/'));
                if root.join(&candidate).is_file() && !declared.contains(candidate.as_str()) {
                    violations.push(format!(
                        "{doc}: {field} {asset:?} loads module {module:?}, which resolves to {candidate:?} inside a declared module_root, but that file is not listed in module_files"
                    ));
                }
            }
        }
    }
}

/// Package-style `use`/`require` targets only.
///
/// Single-segment names (`use Helper;`) count as well as `::`-qualified ones —
/// a fixture module need not be nested. Pragmas (`strict`, `feature`) are
/// lowercase and version forms (`use v5.36`) are numeric, so requiring an
/// uppercase initial excludes them; core and CPAN modules are excluded later by
/// simply not resolving to a file inside a declared fixture root.
fn used_module_names(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in source.lines() {
        let line = line.trim_start();
        let rest = line
            .strip_prefix("use ")
            .or_else(|| line.strip_prefix("require "))
            .map(str::trim_start);
        let Some(rest) = rest else { continue };

        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':')
            .collect();
        if name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && !name.ends_with(':')
            && !name.contains(":::")
        {
            names.insert(name);
        }
    }
    names
}

/// A declared load file must be a readable file. An existing directory satisfies
/// a bare existence check but nothing can load it.
fn require_regular_file(
    root: &Path,
    doc: &str,
    field: &str,
    value: &str,
    violations: &mut Vec<String>,
) {
    let path = root.join(value);
    if path.exists() && !path.is_file() {
        violations.push(format!("{doc}: {field} {value:?} is not a regular file"));
    }
}

fn has_traversal_segment(path: &str) -> bool {
    path.split('/').any(|segment| segment == "..")
}

/// Purely lexical containment on repo-relative slash paths, used only after
/// [`has_traversal_segment`] has ruled out `..`. Existence and repo-root escape
/// are checked separately by `validate_relative_existing_path`.
///
/// The prefix test requires a following `/` so that a sibling directory sharing
/// the root's name (root `fixtures` vs path `fixtures_evil/x.pm`) is not
/// mistaken for containment.
fn is_contained_by(path: &str, module_root: &str) -> bool {
    let root = module_root.trim_end_matches('/');
    if root.is_empty() || has_traversal_segment(root) {
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
                r##"    {"comparison_class": "CompileEffect", "contract_version": "v1", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"], "pending_fact_families": ["compile_effects", "dynamic_boundaries"]}"##,
                r##"    {"comparison_class": "CompileEffect", "contract_version": "v1", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"], "pending_fact_families": ["compile_effects", "dynamic_boundaries"]},
    {"comparison_class": "CompileEffect", "contract_version": "v2", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"], "pending_fact_families": ["compile_effects", "dynamic_boundaries"]}"##,
            ),
        )?;

        assert_violation(
            tempdir.path(),
            r#"duplicate class_contract for comparison class "CompileEffect""#,
        )
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
    {"comparison_class": "CompileEffect", "contract_version": "v1", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"], "pending_fact_families": ["compile_effects", "dynamic_boundaries"]}"##,
                "",
            ),
        )?;

        assert_violation(
            tempdir.path(),
            r#"comparison class "CompileEffect" has no class_contract"#,
        )
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

        assert_violation(
            tempdir.path(),
            "coverage is \"declared\" but no fixture declares this comparison class",
        )
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

        assert_violation(
            tempdir.path(),
            "coverage is \"pending_fixture\" but a fixture already declares",
        )
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

        assert_violation(
            tempdir.path(),
            r#"expected_fact_families entry "prototype_entries" is not compared by any"#,
        )
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

        assert_violation(
            tempdir.path(),
            r#"owner "perl-lsp maintainers" must be an issue reference"#,
        )
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

        assert_violation(
            tempdir.path(),
            r#"module_files "outside/Helper.pm" is not contained by any declared module_root"#,
        )
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

        assert_violation(tempdir.path(), "module_files points to missing path fixtures/Absent.pm")
    }

    /// Devin Review's negative control: drop the `imports_exports` producer from
    /// `module_files` while leaving the file on disk. Before the module-graph
    /// check this passed, because nothing could tell a required entry was absent.
    #[test]
    fn rejects_dropping_a_required_module_file_from_the_repository_manifest() -> TestResult {
        let root = project_root()?;
        let text = fs::read_to_string(root.join(MANIFEST_PATH))?;
        let stripped = text.replace(
            "\"crates/perl-corpus/fixtures/parser_accuracy/Accuracy/ImportsExports.pm\"\n      ",
            "",
        );
        assert_ne!(stripped, text, "manifest no longer declares the producer module");

        let tempdir = mirror_repository_manifest(&root, &stripped)?;

        assert_violation(tempdir.path(), r#"loads module "Accuracy::ImportsExports""#)
    }

    /// A symlink written inside a declared root but pointing at a repo-internal
    /// file outside it passes both the lexical containment test and the repo-root
    /// escape check. Only comparing resolved paths catches it.
    #[test]
    fn rejects_module_file_symlinked_outside_its_declared_root() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        fs::create_dir_all(tempdir.path().join("outside"))?;
        fs::write(tempdir.path().join("outside/Helper.pm"), "package Helper; 1;\n")?;
        if !create_file_symlink_for_test(
            &tempdir.path().join("outside/Helper.pm"),
            &tempdir.path().join("fixtures/Helper.pm"),
        )? {
            return Ok(()); // Windows session without the symlink privilege.
        }
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(r#""module_files": []"#, r#""module_files": ["fixtures/Helper.pm"]"#),
        )?;

        assert_violation(tempdir.path(), "resolves outside every declared module_root")
    }

    /// An existing directory satisfies a bare existence check but nothing can
    /// load it, so it must not pass as a declared load file.
    #[test]
    fn rejects_directory_declared_as_a_module_file() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        fs::create_dir_all(tempdir.path().join("fixtures/NotAFile.pm"))?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(r#""module_files": []"#, r#""module_files": ["fixtures/NotAFile.pm"]"#),
        )?;

        assert_violation(tempdir.path(), "is not a regular file")
    }

    /// A well-formed but wrong issue number must not pass as an owner.
    #[test]
    fn rejects_well_formed_but_incorrect_class_owner() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(&manifest_path, text.replace(r##""owner": "#13626""##, r##""owner": "#1""##))?;

        assert_violation(tempdir.path(), r##"the authoritative owner for this class is "#13626""##)
    }

    #[test]
    fn class_owner_table_covers_exactly_the_required_comparison_classes() {
        let owned = CLASS_OWNERS.iter().map(|(class, _)| *class).collect::<BTreeSet<&str>>();
        let required = REQUIRED_COMPARISON_CLASSES.iter().copied().collect::<BTreeSet<&str>>();

        assert_eq!(owned, required, "class owner table and comparison-class list disagree");
    }

    /// Only package-style module loads count; pragmas and feature/version forms
    /// never name a fixture module file.
    #[test]
    fn used_module_names_selects_only_package_style_loads() {
        let names = used_module_names(
            "use strict;\nuse warnings;\nuse v5.36;\nuse Accuracy::ImportsExports;\n  require Deep::Nested::Thing;\nuse feature 'signatures';\n",
        );

        assert!(names.contains("Accuracy::ImportsExports"));
        assert!(names.contains("Deep::Nested::Thing"));
        assert_eq!(names.len(), 2, "unexpected extra module names: {names:?}");
    }

    /// Repository convention (see `xtask/src/publication_drift`): Windows keeps
    /// symlink coverage but skips visibly when the session lacks the privilege
    /// (os error 1314), rather than dropping the platform entirely.
    #[cfg(unix)]
    fn create_file_symlink_for_test(target: &Path, link: &Path) -> TestResult<bool> {
        std::os::unix::fs::symlink(target, link)?;
        Ok(true)
    }

    #[cfg(windows)]
    fn create_file_symlink_for_test(target: &Path, link: &Path) -> TestResult<bool> {
        if perl_tdd_support::symlink_test_decision().skip_visibly() {
            return Ok(false);
        }
        Ok(perl_tdd_support::try_create_file_symlink(target, link)?.is_some())
    }

    /// A single-segment module is still a fixture dependency, and a declared
    /// module file can pull in a further one. Both must be discovered.
    #[test]
    fn rejects_undeclared_single_segment_and_transitive_module_dependencies() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        // package_basic.pl loads Helper; Helper.pm loads Deeper.
        fs::write(
            tempdir.path().join("fixtures/package_basic.pl"),
            "package Demo;\nuse strict;\nuse Helper;\n1;\n",
        )?;
        fs::write(tempdir.path().join("fixtures/Helper.pm"), "package Helper;\nuse Deeper;\n1;\n")?;
        fs::write(tempdir.path().join("fixtures/Deeper.pm"), "package Deeper;\n1;\n")?;

        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;

        // Single-segment dependency undeclared.
        assert_violation(tempdir.path(), r#"loads module "Helper""#)?;

        // Declaring it surfaces the transitive one.
        fs::write(
            &manifest_path,
            text.replace(r#""module_files": []"#, r#""module_files": ["fixtures/Helper.pm"]"#),
        )?;

        assert_violation(tempdir.path(), r#"loads module "Deeper""#)
    }

    /// Existence and file type are not readability. A file whose bytes are not
    /// UTF-8 cannot be read as source on any platform, and must not pass.
    #[test]
    fn rejects_unreadable_fixture_source() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        fs::write(tempdir.path().join("fixtures/package_basic.pl"), [0xff, 0xfe, 0x00, 0x9f])?;

        assert_violation(tempdir.path(), "could not be read")
    }

    /// Copy the repository's real schema, spec, fixture assets, and a (possibly
    /// mutated) manifest into a tempdir so a repository-scale mutation can be
    /// validated without touching the working tree.
    fn mirror_repository_manifest(
        real_root: &Path,
        manifest_text: &str,
    ) -> TestResult<tempfile::TempDir> {
        let tempdir = tempfile::tempdir()?;
        for relative in [SCHEMA_PATH, ORACLE_SPEC, MANIFEST_PATH] {
            let destination = tempdir.path().join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(real_root.join(relative), &destination)?;
        }
        let fixtures = "crates/perl-corpus/fixtures/parser_accuracy";
        copy_tree(&real_root.join(fixtures), &tempdir.path().join(fixtures))?;
        fs::write(tempdir.path().join(MANIFEST_PATH), manifest_text)?;
        Ok(tempdir)
    }

    fn copy_tree(from: &Path, to: &Path) -> TestResult {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_tree(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), &target)?;
            }
        }
        Ok(())
    }

    /// The schema declares `additionalProperties: false`, but the Rust structs
    /// ignore unknown keys — so this only fails if the schema is actually applied.
    /// It is the proof that the schema file is enforcement, not documentation.
    #[test]
    fn rejects_unknown_key_that_only_the_schema_can_catch() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""id": "package_basic","#,
                r#""id": "package_basic", "smuggled": true,"#,
            ),
        )?;

        assert_violation(tempdir.path(), "schema violation")
    }

    /// `coverage: "declared"` is class-granular, so without this rule a class can
    /// read as covered while one of its fact families has no fixture at all.
    /// Dropping a family from `pending_fact_families` must surface that.
    #[test]
    fn rejects_fact_family_neither_attested_nor_pending() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""pending_fact_families": ["compile_effects", "dynamic_boundaries"]"#,
                r#""pending_fact_families": ["compile_effects"]"#,
            ),
        )?;

        assert_violation(
            tempdir.path(),
            r#"fact family "dynamic_boundaries" is neither attested by a fixture nor listed in pending_fact_families"#,
        )
    }

    /// The opposite contradiction: a family a fixture does attest may not also be
    /// reported as still pending.
    #[test]
    fn rejects_pending_fact_family_that_a_fixture_attests() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""fact_families": ["generated_members"], "pending_fact_families": []"#,
                r#""fact_families": ["generated_members"], "pending_fact_families": ["generated_members"]"#,
            ),
        )?;

        assert_violation(
            tempdir.path(),
            r#"fact family "generated_members" is listed as pending but a fixture already attests it"#,
        )
    }

    #[test]
    fn rejects_pending_fact_family_outside_the_class_contract() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""fact_families": ["generated_members"], "pending_fact_families": []"#,
                r#""fact_families": ["generated_members"], "pending_fact_families": ["packages"]"#,
            ),
        )?;

        assert_violation(
            tempdir.path(),
            r#"pending_fact_families entry "packages" is not one of this class's fact_families"#,
        )
    }

    #[test]
    fn rejects_class_contract_for_an_undeclared_comparison_class() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""comparison_class": "CompileEffect""#,
                r#""comparison_class": "Invented""#,
            ),
        )?;

        assert_violation(
            tempdir.path(),
            r#"class_contracts declares unknown comparison class "Invented""#,
        )
    }

    #[test]
    fn rejects_malformed_contract_version() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r##""owner": "#13645", "coverage""##,
                r##""owner": "#13645", "contract_version_marker": "", "coverage""##,
            )
            .replace(
                r##""contract_version": "v1", "owner": "#13645""##,
                r##""contract_version": "1.0", "owner": "#13645""##,
            ),
        )?;

        assert_violation(tempdir.path(), r#"contract_version "1.0" must look like "v1""#)
    }

    #[test]
    fn rejects_class_contract_owner_that_is_not_an_issue_reference() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(r##""owner": "#13626""##, r#""owner": "team-oracle""#),
        )?;

        assert_violation(tempdir.path(), r#"owner "team-oracle" must be an issue reference"#)
    }

    #[test]
    fn rejects_unknown_class_coverage_value() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r##""owner": "#13626", "coverage": "pending_fixture""##,
                r##""owner": "#13626", "coverage": "probably""##,
            ),
        )?;

        assert_violation(tempdir.path(), r#"coverage "probably" is not allowed"#)
    }

    /// The class contract's fact families are pinned to the specification table,
    /// so a contract may not quietly narrow or widen what its class compares.
    #[test]
    fn rejects_class_fact_families_that_disagree_with_the_specification() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""fact_families": ["import_specs", "export_sets", "visible_symbols"], "pending_fact_families": ["import_specs", "export_sets", "visible_symbols"]"#,
                r#""fact_families": ["import_specs", "export_sets"], "pending_fact_families": ["import_specs", "export_sets"]"#,
            ),
        )?;

        assert_violation(
            tempdir.path(),
            r#"fact_families missing required entry "visible_symbols""#,
        )
    }

    /// A `..` segment resolves outside the declared root while still living in
    /// the repository, so neither the lexical prefix test nor the repo-root check
    /// would catch it on its own.
    #[test]
    fn rejects_module_file_escaping_its_root_via_parent_traversal() -> TestResult {
        let tempdir = valid_manifest_workspace()?;
        fs::create_dir_all(tempdir.path().join("outside"))?;
        fs::write(tempdir.path().join("outside/Helper.pm"), "package Helper; 1;\n")?;
        let manifest_path = tempdir.path().join(MANIFEST_PATH);
        let text = fs::read_to_string(&manifest_path)?;
        fs::write(
            &manifest_path,
            text.replace(
                r#""module_files": []"#,
                r#""module_files": ["fixtures/../outside/Helper.pm"]"#,
            ),
        )?;

        assert_violation(tempdir.path(), r#"must not contain a ".." path segment"#)
    }

    /// A sibling directory whose name starts with the root's name is not inside
    /// the root. `fixtures_evil/` must not pass as `fixtures/`.
    #[test]
    fn sibling_directory_sharing_a_root_name_is_not_contained() {
        assert!(is_contained_by("fixtures/a.pm", "fixtures"));
        assert!(is_contained_by("fixtures/sub/a.pm", "fixtures/"));
        assert!(!is_contained_by("fixtures_evil/a.pm", "fixtures"));
        assert!(!is_contained_by("fixtures", "fixtures"), "the root itself is not a file in it");
        assert!(!is_contained_by("/abs/fixtures/a.pm", "fixtures"));
        assert!(!is_contained_by("fixtures/a.pm", ""), "an empty root contains nothing");
    }

    /// Assert that the manifest under `root` fails, and that it fails for the
    /// named reason — not merely that some rule somewhere rejected it.
    fn assert_violation(root: &Path, expected: &str) -> TestResult {
        let (_, violations) = evaluate(root)?;
        assert!(
            violations.iter().any(|violation| violation.contains(expected)),
            "expected a violation containing {expected:?}, got: {violations:#?}"
        );
        validate(root).expect_err("a manifest with violations must fail the check");
        Ok(())
    }

    fn valid_manifest_workspace() -> TestResult<tempfile::TempDir> {
        let tempdir = tempfile::tempdir()?;
        fs::create_dir_all(tempdir.path().join("schemas"))?;
        fs::create_dir_all(tempdir.path().join("docs/specs"))?;
        fs::create_dir_all(tempdir.path().join("crates/perl-corpus/fixtures/differential_oracle"))?;
        fs::create_dir_all(tempdir.path().join("fixtures"))?;
        // The real schema, so tempdir tests are gated by the same document the
        // repository ships rather than a permissive stub.
        fs::write(
            tempdir.path().join(SCHEMA_PATH),
            include_str!("../../../schemas/oracle_fixture_manifest.v1.schema.json"),
        )?;
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
    {{"comparison_class": "PackageSubTable", "contract_version": "v1", "owner": "#13645", "coverage": "pending_fixture", "fact_families": ["packages", "named_subs", "source_ranges", "stash_entries"], "pending_fact_families": ["packages", "named_subs", "source_ranges", "stash_entries"]}},
    {{"comparison_class": "ImportExport", "contract_version": "v1", "owner": "#13624", "coverage": "pending_fixture", "fact_families": ["import_specs", "export_sets", "visible_symbols"], "pending_fact_families": ["import_specs", "export_sets", "visible_symbols"]}},
    {{"comparison_class": "IsaComposition", "contract_version": "v1", "owner": "#13626", "coverage": "pending_fixture", "fact_families": ["isa_entries", "inheritance_facts", "role_composition_facts"], "pending_fact_families": ["isa_entries", "inheritance_facts", "role_composition_facts"]}},
    {{"comparison_class": "ConstantPrototype", "contract_version": "v1", "owner": "#13629", "coverage": "pending_fixture", "fact_families": ["constants", "prototype_entries", "compile_effects"], "pending_fact_families": ["constants", "prototype_entries", "compile_effects"]}},
    {{"comparison_class": "FrameworkGeneratedMember", "contract_version": "v1", "owner": "#4766", "coverage": "declared", "fact_families": ["generated_members"], "pending_fact_families": []}},
    {{"comparison_class": "CompileEffect", "contract_version": "v1", "owner": "#13632", "coverage": "pending_fixture", "fact_families": ["compile_effects", "dynamic_boundaries"], "pending_fact_families": ["compile_effects", "dynamic_boundaries"]}}
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
