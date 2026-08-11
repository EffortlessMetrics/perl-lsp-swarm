//! Contract for the post-merge status generator's Cargo cache boundary.

use std::{fs, path::PathBuf};

use serde_yaml_ng::Value;

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

#[test]
fn status_generator_keeps_target_artifacts_out_of_the_shared_cache()
    -> Result<(), Box<dyn std::error::Error>>
{
    let workflow_path = project_root().join(".github/workflows/post-merge-status.yml");
    let source = fs::read_to_string(&workflow_path)?;
    let workflow: Value = serde_yaml_ng::from_str(&source)?;
    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or("post-merge-status.yml must declare jobs")?;

    let exact_generator_command = "cargo run -p xtask -- update-status --write";
    let generator_jobs: Vec<_> = jobs
        .iter()
        .filter(|(_, job)| {
            job.get("steps").and_then(Value::as_sequence).is_some_and(|steps| {
                steps.iter().any(|step| {
                    step.get("run").and_then(Value::as_str).map(str::trim)
                        == Some(exact_generator_command)
                })
            })
        })
        .collect();
    assert_eq!(
        generator_jobs.len(),
        1,
        "post-merge-status.yml must have exactly one job executing the status generator"
    );
    let (_job_name, generator) = generator_jobs[0];

    let steps = generator
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or("the status generator job must declare steps")?;
    assert_eq!(
        steps
            .iter()
            .filter(|step| {
                step.get("run").and_then(Value::as_str).map(str::trim)
                    == Some(exact_generator_command)
            })
            .count(),
        1,
        "the status generator job must execute the exact command once"
    );

    let cache_steps: Vec<_> = steps
        .iter()
        .filter(|step| {
            step.get("uses")
                .and_then(Value::as_str)
                .is_some_and(|uses| uses.starts_with("Swatinem/rust-cache@"))
        })
        .collect();
    assert_eq!(
        cache_steps.len(),
        1,
        "the status generator job must have one canonical rust-cache step"
    );

    let cache_with = cache_steps[0]
        .get("with")
        .and_then(Value::as_mapping)
        .ok_or("the status generator rust-cache step must declare inputs")?;
    assert_eq!(
        cache_with.get("cache-targets"),
        Some(&Value::Bool(false)),
        "post-merge status generation must not restore or save workspace target artifacts"
    );
    assert_eq!(
        cache_with.get("cache-on-failure"),
        Some(&Value::Bool(true)),
        "dependency/tool cache reuse should remain available after failed generation"
    );

    let shared_key = cache_with
        .get("shared-key")
        .and_then(Value::as_str)
        .ok_or("the status generator cache must retain a shared dependency key")?;
    assert!(
        shared_key.starts_with("post-merge-status-")
            && shared_key.contains("hashFiles('Cargo.lock')"),
        "the cache must remain scoped to post-merge status dependencies and Cargo.lock"
    );

    Ok(())
}
