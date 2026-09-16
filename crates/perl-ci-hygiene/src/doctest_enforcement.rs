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
//! keeping a second copy. Four laws follow from it:
//!
//! 1. the route still exists, is still a `tier: merge_gate` row with
//!    `required: true` and `quarantine: false`, still passes `--doc`, and
//!    still selects at least one package — demoting it to advisory,
//!    quarantining it, emptying it, dropping `--doc`, or deleting it each
//!    disarms every contract at once, so each fails loudly here;
//! 2. every workspace package whose source carries a `compile_fail` doctest
//!    fence appears in the route's package list;
//! 3. every package the route names is a real workspace package, so a rename
//!    cannot silently drop a crate out of the route;
//! 4. the inventory cannot fail open: an unreadable or unparsable member
//!    manifest, a wildcard `workspace.members` entry, and an unreadable
//!    source file are all errors, never an empty pass.
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
//! Fence forms: the detector counts `///` and `//!` line doc comments and
//! `/**` and `/*!` block doc comments, each with a fence delimiter of three
//! or more backticks or tildes — the spellings rustdoc collects. Still out of
//! scope are `#[doc = "…"]` attribute forms; fences under `tests/` (rustdoc
//! does not collect them as doctests); and fences in non-library targets such
//! as `src/bin/**` or a package's `main.rs`, because `cargo test --doc`
//! collects only the *library's* documentation. A contract written in one of
//! those places needs a gate-run target of its own, like the migrated
//! `perl-parser` target.
//!
//! The block-comment scanner tracks `/*` and `*/` nesting depth textually, so
//! doctest source containing comment-looking tokens can skew the depth of one
//! file. That can hide a fence from the ratchet (the same direction as the
//! other documented carve-outs); it cannot invent one, because a reported
//! site is always a literal `compile_fail` fence line.

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
/// second list that could drift. A missing row, a row that is not a required
/// un-quarantined merge gate, a row without a command, or a command that
/// selects no package is an error, not an empty pass: each of those disarms
/// every contract at once and must be louder than a silent green.
///
/// # Errors
///
/// Returns an error when the route is absent from the policy text, is not a
/// `tier: merge_gate` row with `required: true` and `quarantine: false`, no
/// longer passes `--doc`, or selects no package.
pub fn route_packages(policy: &str) -> Result<BTreeSet<String>> {
    let block = gate_block(policy, GATE_NAME).ok_or_else(|| {
        eyre!(
            "{GATE_POLICY_PATH} has no `{GATE_NAME}` gate. That route is what executes \
             `cargo test --doc`; without it every `compile_fail` contract in the workspace is \
             unenforced (#13774)."
        )
    })?;
    let tier = gate_field(&block, "tier").unwrap_or_default();
    if tier != "merge_gate" {
        return Err(eyre!(
            "gate `{GATE_NAME}` in {GATE_POLICY_PATH} is `tier: {tier}`, not `merge_gate`. An \
             advisory route executes no contract on the merge path (#13774); restore \
             `tier: merge_gate` or migrate the contracts it used to cover."
        ));
    }
    if gate_field(&block, "required").as_deref() != Some("true") {
        return Err(eyre!(
            "gate `{GATE_NAME}` in {GATE_POLICY_PATH} is not `required: true`. An optional \
             route fails no merge, so its doctest run enforces nothing (#13774); restore \
             `required: true` or migrate the contracts."
        ));
    }
    if gate_field(&block, "quarantine").as_deref() != Some("false") {
        return Err(eyre!(
            "gate `{GATE_NAME}` in {GATE_POLICY_PATH} is not `quarantine: false`. A \
             quarantined route cannot block a merge, so its doctest run enforces nothing \
             (#13774); restore `quarantine: false` or migrate the contracts."
        ));
    }
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

/// Read one top-level `key: value` field from a gate block, comment-stripped.
fn gate_field(block: &str, key: &str) -> Option<String> {
    block.lines().find_map(|line| {
        let trimmed = line.trim_start();
        let value = trimmed.strip_prefix(key)?.strip_prefix(':')?.trim();
        let value = value.split('#').next()?.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
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

/// Whether a source line opens a `compile_fail` doctest fence in a line doc
/// comment.
///
/// Matches `/// ```compile_fail`, `//! ```rust,compile_fail`, four-or-more
/// backtick and tilde delimiters, and the other comma-separated attribute
/// spellings — every line-doc fence form rustdoc collects. A bare
/// `compile_fail` string in ordinary code or a non-doc comment is not a
/// contract and does not match. Block doc comments (`/**`, `/*!`) are
/// recognized by [`scan_contracts`], which tracks comment state across lines.
pub fn is_compile_fail_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("///").or_else(|| trimmed.strip_prefix("//!")) else {
        return false;
    };
    is_compile_fail_fence_body(rest)
}

/// Whether the text after a doc-comment marker opens a `compile_fail` fence.
///
/// A fence delimiter is a run of at least three backticks or at least three
/// tildes; the info string runs to the end of the line and splits on commas.
fn is_compile_fail_fence_body(text: &str) -> bool {
    let text = text.trim_start();
    let attributes = if let Some(rest) = text.strip_prefix('`') {
        let info = rest.trim_start_matches('`');
        let length = 1 + (rest.len() - info.len());
        (length >= 3).then_some(info)
    } else if let Some(rest) = text.strip_prefix('~') {
        let info = rest.trim_start_matches('~');
        let length = 1 + (rest.len() - info.len());
        (length >= 3).then_some(info)
    } else {
        None
    };
    attributes.is_some_and(|attributes| {
        attributes.split(',').any(|attribute| attribute.trim() == "compile_fail")
    })
}

/// Read the workspace member paths declared by the root manifest.
///
/// # Errors
///
/// Returns an error when the root manifest cannot be read or parsed, or when
/// a member entry is a glob: wildcard members expand at build time, and a
/// package admitted through one could grow a contract that this inventory
/// never sees. Failing closed keeps a clean ratchet honest.
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
    let mut paths = Vec::new();
    for member in members.iter().filter_map(toml::Value::as_str) {
        if member.contains('*') || member.contains('?') || member.contains('[') {
            return Err(eyre!(
                "workspace member `{member}` is a glob; the doctest inventory needs literal \
                 member paths so every package is accounted for individually"
            ));
        }
        paths.push(root.join(member));
    }
    Ok(paths)
}

/// Gather the doctest facts for every workspace member, plus `xtask`.
///
/// `xtask` is not a `workspace.members` entry but is a package in this
/// repository with a library target and its own `compile_fail` contract, so
/// leaving it out would put a real contract outside the denominator.
///
/// # Errors
///
/// Returns an error when the root manifest cannot be read or parsed, when any
/// member manifest cannot be read, or when a member declares no package name.
/// Each of those would otherwise silently shrink the denominator below the
/// workspace's real contract surface.
pub fn package_facts(root: &Path) -> Result<BTreeMap<String, PackageFacts>> {
    let mut directories = workspace_members(root)?;
    let xtask = root.join("xtask");
    if xtask.is_dir() && !directories.contains(&xtask) {
        directories.push(xtask);
    }

    let mut facts = BTreeMap::new();
    for directory in directories {
        let manifest_path = directory.join("Cargo.toml");
        let manifest = fs::read_to_string(&manifest_path).map_err(|error| {
            eyre!(
                "failed to read workspace member manifest {}: {error}; the doctest inventory \
                 cannot account for a package it cannot read",
                manifest_path.display()
            )
        })?;
        let Some(name) = package_name(&manifest) else {
            return Err(eyre!(
                "{} declares no `package.name`; the doctest inventory cannot account for it",
                manifest_path.display()
            ));
        };
        let contracts = scan_contracts(root, &directory.join("src"))?;
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
///
/// Recognizes fences in `///`/`//!` line doc comments and in `/**`/`/*!`
/// block doc comments, with three or more backticks or tildes as the
/// delimiter — the forms rustdoc collects. The block scanner tracks `/*` and
/// `*/` nesting depth textually; see the module docs for the known limits of
/// that.
///
/// # Errors
///
/// Returns an error when a source file under `src/` cannot be read, so an
/// instrument failure can never look like an empty contract surface.
fn scan_contracts(root: &Path, src: &Path) -> Result<Vec<ContractSite>> {
    let mut sites = Vec::new();
    for path in crate::walk_rs_files(src) {
        let contents = fs::read_to_string(&path).map_err(|error| {
            eyre!(
                "failed to read {}: {error}; the doctest inventory cannot fail open here",
                path.display()
            )
        })?;
        let display = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        for line in compile_fail_fence_lines(&contents) {
            sites.push(ContractSite { file: display.replace('\\', "/"), line });
        }
    }
    sites.sort();
    Ok(sites)
}

/// Return the 1-based line numbers of `compile_fail` fence openers in one
/// source file, aware of block doc comments.
fn compile_fail_fence_lines(contents: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut in_block = false;
    let mut depth: usize = 0;
    for (index, line) in contents.lines().enumerate() {
        let line_number = index + 1;
        if in_block {
            scan_block_segment(line, &mut depth, &mut in_block, line_number, &mut found);
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            // A line comment cannot open a block comment, so the block-doc
            // opener search below must not see through it.
            if is_compile_fail_fence(trimmed) {
                found.push(line_number);
            }
            continue;
        }
        if let Some(position) = find_block_doc_opener(trimmed) {
            in_block = true;
            depth = 1;
            scan_block_segment(
                &trimmed[position + 3..],
                &mut depth,
                &mut in_block,
                line_number,
                &mut found,
            );
        }
    }
    found
}

/// Scan one line's segment inside a block doc comment for fences and comment
/// tokens, updating the nesting depth.
fn scan_block_segment(
    segment: &str,
    depth: &mut usize,
    in_block: &mut bool,
    line_number: usize,
    found: &mut Vec<usize>,
) {
    let mut rest = segment;
    loop {
        match next_comment_token(rest) {
            None => {
                if block_interior_is_fence(rest) {
                    found.push(line_number);
                }
                break;
            }
            Some((position, token)) => {
                if block_interior_is_fence(&rest[..position]) {
                    found.push(line_number);
                }
                rest = &rest[position + token.len()..];
                if token == "/*" {
                    *depth += 1;
                } else {
                    *depth -= 1;
                    if *depth == 0 {
                        *in_block = false;
                        // The remainder of the line is code again; it is not
                        // scanned for further doc blocks on this line.
                        break;
                    }
                }
            }
        }
    }
}

/// Find the next `/*` or `*/` token in `text`.
fn next_comment_token(text: &str) -> Option<(usize, &'static str)> {
    let open = text.find("/*");
    let close = text.find("*/");
    match (open, close) {
        (Some(open), Some(close)) if close < open => Some((close, "*/")),
        (Some(open), _) => Some((open, "/*")),
        (None, Some(close)) => Some((close, "*/")),
        (None, None) => None,
    }
}

/// Find a `/**` or `/*!` block-doc opener, skipping the plain-comment
/// lookalike `/**/`.
fn find_block_doc_opener(text: &str) -> Option<usize> {
    let mut offset = 0;
    let mut search = text;
    while let Some(position) = search.find("/*") {
        let after = &search[position + 2..];
        if after.starts_with('*') {
            if let Some(rest) = after.strip_prefix("*/") {
                // `/**/` is an empty plain comment, not a doc block.
                offset += position + 2 + (after.len() - rest.len());
                search = rest;
                continue;
            }
            return Some(offset + position);
        }
        if after.starts_with('!') {
            return Some(offset + position);
        }
        // A plain `/*`: keep searching past it for a doc opener.
        let advance = position + 2;
        offset += advance;
        search = &search[advance..];
    }
    None
}

/// Whether a segment inside a block doc comment opens a `compile_fail` fence.
///
/// Continuation lines are frequently starred (` * ```compile_fail`), so an
/// optional leading `*` is stripped before the fence check.
fn block_interior_is_fence(segment: &str) -> bool {
    let trimmed = segment.trim_start();
    let trimmed = match trimmed.strip_prefix('*') {
        Some(rest) => rest.trim_start(),
        None => trimmed,
    };
    is_compile_fail_fence_body(trimmed)
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
    use perl_test_must::{must_err_with, must_with};

    use super::{
        ContractSite, PackageFacts, Violation, compile_fail_fence_lines, is_compile_fail_fence,
        package_facts, package_name, route_packages, violations,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

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
            "    quarantine: false",
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

    /// A policy whose doctest route has one field overridden, for demoting,
    /// quarantining, or unrequiring the route.
    fn policy_with_route_field(field: &str, value: &str) -> String {
        let overridden = format!("    {field}: {value}");
        let lines: Vec<String> =
            ["    tier: merge_gate", "    required: true", "    quarantine: false"]
                .iter()
                .map(|line| line.to_string())
                .filter(|line| !line.starts_with(&format!("    {field}:")))
                .collect();
        let mut all = vec!["gates:".to_string(), "  - name: doctest_contract_proof".to_string()];
        all.push(overridden);
        all.extend(lines);
        all.push("    command: cargo test --locked --doc -p perl-token".to_string());
        all.push(String::new());
        all.join("\n")
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
            "    required: true",
            "    quarantine: false",
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
            "    required: true",
            "    quarantine: false",
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

    fn temp_repo_dir(label: &str) -> PathBuf {
        let nanos = must_with(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH),
            "the system clock is after the epoch",
        )
        .as_nanos();
        let directory = std::env::temp_dir()
            .join(format!("perl-ci-hygiene-doctest-{label}-{}", std::process::id()))
            .join(nanos.to_string());
        must_with(
            std::fs::create_dir_all(&directory),
            "the temporary repository directory can be created",
        );
        directory
    }

    #[test]
    fn a_glob_workspace_member_is_an_error() {
        // FC2: Cargo expands globs at build time; a hand-rolled inventory
        // that joins them literally would silently miss every package added
        // through one. Fail closed instead of under-reporting.
        let root = temp_repo_dir("glob-member");
        must_with(
            std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/*\"]\n"),
            "the fixture root manifest can be written",
        );
        let found = package_facts(&root);
        assert!(
            found.is_err(),
            "a wildcard member must fail the inventory, not shrink the denominator"
        );
    }

    #[test]
    fn a_missing_member_manifest_is_an_error_not_a_skip() {
        // FC2: `let Ok(manifest) = ... else { continue }` would drop the
        // package from the denominator and report the ratchet clean.
        let root = temp_repo_dir("missing-manifest");
        must_with(
            std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"a\"]\n"),
            "the fixture root manifest can be written",
        );
        must_with(
            std::fs::create_dir_all(root.join("a")),
            "the fixture member directory can be created",
        );
        let found = package_facts(&root);
        assert!(
            found.is_err(),
            "an unreadable member manifest must fail the inventory, not pass quietly"
        );
    }

    #[test]
    fn a_member_without_a_package_name_is_an_error() {
        let root = temp_repo_dir("no-name");
        must_with(
            std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"a\"]\n"),
            "the fixture root manifest can be written",
        );
        must_with(
            std::fs::create_dir_all(root.join("a")),
            "the fixture member directory can be created",
        );
        must_with(
            std::fs::write(root.join("a").join("Cargo.toml"), "[package]\nversion = \"0.1.0\"\n"),
            "the fixture member manifest can be written",
        );
        let found = package_facts(&root);
        assert!(
            found.is_err(),
            "a member the inventory cannot name must fail, not vanish from the denominator"
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
    fn a_demoted_route_is_an_error() {
        // FC3: an advisory route executes no contract on the merge path, yet
        // the ratchet would stay green because the row still exists with a
        // `--doc` command. The error must name the observed tier, so a test
        // can pin the exact failure instead of accepting any error.
        let policy = policy_with_route_field("tier", "pr_fast");
        let error = must_err_with(route_packages(&policy), "the fixture demotes the route");
        let message = format!("{error:#}");
        assert!(
            message.contains("is `tier: pr_fast`, not `merge_gate`"),
            "the demotion error must name the observed and required tiers; got: {message}"
        );
    }

    #[test]
    fn an_unrequired_route_is_an_error() {
        let policy = policy_with_route_field("required", "false");
        let error = must_err_with(route_packages(&policy), "the fixture unrequires the route");
        let message = format!("{error:#}");
        assert!(
            message.contains("is not `required: true`"),
            "the unrequired error must name the missing requirement; got: {message}"
        );
    }

    #[test]
    fn a_quarantined_route_is_an_error() {
        let policy = policy_with_route_field("quarantine", "true");
        let error = must_err_with(route_packages(&policy), "the fixture quarantines the route");
        let message = format!("{error:#}");
        assert!(
            message.contains("is not `quarantine: false`"),
            "the quarantine error must name the violated field; got: {message}"
        );
    }

    #[test]
    fn only_doc_comment_fences_count_as_contracts() {
        assert!(is_compile_fail_fence("/// ```compile_fail"));
        assert!(is_compile_fail_fence("//! ```compile_fail"));
        assert!(is_compile_fail_fence("    /// ```rust,compile_fail"));
        assert!(is_compile_fail_fence("/// ```compile_fail,edition2024"));

        // Every fence delimiter rustdoc accepts, not just three backticks:
        // four-or-more backticks and tilde fences are real doctests, and a
        // detector blind to them would under-report the denominator (FC2).
        assert!(is_compile_fail_fence("/// ````compile_fail"), "four backticks");
        assert!(is_compile_fail_fence("/// ~~~compile_fail"), "three tildes");
        assert!(is_compile_fail_fence("//! ~~~~rust,compile_fail"), "four tildes");
        assert!(is_compile_fail_fence("/// `````compile_fail"), "five backticks");

        // A `compile_fail` mention that is not a doc-comment fence is not a
        // contract: `xtask` carries the string as ordinary data, and counting
        // it would demand enforcement for a package that declares none.
        assert!(!is_compile_fail_fence("        \"compile_fail\","));
        assert!(!is_compile_fail_fence("// ```compile_fail"), "a plain comment is not a doctest");
        assert!(!is_compile_fail_fence("/// ```compile_failure"), "the attribute must match whole");
        assert!(!is_compile_fail_fence("/// ```"), "an ordinary doctest is not a contract");
        assert!(
            !is_compile_fail_fence("/// ``compile_fail"),
            "two backticks are not a fence delimiter"
        );
        assert!(!is_compile_fail_fence("/// ~compile_fail"), "one tilde is not a fence delimiter");
    }

    #[test]
    fn block_doc_comment_fences_are_counted() {
        // Rustdoc collects `/**` and `/*!` blocks as doctests; a scanner that
        // saw only line-doc comments would let a contract hide in one (FC2).
        // The inner-doc opener is spelled with a `\u{21}` escape so this test
        // file itself does not carry a fence-shaped literal the ratchet
        // would have to report.
        let contents = [
            "mod outer {",
            "/**",
            " * ```compile_fail",
            " * let x: u32 = \"no\";",
            " * ```",
            " */",
            "pub struct A;",
            "/*\u{21} ```compile_fail */",
            "pub struct B;",
            "fn code() { /* ```compile_fail */ }",
            "",
        ]
        .join("\n");
        let lines = compile_fail_fence_lines(&contents);
        assert_eq!(lines, vec![3, 8], "block doc fences are counted, plain block comments are not");
    }

    #[test]
    fn fence_detection_survives_nested_block_comments() {
        // A nested plain comment inside a block doc must not end the doc
        // block early.
        let contents =
            ["/**", " * ```compile_fail", " * /* nested */", " * ```", " */", "pub struct A;", ""]
                .join("\n");
        assert_eq!(compile_fail_fence_lines(&contents), vec![2], "only the fence line is reported");
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
