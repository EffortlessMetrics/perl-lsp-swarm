//! Contract test for the packaged-bundle journey schedule gate (#12841).
//!
//! The journey asserts observability the last published marketplace build
//! cannot have, so it must never run on the Monday marketplace schedule —
//! only for a workflow_dispatch bound to candidate identity inputs.

use std::fs;
use std::path::PathBuf;

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn workflow_step<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("- name: {name}");
    let start = content.find(&needle)?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(needle.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(
            |(offset, line)| {
                if line.trim_start().starts_with("- name:") { Some(offset) } else { None }
            },
        )
        .unwrap_or(rest.len());
    Some(&rest[..next])
}

#[test]
fn packaged_bundle_journey_runs_only_for_bound_dispatch() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let workflow =
        fs::read_to_string(root.join(".github/workflows/vscode-published-extension-smoke.yml"))?;

    let step = workflow_step(&workflow, "Run packaged bundled-server journey under Xvfb")
        .ok_or("missing packaged bundled-server journey step")?;

    assert!(
        step.contains("github.event_name == 'workflow_dispatch'"),
        "the journey must not run on the marketplace schedule: {step}"
    );
    assert!(
        step.contains("inputs.candidate_id != ''")
            && step.contains("inputs.frozen_product_sha != ''"),
        "the journey must require candidate-bound identity inputs: {step}"
    );
    assert!(workflow.contains("#12841"), "the gate must name its governing issue");

    // The ordinary published smoke steps are untouched — they still run on
    // every event including the schedule.
    let smoke_step = workflow_step(&workflow, "Run published extension smoke under Xvfb")
        .ok_or("missing published smoke step")?;
    assert!(
        smoke_step.contains("- name: Run published extension smoke under Xvfb\n        if: runner.os == 'Linux'\n        run: xvfb-run"),
        "the ordinary published smoke must keep its plain schedule-runnable gate: {smoke_step}"
    );

    Ok(())
}
