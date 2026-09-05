//! Cargo Dependabot names must be dependencies this workspace actually has (#14178).
//!
//! `.github/dependabot.yml` names crates in four places that only do work when
//! the name resolves to a real workspace dependency: literal
//! `groups.*.patterns` and `groups.*.exclude-patterns` entries, and
//! `ignore[].dependency-name` and `allow[].dependency-name` entries. A literal
//! that matches nothing is inert — Dependabot silently never acts on it — while
//! `docs/how-to/DEPENDENCY_MANAGEMENT.md` keeps advertising the coverage to
//! readers. That is how the `lsp` group came to claim `lsp-server`, which no
//! workspace manifest declares and which is absent from `Cargo.lock`, and how
//! the doc kept describing the group as "LSP protocol crates (lsp-types,
//! lsp-server)".
//!
//! PR #13481 removed the same stale-claim shape for `tower-lsp`, but only from
//! the prose; nothing in the repository could see that the config half was
//! making an equally empty claim. These tests close that gap from both sides:
//! the config may not name a crate the workspace does not declare, and the
//! Cargo section of the guide may not advertise one either.
//!
//! Scope is deliberately literal names only. Wildcard patterns such as
//! `serde*` or `tokio*` are forward-looking by construction — they are meant to
//! catch crates that may be added later — so matching nothing today is not a
//! false claim. A literal name makes a definite claim about the present tree,
//! and that is what is checked here.
//!
//! Both checks parse the committed config, the committed manifests, and the
//! committed guide. Unrecognised structure fails closed rather than passing on
//! a section the parser could not find, so a rewrite cannot turn these green by
//! making them blind.

use std::{collections::BTreeSet, fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};

const DEPENDABOT_YML: &str = ".github/dependabot.yml";
const DEPENDENCY_DOC: &str = "docs/how-to/DEPENDENCY_MANAGEMENT.md";

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

/// The dependency-table keys Cargo recognises.
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Collect the dependency names the Cargo workspace actually declares.
///
/// Dependabot's `cargo` entry for directory `/` resolves against this
/// workspace, so the honest denominator is the root manifest's
/// `[workspace.dependencies]` plus the dependency tables of each declared
/// workspace member. Only the locations Cargo itself treats as dependency
/// tables count: a member's `[dependencies]`, `[dev-dependencies]`,
/// `[build-dependencies]`, and the same three under
/// `[target.<cfg>]`. Both the table key and any `package = "..."` rename
/// target count, because either spelling can be the name Dependabot matches.
///
/// This is deliberately narrower than "every `Cargo.toml` under the root, and
/// every nested table named `dependencies`". That broader sweep would let a
/// test fixture manifest or a `[package.metadata.*.dependencies]` table make a
/// name look declared, so a stale Dependabot claim could be restored and still
/// pass — the ratchet would not ratchet. Excluded manifests
/// (`[workspace.exclude]`) are outside the workspace Dependabot updates here,
/// so they cannot vouch for a name either.
fn declared_dependency_names() -> Result<BTreeSet<String>> {
    let root = project_root();
    let root_manifest_path = root.join("Cargo.toml");
    let root_text = fs::read_to_string(&root_manifest_path)
        .with_context(|| format!("reading {}", root_manifest_path.display()))?;
    let root_manifest: toml::Value = toml::from_str(&root_text)
        .with_context(|| format!("parsing {}", root_manifest_path.display()))?;

    let workspace =
        root_manifest.get("workspace").and_then(toml::Value::as_table).ok_or_else(|| {
            anyhow!("{} must declare a [workspace] table", root_manifest_path.display())
        })?;

    let mut names = BTreeSet::new();

    // Shared versions live here and are the most common place a grouped or
    // ignored crate is declared exactly once.
    if let Some(shared) = workspace.get("dependencies").and_then(toml::Value::as_table) {
        insert_dependency_table(shared, &mut names);
    }

    let members = workspace
        .get("members")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow!("[workspace] must declare `members`"))?;
    if members.is_empty() {
        bail!(
            "[workspace.members] is empty; refusing to judge Dependabot names against no manifests (#14178)"
        );
    }

    let mut parsed_members = 0usize;
    for member in members {
        let member = member.as_str().ok_or_else(|| {
            anyhow!("every [workspace.members] entry must be a string; found {member:?}")
        })?;
        let manifest_path = root.join(member).join("Cargo.toml");
        // A member path that no longer exists is a workspace defect, not
        // something this test should quietly absorb: it would silently shrink
        // the denominator and turn a real dependency into a stale-looking name.
        let text = fs::read_to_string(&manifest_path).with_context(|| {
            format!("reading workspace member manifest {}", manifest_path.display())
        })?;
        let manifest: toml::Value = toml::from_str(&text)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;
        collect_member_dependency_names(&manifest, &mut names);
        parsed_members += 1;
    }

    if parsed_members == 0 {
        bail!("no workspace member manifest was parsed; the denominator would be empty (#14178)");
    }
    Ok(names)
}

