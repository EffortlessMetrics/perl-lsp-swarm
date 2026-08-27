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
    // Job entries sit at exactly two-space indent; nested job fields and step
    // keys live deeper, so boundaries are the following 2-space "key:" line.
    let anchor = format!("\n  {job_id}:");
    let start = workflow.find(&anchor)?;
    let body_start = start + anchor.len();
    let rest = &workflow[body_start..];
    let mut end = rest.len();
    for (offset, line) in rest.split('\n').enumerate() {
        if offset == 0 {
            continue;
        }
        let nested = line.starts_with("  ")
            && !line.starts_with("   ")
            && line
                .trim_end()
                .chars()
                .nth(2)
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if nested {
            end = rest[..].split('\n').take(offset).map(|l| l.len() + 1).sum();
            break;
        }
    }
    Some(&workflow[start..body_start + end])
}

fn pull_request_label_gate(section: &str) -> Option<&str> {
    let anchor = "contains(github.event.pull_request.labels.*.name, '";
    let job_indent = section.lines().find_map(|line| {
        let trimmed = line.trim_start();
        (!trimmed.is_empty() && !trimmed.starts_with('#')).then_some(line.len() - trimmed.len())
    })?;
    let field_indent = job_indent.saturating_add(2);
    let mut lines = section.lines();
    let if_line = lines.find(|line| {
        let trimmed = line.trim_start();
        line.len() - trimmed.len() == field_indent && trimmed.starts_with("if:")
    })?;
    let mut is_header = true;

    for line in std::iter::once(if_line).chain(lines) {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if !is_header && !trimmed.is_empty() && indent <= field_indent {
            break;
        }
        is_header = false;
        if trimmed.starts_with('#') {
            continue;
        }

        let expression = trimmed.strip_prefix("if:").map_or(trimmed, str::trim);
        let active_expression = expression.split_once('#').map_or(expression, |(active, _)| active);
        if let Some((_, rest)) = active_expression.split_once(anchor) {
            return Some(rest.split_once('\'')?.0);
        }
    }

    None
}

#[test]
fn pull_request_label_gate_ignores_commented_examples() {
    let section = r#"
  public-api-check:
    # Legacy example: contains(github.event.pull_request.labels.*.name, 'ci:public-api')
    if: |
      github.event_name == 'pull_request' &&
      contains(github.event.pull_request.labels.*.name, 'ci:nonexistent')
    steps: []
"#;

    assert_eq!(pull_request_label_gate(section), Some("ci:nonexistent"));
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
        assert!(workflow.contains(facade_path), "scope selector must cover {facade_path}");
    }
    assert!(
        workflow.contains("api_scope=true") && workflow.contains("api_scope=false"),
        "draft-pr-check must emit both scope verdicts explicitly"
    );
    assert!(
        workflow.contains("/pulls/{pr_number}/files"),
        "scope must derive from the pull request's own file set, not the two-dot base diff"
    );
    assert!(
        workflow.contains("using changed-file fallback"),
        "API probing must fall back to the local diff scan instead of failing open"
    );
    assert!(
        workflow.contains("file=sys.stderr"),
        "API probing diagnostics must stay off stdout so api_scope remains a scalar output"
    );
    // Re-export closure (P2 on #12850): crates whose public items flow into a
    // facade baseline via re-export must select the rails too, and the
    // committed baselines are the derivation authority rather than a second
    // hand-written path list.
    assert!(
        workflow.contains("derive_prefixes"),
        "scope selector must derive re-export closure from the committed baselines"
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
        // Contract-model classification: both rails are preconditioned jobs
        // (event + preflight + run_ci), so applicability is "conditional" per
        // validate_gate_enforcement_contract.py; scoped-noop green settlement
        // happens at the step level inside each job.
        assert!(
            row.contains("applicability = \"conditional\""),
            "{context} follows the prerequisite-selected applicability convention"
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

#[test]
fn nightly_public_api_label_is_governed_and_provisioned() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root()?;
    let nightly = read(&root, ".github/workflows/ci-nightly.yml")?;
    let public_api = job_section(&nightly, "public-api-check")
        .ok_or("ci-nightly.yml must define the public-api-check job")?;
    let label = pull_request_label_gate(public_api)
        .ok_or("public-api-check must expose its pull-request label gate")?;

    assert_eq!(label, "ci:public-api", "the public API lane owns one stable trigger label");

    let docs = read(&root, "docs/ci/labels.md")?;
    let governed_row = docs
        .lines()
        .find(|line| line.contains(&format!("`{label}`")))
        .ok_or_else(|| format!("{label} must be present in docs/ci/labels.md"))?;
    assert!(
        governed_row.contains("20-minute") && governed_row.contains("fail-closed"),
        "the governed row must state the lane cost cap and proof intent"
    );

    let provisioning = read(&root, "scripts/gh/ensure-labels.sh")?;
    let expected = format!("ensure \"{label}\" \"0052cc\" \"Run public API surface validation\"");
    assert!(
        provisioning.lines().any(|line| line.trim() == expected),
        "the workflow label must use the governed provisioning metadata"
    );

    Ok(())
}
