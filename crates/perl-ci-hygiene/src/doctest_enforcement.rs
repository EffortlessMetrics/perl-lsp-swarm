//! Keep `compile_fail` doctest contracts inside a gate that actually runs
//! them (#13774).
//!
//! A `compile_fail` doctest is the repository's only idiom for a negative
//! type-level contract, and until #13774 no gate, workflow, or `justfile`
//! recipe ever executed `cargo test --doc`. Every such contract was therefore
//! documentation that looked like proof: reintroducing the very construct one
//! forbids left CI fully green.
//!
//! The fix has two halves. The `doctest_contract_proof` merge-gate route runs
//! `cargo test --doc` over a bounded package list, and this check keeps that
//! list honest: it is the ratchet that stops the gap from silently reopening
//! the next time someone writes a `compile_fail` fence in a crate the route
//! does not select.
//!
//! # What is checked
//!
//! The gate row is the single authority for which packages are enforced; this
//! check reads the package list back out of the row's own command rather than
//! keeping a second copy. Three laws follow from it:
//!
//! 1. the route still exists, still passes `--doc`, and still selects at
//!    least one package — deleting it, emptying it, or quietly dropping
//!    `--doc` disarms every contract at once, so each fails loudly here;
//! 2. every workspace package whose `src/` carries a `compile_fail` doctest
//!    fence appears in the route's package list;
//! 3. every package the route names is a real workspace package, so a rename
//!    cannot silently drop a crate out of the route.
//!
//! # `doctest = false` is not an exclusion
//!
//! It would be natural to assume a package declaring `[lib] doctest = false`
//! cannot be covered by this route. Measured on the pinned 1.95.0 toolchain,
//! that is wrong: the field removes doctests from the *default* `cargo test`
//! target selection, but an explicit `cargo test -p <pkg> --doc` collects and
//! runs them anyway. `perl-parser-core` declares `doctest = false` and still
//! reports 18 passed / 3 failed under `--doc`.
//!
//! So the field is not a reason to leave a crate out of the route, and this
//! check deliberately enforces no law about it. A real exclusion is a
//! `#[cfg(feature = "…")]` module behind a feature the route does not enable:
//! those doctests are never compiled, which is the shape that kept
//! `perl-parser`'s `incremental` contracts inert.
//!
//! # What is not checked
//!
//! Whether a doctest body actually exercises anything. A doctest that wraps
//! its code in a hidden, never-called `fn` type-checks and passes without
//! asserting a thing; so does a `compile_fail` block that fails to compile for
//! an unrelated reason, such as a typo in a path. Neither is detectable from
//! the fence line, and both are called out in
//! `docs/ci/test-evidence-lanes.md` for authors instead.
//!
//! Feature-gated reachability is likewise not checked: whether the route's
//! feature selection reaches a given fence is a per-crate judgement, recorded
//! in that crate's guidance rather than inferred here.
//!
//! Only fences spelled in `///` or `//!` doc comments under a package's
//! `src/` directory are counted. `#[doc = "…"]` attribute forms and fences in
//! `tests/` (which rustdoc does not collect as doctests) are out of scope.

use color_eyre::eyre::{Result, eyre};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The merge-gate route that executes `cargo test --doc`.
pub const GATE_NAME: &str = "doctest_contract_proof";

/// Repository-relative path of the gate policy that owns the route.
pub const GATE_POLICY_PATH: &str = ".ci/gate-policy.yaml";

/// One `compile_fail` doctest fence found in a package's `src/` tree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContractSite {
    /// Repository-relative path of the file carrying the fence.
    pub file: String,
    /// 1-based line of the fence.
    pub line: usize,
}

/// What the workspace declares about one package's doctest surface.
#[derive(Debug, Clone)]
pub struct PackageFacts {
    /// Cargo package name.
    pub name: String,
    /// `compile_fail` fences found under the package's `src/`.
    pub contracts: Vec<ContractSite>,
}

