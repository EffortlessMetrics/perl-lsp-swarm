//! Contract tests for the native parser incremental authority ledger.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Debug, Deserialize)]
struct AuthorityManifest {
    schema_version: u32,
    owner_issue: u64,
    canonical: CanonicalSurface,
    compatibility: CompatibilitySurface,
    lower_tier: Vec<LowerTierSurface>,
    modules: Vec<ModuleSurface>,
}

#[derive(Debug, Deserialize)]
struct CanonicalSurface {
    module: String,
    path: String,
    entry_points: Vec<String>,
    production_eligible: bool,
    eligibility_blockers: Vec<u64>,
    decision: String,
}

#[derive(Debug, Deserialize)]
struct CompatibilitySurface {
    package: String,
    status: String,
    implementation_owner: String,
    behavior_authority: bool,
    exit_issue: u64,
}

#[derive(Debug, Deserialize)]
struct LowerTierSurface {
    package: String,
    module: String,
    path: String,
    status: String,
    entry_points: Vec<String>,
    behavior_authority: bool,
    production_eligible: bool,
    allowed_consumers: Vec<String>,
    next_action: String,
    owner_issue: u64,
    decision: String,
}

#[derive(Debug, Deserialize)]
struct ModuleSurface {
    module: String,
    status: String,
    public_reexport: bool,
    production_eligible: bool,
    next_action: String,
    owner_issue: u64,
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: impl AsRef<Path>) -> Result<String, std::io::Error> {
    fs::read_to_string(path)
}

fn load_manifest() -> Result<AuthorityManifest, Box<dyn std::error::Error>> {
    let source = read(crate_root().join("incremental_authority.json"))?;
    Ok(serde_json::from_str(&source)?)
}

fn compact_whitespace(source: &str) -> String {
    source.split_whitespace().collect()
}

fn public_incremental_modules(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("pub mod ")
                .and_then(|rest| rest.strip_suffix(';'))
                .filter(|module| module.starts_with("incremental_"))
                .map(str::to_owned)
        })
        .collect()
}

fn facade_incremental_reexports(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("pub use incremental::")
                .and_then(|rest| rest.strip_suffix(';'))
                .filter(|module| module.starts_with("incremental_"))
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn ledger_names_one_canonical_candidate_without_claiming_readiness() -> TestResult {
    let manifest = load_manifest()?;

    assert_eq!(manifest.schema_version, 2);
    assert_eq!(manifest.owner_issue, 6701);
    assert_eq!(manifest.canonical.module, "incremental");
    assert_eq!(manifest.canonical.path, "perl_parser::incremental");
    assert_eq!(
        manifest.canonical.entry_points.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "Edit".to_string(),
            "IncrementalState".to_string(),
            "apply_edits".to_string(),
        ])
    );
    assert!(
        !manifest.canonical.production_eligible,
        "canonical ownership must not be mistaken for production readiness"
    );
    assert_eq!(
        manifest.canonical.eligibility_blockers.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([2327, 6704, 6710, 6714])
    );
    assert!(!manifest.canonical.decision.trim().is_empty());

    assert_eq!(manifest.compatibility.package, "perl-incremental-parsing");
    assert_eq!(manifest.compatibility.status, "compatibility");
    assert_eq!(manifest.compatibility.implementation_owner, "perl-parser");
    assert!(!manifest.compatibility.behavior_authority);
    assert_eq!(manifest.compatibility.exit_issue, 6701);

    Ok(())
}

