//! Dependabot YAML helpers for the GitHub Actions and npm title-scope contract
//! (#14180).
//!
//! GitHub's `include: "scope"` rule (Dependabot options reference): the
//! configured prefix is followed by the dependency type in parentheses,
//! `deps` or `deps-dev`. `prefix: "chore(deps)"` plus that include therefore
//! renders `chore(deps)(deps)` / `chore(deps)(deps-dev)`.
//!
//! Cargo is out of this claim. #13482 already repaired the cargo `/` row, and
//! #14898 owns the Cargo-only source contract. A doubled Cargo prefix, or a
//! missing/duplicate cargo row, must not turn this suite red or green.

// `#[path]` support modules are compiled fresh per integration-test binary;
// unused items here are false-positive dead code, not unreachable production.
#![allow(dead_code)]

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};

pub const DEPENDABOT_YML: &str = ".github/dependabot.yml";

#[derive(Clone, Copy)]
pub struct EcosystemRow {
    pub ecosystem: &'static str,
    pub directory: &'static str,
}

impl EcosystemRow {
    pub fn label(self) -> String {
        format!("{} `{}`", self.ecosystem, self.directory)
    }
}

pub const GITHUB_ACTIONS: EcosystemRow =
    EcosystemRow { ecosystem: "github-actions", directory: "/" };

pub const NPM: EcosystemRow = EcosystemRow { ecosystem: "npm", directory: "/vscode-extension" };

/// The two rows this claim governs. Cargo is intentionally absent.
pub const GOVERNED_ROWS: [EcosystemRow; 2] = [GITHUB_ACTIONS, NPM];

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

fn updates_sequence(doc: &serde_yaml_ng::Value) -> Result<&Vec<serde_yaml_ng::Value>> {
    doc.get("updates")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| anyhow!("{DEPENDABOT_YML} must declare an `updates` sequence"))
}

fn is_row(update: &serde_yaml_ng::Value, row: EcosystemRow) -> bool {
    update.get("package-ecosystem").and_then(serde_yaml_ng::Value::as_str) == Some(row.ecosystem)
        && update.get("directory").and_then(serde_yaml_ng::Value::as_str) == Some(row.directory)
}

/// The unique row for one governed (ecosystem, directory) pair.
pub fn unique_row_in(
    doc: &serde_yaml_ng::Value,
    row: EcosystemRow,
) -> Result<serde_yaml_ng::Value> {
    let matches: Vec<&serde_yaml_ng::Value> =
        updates_sequence(doc)?.iter().filter(|update| is_row(update, row)).collect();
    match matches.as_slice() {
        [only] => Ok((*only).clone()),
        [] => bail!("{DEPENDABOT_YML} must keep a {} update entry", row.label()),
        many => bail!(
            "{DEPENDABOT_YML} has {} {} update entries; this contract cannot tell \
             which row is current (#14180)",
            many.len(),
            row.label()
        ),
    }
}

pub struct CommitMessage {
    pub prefix: String,
    pub include: String,
}

pub fn commit_message(entry: &serde_yaml_ng::Value, row: EcosystemRow) -> Result<CommitMessage> {
    let commit_message = entry.get("commit-message").ok_or_else(|| {
        anyhow!("the {} update entry must declare `commit-message` (#14180)", row.label())
    })?;
    if !commit_message.is_mapping() {
        bail!(
            "{} `commit-message` must be a mapping; found {commit_message:?} (#14180)",
            row.label()
        );
    }
    let prefix = commit_message
        .get("prefix")
        .and_then(serde_yaml_ng::Value::as_str)
        .ok_or_else(|| {
            anyhow!("{} `commit-message.prefix` must be a string (#14180)", row.label())
        })?
        .to_owned();
    let include =
        commit_message.get("include").and_then(serde_yaml_ng::Value::as_str).ok_or_else(|| {
            anyhow!(
                "{} `commit-message.include` must be `scope` so Dependabot still \
                 appends deps / deps-dev; omitting it drops that discrimination (#14180)",
                row.label()
            )
        })?;
    Ok(CommitMessage { prefix, include: include.to_owned() })
}

/// GitHub's `include: "scope"` composition: the prefix is followed by
/// `(deps)` or `(deps-dev)`.
pub fn rendered_title_prefix(prefix: &str, include: &str, dependency_type: &str) -> String {
    if include == "scope" { format!("{prefix}({dependency_type})") } else { prefix.to_owned() }
}

/// Accept only `prefix: "chore"` plus `include: "scope"`.
///
/// That is the combination that renders `chore(deps)` / `chore(deps-dev)`
/// once. `prefix: "chore(deps)"` plus `include: "scope"` is the live failure
/// mode pinned by #12212 / #12210 (GitHub Actions) and #12207 / #12206 (npm).
pub fn assert_single_scope(msg: &CommitMessage, row: EcosystemRow) -> Result<()> {
    if msg.include != "scope" {
        bail!(
            "{} `commit-message.include` must be `scope` (found `{}`); GitHub \
             only supports that value, and it is what appends deps / deps-dev (#14180)",
            row.label(),
            msg.include
        );
    }
    if msg.prefix != "chore" {
        bail!(
            "{} `commit-message.prefix` must be `chore` so `include: scope` \
             renders `chore(deps): ...` once. `prefix: \"{}\"` plus `include: \
             \"scope\"` renders `{}` (#14180)",
            row.label(),
            msg.prefix,
            rendered_title_prefix(&msg.prefix, &msg.include, "deps")
        );
    }
    Ok(())
}

/// Both governed rows keep the single-scope composition.
pub fn assert_governed_rows(doc: &serde_yaml_ng::Value) -> Result<()> {
    for row in GOVERNED_ROWS {
        let entry = unique_row_in(doc, row)?;
        let msg = commit_message(&entry, row)?;
        assert_single_scope(&msg, row)?;
    }
    Ok(())
}

pub fn assert_committed_governed_rows() -> Result<()> {
    assert_governed_rows(&dependabot_document()?)
}
