//! Post-merge status regeneration validation tests.
//!
//! Tests for issue #2801: split monolithic CURRENT_STATUS.md into modular status files.
//! Tests for issue #2296: infra: centralize CURRENT_STATUS.md rendering (post-merge regeneration).
//!
//! Validates:
//! - The `policy_checks` gate no longer blocks PRs with a `--check` on CURRENT_STATUS.md.
//! - A post-merge workflow exists to auto-regenerate the status subsystem files.
//! - The GATE_REGISTRY.toml policy gate command does not require a freshness check.
//! - The modular structure (4 generated files + stable stub) is correctly wired.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde_yaml_ng::Value;
use toml::Value as TomlValue;

fn required_workflows(policy: &TomlValue) -> BTreeSet<String> {
    ["check", "checks"]
        .into_iter()
        .filter_map(|table| policy.get(table).and_then(TomlValue::as_array))
        .flatten()
        .filter(|check| check.get("required").and_then(TomlValue::as_bool) == Some(true))
        .filter_map(|check| check.get("workflow").and_then(TomlValue::as_str))
        .filter_map(|workflow| workflow.rsplit('/').next())
        .map(str::to_owned)
        .collect()
}

fn workflow_dispatch_trigger(workflow: &Value) -> bool {
    let Some(triggers) = workflow.as_mapping().and_then(|mapping| {
        mapping.iter().find_map(|(key, value)| match key {
            Value::String(key) if key == "on" => Some(value),
            Value::Bool(true) => Some(value),
            _ => None,
        })
    }) else {
        return false;
    };

    match triggers {
        Value::Mapping(mapping) => {
            mapping.keys().any(|key| key.as_str() == Some("workflow_dispatch"))
        }
        Value::Sequence(events) => {
            events.iter().any(|event| event.as_str() == Some("workflow_dispatch"))
        }
        Value::String(event) => event == "workflow_dispatch",
        _ => false,
    }
}

#[cfg(unix)]
fn assert_dispatch_loop_behavior(
    dispatch_run: &str,
    dispatch_order: &[String],
    branch: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::process::{Command, Output};

    let temp_dir = tempfile::tempdir()?;
    let stub_dir = temp_dir.path().join("bin");
    fs::create_dir(&stub_dir)?;
    let stub_gh = stub_dir.join("gh");
    fs::write(
        &stub_gh,
        "#!/usr/bin/env bash\n\
         printf '%s|%s|%s|%s|%s\\n' \"$1\" \"$2\" \"$3\" \"$4\" \"$5\" >> \"$GH_LOG\"\n\
         if [ \"$#\" -ne 5 ]; then exit 2; fi\n\
         if [ \"${FAIL_WORKFLOW:-}\" = \"$3\" ]; then exit 1; fi\n",
    )?;
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(&stub_gh)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&stub_gh, permissions)?;
    let simulation_run =
        dispatch_run.replacen("gh workflow run", &format!("{} workflow run", stub_gh.display()), 1);
    assert_ne!(simulation_run, dispatch_run, "dispatch step must invoke gh workflow run");

    let run_dispatch = |fail_workflow: Option<&str>, log_name: &str| {
        let log_path = temp_dir.path().join(log_name);
        let existing_path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(stub_dir.clone()).chain(std::env::split_paths(&existing_path)),
        )?;
        let mut command = Command::new("/bin/bash");
        command
            .arg("-c")
            .arg(&simulation_run)
            .env("PATH", path)
            .env("BRANCH", branch)
            .env("GH_LOG", &log_path);
        if let Some(fail_workflow) = fail_workflow {
            command.env("FAIL_WORKFLOW", fail_workflow);
        } else {
            command.env_remove("FAIL_WORKFLOW");
        }
        let output: Output = command.output().map_err(|error| {
            format!("failed to execute dispatch shell for {}: {error}", log_path.display())
        })?;
        let calls = fs::read_to_string(&log_path)
            .map_err(|error| {
                format!(
                    "failed to read {}: {error}; stdout={}; stderr={}",
                    log_path.display(),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )
            })?
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        Ok::<_, Box<dyn std::error::Error>>((output, calls))
    };

    let (success_output, success_calls) = run_dispatch(None, "all-success.log")?;
    assert!(
        success_output.status.success(),
        "all-success dispatch run failed: {}",
        String::from_utf8_lossy(&success_output.stderr)
    );
    let expected_calls = dispatch_order
        .iter()
        .map(|workflow| format!("workflow|run|{workflow}|--ref|{branch}"))
        .collect::<Vec<_>>();
    assert_eq!(success_calls, expected_calls, "all required dispatches must run in workflow order");

    let middle_workflow = dispatch_order
        .get(dispatch_order.len() / 2)
        .ok_or("required workflow set must not be empty")?;
    let (failure_output, failure_calls) =
        run_dispatch(Some(middle_workflow), "middle-failure.log")?;
    assert!(!failure_output.status.success(), "a failed dispatch must fail the step");
    assert_eq!(
        failure_calls, expected_calls,
        "a middle dispatch failure must not skip later required workflows"
    );

    Ok(())
}

