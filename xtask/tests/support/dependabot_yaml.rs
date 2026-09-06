//! Shared Dependabot YAML helpers for Cargo-row contract tests.
//!
//! Two Cargo-row suites walk `.github/dependabot.yml`: crate-name resolution
//! (#14178) and commit-message composition (#13477). One loader keeps them on
//! the same `cargo` + `/` row and fails closed when that row is missing or
//! duplicated.
//!
//! GitHub's `include: "scope"` rule (Dependabot options reference): the
//! configured prefix is followed by the dependency type in parentheses,
//! `deps` or `deps-dev`. `prefix: "chore(deps)"` plus that include therefore
//! renders `chore(deps)(deps)`.

// `#[path]` support modules are compiled fresh per integration-test binary;
// unused items here are false-positive dead code, not unreachable production.
#![allow(dead_code)]

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};

pub const DEPENDABOT_YML: &str = ".github/dependabot.yml";

pub fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

pub fn dependabot_document() -> Result<serde_yaml_ng::Value> {
    let path = project_root().join(DEPENDABOT_YML);
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml_ng::from_str(&content).with_context(|| format!("parsing {}", path.display()))
}

fn is_cargo_workspace_row(update: &serde_yaml_ng::Value) -> bool {
    update.get("package-ecosystem").and_then(serde_yaml_ng::Value::as_str) == Some("cargo")
        && update.get("directory").and_then(serde_yaml_ng::Value::as_str) == Some("/")
}

/// The unique Cargo workspace (`package-ecosystem: cargo`, `directory: /`) row.
pub fn cargo_update_entry() -> Result<serde_yaml_ng::Value> {
    cargo_update_entry_in(&dependabot_document()?)
}

/// Same lookup against an already-parsed document, so synthetic fixtures can
/// exercise the checker without mutating the committed file.
pub fn cargo_update_entry_in(doc: &serde_yaml_ng::Value) -> Result<serde_yaml_ng::Value> {
    let updates = doc
        .get("updates")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| anyhow!("{DEPENDABOT_YML} must declare an `updates` sequence"))?;
    let matches: Vec<&serde_yaml_ng::Value> =
        updates.iter().filter(|update| is_cargo_workspace_row(update)).collect();
    match matches.as_slice() {
        [only] => Ok((*only).clone()),
        [] => {
            bail!("{DEPENDABOT_YML} must keep a cargo update entry for the `/` workspace")
        }
        many => bail!(
            "{DEPENDABOT_YML} has {} cargo update entries for `/`; the Cargo \
             commit-message and name contracts cannot tell which row is current (#13477)",
            many.len()
        ),
    }
}

pub struct CargoCommitMessage {
    pub prefix: String,
    pub include: String,
}

pub fn cargo_commit_message(entry: &serde_yaml_ng::Value) -> Result<CargoCommitMessage> {
    let commit_message = entry.get("commit-message").ok_or_else(|| {
        anyhow!("the cargo `/` update entry must declare `commit-message` (#13477)")
    })?;
    if !commit_message.is_mapping() {
        bail!("cargo `commit-message` must be a mapping; found {commit_message:?}");
    }
    let prefix = commit_message
        .get("prefix")
        .and_then(serde_yaml_ng::Value::as_str)
        .ok_or_else(|| anyhow!("cargo `commit-message.prefix` must be a string (#13477)"))?
        .to_owned();
    let include =
        commit_message.get("include").and_then(serde_yaml_ng::Value::as_str).ok_or_else(|| {
            anyhow!(
                "cargo `commit-message.include` must be `scope` so Dependabot still \
                 appends deps / deps-dev; omitting it drops that discrimination (#13477)"
            )
        })?;
    Ok(CargoCommitMessage { prefix, include: include.to_owned() })
}

/// GitHub's `include: "scope"` composition: the prefix is followed by
/// `(deps)` or `(deps-dev)`.
pub fn rendered_title_prefix(prefix: &str, include: &str, dependency_type: &str) -> String {
    if include == "scope" { format!("{prefix}({dependency_type})") } else { prefix.to_owned() }
}

fn scope_count(rendered: &str) -> usize {
    rendered.chars().filter(|c| *c == '(').count()
}

/// Accept only `prefix: "chore"` plus `include: "scope"`.
///
/// That is the combination that renders `chore(deps)` / `chore(deps-dev)`
/// once. `prefix: "chore(deps)"` plus `include: "scope"` is the live failure
/// mode pinned by #12208 and #12209.
pub fn assert_single_cargo_scope(msg: &CargoCommitMessage) -> Result<()> {
    if msg.include != "scope" {
        bail!(
            "cargo `commit-message.include` must be `scope` (found `{}`); GitHub \
             only supports that value, and it is what appends deps / deps-dev (#13477)",
            msg.include
        );
    }
    if msg.prefix != "chore" {
        bail!(
            "cargo `commit-message.prefix` must be `chore` so `include: scope` \
             renders `chore(deps): ...` once. `prefix: \"{}\"` plus `include: \
             \"scope\"` renders `{}` (#13477)",
            msg.prefix,
            rendered_title_prefix(&msg.prefix, &msg.include, "deps")
        );
    }
    for dependency_type in ["deps", "deps-dev"] {
        let rendered = rendered_title_prefix(&msg.prefix, &msg.include, dependency_type);
        if scope_count(&rendered) != 1 {
            bail!(
                "rendered Cargo title prefix `{rendered}` must contain exactly one \
                 parenthetical scope; found {} (#13477)",
                scope_count(&rendered)
            );
        }
        let expected = format!("chore({dependency_type})");
        if rendered != expected {
            bail!("expected rendered prefix `{expected}`, got `{rendered}` (#13477)");
        }
    }
    Ok(())
}
