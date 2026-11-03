//! Reporting front end for the `compile_fail` doctest ratchet (#13774).
//!
//! Every law, and every test that falsifies one, lives in
//! [`perl_ci_hygiene::doctest_enforcement`] rather than here. That split is
//! not cosmetic: the merge gate that covers this crate runs
//! `cargo test --locked --lib -p perl-ci-hygiene`, so a `#[cfg(test)]` module
//! under `main.rs` is compiled by nobody — the same shape of unenforced proof
//! this command exists to prevent.
//!
//! What stays here is the terminal output and the exit code.

use color_eyre::eyre::{Result, eyre};
use std::fs;
use std::path::Path;

use crate::{GREEN, NC, RED, YELLOW};
use perl_ci_hygiene::doctest_enforcement::{
    GATE_NAME, GATE_POLICY_PATH, PackageFacts, Violation, package_facts, route_packages, violations,
};

/// Run the ratchet against the working tree.
///
/// Returns the process exit code: `0` when every `compile_fail` contract sits
/// inside the enforced route, `1` otherwise.
///
/// # Errors
///
/// Returns an error when the gate policy or the root manifest cannot be read,
/// or when the doctest route is missing, no longer passes `--doc`, or selects
/// no package.
pub(crate) fn check(repo_root: &Path) -> Result<i32> {
    let policy_path = repo_root.join(GATE_POLICY_PATH);
    let policy = fs::read_to_string(&policy_path)
        .map_err(|error| eyre!("failed to read {}: {error}", policy_path.display()))?;
    let route = route_packages(&policy)?;
    let facts = package_facts(repo_root)?;
    let found = violations(&route, &facts);

    let enforced: Vec<&PackageFacts> =
        facts.values().filter(|facts| route.contains(&facts.name)).collect();
    let contract_total: usize = facts.values().map(|facts| facts.contracts.len()).sum();

    println!("Doctest enforcement route `{GATE_NAME}` ({GATE_POLICY_PATH})");
    println!(
        "  selected packages: {}",
        route.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
    );
    println!(
        "  compile_fail contracts: {contract_total} across {} package(s); {} package(s) selected",
        facts.values().filter(|facts| !facts.contracts.is_empty()).count(),
        enforced.len()
    );

    if found.is_empty() {
        println!("{GREEN}✅ Every compile_fail contract is inside the enforced doctest route{NC}");
        return Ok(0);
    }

    println!("{RED}❌ compile_fail contracts are not enforced{NC}");
    for violation in &found {
        match violation {
            Violation::ContractOutsideRoute { package, sites } => {
                println!(
                    "  {YELLOW}{package}{NC}: {} compile_fail contract(s), but `{GATE_NAME}` does \
                     not select it",
                    sites.len()
                );
                for site in sites {
                    println!("    {}:{}", site.file, site.line);
                }
            }
            Violation::SelectedPackageUnknown { package } => println!(
                "  {YELLOW}{package}{NC}: selected by `{GATE_NAME}` but is not a workspace package"
            ),
            Violation::ContractInUnexecutableTarget { package, sites } => {
                println!(
                    "  {YELLOW}{package}{NC}: {} compile_fail contract(s) in a non-library \
                     target; `cargo test --doc` collects only the library, so selecting the \
                     package cannot execute them",
                    sites.len()
                );
                for site in sites {
                    println!("    {}:{}", site.file, site.line);
                }
            }
        }
    }
    println!();
    println!(
        "Add the package to the `{GATE_NAME}` command in {GATE_POLICY_PATH}, or migrate the \
         contract to a target a gate already runs — see \
         `crates/perl-parser/tests/incremental_state_read_only_authority.rs` for the migration \
         shape and `docs/ci/test-evidence-lanes.md` for when to choose which."
    );
    Ok(1)
}