/// A law this check enforces, and the package that broke it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// A package carries `compile_fail` contracts but the route does not
    /// select it, so nothing compiles them.
    ContractOutsideRoute {
        /// The unselected package.
        package: String,
        /// Where its contracts live.
        sites: Vec<ContractSite>,
    },
    /// The route selects a package the workspace does not contain.
    SelectedPackageUnknown {
        /// The name that matched no workspace member.
        package: String,
    },
}

impl Violation {
    /// The package this violation is about.
    fn package(&self) -> &str {
        match self {
            Self::ContractOutsideRoute { package, .. }
            | Self::SelectedPackageUnknown { package } => package,
        }
    }
}

/// Extract the packages the doctest route selects, from the route's own
/// `command`.
///
/// The gate row is the authority; this reads it back rather than keeping a
/// second list that could drift. A missing row, a row without a command, or a
/// command that selects no package is an error, not an empty pass: each of
/// those disarms every contract at once and must be louder than a silent
/// green.
///
/// # Errors
///
/// Returns an error when the route is absent from the policy text, no longer
/// passes `--doc`, or selects no package.
pub fn route_packages(policy: &str) -> Result<BTreeSet<String>> {
    let block = gate_block(policy, GATE_NAME).ok_or_else(|| {
        eyre!(
            "{GATE_POLICY_PATH} has no `{GATE_NAME}` gate. That route is what executes \
             `cargo test --doc`; without it every `compile_fail` contract in the workspace is \
             unenforced (#13774)."
        )
    })?;
    let command = gate_command(&block)
        .ok_or_else(|| eyre!("gate `{GATE_NAME}` in {GATE_POLICY_PATH} has no `command:` value"))?;
    if !command.split_whitespace().any(|token| token == "--doc") {
        return Err(eyre!(
            "gate `{GATE_NAME}` no longer passes `--doc`. Without it the route runs ordinary \
             tests and collects no doctest, so every `compile_fail` contract it is supposed to \
             enforce becomes inert again (#13774)."
        ));
    }
    let packages = selected_packages(&command);
    if packages.is_empty() {
        return Err(eyre!(
            "gate `{GATE_NAME}` selects no package with `-p`; an empty doctest route collects \
             nothing and reports success"
        ));
    }
    Ok(packages)
}

/// Return the lines of one `- name: <gate>` block from the policy text.
///
/// Gate rows are a two-space-indented sequence; a block runs until the next
/// sibling `  - ` entry or the end of the `gates:` sequence. This is the same
/// textual extraction `xtask/tests/issue_4657_ci_workflow_contract.rs` uses
/// against this file, kept here so the check needs no YAML dependency.
fn gate_block(policy: &str, gate: &str) -> Option<String> {
    let header = format!("  - name: {gate}");
    let mut lines = policy.lines().skip_while(|line| line.trim_end() != header);
    let first = lines.next()?;
    let mut block = String::from(first);
    for line in lines {
        // A sibling sequence entry, or any line that dedents out of the
        // `gates:` sequence, ends this block.
        let ends_block = line.starts_with("  - ")
            || (!line.trim().is_empty() && !line.starts_with("   ") && !line.starts_with("  #"));
        if ends_block {
            break;
        }
        block.push('\n');
        block.push_str(line);
    }
    Some(block)
}

/// Join a gate block's `command:` value, following block-scalar continuations.
///
/// Handles both the inline form (`command: cargo test …`) and the folded form
/// (`command: >-` followed by deeper-indented lines), which are the two shapes
/// this policy file uses.
fn gate_command(block: &str) -> Option<String> {
    let mut lines = block.lines().skip_while(|line| !line.trim_start().starts_with("command:"));
    let first = lines.next()?;
    let inline = first.trim_start().trim_start_matches("command:").trim();
    if !inline.is_empty() && !matches!(inline, ">" | ">-" | ">+" | "|" | "|-" | "|+") {
        return Some(inline.to_owned());
    }
    // Folded/literal scalar: take the deeper-indented continuation lines.
    let mut command = String::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        // Continuation lines are indented past the `command:` key itself.
        if !line.starts_with("      ") {
            break;
        }
        if !command.is_empty() {
            command.push(' ');
        }
        command.push_str(line.trim());
    }
    (!command.is_empty()).then_some(command)
}