fn project_root() -> PathBuf {
    // Walk up from the manifest directory to the workspace root.
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // xtask is at <root>/xtask -- go up one level
    dir.pop();
    dir
}

fn assert_marker_count(
    content: &str,
    target_file: &str,
    marker_name: &str,
    expected_count: usize,
    marker_kind: &str,
    marker_text: &str,
) {
    let actual_count = content.matches(marker_text).count();
    assert!(
        actual_count == expected_count,
        "status marker contract violation: {marker_kind} marker `{marker_name}` in `{target_file}` expected {expected_count} occurrence(s), found {actual_count}.\n\
         expected {marker_kind} string: {marker_text}"
    );
}

/// The `policy_checks` gate in gate-policy.yaml must not run
/// `update-current-status.py --check` as part of a PR merge gate.
/// That check is now handled post-merge by the dedicated workflow.
#[test]
fn test_policy_checks_gate_does_not_block_on_stale_status() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root();
    let gate_policy_path = root.join(".ci/gate-policy.yaml");
    let content = fs::read_to_string(&gate_policy_path)?;

    // Find the policy_checks block
    let policy_block_start = content
        .find("name: policy_checks")
        .ok_or("policy_checks gate must exist in gate-policy.yaml")?;

    // Extract the policy_checks gate section (up to next gate entry or end of file)
    let policy_section = &content[policy_block_start..];
    let section_end =
        policy_section[1..].find("\n  - name:").map(|i| i + 1).unwrap_or(policy_section.len());
    let policy_section = &policy_section[..section_end];

    // The --check on update-current-status.py must NOT appear in the policy_checks gate.
    // Stale CURRENT_STATUS.md is regenerated post-merge, not blocked in PRs.
    assert!(
        !policy_section.contains("update-current-status.py --check"),
        "policy_checks gate must not run `update-current-status.py --check`.\n\
         This check causes PR merge conflicts. Regeneration is now post-merge.\n\
         Found in gate-policy.yaml policy_checks section:\n{}",
        policy_section
    );
    Ok(())
}

/// The GATE_REGISTRY.toml policy gate must not require CURRENT_STATUS.md freshness check.
#[test]
fn test_gate_registry_policy_does_not_require_status_check()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let registry_path = root.join(".ci/GATE_REGISTRY.toml");
    let content = fs::read_to_string(&registry_path)?;

    // Find the policy gate section
    let policy_start =
        content.find("id = \"policy\"").ok_or("policy gate must exist in GATE_REGISTRY.toml")?;

    let policy_section = &content[policy_start..];
    let section_end =
        policy_section[1..].find("\n[[gate]]").map(|i| i + 1).unwrap_or(policy_section.len());
    let policy_section = &policy_section[..section_end];

    assert!(
        !policy_section.contains("update-current-status.py --check"),
        "GATE_REGISTRY.toml policy gate must not require CURRENT_STATUS.md freshness check.\n\
         Regeneration is handled post-merge, not blocked in PRs.\n\
         Found in GATE_REGISTRY.toml policy section:\n{}",
        policy_section
    );
    Ok(())
}

/// A post-merge workflow must exist that regenerates status subsystem files on push to master.
#[test]
fn test_post_merge_status_workflow_exists() {
    let root = project_root();
    let workflow_path = root.join(".github/workflows/post-merge-status.yml");

    assert!(
        workflow_path.exists(),
        "Missing post-merge status workflow at .github/workflows/post-merge-status.yml.\n\
         This workflow is required to auto-regenerate status files after merges.\n\
         See issue #2296 and issue #2801."
    );
}