#[test]
fn every_public_incremental_generation_has_one_non_production_disposition() -> TestResult {
    let manifest = load_manifest()?;
    let module_source = read(crate_root().join("src/incremental/mod.rs"))?;
    let facade_source = read(crate_root().join("src/lib.rs"))?;

    let mut classified = BTreeMap::new();
    for surface in &manifest.modules {
        assert!(
            classified.insert(surface.module.clone(), surface).is_none(),
            "duplicate incremental authority entry: {}",
            surface.module
        );
        assert!(
            matches!(surface.status.as_str(), "experimental" | "internal" | "retire"),
            "unsupported incremental authority status for {}: {}",
            surface.module,
            surface.status
        );
        assert!(
            !surface.production_eligible,
            "non-canonical generation {} cannot be production-eligible",
            surface.module
        );
        assert!(!surface.next_action.trim().is_empty());
        assert!(surface.owner_issue > 0);
    }

    let declared = public_incremental_modules(&module_source);
    let classified_names = classified.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(declared, classified_names);

    let facade_reexports = facade_incremental_reexports(&facade_source);
    let manifest_reexports = manifest
        .modules
        .iter()
        .filter(|surface| surface.public_reexport)
        .map(|surface| surface.module.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(facade_reexports, manifest_reexports);

    let retired_handler = classified
        .get("incremental_handler_v2")
        .ok_or("incremental_handler_v2 is missing from the authority ledger")?;
    assert_eq!(retired_handler.status, "retire");

    let compact_facade = compact_whitespace(&facade_source);
    assert!(
        compact_facade.contains("pubuseincremental::{Edit,IncrementalState,apply_edits};"),
        "the canonical state/edit/apply_edits facade re-export is missing"
    );

    Ok(())
}

#[test]
fn active_lower_tier_kernel_and_consumer_are_explicitly_classified() -> TestResult {
    let manifest = load_manifest()?;
    assert_eq!(
        manifest.lower_tier.len(),
        1,
        "every active lower-tier incremental implementation needs one disposition"
    );

    let kernel = &manifest.lower_tier[0];
    assert_eq!(kernel.package, "perl-parser-core");
    assert_eq!(kernel.module, "incremental");
    assert_eq!(kernel.path, "perl_parser_core::incremental");
    assert_eq!(kernel.status, "lower_tier_kernel");
    assert_eq!(
        kernel.entry_points.iter().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "IncrementalEdit".to_string(),
            "IncrementalState".to_string(),
            "IncrementalState::reparse".to_string(),
        ])
    );
    assert!(!kernel.behavior_authority);
    assert!(!kernel.production_eligible);
    assert_eq!(
        kernel.allowed_consumers,
        vec!["tree_sitter_perl_rs::Parser::parse_with_old_tree".to_string()]
    );
    assert!(!kernel.next_action.trim().is_empty());
    assert_eq!(kernel.owner_issue, 6707);
    assert!(!kernel.decision.trim().is_empty());

    let core_facade = compact_whitespace(&read(
        crate_root().join("../perl-parser-core/src/lib.rs"),
    )?);
    let kernel_source = compact_whitespace(&read(
        crate_root().join("../perl-parser-core/src/incremental.rs"),
    )?);
    let tree_sitter_facade = compact_whitespace(&read(
        crate_root().join("../tree-sitter-perl-rs/src/lib.rs"),
    )?);

    assert!(
        core_facade.contains("pubmodincremental;"),
        "the classified lower-tier kernel is no longer publicly exported"
    );
    assert!(kernel_source.contains("pubstructIncrementalEdit"));
    assert!(kernel_source.contains("pubstructIncrementalState"));
    assert!(
        kernel_source.contains("pubfnreparse(&mutself,new_source:&str,edit:&IncrementalEdit)"),
        "the classified lower-tier reparse entry point changed"
    );
    assert!(
        tree_sitter_facade
            .contains("pubfnparse_with_old_tree(&mutself,source:&str,old_tree:&Tree)->Option<Tree>"),
        "the classified tree-sitter consumer changed or disappeared"
    );
    assert!(
        tree_sitter_facade.contains("state.reparse(source,&incremental_edit)"),
        "the classified consumer no longer calls the lower-tier kernel"
    );

    Ok(())
}

#[test]
fn compatibility_crate_forwards_the_canonical_implementation() -> TestResult {
    let manifest = load_manifest()?;
    let compatibility_source = read(
        crate_root().join("../perl-incremental-parsing/src/lib.rs"),
    )?;
    let compact = compact_whitespace(&compatibility_source);

    assert!(!manifest.compatibility.behavior_authority);
    assert!(
        compact.contains("pubuseperl_parser::incremental;"),
        "compatibility crate must forward the canonical incremental module"
    );
    assert!(
        compact.contains("pubuseperl_parser::incremental::*;"),
        "compatibility crate must forward the canonical incremental exports"
    );
    assert!(
        !compact.contains("modincremental"),
        "compatibility crate must not define a second incremental implementation"
    );

    Ok(())
}
