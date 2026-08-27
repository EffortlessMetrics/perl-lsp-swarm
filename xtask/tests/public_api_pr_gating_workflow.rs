//! Contract tests for #2266 public-API/semver PR-time auto-selection.
//!
//! The nightly-only, label-gated compatibility rails must also select
//! themselves on pull requests whose diff touches a published facade surface,
//! while out-of-scope diffs settle as green scoped-noops rather than skipped
//! contexts.

use std::fs;
use std::path::{Path, PathBuf};

fn project_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn read(root: &Path, rel: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(root.join(rel))?)
}

fn job_section<'a>(workflow: &'a str, job_id: &str) -> Option<&'a str> {
    let start = workflow.find(&format!("\n  {job_id}:"))?;
    let tail = &workflow[start..];
    let end = tail[1..].find("\n  ").map(|i| i + 1).unwrap_or(tail.len());
    Some(&tail[..end])
}

#[test]
fn ci_yml_runs_both_compatibility_rails_on_pull_requests() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let workflow = read(&root, ".github/workflows/ci.yml")?;

    let public_api = job_section(&workflow, "public-api-pr")
        .ok_or("ci.yml must define the public-api-pr rail (#2266)")?;
    let semver = job_section(&workflow, "semver-pr")
        .ok_or("ci.yml must define the advisory semver-pr rail (#2266)")?;

    for section in [public_api, semver] {
        assert!(
            section.contains("needs.draft-pr-check.outputs.api_scope"),
            "both rails must consume the draft-pr-check api_scope selector"
        );
        assert!(
            section.contains("github.event_name == 'pull_request'"),
            "rails are PR-scoped; schedule/manual coverage stays on ci-nightly.yml"
        );
        assert!(
            section.contains("needs.preflight-latest-check.outputs.is_latest == 'true'"),
            "rails must respect superseded-SHA skipping like the other merge-gate jobs"
        );
        assert!(
            section.contains(".ci/public-api-baselines"),
            "baseline-set edits are part of the API surface",
        );
    }

    assert!(
        public_api.contains("just public-api-check"),
        "public-api-pr must reuse the canonical baseline ratchet recipe"
    );
    assert!(
        !public_api.contains("continue-on-error"),
        "public-api-pr is the hard ratchet and must propagate breakage"
    );
    assert!(
        semver.contains("continue-on-error: true"),
        "semver-pr stays advisory until #2266's confidence window closes"
    );
    assert!(
        semver.contains("cargo-semver-checks --version 0.47.0 --locked"),
        "semver-pr must pin the same cargo-semver-checks version as the nightly lane"
    );

    Ok(())
}

#[test]
fn scope_selection_is_job_level_and_never_label_gated() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = read(&root, ".github/workflows/ci.yml")?;

    // House rule (#2914, REQUIRED_STYLE_SELF_FILTERED): a workflow-level
    // `paths:` filter leaves statuses unreported; selection stays inside jobs.
    assert!(
        !workflow.contains("\n    paths:"),
        "ci.yml must not gain a workflow-level paths filter"
    );
    assert!(
        !workflow.contains("labels.*.name"),
        "ci.yml excludes the labeled event on purpose; labels must not gate its jobs"
    );

    let trigger_facade_paths = [
        "crates/perl-parser/",
        "crates/perl-lexer/",
        "crates/perl-parser-core/",
        "crates/perl-lsp-rs/",
        "crates/perl-uri/",
        "crates/perl-dap/",
        "crates/perllsp/",
        ".ci/public-api-baselines/",
    ];
    for facade_path in trigger_facade_paths {
        assert!(
            workflow.contains(facade_path),
            "scope selector must cover {facade_path}"
        );
    }
    assert!(
        workflow.contains("api_scope=true") && workflow.contains("api_scope=false"),
        "draft-pr-check must emit both scope verdicts explicitly"
    );

    Ok(())
}

#[test]
fn registry_records_the_new_advisory_contexts() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let policy = read(&root, ".ci/policies/required-checks.toml")?;

    for (context, job) in [
        ("Public API Surface (facade PR)", "public-api-pr"),
        ("Semver Compatibility (facade PR)", "semver-pr"),
    ] {
        let row_start = policy
            .find(&format!("name = \"{context}\""))
            .ok_or_else(|| format!("required-checks.toml must inventory {context}"))?;
        let row = &policy[row_start..];
        let row_end = row.find("[[checks]]").unwrap_or(row.len());
        let row = &row[..row_end];
        assert!(row.contains(&format!("job = \"{job}\"")), "{context} binds {job}");
        assert!(
            row.contains("required = false"),
            "{context} starts advisory; ruleset promotion is a separate owner act"
        );
        assert!(
            row.contains("applicability = \"always-or-scoped-noop\""),
            "{context} follows the scoped-noop applicability convention"
        );
    }

    Ok(())
}

#[test]
fn nightly_label_gates_and_baseline_ratchet_survive_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let nightly = read(&root, ".github/workflows/ci-nightly.yml")?;
    assert!(
        nightly.contains("'ci:semver'") && nightly.contains("'ci:public-api'"),
        "manual label widening remains available on the nightly lane"
    );

    let justfile = read(&root, "justfile")?;
    assert!(
        justfile.contains("public-api-check:"),
        "canonical public API baseline recipe must stay registered in the justfile"
    );

    Ok(())
}