/// The post-merge workflow must trigger on push to master.
#[test]
fn test_post_merge_workflow_triggers_on_push_to_master() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow_path = root.join(".github/workflows/post-merge-status.yml");
    let content = fs::read_to_string(&workflow_path)?;

    assert!(content.contains("push:"), "post-merge-status.yml must have a push trigger");
    assert!(
        content.contains("master"),
        "post-merge-status.yml push trigger must include master branch"
    );
    Ok(())
}

/// Required contexts raised on a generated PR must be represented as reachable
/// through the workflow-dispatch route used by the post-merge writer (#11731).
#[test]
fn test_required_checks_record_workflow_dispatch_route() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let policy_path = root.join(".ci/policies/required-checks.toml");
    let policy_text = fs::read_to_string(policy_path)?;
    let policy: TomlValue = toml::from_str(&policy_text)?;
    let checks = policy
        .get("checks")
        .and_then(TomlValue::as_array)
        .ok_or("required-checks.toml must declare a checks array")?;

    for required_name in ["Perl LSP Rust Small Result", "ripr+ New Gap Gate", "validate-title"] {
        let check = checks
            .iter()
            .find(|check| check.get("name").and_then(TomlValue::as_str) == Some(required_name))
            .ok_or_else(|| format!("required-checks.toml is missing `{required_name}`"))?;
        let events = check
            .get("events")
            .and_then(TomlValue::as_array)
            .ok_or_else(|| format!("`{required_name}` must declare events"))?;
        assert!(
            events.iter().any(|event| event.as_str() == Some("workflow_dispatch")),
            "`{required_name}` must record workflow_dispatch as a supported route"
        );
    }

    Ok(())
}

/// The generated-PR workflow must dispatch every workflow that owns a required
/// check and retain a failing step result when any individual dispatch fails
/// (#11731).
#[test]
fn test_post_merge_workflow_dispatches_all_required_checks()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow_path = root.join(".github/workflows/post-merge-status.yml");
    let content = fs::read_to_string(&workflow_path)?;
    let workflow: Value = serde_yaml_ng::from_str(&content)?;
    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or("post-merge-status.yml must declare jobs")?;
    let dispatch_step = jobs
        .values()
        .filter_map(|job| job.get("steps").and_then(Value::as_sequence))
        .flat_map(|steps| steps.iter())
        .find(|step| {
            step.get("name").and_then(Value::as_str) == Some("Raise CI on the generated PR")
        })
        .ok_or("post-merge-status.yml must define the generated-PR dispatch step")?;
    let dispatch_run = dispatch_step
        .get("run")
        .and_then(Value::as_str)
        .ok_or("generated-PR dispatch step must define a shell body")?;
    let dispatch_branch = dispatch_step
        .get("env")
        .and_then(Value::as_mapping)
        .and_then(|env| {
            env.iter().find_map(|(key, value)| match key {
                Value::String(key) if key == "BRANCH" => value.as_str(),
                _ => None,
            })
        })
        .ok_or("generated-PR dispatch step must define BRANCH")?;
    assert_eq!(
        dispatch_branch, "automation/post-merge-status",
        "generated-PR dispatch must target its automation branch"
    );

    let policy_path = root.join(".ci/policies/required-checks.toml");
    let policy_text = fs::read_to_string(policy_path)?;
    let policy: TomlValue = toml::from_str(&policy_text)?;
    let required_workflows = required_workflows(&policy);
    assert!(
        !required_workflows.is_empty(),
        "required-checks.toml must declare at least one required workflow"
    );

    let dispatch_start = dispatch_run
        .split_once("for workflow in")
        .map(|(_, remainder)| remainder)
        .ok_or("dispatch step must iterate over workflow names")?;
    let (dispatch_names, _) = dispatch_start
        .split_once("; do")
        .ok_or("dispatch workflow loop must use a shell `; do` delimiter")?;
    let dispatch_order: Vec<String> =
        dispatch_names.split_whitespace().map(str::to_owned).collect();
    let dispatched_workflows: BTreeSet<String> = dispatch_order.iter().cloned().collect();
    assert_eq!(
        dispatched_workflows, required_workflows,
        "generated-PR dispatch set must equal the unique workflow paths for required checks"
    );
    for workflow_name in &required_workflows {
        let workflow_path = root.join(".github/workflows").join(workflow_name);
        let workflow_text = fs::read_to_string(&workflow_path)?;
        let workflow: Value = serde_yaml_ng::from_str(&workflow_text)?;
        assert!(
            workflow_dispatch_trigger(&workflow),
            "{workflow_name} must declare an on.workflow_dispatch trigger"
        );
    }
    assert!(
        dispatch_run.contains("set +e")
            && dispatch_run.contains("failed=1")
            && dispatch_run.contains("exit \"$failed\""),
        "generated-PR dispatch step must continue after an individual failure and fail overall"
    );

    #[cfg(unix)]
    assert_dispatch_loop_behavior(dispatch_run, &dispatch_order, dispatch_branch)?;

    Ok(())
}