/// Names from one member manifest's Cargo-recognised dependency tables.
///
/// Walks the three top-level tables plus `[target.<cfg>.<table>]`, and nothing
/// else, so `[package.metadata]` and other nested tables that happen to contain
/// a `dependencies` key cannot contribute a name.
fn collect_member_dependency_names(manifest: &toml::Value, names: &mut BTreeSet<String>) {
    for table in DEPENDENCY_TABLES {
        if let Some(deps) = manifest.get(table).and_then(toml::Value::as_table) {
            insert_dependency_table(deps, names);
        }
    }
    let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) else {
        return;
    };
    for cfg in targets.values() {
        for table in DEPENDENCY_TABLES {
            if let Some(deps) = cfg.get(table).and_then(toml::Value::as_table) {
                insert_dependency_table(deps, names);
            }
        }
    }
}

fn insert_dependency_table(deps: &toml::Table, names: &mut BTreeSet<String>) {
    for (dep_name, spec) in deps {
        names.insert(dep_name.clone());
        if let Some(renamed) =
            spec.as_table().and_then(|spec| spec.get("package")).and_then(toml::Value::as_str)
        {
            names.insert(renamed.to_owned());
        }
    }
}

fn cargo_update_entry() -> Result<serde_yaml_ng::Value> {
    let path = project_root().join(DEPENDABOT_YML);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let doc: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;
    let updates = doc
        .get("updates")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| anyhow!("{DEPENDABOT_YML} must declare an `updates` sequence"))?;
    updates
        .iter()
        .find(|update| {
            update.get("package-ecosystem").and_then(|value| value.as_str()) == Some("cargo")
                && update.get("directory").and_then(|value| value.as_str()) == Some("/")
        })
        .cloned()
        .ok_or_else(|| {
            anyhow!("{DEPENDABOT_YML} must keep a cargo update entry for the `/` workspace")
        })
}

/// A name that makes a definite claim about the current tree.
///
/// Wildcard patterns are excluded: they are forward-looking by design and
/// matching nothing today is not a false claim.
fn is_literal_name(pattern: &str) -> bool {
    !pattern.contains('*') && !pattern.contains('?') && !pattern.is_empty()
}

/// Pull the literal names out of one pattern sequence, failing closed on a
/// non-string entry rather than silently dropping the one that could not be
/// checked.
fn collect_pattern_claims(
    patterns: &[serde_yaml_ng::Value],
    site: &str,
    claims: &mut Vec<(String, String)>,
) -> Result<()> {
    for pattern in patterns {
        let pattern = pattern.as_str().ok_or_else(|| {
            anyhow!("every pattern in `{site}` must be a string; found {pattern:?}")
        })?;
        if is_literal_name(pattern) {
            claims.push((pattern.to_owned(), site.to_owned()));
        }
    }
    Ok(())
}

