//! Reject pull-request workflows whose candidate-controlled code can mutate
//! the candidate branch.
//!
//! A PR workflow may produce inert read-only evidence. A write-capable job may
//! delegate only to an immutable remote reusable workflow; local steps and
//! local reusable workflows remain candidate-controlled.

// Policy instrument binary — the finding report is this tool's interface.
#![allow(clippy::print_stderr, clippy::print_stdout)]

#[path = "candidate_writer_policy/model.rs"]
mod model;
#[path = "candidate_writer_policy/scan.rs"]
mod scan;

use model::{KNOWN_INCIDENTS, partition_incidents};
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
    let findings = match scan_repository(&root) {
        Ok(findings) => findings,
        Err(error) => {
            eprintln!("candidate-writer policy instrument failure: {error}");
            return ExitCode::from(2);
        }
    };

    let partition = partition_incidents(&findings);
    let mut rejected = false;

    if partition.new_findings.is_empty() {
        println!("candidate-writer policy: no new candidate-defined repository writers found");
    } else {
        rejected = true;
        eprintln!(
            "candidate-writer policy: {} prohibited writer path(s)",
            partition.new_findings.len()
        );
        for finding in &partition.new_findings {
            eprintln!("{finding}");
        }
    }

    for incident in &partition.stale {
        rejected = true;
        eprintln!(
            "candidate-writer policy: recorded incident {}: job `{}` no longer reproduces; remove it from KNOWN_INCIDENTS ({})",
            incident.workflow, incident.job, incident.owning_issue
        );
    }

    if !rejected {
        for incident in KNOWN_INCIDENTS {
            println!(
                "candidate-writer policy: open incident {}: job `{}` accepted under {} ({})",
                incident.workflow, incident.job, incident.owning_issue, incident.note
            );
        }
    }

    if rejected { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

#[cfg(test)]
#[path = "candidate_writer_policy/tests.rs"]
mod tests;