/// Collect every `-p <package>` / `--package <package>` selection in a command.
fn selected_packages(command: &str) -> BTreeSet<String> {
    let mut packages = BTreeSet::new();
    let mut tokens = command.split_whitespace();
    while let Some(token) = tokens.next() {
        match token {
            "-p" | "--package" => {
                if let Some(name) = tokens.next() {
                    packages.insert(name.to_owned());
                }
            }
            _ => {
                if let Some(name) = token.strip_prefix("--package=") {
                    packages.insert(name.to_owned());
                }
            }
        }
    }
    packages
}

/// Whether a source line opens a `compile_fail` doctest fence in a doc comment.
///
/// Matches `/// ```compile_fail`, `//! ```rust,compile_fail`, and the other
/// comma-separated attribute spellings. A bare `compile_fail` string in
/// ordinary code or a non-doc comment is not a contract and does not match.
pub fn is_compile_fail_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("///").or_else(|| trimmed.strip_prefix("//!")) else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(attributes) = rest.strip_prefix("```") else {
        return false;
    };
    attributes.trim().split(',').any(|attribute| attribute.trim() == "compile_fail")
}

/// Read the workspace member paths declared by the root manifest.
fn workspace_members(root: &Path) -> Result<Vec<PathBuf>> {
    let manifest_path = root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| eyre!("failed to read {}: {error}", manifest_path.display()))?;
    // `toml::from_str`, not `str::parse`: in toml 1.x the `FromStr` impl
    // parses a single TOML *value*, so a document starting with `[workspace]`
    // is rejected as trailing content.
    let value: toml::Value = toml::from_str(&manifest)
        .map_err(|error| eyre!("failed to parse {}: {error}", manifest_path.display()))?;
    let members = value
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| eyre!("{} declares no workspace.members", manifest_path.display()))?;
    Ok(members.iter().filter_map(toml::Value::as_str).map(|member| root.join(member)).collect())
}

/// Gather the doctest facts for every workspace member, plus `xtask`.
///
/// `xtask` is not a `workspace.members` entry but is a package in this
/// repository with a library target and its own `compile_fail` contract, so
/// leaving it out would put a real contract outside the denominator.
///
/// # Errors
///
/// Returns an error when the root manifest cannot be read or parsed.
pub fn package_facts(root: &Path) -> Result<BTreeMap<String, PackageFacts>> {
    let mut directories = workspace_members(root)?;
    let xtask = root.join("xtask");
    if xtask.is_dir() && !directories.contains(&xtask) {
        directories.push(xtask);
    }

    let mut facts = BTreeMap::new();
    for directory in directories {
        let manifest_path = directory.join("Cargo.toml");
        let Ok(manifest) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Some(name) = package_name(&manifest) else {
            continue;
        };
        let contracts = scan_contracts(root, &directory.join("src"));
        facts.insert(name.clone(), PackageFacts { name, contracts });
    }
    Ok(facts)
}

/// Read `package.name` from a manifest.
///
/// `toml::from_str`, not `str::parse`: see [`workspace_members`].
pub fn package_name(manifest: &str) -> Option<String> {
    toml::from_str::<toml::Value>(manifest)
        .ok()?
        .get("package")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}

/// Find every `compile_fail` fence under one `src/` directory.
fn scan_contracts(root: &Path, src: &Path) -> Vec<ContractSite> {
    let mut sites = Vec::new();
    for path in crate::walk_rs_files(src) {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let display = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        for (index, line) in contents.lines().enumerate() {
            if is_compile_fail_fence(line) {
                sites.push(ContractSite { file: display.replace('\\', "/"), line: index + 1 });
            }
        }
    }
    sites.sort();
    sites
}