/// The post-merge workflow must run the update-status write command.
#[test]
fn test_post_merge_workflow_runs_status_update() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow_path = root.join(".github/workflows/post-merge-status.yml");
    let content = fs::read_to_string(&workflow_path)?;

    // The workflow must invoke either the xtask command or the python script with --write
    let runs_status_update = content.contains("update-status --write")
        || content.contains("update-current-status.py --write");

    assert!(
        runs_status_update,
        "post-merge-status.yml must run status update with --write flag.\n\
         Expected one of: `update-status --write` or `update-current-status.py --write`.\n\
         Workflow content:\n{}",
        content
    );
    Ok(())
}

/// The post-merge workflow must propose regenerated files through a reviewable
/// pull request, and must never push to the protected default branch itself.
///
/// Before #6012 this asserted the opposite — that the workflow contained a
/// literal `git commit`/`git push`. That direct push ran with `contents: write`
/// alongside repository-owned generator code, and branch protection rejected it
/// anyway. The contract is now the create-pull-request boundary.
#[test]
fn test_post_merge_workflow_proposes_pull_request() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow_path = root.join(".github/workflows/post-merge-status.yml");
    let content = fs::read_to_string(&workflow_path)?;

    assert!(
        content.contains("peter-evans/create-pull-request"),
        "post-merge-status.yml must propose regenerated status files through \
         peter-evans/create-pull-request.\n\
         Workflow content:\n{}",
        content
    );
    // Inspect only executable `run:` bodies. A whole-file substring search would
    // also match prose: this workflow's own comments discuss the direct push that
    // #6012 removed, so a future comment mentioning it would fail the test while
    // no step actually pushes.
    let workflow: Value = serde_yaml_ng::from_str(&content)?;
    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or("post-merge-status.yml must declare jobs")?;
    for (name, job) in jobs {
        let Some(steps) = job.get("steps").and_then(Value::as_sequence) else {
            continue;
        };
        for step in steps {
            let Some(run) = step.get("run").and_then(Value::as_str) else {
                continue;
            };
            assert!(
                !run.contains("git push") && !run.contains("git commit"),
                "post-merge-status.yml must not push directly to the protected \
                 default branch; generated files are proposed through a \
                 reviewable PR. Offending job `{:?}` step:\n{}",
                name,
                run
            );
        }
    }
    Ok(())
}

/// The job that executes the repository's own generator must not hold write
/// authority (issue #6012 acceptance: generation runs with `contents: read`).
#[test]
fn test_post_merge_generator_job_is_read_only() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow_path = root.join(".github/workflows/post-merge-status.yml");
    let content = fs::read_to_string(&workflow_path)?;

    let workflow: Value = serde_yaml_ng::from_str(&content)?;
    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or("post-merge-status.yml must declare jobs")?;

    // Locate the job that actually runs the repository's own generator, rather
    // than trusting a job name that a later edit could rename.
    let (_name, generator) = jobs
        .iter()
        .find(|(_, job)| {
            job.get("steps").and_then(Value::as_sequence).is_some_and(|steps| {
                steps.iter().any(|step| {
                    step.get("run")
                        .and_then(Value::as_str)
                        .is_some_and(|run| run.contains("update-status --write"))
                })
            })
        })
        .ok_or("no job in post-merge-status.yml runs `update-status --write`")?;

    let contents = generator
        .get("permissions")
        .and_then(|perms| perms.get("contents"))
        .and_then(Value::as_str)
        .ok_or("the generating job must declare an explicit `permissions.contents`")?;
    assert_eq!(
        contents, "read",
        "the job running `update-status --write` must hold only `contents: read`"
    );

    let steps = generator
        .get("steps")
        .and_then(Value::as_sequence)
        .ok_or("the generating job must declare steps")?;
    let checkout = steps
        .iter()
        .find(|step| {
            step.get("uses")
                .and_then(Value::as_str)
                .is_some_and(|uses| uses.starts_with("actions/checkout@"))
        })
        .ok_or("the generating job must check out the repository")?;
    assert_eq!(
        checkout.get("with").and_then(|with| with.get("persist-credentials")),
        Some(&Value::Bool(false)),
        "the generating checkout must set `persist-credentials: false`"
    );
    Ok(())
}