/// Every literal crate name the cargo entry claims, tagged with where it came
/// from so a failure names the exact YAML site to repair.
///
/// Covers every cargo-ecosystem key that can carry a definite crate name:
/// `groups.*.patterns`, `groups.*.exclude-patterns`, `ignore[].dependency-name`,
/// and `allow[].dependency-name`. Leaving any of them out would let a stale
/// literal be introduced through the uncovered key while this check stayed
/// green — the same empty-claim shape #14178 exists to catch, just relocated.
fn cargo_literal_claims(entry: &serde_yaml_ng::Value) -> Result<Vec<(String, String)>> {
    let mut claims = Vec::new();

    let groups = entry
        .get("groups")
        .and_then(serde_yaml_ng::Value::as_mapping)
        .ok_or_else(|| anyhow!("the cargo update entry must declare a `groups` mapping"))?;
    for (group_name, group) in groups {
        // Fail closed on a malformed group rather than naming it
        // `<non-string group name>` and carrying on: a group this parser cannot
        // read contributes no claims, so the other groups' valid entries would
        // still let the test pass while part of the config went unexamined.
        let group_name = group_name.as_str().ok_or_else(|| {
            anyhow!("every cargo group key must be a string; found {group_name:?}")
        })?;
        let patterns = group
            .get("patterns")
            .and_then(serde_yaml_ng::Value::as_sequence)
            .ok_or_else(|| anyhow!("cargo group `{group_name}` must declare `patterns`"))?;
        collect_pattern_claims(patterns, &format!("groups.{group_name}.patterns"), &mut claims)?;

        // `exclude-patterns` names crates just as definitely as `patterns`
        // does; a literal there that matches nothing is the same empty claim,
        // and it is optional in the schema so its absence is not a defect.
        if let Some(excluded) = group.get("exclude-patterns") {
            let excluded = excluded.as_sequence().ok_or_else(|| {
                anyhow!("`exclude-patterns` in cargo group `{group_name}` must be a sequence")
            })?;
            collect_pattern_claims(
                excluded,
                &format!("groups.{group_name}.exclude-patterns"),
                &mut claims,
            )?;
        }
    }

    // `allow` is optional, but each entry's `dependency-name` is a literal
    // claim about a crate the workspace is expected to have.
    if let Some(allows) = entry.get("allow") {
        let allows = allows
            .as_sequence()
            .ok_or_else(|| anyhow!("the cargo `allow` entry must be a sequence"))?;
        for rule in allows {
            let Some(name) = rule.get("dependency-name") else {
                // An `allow` rule may select by `dependency-type` alone; that
                // names no crate, so there is nothing to resolve.
                continue;
            };
            let name = name.as_str().ok_or_else(|| {
                anyhow!(
                    "every cargo `allow` rule's `dependency-name` must be a string; found {name:?}"
                )
            })?;
            if is_literal_name(name) {
                claims.push((name.to_owned(), "allow[].dependency-name".to_owned()));
            }
        }
    }

    let ignores = entry
        .get("ignore")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| anyhow!("the cargo update entry must declare an `ignore` sequence"))?;
    for rule in ignores {
        let name = rule
            .get("dependency-name")
            .and_then(serde_yaml_ng::Value::as_str)
            .ok_or_else(|| anyhow!("every cargo `ignore` rule must name a `dependency-name`"))?;
        if is_literal_name(name) {
            claims.push((name.to_owned(), "ignore[].dependency-name".to_owned()));
        }
    }

    if claims.is_empty() {
        bail!(
            "the cargo update entry produced no literal dependency names: the \
             parser is no longer reading {DEPENDABOT_YML} and cannot detect a \
             stale claim (#14178)"
        );
    }
    Ok(claims)
}

