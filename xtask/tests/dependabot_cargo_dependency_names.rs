//! Cargo Dependabot names must be dependencies this workspace actually has (#14178).
//!
//! `.github/dependabot.yml` names crates in two places that only do work when
//! the name resolves to a real workspace dependency: literal `groups.*.patterns`
//! entries and `ignore[].dependency-name` entries. A literal that matches
//! nothing is inert — Dependabot silently never acts on it — while
//! `docs/how-to/DEPENDENCY_MANAGEMENT.md` keeps advertising the coverage to
//! readers. That is how the `lsp` group came to claim `lsp-server`, which is
//! absent from every `Cargo.toml` and from `Cargo.lock`, and how the doc kept
//! describing the group as "LSP protocol crates (lsp-types, lsp-server)".
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

use std::{collections::BTreeSet, ffi::OsStr, fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use walkdir::WalkDir;

const DEPENDABOT_YML: &str = ".github/dependabot.yml";
const DEPENDENCY_DOC: &str = "docs/how-to/DEPENDENCY_MANAGEMENT.md";

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

/// Collect every dependency name declared anywhere in the tree's manifests.
///
/// Recursing over the parsed TOML rather than matching section headers picks up
/// `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`,
/// `[workspace.dependencies]`, and the `[target.'cfg(..)'.dependencies]` forms
/// without enumerating them. Both the table key and any `package = "..."`
/// rename target count, because either spelling can be the name Dependabot
/// matches.
///
/// The set is intentionally permissive — a name declared by any manifest in the
/// tree counts. A stale config entry has to be absent *everywhere* before these
/// tests call it a false claim, so the failure mode is a missed stale name, not
/// a spurious red.
///
/// Symlinks need no special handling here. `WalkDir` leaves `follow_links`
/// disabled by default, so it never descends through a link and a link cycle
/// cannot be walked into; with following disabled, `DirEntry::file_type` also
/// reports the link itself rather than its target, so the `is_file` guard below
/// already skips a symlinked `Cargo.toml` instead of reading through it.
fn declared_dependency_names() -> Result<BTreeSet<String>> {
    let root = project_root();
    let mut names = BTreeSet::new();
    let mut manifests = 0usize;

    for entry in WalkDir::new(&root)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name();
            !(entry.depth() > 0
                && entry.file_type().is_dir()
                && (name == OsStr::new("target") || name == OsStr::new(".git")))
        })
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() || entry.file_name() != OsStr::new("Cargo.toml") {
            continue;
        }
        let text = fs::read_to_string(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        let Ok(value) = toml::from_str::<toml::Value>(&text) else {
            // A manifest this parser cannot read contributes no names. It also
            // cannot mask a stale entry, because absence is what a stale entry
            // must prove.
            continue;
        };
        manifests += 1;
        collect_dependency_names(&value, &mut names);
    }

    if manifests == 0 {
        bail!(
            "no parsable Cargo.toml was found under {}: refusing to report every \
             dependabot name as stale from an empty manifest set (#14178)",
            root.display()
        );
    }
    Ok(names)
}

fn collect_dependency_names(value: &toml::Value, names: &mut BTreeSet<String>) {
    let Some(table) = value.as_table() else {
        return;
    };
    for (key, child) in table {
        if matches!(key.as_str(), "dependencies" | "dev-dependencies" | "build-dependencies") {
            if let Some(deps) = child.as_table() {
                for (dep_name, spec) in deps {
                    names.insert(dep_name.clone());
                    if let Some(renamed) = spec
                        .as_table()
                        .and_then(|spec| spec.get("package"))
                        .and_then(toml::Value::as_str)
                    {
                        names.insert(renamed.to_owned());
                    }
                }
            }
            continue;
        }
        collect_dependency_names(child, names);
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

/// Every literal crate name the cargo entry claims, tagged with where it came
/// from so a failure names the exact YAML site to repair.
fn cargo_literal_claims(entry: &serde_yaml_ng::Value) -> Result<Vec<(String, String)>> {
    let mut claims = Vec::new();

    let groups = entry
        .get("groups")
        .and_then(serde_yaml_ng::Value::as_mapping)
        .ok_or_else(|| anyhow!("the cargo update entry must declare a `groups` mapping"))?;
    for (group_name, group) in groups {
        let group_name = group_name.as_str().unwrap_or("<non-string group name>");
        let patterns = group
            .get("patterns")
            .and_then(serde_yaml_ng::Value::as_sequence)
            .ok_or_else(|| anyhow!("cargo group `{group_name}` must declare `patterns`"))?;
        for pattern in patterns.iter().filter_map(serde_yaml_ng::Value::as_str) {
            if is_literal_name(pattern) {
                claims.push((pattern.to_owned(), format!("groups.{group_name}.patterns")));
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
            "`lsp-server` resolved as a declared dependency, but it is absent from \
             every Cargo.toml and from Cargo.lock. Either the workspace genuinely \
             took the dependency — in which case the #14178 removal should be \
             revisited — or the resolver is matching too loosely and the checks in \
             this file are vacuous."
        );
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
