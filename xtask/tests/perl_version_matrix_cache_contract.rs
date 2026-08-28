//! Contract for the Perl compatibility matrix's restore-only PR cache boundary.

use std::{fs, path::PathBuf};

use serde_yaml_ng::Value;

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

#[test]
fn perl_matrix_uses_default_branch_cache_producers()
-> Result<(), Box<dyn std::error::Error>> {
    let workflow_path = project_root().join(".github/workflows/perl-version-matrix.yml");
    let source = fs::read_to_string(&workflow_path)?;
    let workflow: Value = serde_yaml_ng::from_str(&source)?;
    let job = workflow
        .get("jobs")
        .and_then(|jobs| jobs.get("perl-version-matrix"))
        .ok_or("perl-version-matrix.yml must declare the perl-version-matrix job")?;
    let steps = job
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or("the Perl version matrix job must declare steps")?;

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
        "the Perl version matrix must have one canonical rust-cache step"
    );

    let cache = cache_steps[0];
    assert_eq!(
        cache.get("if").and_then(Value::as_str),
        Some("matrix.run_rust_smoke"),
        "only matrix variants that run Rust smoke proof should allocate the Rust cache"
    );
    let cache_with = cache
        .get("with")
        .and_then(Value::as_mapping)
        .ok_or("the Perl version matrix rust-cache step must declare inputs")?;

    assert_eq!(
        cache_with.get("cache-on-failure"),
        Some(&Value::Bool(true)),
        "failed smoke lanes should retain reusable dependency state on canonical producers"
    );
    assert_eq!(
        cache_with.get("cache-all-crates"),
        Some(&Value::Bool(true)),
        "the matrix cache should cover the workspace crates exercised by the smoke proof"
    );
    assert_eq!(
        cache_with.get("shared-key").and_then(Value::as_str),
        Some("perl-matrix-smoke-${{ matrix.perl }}"),
        "the cache namespace must remain stable while separating Perl variants"
    );
    assert!(
        cache_with.get("key").is_none(),
        "the action already keys on Cargo.lock; a second lock hash blocks restore fallback"
    );
    assert_eq!(
        cache_with.get("save-if").and_then(Value::as_str),
        Some(
            "${{ github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main' }}"
        ),
        "PR matrix runs must restore only while default-branch schedule/manual runs may save"
    );

    Ok(())
}