/// The post-merge workflow must commit files in docs/project/status/ — NOT CURRENT_STATUS.md alone.
#[test]
fn test_post_merge_workflow_commits_status_directory() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow_path = root.join(".github/workflows/post-merge-status.yml");

    let content = fs::read_to_string(&workflow_path)
        .map_err(|e| format!("post-merge-status.yml should be readable: {e}"))?;

    // The workflow must add files from the status/ subdirectory.
    assert!(
        content.contains("docs/project/status/"),
        "post-merge-status.yml must commit files in docs/project/status/ (modular structure).\n\
         After issue #2801, CURRENT_STATUS.md is a stable stub — status/ files are generated.\n\
         Workflow content:\n{}",
        content
    );
    Ok(())
}

/// The post-merge workflow must NOT git-add CURRENT_STATUS.md (it is now human-owned stable stub).
#[test]
fn test_post_merge_workflow_does_not_add_current_status() -> Result<(), Box<dyn std::error::Error>>
{
    let root = project_root();
    let workflow_path = root.join(".github/workflows/post-merge-status.yml");

    let content = fs::read_to_string(&workflow_path)
        .map_err(|e| format!("post-merge-status.yml should be readable: {e}"))?;

    // The workflow must not try to commit CURRENT_STATUS.md — it's a stable human-owned stub now.
    assert!(
        !content.contains("git add docs/project/CURRENT_STATUS.md"),
        "post-merge-status.yml must not git-add CURRENT_STATUS.md.\n\
         After issue #2801, CURRENT_STATUS.md is a stable human-owned stub.\n\
         Generated metrics are in docs/project/status/*.md\n\
         Workflow content:\n{}",
        content
    );
    Ok(())
}

/// The stub CURRENT_STATUS.md must not contain any <!-- BEGIN: --> markers.
/// If it does, `xtask update-status --check` will try to patch it and fail.
#[test]
fn test_stub_current_status_has_no_begin_markers() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let stub_path = root.join("docs/project/CURRENT_STATUS.md");

    let content = fs::read_to_string(&stub_path)
        .map_err(|e| format!("docs/project/CURRENT_STATUS.md should exist as a stub: {e}"))?;

    assert!(
        !content.contains("<!-- BEGIN:"),
        "CURRENT_STATUS.md must not contain <!-- BEGIN: --> markers.\n\
         It is now a stable stub — generated content belongs in docs/project/status/*.md\n\
         Remove all <!-- BEGIN: ... --> blocks from CURRENT_STATUS.md."
    );
    Ok(())
}