/// Literal names in `.github/dependabot.yml` resolve to real dependencies.
///
/// Restoring `lsp-server` to the `lsp` group patterns — or adding any other
/// literal the workspace does not declare — must fail here. Wildcard groups
/// and every declared name stay green.
#[test]
fn cargo_dependabot_literals_name_declared_dependencies() -> Result<()> {
    let declared = declared_dependency_names()?;
    let entry = cargo_update_entry()?;
    let stale: Vec<String> = cargo_literal_claims(&entry)?
        .into_iter()
        .filter(|(name, _)| !declared.contains(name))
        .map(|(name, site)| format!("`{name}` at {site}"))
        .collect();

    if !stale.is_empty() {
        bail!(
            "{DEPENDABOT_YML} names {} crate(s) the workspace does not declare: {}. \
             A literal Dependabot name that matches no manifest entry is inert — \
             the group or ignore rule can never act on it — while the config and \
             {DEPENDENCY_DOC} keep advertising the coverage. Drop the name, or \
             name a crate the workspace actually depends on (#14178).",
            stale.len(),
            stale.join(", ")
        );
    }
    Ok(())
}

/// Slice out one `## ...`/`### ...` section of the guide by heading text.
fn doc_section<'a>(doc: &'a str, heading: &str, next_heading: &str) -> Result<&'a str> {
    let start = doc
        .find(heading)
        .ok_or_else(|| anyhow!("{DEPENDENCY_DOC} must keep the `{heading}` heading"))?;
    let rest = &doc[start + heading.len()..];
    let end = rest
        .find(next_heading)
        .ok_or_else(|| anyhow!("{DEPENDENCY_DOC} must keep the `{next_heading}` heading"))?;
    Ok(&rest[..end])
}

/// Bullet lines directly under a bolded label, stopping at the next blank-line
/// separated block.
fn labelled_bullets<'a>(section: &'a str, label: &str) -> Result<Vec<&'a str>> {
    let start = section
        .find(label)
        .ok_or_else(|| anyhow!("the Cargo section of {DEPENDENCY_DOC} must keep `{label}`"))?;
    let bullets: Vec<&str> = section[start + label.len()..]
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("- "))
        .take_while(|line| line.trim_start().starts_with("- "))
        .collect();
    if bullets.is_empty() {
        bail!("the `{label}` block in {DEPENDENCY_DOC} must list at least one bullet");
    }
    Ok(bullets)
}

/// Crate names the Cargo section of the guide advertises, tagged with their
/// source bullet.
///
/// Under **Grouped Dependencies** the leading backticked token is the *group*
/// name, so only the trailing parenthetical holds crate names. Under
/// **Major Version Exclusions** the leading backticked token is itself a crate.
fn doc_cargo_claims(doc: &str) -> Result<Vec<(String, String)>> {
    let cargo = doc_section(doc, "### Cargo Dependencies", "### GitHub Actions")?;
    let mut claims = Vec::new();

    for bullet in labelled_bullets(cargo, "**Grouped Dependencies**")? {
        let Some(open) = bullet.rfind('(') else { continue };
        let Some(close) = bullet[open..].find(')') else {
            bail!("unterminated parenthetical in {DEPENDENCY_DOC} bullet: {bullet}");
        };
        for token in bullet[open + 1..open + close].split(',') {
            let token = token.trim().trim_matches('`');
            if token.is_empty() || token == "etc." {
                continue;
            }
            claims.push((
                token.to_owned(),
                format!("Grouped Dependencies bullet `{}`", bullet.trim()),
            ));
        }
    }

    for bullet in labelled_bullets(cargo, "**Major Version Exclusions**")? {
        let trimmed = bullet.trim_start().trim_start_matches("- ");
        let name = trimmed
            .strip_prefix('`')
            .and_then(|rest| rest.split_once('`'))
            .map(|(name, _)| name)
            .ok_or_else(|| {
                anyhow!(
                    "every Major Version Exclusions bullet in {DEPENDENCY_DOC} must \
                     open with a backticked crate name; found: {bullet}"
                )
            })?;
        claims.push((
            name.to_owned(),
            format!("Major Version Exclusions bullet `{}`", bullet.trim()),
        ));
    }

    if claims.is_empty() {
        bail!(
            "the Cargo section of {DEPENDENCY_DOC} produced no crate names: the \
             parser is no longer reading the guide and cannot detect a stale \
             claim (#14178)"
        );
    }
    Ok(claims)
}

