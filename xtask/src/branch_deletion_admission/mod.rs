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
    DeletionExecutor, LiveCollection, ReadOnlyCommands, RemoteIdentity, SystemCommands,
    SystemDeletion, collect_request, execute_admitted_deletion, parse_remote_identity,
    repository_from_remote_url, verify_remote_identity,
};
pub use model::{
    AdmissionOutcome, AdmissionRequest, BRANCH_DELETION_ADMISSION_POLICY_VERSION,
    BRANCH_DELETION_ADMISSION_SCHEMA_VERSION, BranchSubject, DeletionAdmission, GraphCompleteness,
    Mergeability, NextOwner, ObservedPullRequest, OpenChildGraph, ParentSubject, ParentTerminality,
    PullRequestState, RepositoryId, RetainedChild, WorktreeOwnership,
};
pub use route::{
    RecheckGate, branch_deletion_command, merge_command, recheck_gate, remote_verification_command,
    render_disposition, render_snapshot_disposition,
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

    /// Collect live subjects, evaluate, and perform the admitted deletion.
    ///
    /// The only mutating entry point. It deletes exactly when `plan` would
    /// report `SAFE_TO_DELETE`, re-verifies the remote's identity immediately
    /// before deleting, and runs the leased command as argv — never through a
    /// shell — so a branch name carrying shell metacharacters cannot become a
    /// command. Exits `RETAIN_EXIT_CODE` and deletes nothing on any retaining
    /// outcome.
    Cleanup {
        /// The merged parent pull request whose head branch is the subject.
        #[arg(long)]
        pr: u64,

        /// Git remote the deletion would target.
        #[arg(long, default_value = "origin")]
        remote: String,
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
            let collected = collect_request(&commands, pr, &remote)?;
            let outcome = evaluate(&collected.request);

            // The verification command is not merely named here: for an
            // admitted outcome it is actually run, against the full identity
            // collection observed (host included), so a caller pointed at a
            // different server fails before any deletion is emitted.
            if outcome.admission.admits_deletion() {
                verify_remote_identity(&commands, &remote, &collected.remote_identity)?;
            }

            emit(&outcome, json)?;
            if !outcome.admission.admits_deletion() {
                std::process::exit(RETAIN_EXIT_CODE);
            }
            Ok(())
        }
        BranchDeletionAdmissionCommand::Cleanup { pr, remote } => {
            let commands = SystemCommands;
            // Collect and delete in one process: the window between the graph
            // read and the deletion is as small as this design can make it.
            // It cannot be zero — see the residual on `branch_deletion_command`.
            let collected = collect_request(&commands, pr, &remote)?;
            let outcome = evaluate(&collected.request);
            emit(&outcome, false)?;

            if !outcome.admission.admits_deletion() {
                std::process::exit(RETAIN_EXIT_CODE);
            }

            // Re-read every live subject immediately before deleting, after the
            // reporting above has done its I/O. The first read authorized; this
            // one is what the deletion actually stands on, so a child opened, a
            // tip moved, a remote repointed or a worktree claimed in between
            // retains instead of being deleted.
            //
            // This NARROWS the window to the gap between this read and the push;
            // it does not close it. A child opened inside that gap is still
            // auto-closed — the residual documented on `branch_deletion_command`,
            // which needs an integration lock or deferred deletion to remove.
            //
            // The decision between the two reads is `recheck_gate`, kept pure
            // so it is falsifiable without a live graph; this arm only supplies
            // the reads and routes its verdict.
            let recollected = collect_request(&commands, pr, &remote)?;
            let recheck = evaluate(&recollected.request);
            if let RecheckGate::Retain { detail } = recheck_gate(&outcome, &recheck) {
                write_err(&format!("branch-deletion-admission: retaining — {detail}\n"))?;
                std::process::exit(RETAIN_EXIT_CODE);
            }

            execute_admitted_deletion(
                &commands,
                &SystemDeletion,
                &recheck,
                &recollected.remote_identity,
            )?;
            Ok(())
        }
        BranchDeletionAdmissionCommand::Admit { request, json } => {
            let raw = read_request(&request)?;
            let parsed: AdmissionRequest = serde_json::from_str(&raw)
                .wrap_err("parsing the branch-deletion admission request")?;
            let outcome = evaluate(&parsed);

            // Snapshot evaluation, not authorization: this request came from
            // the caller, so a structurally valid but forged one could reach
            // SAFE_TO_DELETE. Nothing runnable is emitted, and the deletion
            // paths (`plan`/`cleanup`) read the live subjects themselves.
            emit_snapshot(&outcome, json)?;
            if !outcome.admission.admits_deletion() {
                std::process::exit(RETAIN_EXIT_CODE);
            }
            Ok(())
        }
    }
}

/// Render a snapshot-derived outcome, with nothing runnable attached.
fn emit_snapshot(outcome: &AdmissionOutcome, json: bool) -> Result<()> {
    let rendered = if json {
        let envelope = serde_json::json!({
            "outcome": outcome,
            "authorizing": false,
            "note": "snapshot evaluation; authorization requires `plan` or `cleanup`",
        });
        format!(
            "{}\n",
            serde_json::to_string_pretty(&envelope)
                .wrap_err("serializing the admission outcome")?
        )
    } else {
        format!("{}\n", render_snapshot_disposition(outcome))
    };
    write_out(&rendered)
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

    write_out(&rendered)
}

fn write_out(rendered: &str) -> Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(rendered.as_bytes()).wrap_err("writing the admission outcome")?;
    handle.flush().wrap_err("flushing the admission outcome")?;
    Ok(())
}

/// Report a retention on stderr, keeping stdout reserved for the outcome a
/// caller parses.
///
/// This is the stderr sibling of `write_out` rather than `eprintln!`, which
/// the workspace's `clippy::print_stderr` lint denies in library code — a
/// library must not decide where a process's diagnostics land.
fn write_err(rendered: &str) -> Result<()> {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    handle.write_all(rendered.as_bytes()).wrap_err("writing the retention reason")?;
    handle.flush().wrap_err("flushing the retention reason")?;
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