/// All subsystem status files must contain their expected marker blocks.
///
/// This is an integration-level gate: if any marker is missing the xtask
/// `update-status` command will fail with "Expected 1 match ... got 0".
#[test]
fn test_subsystem_files_have_markers() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let status_dir = root.join("docs/project/status");

    let lsp = fs::read_to_string(status_dir.join("lsp.md"))?;
    assert!(lsp.contains("<!-- BEGIN: LSP_COVERAGE -->"), "lsp.md missing LSP_COVERAGE block");
    assert!(
        lsp.contains("<!-- BEGIN: LSP_METRICS_BULLETS -->"),
        "lsp.md missing LSP_METRICS_BULLETS block"
    );
    assert!(
        lsp.contains("<!-- BEGIN: COMPLIANCE_TABLE -->"),
        "lsp.md missing COMPLIANCE_TABLE block"
    );

    let tests_md = fs::read_to_string(status_dir.join("tests.md"))?;
    assert!(
        tests_md.contains("<!-- BEGIN: TESTS_TABLE_ROWS -->"),
        "tests.md missing TESTS_TABLE_ROWS block"
    );
    assert!(
        tests_md.contains("<!-- BEGIN: TESTS_METRICS_BULLETS -->"),
        "tests.md missing TESTS_METRICS_BULLETS block"
    );

    let parser = fs::read_to_string(status_dir.join("parser.md"))?;
    assert!(
        parser.contains("<!-- BEGIN: PARSER_TRACKING_TABLE -->"),
        "parser.md missing PARSER_TRACKING_TABLE block"
    );
    assert!(
        parser.contains("<!-- BEGIN: PARSER_METRICS_BULLETS -->"),
        "parser.md missing PARSER_METRICS_BULLETS block"
    );
    assert!(
        parser.contains("<!-- BEGIN: PARSER_NODEKIND_ROW -->"),
        "parser.md missing PARSER_NODEKIND_ROW block"
    );
    assert!(
        parser.contains("<!-- BEGIN: PARSER_RELIABILITY_ROW -->"),
        "parser.md missing PARSER_RELIABILITY_ROW block"
    );
    assert!(
        parser.contains("<!-- BEGIN: PARSER_STRICT_CLEAN_ROW -->"),
        "parser.md missing PARSER_STRICT_CLEAN_ROW block"
    );
    assert!(
        parser.contains("<!-- BEGIN: PARSER_PERFORMANCE_TABLE -->"),
        "parser.md missing PARSER_PERFORMANCE_TABLE block"
    );

    let quality = fs::read_to_string(status_dir.join("quality.md"))?;
    assert!(
        quality.contains("<!-- BEGIN: QUALITY_METRICS_BULLETS -->"),
        "quality.md missing QUALITY_METRICS_BULLETS block"
    );
    assert!(
        quality.contains("<!-- BEGIN: QUALITY_CRATE_TABLE -->"),
        "quality.md missing QUALITY_CRATE_TABLE block"
    );

    let dap = fs::read_to_string(status_dir.join("dap.md"))?;
    assert!(
        dap.contains("<!-- BEGIN: DAP_TEST_COUNTS -->"),
        "dap.md missing DAP_TEST_COUNTS block"
    );

    let workspace_md = fs::read_to_string(status_dir.join("workspace.md"))?;
    assert!(
        workspace_md.contains("<!-- BEGIN: WORKSPACE_STALE_RATE -->"),
        "workspace.md missing WORKSPACE_STALE_RATE block"
    );
    assert!(
        workspace_md.contains("<!-- BEGIN: WORKSPACE_SLO_TABLE -->"),
        "workspace.md missing WORKSPACE_SLO_TABLE block"
    );
    assert!(
        workspace_md.contains("<!-- BEGIN: WORKSPACE_MULTIROOT -->"),
        "workspace.md missing WORKSPACE_MULTIROOT block"
    );
    assert!(
        workspace_md.contains("<!-- BEGIN: WORKSPACE_FIXTURES -->"),
        "workspace.md missing WORKSPACE_FIXTURES block"
    );
    assert!(
        workspace_md.contains("<!-- BEGIN: WORKSPACE_METRICS_BULLETS -->"),
        "workspace.md missing WORKSPACE_METRICS_BULLETS block"
    );

    Ok(())
}

/// Parser status marker contract: every marker used by parser status generation
/// must appear exactly once as BEGIN and exactly once as END in parser.md.
#[test]
fn test_status_marker_parser_contract() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let parser_path = root.join("docs/project/status/parser.md");
    let parser = fs::read_to_string(&parser_path)?;
    let target_file = "docs/project/status/parser.md";
    let parser_markers = [
        "PARSER_TRACKING_TABLE",
        "PARSER_PERFORMANCE_TABLE",
        "PARSER_METRICS_BULLETS",
        "TOKEN_HEALTH_TABLE",
        "PARSER_NODEKIND_ROW",
        "PARSER_RELIABILITY_ROW",
        "PARSER_ERROR_DENSITY_ROW",
        "PARSER_RECOVERY_SALVAGE_ROW",
        "PARSER_STRICT_CLEAN_ROW",
    ];

    for marker in parser_markers {
        let begin = format!("<!-- BEGIN: {marker} -->");
        let end = format!("<!-- END: {marker} -->");
        assert_marker_count(&parser, target_file, marker, 1, "BEGIN", &begin);
        assert_marker_count(&parser, target_file, marker, 1, "END", &end);
    }

    Ok(())
}
