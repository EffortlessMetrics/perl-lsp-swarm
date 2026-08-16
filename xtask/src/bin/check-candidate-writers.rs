//! Reject pull-request workflows whose candidate-controlled code can mutate
//! the candidate branch.
//!
//! A PR workflow may produce inert read-only evidence. A write-capable job may
//! delegate only to an immutable remote reusable workflow; local steps and
//! local reusable workflows remain candidate-controlled.

// This is a user-facing CLI gate: its findings are its output. Matches the
// `compiler-concepts` bin convention for the workspace print lints.
#![allow(clippy::print_stdout, clippy::print_stderr)]

#[path = "candidate_writer_policy/model.rs"]
mod model;
#[path = "candidate_writer_policy/scan.rs"]
mod scan;

use scan::{is_known_unconverted, project_root, scan_repository, stale_known_writers};
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = match project_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("candidate-writer policy instrument failure: {error}");
            return ExitCode::from(2);
        }
    };
    match scan_repository(&root) {
        Ok(findings) => {
            let (known, new): (Vec<_>, Vec<_>) =
                findings.iter().partition(|finding| is_known_unconverted(finding));

            let stale = stale_known_writers(&findings);
            if !stale.is_empty() {
                eprintln!(
                    "candidate-writer policy: {} stale known-writer entr(ies) — the writer is gone, so remove the baseline entry",
                    stale.len()
                );
                for (workflow, job) in stale {
                    eprintln!("{workflow}: job `{job}` no longer matches any finding");
                }
                return ExitCode::FAILURE;
            }

            for finding in &known {
                println!("known unconverted writer (tracked by #6873): {finding}");
            }

            if new.is_empty() {
                println!(
                    "candidate-writer policy: no new candidate-defined repository writers ({} known, tracked)",
                    known.len()
                );
                return ExitCode::SUCCESS;
            }

            eprintln!("candidate-writer policy: {} prohibited writer path(s)", new.len());
            for finding in new {
                eprintln!("{finding}");
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("candidate-writer policy instrument failure: {error}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
#[path = "candidate_writer_policy/tests.rs"]
mod tests;
