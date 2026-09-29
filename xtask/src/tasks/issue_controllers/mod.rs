//! `cargo xtask issue-controllers` — tooling over the issue-controller train
//! (T02 `#11765`).
//!
//! Implements exactly the static plane: independent validation of the stable
//! `issue_controller_train.v1` manifest (T01 `#11764`), the sole checked human
//! train projection derived from it, and per-node static explanation. All
//! operations are deterministic and offline: no GitHub reads/writes, no
//! current-tree probes, no readiness/frontier computation, no packet
//! generation — those planes are owned by T03/T04/T05/T06/T07/T08 and are
//! refused here instead of being approximated.

pub mod digest;
pub mod model;
pub mod projection;
pub mod validate;

#[cfg(test)]
mod tests;

use std::path::Path;

use clap::Subcommand;
use color_eyre::eyre::{Context, Result, bail};

use projection::{known_node_ids, render_explanation, render_projection};
use validate::{GraphSummary, StaticReport, render_report, require_valid};

/// Location of the stable manifest inside the repository.
pub const MANIFEST_RELATIVE_PATH: &str = ".spec/11764-controller-train-graph/train.manifest.json";
/// Location of the sole checked human train projection.
pub const PROJECTION_RELATIVE_PATH: &str = ".spec/11764-controller-train-graph/train.projection.md";

#[derive(Debug, Subcommand)]
pub enum IssueControllersCommand {
    /// Operate on the stable issue-controller train.
    Train {
        #[command(subcommand)]
        command: TrainCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum TrainCommand {
    /// Validate the stable manifest. Only the static plane exists in T02.
    Check {
        /// Validate the stable contract bytes (structure, laws, digests).
        /// Non-static planes are owned by T03/T04 and fail closed here.
        #[arg(long = "static")]
        static_plane: bool,
    },
    /// Generate the checked human train projection. With `--check`, verify
    /// the on-disk projection matches a fresh generation and fail on drift.
    Graph {
        /// Verify the checked-in projection instead of regenerating it.
        #[arg(long)]
        check: bool,
    },
    /// Print the deterministic static explanation for one node id.
    ExplainStatic { node: String },
}

pub fn run(command: IssueControllersCommand) -> Result<()> {
    match command {
        IssueControllersCommand::Train { command } => run_train(command),
    }
}

fn run_train(command: TrainCommand) -> Result<()> {
    let root = crate::utils::project_root()
        .with_context(|| "locating the repository root for the train bundle")?;
    let manifest_path = root.join(MANIFEST_RELATIVE_PATH);
    let projection_path = root.join(PROJECTION_RELATIVE_PATH);
    run_train_at(command, &manifest_path, &projection_path)
}

/// Path-injectable core so tests can run commands against temporary copies.
pub(crate) fn run_train_at(
    command: TrainCommand,
    manifest_path: &Path,
    projection_path: &Path,
) -> Result<()> {
    match command {
        TrainCommand::Check { static_plane } => {
            if !static_plane {
                bail!(
                    "refusing to run a non-static check: T02 owns only the static validation \
                     plane; current-tree probes are T03 #11769 and offline frontier is T04 \
                     #11771 — rerun with `--static`"
                );
            }
            run_check_static(manifest_path)
        }
        TrainCommand::Graph { check } => {
            if check {
                run_graph_check(manifest_path, projection_path)
            } else {
                run_graph_generate(manifest_path, projection_path)
            }
        }
        TrainCommand::ExplainStatic { node } => run_explain_static(manifest_path, &node),
    }
}

fn load_report(manifest_path: &Path) -> Result<(StaticReport, Vec<u8>)> {
    let raw = std::fs::read(manifest_path)
        .with_context(|| format!("reading train manifest {}", manifest_path.display()))?;
    let report = validate::validate_static_bytes(&raw);
    Ok((report, raw))
}

fn run_check_static(manifest_path: &Path) -> Result<()> {
    let (report, _raw) = load_report(manifest_path)?;
    if !report.is_valid() {
        println!("{}", render_report(&report)?);
        bail!(
            "static validation failed with {} diagnostic(s); the manifest is NOT projected",
            report.diagnostics.len()
        );
    }
    let (_manifest, _digest) = require_valid(&report)?;
    println!("{}", render_report(&report)?);
    Ok(())
}

fn run_graph_generate(manifest_path: &Path, projection_path: &Path) -> Result<()> {
    let (report, _) = load_report(manifest_path)?;
    let (manifest, digest) = require_valid(&report).with_context(
        || "refusing to project an invalid manifest — fix the static diagnostics first",
    )?;
    let summary = GraphSummary::of(manifest);
    let document = render_projection(manifest, digest, &summary);
    std::fs::write(projection_path, document)
        .with_context(|| format!("writing train projection {}", projection_path.display()))?;
    println!(
        "ISSUE_CONTROLLER_TRAIN_PROJECTION=WRITTEN path={} semantic_sha256={digest}",
        projection_path.display()
    );
    Ok(())
}

fn run_graph_check(manifest_path: &Path, projection_path: &Path) -> Result<()> {
    let (report, _) = load_report(manifest_path)?;
    let (manifest, digest) = require_valid(&report).with_context(
        || "refusing to project an invalid manifest — fix the static diagnostics first",
    )?;
    let summary = GraphSummary::of(manifest);
    let expected = render_projection(manifest, digest, &summary);
    let actual = std::fs::read_to_string(projection_path).with_context(|| {
        format!(
            "reading checked train projection {} — regenerate it with `cargo xtask \
             issue-controllers train graph`",
            projection_path.display()
        )
    })?;
    if actual != expected {
        bail!(
            "checked train projection drifted from the manifest: {} does not match a fresh \
             deterministic generation (semantic_sha256={digest}); regenerate it with `cargo \
             xtask issue-controllers train graph` — never edit generated artifacts by hand",
            projection_path.display()
        );
    }
    println!(
        "ISSUE_CONTROLLER_TRAIN_PROJECTION=CHECKED path={} semantic_sha256={digest}",
        projection_path.display()
    );
    Ok(())
}

fn run_explain_static(manifest_path: &Path, node: &str) -> Result<()> {
    let (report, _) = load_report(manifest_path)?;
    let (manifest, digest) = require_valid(&report)?;
    let explanation = render_explanation(manifest, digest, node);
    if explanation.is_empty() {
        bail!(
            "unknown train node '{node}'; valid node ids: [{}]",
            known_node_ids(manifest).join(", ")
        );
    }
    print!("{explanation}");
    Ok(())
}