/// Apply the three laws to a gathered inventory.
pub fn violations(
    route: &BTreeSet<String>,
    facts: &BTreeMap<String, PackageFacts>,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    for package in route {
        if !facts.contains_key(package) {
            violations.push(Violation::SelectedPackageUnknown { package: package.clone() });
        }
    }

    for facts in facts.values() {
        if !facts.contracts.is_empty() && !route.contains(&facts.name) {
            violations.push(Violation::ContractOutsideRoute {
                package: facts.name.clone(),
                sites: facts.contracts.clone(),
            });
        }
    }

    violations.sort_by(|left, right| left.package().cmp(right.package()));
    violations
}

#[cfg(test)]
mod tests {
    use perl_test_must::must_with;

    use super::{
        ContractSite, PackageFacts, Violation, is_compile_fail_fence, package_name, route_packages,
        violations,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn policy_with(command: &str) -> String {
        [
            "gates:",
            "  - name: other_gate",
            "    tier: merge_gate",
            "    command: true",
            "",
            "  - name: doctest_contract_proof",
            "    tier: merge_gate",
            "    required: true",
            &format!("    command: {command}"),
            "    timeout_seconds: 300",
            "",
            "  - name: later_gate",
            "    tier: merge_gate",
            "    command: true",
            "",
        ]
        .join("\n")
    }

    #[test]
    fn route_packages_reads_the_selection_out_of_the_gate_command() {
        let policy = policy_with("cargo test --locked --doc -p perl-token -p xtask");
        let packages = must_with(route_packages(&policy), "the fixture policy declares the route");
        assert_eq!(
            packages,
            BTreeSet::from(["perl-token".to_owned(), "xtask".to_owned()]),
            "the gate row is the authority for the enforced package list"
        );
    }

    #[test]
    fn route_packages_reads_a_folded_block_scalar_command() {
        let policy = [
            "gates:",
            "  - name: doctest_contract_proof",
            "    tier: merge_gate",
            "    command: >-",
            "      cargo test --locked --doc",
            "      -p perl-token",
            "      -p perl-module",
            "    timeout_seconds: 300",
            "",
        ]
        .join("\n");
        let packages = must_with(route_packages(&policy), "the fixture policy declares the route");
        assert_eq!(
            packages,
            BTreeSet::from(["perl-module".to_owned(), "perl-token".to_owned()]),
            "the policy file writes long commands as folded scalars"
        );
    }

    #[test]
    fn route_packages_does_not_read_a_neighbouring_gate_command() {
        // The block extractor must stop at the next sibling entry; reading on
        // would silently import another gate's `-p` selections and report
        // packages as enforced that this route never compiles.
        let policy = [
            "gates:",
            "  - name: doctest_contract_proof",
            "    tier: merge_gate",
            "    command: cargo test --locked --doc -p perl-token",
            "",
            "  - name: unit_core",
            "    tier: pr_fast",
            "    command: cargo test -p perl-lexer --lib",
            "",
        ]
        .join("\n");
        let packages = must_with(route_packages(&policy), "the fixture policy declares the route");
        assert_eq!(
            packages,
            BTreeSet::from(["perl-token".to_owned()]),
            "perl-lexer belongs to unit_core, not to the doctest route"
        );
    }

    #[test]
    fn a_missing_route_is_an_error_not_an_empty_pass() {
        let policy = "gates:\n  - name: unit_core\n    tier: pr_fast\n    command: cargo test\n";
        assert!(
            route_packages(policy).is_err(),
            "deleting the route disarms every contract and must fail loudly"
        );
    }

    #[test]
    fn a_route_selecting_no_package_is_an_error() {
        let policy = policy_with("cargo test --locked --doc --workspace");
        assert!(
            route_packages(&policy).is_err(),
            "a route with no -p selection is not a bounded package list"
        );
    }

    #[test]
    fn a_manifest_document_is_read_as_a_document_not_a_value() {
        // `str::parse::<toml::Value>()` parses a single TOML *value* in toml
        // 1.x, so a real manifest is rejected as trailing content after
        // `[package]`. Reading it as `None` would silently drop the package
        // from the denominator and report a clean inventory.
        let manifest = [
            "[package]",
            "name = \"perl-token\"",
            "version.workspace = true",
            "",
            "[lib]",
            "doctest = false",
            "",
            "[dependencies]",
            "serde = \"1\"",
            "",
        ]
        .join("\n");
        assert_eq!(
            package_name(&manifest).as_deref(),
            Some("perl-token"),
            "every workspace member must be readable, or the ratchet under-reports"
        );
    }

    #[test]
    fn a_route_that_stops_passing_doc_is_an_error() {
        // Dropping `--doc` from the command turns the route into an ordinary
        // test run that collects no doctest at all, which is the original
        // #13774 state wearing the gate's name.
        let policy = policy_with("cargo test --locked -p perl-token");
        assert!(
            route_packages(&policy).is_err(),
            "a route without --doc enforces no doctest contract"
        );
    }

    #[test]
    fn only_doc_comment_fences_count_as_contracts() {
        assert!(is_compile_fail_fence("/// ```compile_fail"));
        assert!(is_compile_fail_fence("//! ```compile_fail"));
        assert!(is_compile_fail_fence("    /// ```rust,compile_fail"));
        assert!(is_compile_fail_fence("/// ```compile_fail,edition2024"));

        // A `compile_fail` mention that is not a doc-comment fence is not a
        // contract: `xtask` carries the string as ordinary data, and counting
        // it would demand enforcement for a package that declares none.
        assert!(!is_compile_fail_fence("        \"compile_fail\","));
        assert!(!is_compile_fail_fence("// ```compile_fail"), "a plain comment is not a doctest");
        assert!(!is_compile_fail_fence("/// ```compile_failure"), "the attribute must match whole");
        assert!(!is_compile_fail_fence("/// ```"), "an ordinary doctest is not a contract");
    }

    fn facts(name: &str, contracts: usize) -> PackageFacts {
        PackageFacts {
            name: name.to_owned(),
            contracts: (0..contracts)
                .map(|index| ContractSite {
                    file: format!("crates/{name}/src/lib.rs"),
                    line: index + 1,
                })
                .collect(),
        }
    }

    fn inventory(packages: Vec<PackageFacts>) -> BTreeMap<String, PackageFacts> {
        packages.into_iter().map(|facts| (facts.name.clone(), facts)).collect()
    }

    #[test]
    fn a_contract_inside_the_route_is_clean() {
        let route = BTreeSet::from(["perl-token".to_owned()]);
        let found = violations(&route, &inventory(vec![facts("perl-token", 20)]));
        assert!(found.is_empty(), "an enforced contract raises nothing");
    }

    #[test]
    fn a_contract_in_an_unselected_package_is_reported() {
        // This is the regression the whole check exists for: writing a
        // `compile_fail` in a crate the route does not name reopens #13774.
        let route = BTreeSet::from(["perl-token".to_owned()]);
        let found =
            violations(&route, &inventory(vec![facts("perl-token", 1), facts("perl-module", 2)]));
        assert_eq!(found.len(), 1, "exactly the unselected package is reported");
        assert!(
            matches!(&found[0], Violation::ContractOutsideRoute { package, sites }
                if package == "perl-module" && sites.len() == 2),
            "got {found:?}"
        );
    }

    #[test]
    fn a_selected_package_that_does_not_exist_is_reported() {
        let route = BTreeSet::from(["perl-renamed".to_owned()]);
        let found = violations(&route, &inventory(vec![facts("perl-token", 1)]));
        assert!(
            found.contains(&Violation::SelectedPackageUnknown {
                package: "perl-renamed".to_owned()
            }),
            "a stale selection silently drops a package from the route; got {found:?}"
        );
    }

    #[test]
    fn a_package_without_contracts_need_not_be_selected() {
        let route = BTreeSet::from(["perl-token".to_owned()]);
        let found =
            violations(&route, &inventory(vec![facts("perl-token", 1), facts("perl-lexer", 0)]));
        assert!(found.is_empty(), "the route is a floor for contracts, not a workspace sweep");
    }
}
