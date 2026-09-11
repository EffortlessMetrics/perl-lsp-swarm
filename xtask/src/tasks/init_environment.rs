//! `cargo xtask init-environment` — CLI surface for the initialize-operation
//! phase and owner ledger (#10040).
//!
//! The ledger, the derived census, and every checking rule live in
//! [`xtask::init_environment`]. This module only adapts them to the command
//! line so the contract stays testable without a subprocess.

use color_eyre::eyre::{Result, eyre};
use xtask::init_environment::census::{self, Census};
use xtask::init_environment::{
    CENSUS_ROOTS, by_phase, by_wave, ledger_errors, ledger_rows, render_json,
};

/// Locate the workspace root from the xtask manifest directory.
fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn build_census() -> Result<Census> {
    Census::from_workspace(&workspace_root())
        .map_err(|error| eyre!("failed to build initialize census: {error}"))
}

/// `cargo xtask init-environment check`
pub fn run_check() -> Result<()> {
    let census = build_census()?;
    let rows = ledger_rows();
    let errors = ledger_errors(&rows, &census);

    if errors.is_empty() {
        println!("initialize-operation ledger: {} rows, no findings", rows.len());
        return Ok(());
    }

    for error in &errors {
        eprintln!("init-environment: {error}");
    }
    Err(eyre!("initialize-operation ledger has {} finding(s)", errors.len()))
}

/// `cargo xtask init-environment render`
pub fn run_render() -> Result<()> {
    print!("{}", render_json(&ledger_rows()));
    Ok(())
}

/// `cargo xtask init-environment list`
pub fn run_list() -> Result<()> {
    let rows = ledger_rows();
    println!("initialize-operation ledger ({} rows)", rows.len());
    for (phase, ids) in by_phase(&rows) {
        println!("\n{phase}");
        for id in ids {
            println!("  {id}");
        }
    }
    println!("\nmigration waves");
    for (wave, ids) in by_wave(&rows) {
        println!("  {wave}: {}", ids.join(", "));
    }
    Ok(())
}

/// `cargo xtask init-environment census` — dump derived facts for the roots.
///
/// This exists so the ledger can be authored from derived evidence rather than
/// from reading the code and guessing.
pub fn run_census() -> Result<()> {
    let census = build_census()?;

    println!("indexed functions: {}", census.len());
    println!(
        "names with more than one definition: {} (resolved per call site, not automatically \
         dropped)",
        census.colliding_names().len()
    );

    for (file, function) in CENSUS_ROOTS {
        println!("\n== {file}::{function} ==");
        let Some(root) = census.resolve(file, function) else {
            println!("  (absent from scanned source)");
            continue;
        };
        let exposures = census.transitive_exposures(root, census::MAX_DEPTH);
        if exposures.is_empty() {
            println!("  no derived blocking exposure");
        }
        for witness in exposures.values() {
            println!("  {}", witness.render());
        }
        let reached = census.reachable_from(root, census::MAX_DEPTH);
        println!("  reachable functions: {}", reached.len());
        for index in reached.into_keys() {
            let direct = census.direct_exposures(index);
            if direct.is_empty() {
                continue;
            }
            let kinds: Vec<&str> = direct.iter().map(|exposure| exposure.label()).collect();
            println!("    carries [{}] {}", kinds.join(", "), census.qualified(index));
        }
    }
    Ok(())
}
