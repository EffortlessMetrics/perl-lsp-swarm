//! Branch-deletion admission for protected integration (#12885).
//!
//! An open pull request that names a branch as its base is a live dependency
//! on that branch. Deleting the branch closes the child — that is how PRs
//! #7810 and #7819 were auto-closed when #7799 was squash-merged with
//! `--delete-branch` during the August 15 backlog convergence.
//!
//! This module owns the admission decision and nothing else. It reads no
//! network, runs no git, retargets nothing, closes nothing, and deletes
//! nothing: callers feed it a graph snapshot and route the typed outcome.

mod evaluate;
mod live;
mod model;
mod route;

pub use evaluate::evaluate;
pub use live::{
    ReadOnlyCommands, SystemCommands, collect_request, repository_from_remote_url,
    verify_remote_identity,
};
pub use model::{
    AdmissionOutcome, AdmissionRequest, BRANCH_DELETION_ADMISSION_POLICY_VERSION,
    BRANCH_DELETION_ADMISSION_SCHEMA_VERSION, BranchSubject, DeletionAdmission, GraphCompleteness,
    Mergeability, NextOwner, ObservedPullRequest, OpenChildGraph, ParentSubject, ParentTerminality,
    PullRequestState, RepositoryId, RetainedChild, WorktreeOwnership,
};
pub use route::{
    branch_deletion_command, merge_command, remote_verification_command, render_disposition,
};

use clap::{Parser, Subcommand};
use color_eyre::eyre::{Context, Result};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// Process exit code returned when the branch must be retained.
///
/// Distinct from 1 so a shell caller can tell "retained by policy" from
/// "the check itself failed".
pub const RETAIN_EXIT_CODE: i32 = 3;

#[derive(Debug, Parser)]
#[command(about = "Decide whether a merged pull request's head branch may be deleted")]
struct Args {
    #[command(subcommand)]
    command: BranchDeletionAdmissionCommand,
}

#[derive(Debug, Subcommand)]
enum BranchDeletionAdmissionCommand {
    /// Collect live subjects for a merged pull request, evaluate them, and
    /// report the typed outcome.
    ///
    /// Read-only: it issues `git ls-remote`, `git remote get-url`,
    /// `git worktree list` and `gh` reads, and mutates nothing. Exits
    /// `RETAIN_EXIT_CODE` unless deletion is admitted, so a shell caller can
    /// gate cleanup on the exit status alone.
    Plan {
        /// The merged parent pull request whose head branch is the subject.
        #[arg(long)]
        pr: u64,

        /// Git remote the deletion would target.
        #[arg(long, default_value = "origin")]
        remote: String,

        /// Emit the complete typed outcome as JSON instead of one human line.
        #[arg(long)]
        json: bool,
    },

    /// Evaluate one admission request and report the typed outcome.
    Admit {
        /// Path to the JSON admission request, or `-` to read stdin.
        #[arg(long, default_value = "-")]
        request: PathBuf,

        /// Emit the complete typed outcome as JSON instead of one human line.
        #[arg(long)]
        json: bool,
    },
}

pub fn run_from_env() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    run(args)
}

fn run(args: Args) -> Result<()> {
    match args.command {
        BranchDeletionAdmissionCommand::Plan { pr, remote, json } => {
            let commands = SystemCommands;
            let request = collect_request(&commands, pr, &remote)?;
            let outcome = evaluate(&request);

            // The verification command is not merely named here: for an
            // admitted outcome it is actually run, so a caller pointed at a
            // different repository fails before any deletion is emitted.
            if outcome.admission.admits_deletion() {
                verify_remote_identity(&commands, &remote, &outcome.repository)?;
            }

            emit(&outcome, json)?;
            if !outcome.admission.admits_deletion() {
                std::process::exit(RETAIN_EXIT_CODE);
            }
            Ok(())
        }
        BranchDeletionAdmissionCommand::Admit { request, json } => {
            let raw = read_request(&request)?;
            let parsed: AdmissionRequest = serde_json::from_str(&raw)
                .wrap_err("parsing the branch-deletion admission request")?;
            let outcome = evaluate(&parsed);

            emit(&outcome, json)?;
            if !outcome.admission.admits_deletion() {
                std::process::exit(RETAIN_EXIT_CODE);
            }
            Ok(())
        }
    }
}

/// Render an outcome to stdout in the caller's chosen shape.
fn emit(outcome: &AdmissionOutcome, json: bool) -> Result<()> {
    let rendered = if json {
        // Envelope so a JSON consumer gets the leased command too, rather
        // than composing an unleased deletion of its own.
        let envelope = serde_json::json!({
            "outcome": outcome,
            "deletion_command": branch_deletion_command(outcome),
        });
        format!(
            "{}\n",
            serde_json::to_string_pretty(&envelope)
                .wrap_err("serializing the admission outcome")?
        )
    } else {
        format!("{}\n", render_disposition(outcome))
    };

    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(rendered.as_bytes()).wrap_err("writing the admission outcome")?;
    handle.flush().wrap_err("flushing the admission outcome")?;
    Ok(())
}

fn read_request(path: &PathBuf) -> Result<String> {
    if path.as_os_str() == "-" {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer).wrap_err("reading the request from stdin")?;
        return Ok(buffer);
    }

    std::fs::read_to_string(path)
        .wrap_err_with(|| format!("reading the request from {}", path.display()))
}
