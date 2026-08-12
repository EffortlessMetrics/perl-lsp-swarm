//! Reject pull-request workflows whose candidate-controlled code can mutate
//! the candidate branch.
//!
//! A PR workflow may produce inert read-only evidence. A write-capable job may
//! delegate only to an immutable remote reusable workflow; local steps and
//! local reusable workflows remain candidate-controlled.

#[path = "candidate_writer_policy/model.rs"]
mod model;
#[path = "candidate_writer_policy/scan.rs"]
mod scan;

use scan::{project_root, scan_repository};
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
        Ok(findings) if findings.is_empty() => {
            println!("candidate-writer policy: no candidate-defined repository writers found");
            ExitCode::SUCCESS
        }
        Ok(findings) => {
            eprintln!(
                "candidate-writer policy: {} prohibited writer path(s)",
                findings.len()
            );
            for finding in findings {
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