/// The guide's Cargo section advertises only crates the workspace declares.
///
/// This is the prose half of the same claim. Reverting the guide to "LSP
/// protocol crates (lsp-types, lsp-server)" must fail here even though the
/// config itself is clean.
#[test]
fn dependency_guide_advertises_declared_cargo_dependencies() -> Result<()> {
    let declared = declared_dependency_names()?;
    let path = project_root().join(DEPENDENCY_DOC);
    let doc = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    let stale: Vec<String> = doc_cargo_claims(&doc)?
        .into_iter()
        .filter(|(name, _)| !declared.contains(name))
        .map(|(name, site)| format!("`{name}` in {site}"))
        .collect();

    if !stale.is_empty() {
        bail!(
            "{DEPENDENCY_DOC} advertises {} crate(s) the workspace does not declare: \
             {}. The guide describes what Dependabot actually covers, so a name the \
             config can never act on misleads every reader who checks whether a \
             crate is grouped or excluded (#14178).",
            stale.len(),
            stale.join(", ")
        );
    }
    Ok(())
}

/// Negative control: the resolver distinguishes declared from undeclared names.
///
/// Without this, both checks above could pass by treating every name as
/// declared — the classic always-green scanner. `lsp-types` is the real
/// dependency the `lsp` group keeps; `lsp-server` is the name #14178 removed
/// and must stay unresolvable.
#[test]
fn declared_name_resolution_is_discriminating() -> Result<()> {
    let declared = declared_dependency_names()?;
    if !declared.contains("lsp-types") {
        bail!(
            "`lsp-types` is declared in the workspace manifests but the resolver \
             did not find it: the checks in this file would under-report stale \
             names (#14178)"
        );
    }
    if declared.contains("lsp-server") {
        bail!(
            "`lsp-server` resolved as a declared dependency, but no workspace \
             manifest declares it and it is absent from Cargo.lock. Either the \
             workspace genuinely took the dependency — in which case the #14178 \
             removal should be revisited — or the resolver is matching too \
             loosely and the checks in this file are vacuous."
        );
    }
    Ok(())
}

/// The denominator is the workspace, not every manifest under the root.
///
/// `libfuzzer-sys` is declared in `fuzz/Cargo.toml`, which the root manifest
/// lists under `[workspace.exclude]`. Dependabot's cargo entry for `/` does not
/// update that manifest, so it cannot vouch for a name either — and a collector
/// that swept in every `Cargo.toml` it could find would report it as declared.
///
/// This is the control for the scope of `declared_dependency_names`. Without
/// it, widening the collector back to a whole-tree walk (or to arbitrary nested
/// tables such as `[package.metadata.*.dependencies]`) would leave every test
/// in this file green while a restored stale Dependabot claim could resolve
/// against a fixture or an excluded manifest.
#[test]
fn excluded_manifests_do_not_vouch_for_dependency_names() -> Result<()> {
    let root = project_root();
    let fuzz_manifest = root.join("fuzz/Cargo.toml");
    let fuzz_text = fs::read_to_string(&fuzz_manifest)
        .with_context(|| format!("reading {}", fuzz_manifest.display()))?;
    if !fuzz_text.contains("libfuzzer-sys") {
        bail!(
            "this control assumes `fuzz/Cargo.toml` declares `libfuzzer-sys`; it no \
             longer does, so pick another dependency unique to an excluded manifest \
             rather than deleting the control (#14178)"
        );
    }

    let declared = declared_dependency_names()?;
    if declared.contains("libfuzzer-sys") {
        bail!(
            "`libfuzzer-sys` resolved as a workspace dependency, but it is declared \
             only by `fuzz/Cargo.toml`, which is listed in [workspace.exclude]. The \
             collector is reading manifests outside the workspace Dependabot \
             updates, so a stale literal could be vouched for by a fixture or an \
             excluded package and the ratchet would not catch it (#14178)."
        );
    }
    Ok(())
}

