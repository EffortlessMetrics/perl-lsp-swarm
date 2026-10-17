//! No-publish rehearsal receipt model, validator, projections, and fixtures (#16785).
//!
//! This module owns the `readiness_rehearsal.v1` contract only. It does not
//! package crates, build a VSIX, assemble archives, contact registries, or
//! mutate tags/releases. Downstream leaves populate generic stages without
//! forking the schema.

mod fixtures;
mod model;
mod validate;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use color_eyre::eyre::{Context, Result, bail};
use model::{project_human, project_json};
use validate::{validate_bytes, validate_receipt, validate_schema_file};

/// Validate one sealed rehearsal receipt and print JSON plus human projections.
pub fn validate_path(path: PathBuf) -> Result<()> {
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let receipt = validate_bytes(&bytes)?;
    println!("{}", project_json(&receipt)?);
    println!();
    print!("{}", project_human(&receipt));
    Ok(())
}

/// Compile the schema and run every deterministic false-green fixture.
pub fn check() -> Result<()> {
    let root = crate::utils::project_root()?;
    validate_schema_file(&root)?;
    let update = std::env::var_os("UPDATE_READINESS_REHEARSAL_FIXTURES").is_some();
    fixtures::sync_fixture_files(&root, update)?;
    fixtures::run_fixture_suite()?;
    let pass = fixtures::fixture_cases()
        .into_iter()
        .find(|case| case.file == "valid_pass.json")
        .ok_or_else(|| color_eyre::eyre::eyre!("valid_pass fixture"))?;
    let valid = validate_receipt(fixtures::build_case(&pass)?)?;
    let json = project_json(&valid)?;
    let human = project_human(&valid);
    if human.contains("/tmp") || human.contains("/home/") || json.contains("observed_at") {
        bail!("projections are not allowed to carry temporary paths or observation timestamps");
    }
    println!(
        "readiness_rehearsal.v1: schema, projections, and {} fixture vectors valid (published_channels=[], release_cut=false)",
        fixtures::fixture_cases().len()
    );
    Ok(())
}
