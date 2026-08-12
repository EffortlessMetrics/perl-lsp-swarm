//! Changie validation for the commit-tier staged-tree gate.
//!
//! The check materializes only Changie's inputs from the tree OID captured by
//! the gate planner, then asks Changie itself to batch every configured project
//! with `--dry-run --keep`. The working tree and live index are never read.

#[path = "commit_checks_changie_config.rs"]
mod config;
#[path = "commit_checks_changie_runner.rs"]
mod runner;
#[cfg(test)]
#[path = "commit_checks_changie_tests.rs"]
mod tests;

use super::{CheckReport, CommitCheckOutcome, Posture};
use crate::tasks::changelog::{self, Fragment};
use crate::tasks::staged::{self, StagedPathText};
use color_eyre::eyre::{Context, Result, bail};
use config::{CONFIG_PATH, RenderSurface, normalize_repo_relative};
use runner::{RenderOutcome, render_with_changie};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const CHECK: &str = "changie_fragment_staged";
const RERUN: &str = "cargo xtask gates --tier commit --staged --gate changie_fragment_staged";

pub(super) fn run(root: &Path, tree_oid: Option<&str>) -> Result<CommitCheckOutcome> {
    run_with_renderer(root, tree_oid, render_with_changie)
}

fn run_with_renderer<F>(
    root: &Path,
    tree_oid: Option<&str>,
    renderer: F,
) -> Result<CommitCheckOutcome>
where
    F: FnOnce(&Path, &[String]) -> Result<RenderOutcome>,
{
    let tree_oid = match tree_oid {
        Some(oid) => oid.to_string(),
        None => staged::staged_tree_oid(root)?,
    };
    let changed = staged::staged_diff_paths(root, Some(&tree_oid))?;

    let config_text = match staged::read_staged_path_text(root, CONFIG_PATH, Some(&tree_oid))? {
        StagedPathText::Present(text) => text,
        StagedPathText::Binary => {
            return Ok(blocked(
                "the staged Changie config is not valid UTF-8",
                "Changie cannot parse or render a binary configuration file",
                vec![CONFIG_PATH.to_string()],
                "restore a text `.changie.yaml`, stage it, and rerun the commit gate",
                "a text Changie configuration and successful dry-render are still required",
            ));
        }
        StagedPathText::Absent => {
            return Ok(blocked(
                "the staged tree does not contain `.changie.yaml`",
                "the pre-commit gate cannot validate the release-note ledger without its render authority",
                vec![CONFIG_PATH.to_string()],
                "restore `.changie.yaml`, stage it, and rerun the commit gate",
                "the Changie configuration and successful dry-render are still required",
            ));
        }
    };

    let validation_config = match changelog::parse_config(&config_text) {
        Ok(config) => config,
        Err(err) => {
            return Ok(blocked(
                format!("the staged Changie config is invalid: {err:#}"),
                "fragment policy and Changie's renderer must agree on one parseable staged configuration",
                vec![CONFIG_PATH.to_string()],
                "repair `.changie.yaml`, stage it, and rerun the commit gate",
                "a parseable staged Changie configuration and successful dry-render are still required",
            ));
        }
    };
    let surface = match RenderSurface::parse(&config_text) {
        Ok(surface) => surface,
        Err(finding) => {
            return Ok(blocked(
                finding,
                "unsafe or ambiguous config-derived paths cannot be materialized outside the frozen staged-tree sandbox",
                vec![CONFIG_PATH.to_string()],
                "use normalized repository-relative Changie paths, stage the config, and rerun",
                "safe render paths and a successful Changie dry-render are still required",
            ));
        }
    };

    let relevant_changed: Vec<String> =
        changed.into_iter().filter(|path| surface.is_input(path)).collect();
    if relevant_changed.is_empty() {
        return Ok(CommitCheckOutcome::Pass("no staged Changie inputs changed".to_string()));
    }

    let mut findings = Vec::new();
    let mut affected = BTreeSet::new();
    let mut present_fragment_count = 0usize;
    for path in relevant_changed.iter().filter(|path| surface.is_fragment(path)) {
        match staged::read_staged_path_text(root, path, Some(&tree_oid))? {
            StagedPathText::Absent => {}
            StagedPathText::Binary => {
                affected.insert(path.clone());
                findings.push(format!("{path}: fragment is not valid UTF-8"));
            }
            StagedPathText::Present(text) => {
                present_fragment_count += 1;
                match serde_yaml_ng::from_str::<Fragment>(&text) {
                    Ok(fragment) => {
                        for finding in changelog::validate_fragment(&fragment, &validation_config) {
                            affected.insert(path.clone());
                            findings.push(format!("{path}: {finding}"));
                        }
                    }
                    Err(err) => {
                        affected.insert(path.clone());
                        findings.push(format!("{path}: malformed YAML: {err}"));
                    }
                }
            }
        }
    }

    if !findings.is_empty() {
        return Ok(blocked(
            findings.join("; "),
            "malformed or misrouted fragments cannot be batched into a trustworthy release note",
            affected.into_iter().collect(),
            "repair or recreate the fragment with `cargo change`, stage it, and rerun",
            "schema-valid staged fragments and a successful Changie dry-render are still required",
        ));
    }

    let temp = tempfile::tempdir().context("failed to create staged Changie render sandbox")?;
    let entries = staged::list_staged_entries(root, &tree_oid)?;
    for entry in entries.into_iter().filter(|entry| surface.is_input(&entry.path)) {
        if entry.mode != "100644" && entry.mode != "100755" {
            return Ok(blocked(
                format!(
                    "Changie input `{}` has unsupported staged mode {}",
                    entry.path, entry.mode
                ),
                "symlinks, gitlinks, and other non-file entries cannot be reproduced faithfully in the render sandbox",
                vec![entry.path],
                "stage a regular text file at that Changie path and rerun",
                "a regular-file staged ledger and successful Changie dry-render are still required",
            ));
        }
        let text = match staged::read_staged_path_text(root, &entry.path, Some(&tree_oid))? {
            StagedPathText::Present(text) => text,
            StagedPathText::Binary => {
                return Ok(blocked(
                    format!("Changie input `{}` is not valid UTF-8", entry.path),
                    "Changie's configuration, templates, fragments, and changelogs are text inputs",
                    vec![entry.path],
                    "replace the staged binary input with text and rerun",
                    "text Changie inputs and a successful dry-render are still required",
                ));
            }
            StagedPathText::Absent => {
                bail!(
                    "staged tree entry `{}` disappeared while materializing immutable tree {}",
                    entry.path,
                    tree_oid
                );
            }
        };
        let safe_path = match normalize_repo_relative(&entry.path, "staged Changie input") {
            Ok(path) => path,
            Err(finding) => {
                return Ok(blocked(
                    finding,
                    "a staged path must remain contained inside the frozen-tree render sandbox",
                    vec![entry.path],
                    "rename the staged path to a normalized repository-relative path and rerun",
                    "contained staged inputs and a successful Changie dry-render are still required",
                ));
            }
        };
        let destination = temp.path().join(safe_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create staged Changie directory {}", parent.display())
            })?;
        }
        fs::write(&destination, text).with_context(|| {
            format!("failed to materialize staged Changie input `{}`", entry.path)
        })?;
    }

    let project_keys = surface.project_keys();
    match renderer(temp.path(), &project_keys)? {
        RenderOutcome::Passed => Ok(CommitCheckOutcome::Pass(format!(
            "{present_fragment_count} staged Changie fragment(s) satisfy policy and dry-render across {} project(s)",
            project_keys.len()
        ))),
        RenderOutcome::Rejected(errors) => Ok(blocked(
            format!("Changie rejected the staged ledger: {}", errors.join("; ")),
            "the exact tree being committed must be batchable by Changie's own renderer, not only by a parallel schema approximation",
            relevant_changed,
            "repair or recreate the affected fragment with `cargo change`, stage the result, and rerun",
            "a successful Changie dry-render of the frozen staged tree is still required",
        )),
    }
}

fn blocked(
    result: impl Into<String>,
    why: impl Into<String>,
    affected: Vec<String>,
    fix: impl Into<String>,
    what_remains: impl Into<String>,
) -> CommitCheckOutcome {
    CommitCheckOutcome::Flagged(CheckReport {
        check: CHECK.to_string(),
        posture: Posture::Blocked,
        result: result.into(),
        why: why.into(),
        affected,
        fix: Some(fix.into()),
        rerun: RERUN.to_string(),
        what_remains: what_remains.into(),
    })
}