/// Every cargo key that can name a crate is actually scanned.
///
/// `groups.*.patterns` and `ignore[].dependency-name` are populated today, so
/// the live config alone cannot show that `exclude-patterns` and
/// `allow[].dependency-name` are covered — that code would sit unexecuted and a
/// stale literal introduced through either key would pass. Feeding a synthetic
/// entry proves each site is read and reported under its own name.
#[test]
fn every_cargo_key_that_names_a_crate_is_scanned() -> Result<()> {
    let entry: serde_yaml_ng::Value = serde_yaml_ng::from_str(
        r#"
package-ecosystem: "cargo"
directory: "/"
groups:
  demo:
    patterns:
      - "from-patterns"
      - "wildcard*"
    exclude-patterns:
      - "from-exclude-patterns"
ignore:
  - dependency-name: "from-ignore"
allow:
  - dependency-name: "from-allow"
  - dependency-type: "direct"
"#,
    )?;

    let sites: Vec<(String, String)> = cargo_literal_claims(&entry)?;
    let found: Vec<&str> = sites.iter().map(|(name, _)| name.as_str()).collect();

    for expected in ["from-patterns", "from-exclude-patterns", "from-ignore", "from-allow"] {
        if !found.contains(&expected) {
            bail!(
                "`{expected}` was not collected: a cargo key that can name a crate is \
                 not being scanned, so a stale literal introduced there would pass \
                 this check (#14178). Collected: {found:?}"
            );
        }
    }
    if found.contains(&"wildcard*") {
        bail!("`wildcard*` must stay excluded as a forward-looking pattern; collected {found:?}");
    }
    // The `allow` rule selecting only by `dependency-type` names no crate.
    if found.len() != 4 {
        bail!("expected exactly the four literal names, collected {found:?}");
    }
    Ok(())
}

/// The literal/wildcard split the checks rely on.
///
/// A resolver that treated `serde*` as a literal would demand a crate literally
/// named `serde*`; one that treated `lsp-server` as a wildcard would never
/// examine it at all. Both directions are pinned.
#[test]
fn wildcard_patterns_are_excluded_from_literal_claims() -> Result<()> {
    for wildcard in ["serde*", "tokio*", "*", "tree-sitter*"] {
        if is_literal_name(wildcard) {
            bail!("`{wildcard}` must be treated as a wildcard, not a literal claim");
        }
    }
    for literal in ["lsp-types", "lsp-server", "tokio"] {
        if !is_literal_name(literal) {
            bail!("`{literal}` must be treated as a literal claim about the current tree");
        }
    }
    Ok(())
}

/// The doc parser reads the Cargo section only.
///
/// The guide repeats a **Grouped Dependencies** block for GitHub Actions and
/// npm. Those name actions and npm packages, which are not Cargo dependencies,
/// so a parser that ran past `### GitHub Actions` would report them all as
/// stale.
#[test]
fn doc_claims_stop_at_the_cargo_section() -> Result<()> {
    let path = project_root().join(DEPENDENCY_DOC);
    let doc = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let claims = doc_cargo_claims(&doc)?;
    for (name, site) in &claims {
        if name.contains('/') || name == "checkout" || name == "upload-artifact" {
            bail!(
                "the Cargo-section parser picked up `{name}` from {site}, which \
                 belongs to another ecosystem's block in {DEPENDENCY_DOC}"
            );
        }
    }
    Ok(())
}
