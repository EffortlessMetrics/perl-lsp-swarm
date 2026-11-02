//! Contract tests for the lane-history generator's "Raise CI on the
//! generated PR" step in `.github/workflows/aggregate-ci-lane-history.yml`
//! (#15100).
//!
//! ci.yml's `workflow_dispatch` declares required `base_sha`/`head_sha`
//! inputs (#13019). The lane-history producer dispatched it bare, every
//! scheduled run failed with HTTP 422, and the generated refresh PR's
//! required checks never populated. These tests pin the repaired contract:
//! the step resolves the created PR's recorded base/head identity, refuses
//! to prove a moved subject, passes both SHAs to ci.yml only, keeps the
//! other dispatches bare and independent, and fails the step when any
//! lookup or dispatch fails.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde_yaml_ng::Value;
use toml::Value as TomlValue;

#[path = "support/external_command.rs"]
mod external_command;

const WORKFLOW_PATH: &str = ".github/workflows/aggregate-ci-lane-history.yml";
const DISPATCH_STEP_NAME: &str = "Raise CI on the generated PR";
const DISPATCH_BRANCH: &str = "automation/ci-lane-history";
const BASE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const HEAD_SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn project_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    dir
}

fn workflow_dispatch_trigger(workflow: &Value) -> bool {
    let triggers = workflow.as_mapping().and_then(|mapping| {
        mapping.iter().find_map(|(key, value)| match key {
            Value::String(key) if key == "on" => Some(value),
            Value::Bool(true) => Some(value),
            _ => None,
        })
    });

    match triggers {
        Some(Value::Mapping(mapping)) => {
            mapping.keys().any(|key| key.as_str() == Some("workflow_dispatch"))
        }
        Some(Value::Sequence(events)) => {
            events.iter().any(|event| event.as_str() == Some("workflow_dispatch"))
        }
        Some(Value::String(event)) => event == "workflow_dispatch",
        _ => false,
    }
}

fn required_workflows(policy: &TomlValue) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let mut workflows = BTreeSet::new();
    for check in ["check", "checks"]
        .into_iter()
        .filter_map(|table| policy.get(table).and_then(TomlValue::as_array))
        .flatten()
        .filter(|check| check.get("required").and_then(TomlValue::as_bool) == Some(true))
    {
        let name =
            check.get("name").and_then(TomlValue::as_str).unwrap_or("<unnamed required check>");
        let workflow = check
            .get("workflow")
            .and_then(TomlValue::as_str)
            .ok_or_else(|| format!("required check `{name}` must declare a workflow path"))?;
        let workflow_name = workflow
            .rsplit('/')
            .next()
            .filter(|workflow_name| !workflow_name.is_empty())
            .ok_or_else(|| format!("required check `{name}` has an empty workflow path"))?;
        workflows.insert(workflow_name.to_owned());
    }
    Ok(workflows)
}

struct DispatchStep {
    run: String,
    env: Vec<(String, String)>,
}

fn dispatch_step() -> Result<DispatchStep, Box<dyn std::error::Error>> {
    let workflow_path = project_root().join(WORKFLOW_PATH);
    let content = fs::read_to_string(&workflow_path)?;
    let workflow: Value = serde_yaml_ng::from_str(&content)?;
    let jobs = workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or("aggregate-ci-lane-history.yml must declare jobs")?;
    let step = jobs
        .values()
        .filter_map(|job| job.get("steps").and_then(Value::as_sequence))
        .flat_map(|steps| steps.iter())
        .find(|step| step.get("name").and_then(Value::as_str) == Some(DISPATCH_STEP_NAME))
        .ok_or("aggregate-ci-lane-history.yml must define the generated-PR dispatch step")?;
    let run = step
        .get("run")
        .and_then(Value::as_str)
        .ok_or("generated-PR dispatch step must define a shell body")?
        .to_string();
    let env = step
        .get("env")
        .and_then(Value::as_mapping)
        .map(|mapping| {
            mapping
                .iter()
                .filter_map(|(key, value)| {
                    Some((key.as_str()?.to_string(), value.as_str()?.to_string()))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(DispatchStep { run, env })
}

fn dispatch_order(run: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let dispatch_start = run
        .split_once("for workflow in")
        .map(|(_, remainder)| remainder)
        .ok_or("dispatch step must iterate over workflow names")?;
    let (dispatch_names, _) = dispatch_start
        .split_once("; do")
        .ok_or("dispatch workflow loop must use a shell `; do` delimiter")?;
    Ok(dispatch_names.split_whitespace().map(str::to_owned).collect())
}

/// The dispatch step must bind the created PR's identity from the
/// create-pull-request outputs before any dispatch is attempted (#15100).
#[test]
fn test_dispatch_step_declares_subject_identity_env() -> Result<(), Box<dyn std::error::Error>> {
    let step = dispatch_step()?;
    let env: BTreeSet<&str> = step.env.iter().map(|(key, _)| key.as_str()).collect();
    for required in ["GH_TOKEN", "BRANCH", "PR_NUMBER", "CREATED_HEAD_SHA"] {
        assert!(env.contains(required), "dispatch step must declare {required} env");
    }
    let branch = step
        .env
        .iter()
        .find(|(key, _)| key == "BRANCH")
        .map(|(_, value)| value.as_str())
        .ok_or("dispatch step must define BRANCH")?;
    assert_eq!(branch, DISPATCH_BRANCH, "dispatch must target its automation branch");
    let env_value =
        |name: &str| step.env.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str());
    assert_eq!(
        env_value("PR_NUMBER"),
        Some("${{ steps.create-pr.outputs.pull-request-number }}"),
        "PR_NUMBER must come from the create-pull-request output"
    );
    assert_eq!(
        env_value("CREATED_HEAD_SHA"),
        Some("${{ steps.create-pr.outputs.pull-request-head-sha }}"),
        "CREATED_HEAD_SHA must come from the create-pull-request head output"
    );
    Ok(())
}

/// The step must resolve the PR's recorded base/head through gh, pass both
/// to ci.yml, and refuse to dispatch ci.yml for a moved subject (#15100).
#[test]
fn test_dispatch_step_binds_ci_subject_inputs() -> Result<(), Box<dyn std::error::Error>> {
    let step = dispatch_step()?;
    assert!(
        step.run.contains("gh pr view \"$PR_NUMBER\" --json baseRefOid,headRefOid"),
        "dispatch step must resolve the created PR's recorded base/head identity:\n{}",
        step.run
    );
    assert!(
        step.run.contains("-f \"base_sha=$base_sha\"")
            && step.run.contains("-f \"head_sha=$head_sha\""),
        "ci.yml dispatch must pass the resolved base_sha/head_sha inputs (#13019):\n{}",
        step.run
    );
    assert!(
        step.run.contains("\"$head_sha\" != \"$CREATED_HEAD_SHA\""),
        "dispatch step must refuse a subject that moved since creation:\n{}",
        step.run
    );
    assert!(
        step.run.contains("set +e")
            && step.run.contains("failed=1")
            && step.run.contains("exit \"$failed\""),
        "dispatch step must continue after an individual failure and fail overall"
    );
    Ok(())
}

/// Every required-check workflow must be raised on the generated PR, plus
/// the lane-history validator, and each must accept workflow_dispatch.
#[test]
fn test_dispatch_set_covers_required_checks() -> Result<(), Box<dyn std::error::Error>> {
    let step = dispatch_step()?;
    let order = dispatch_order(&step.run)?;
    let dispatched: BTreeSet<String> = order.iter().cloned().collect();

    let policy_path = project_root().join(".ci/policies/required-checks.toml");
    let policy: TomlValue = toml::from_str(&fs::read_to_string(policy_path)?)?;
    let required = required_workflows(&policy)?;
    assert!(
        required.is_subset(&dispatched),
        "generated-PR dispatch set must cover every required-check workflow; missing: {:?}",
        required.difference(&dispatched).collect::<Vec<_>>()
    );
    assert!(
        dispatched.contains("validate-ci-lane-history.yml"),
        "the reader-side payload oracle must also run on the refresh PR (#11731)"
    );

    for workflow_name in &dispatched {
        let workflow_path = project_root().join(".github/workflows").join(workflow_name);
        let workflow_text = fs::read_to_string(&workflow_path)?;
        let workflow: Value = serde_yaml_ng::from_str(&workflow_text)?;
        assert!(
            workflow_dispatch_trigger(&workflow),
            "{workflow_name} must declare an on.workflow_dispatch trigger"
        );
    }
    Ok(())
}

#[cfg(unix)]
mod simulation {
    use super::external_command::{
        ExpectedExternalCommand, ExternalCommandFixture, HermeticCommandEnv,
        ObservedExternalCommand,
    };
    use super::*;
    use std::process::Output;

    const PR_NUMBER: &str = "14668";

    fn strs(args: &[&str]) -> Vec<String> {
        args.iter().map(|argument| (*argument).to_string()).collect()
    }

    fn pr_view(stdout: &str, times: usize, fail: bool) -> ExpectedExternalCommand {
        let spec = ExpectedExternalCommand::exact(
            "gh",
            strs(&["pr", "view", PR_NUMBER, "--json", "baseRefOid,headRefOid"]),
        )
        .json_fields_are_a_set()
        .times(times);
        if fail { spec.error("lookup failed") } else { spec.stdout(format!("{stdout}\n")) }
    }

    fn workflow_run(workflow: &str, fail: bool, with_subject: bool) -> ExpectedExternalCommand {
        let mut argv = strs(&["workflow", "run", workflow, "--ref", DISPATCH_BRANCH]);
        if with_subject {
            argv.push("-f".into());
            argv.push(format!("base_sha={BASE_SHA}"));
            argv.push("-f".into());
            argv.push(format!("head_sha={HEAD_SHA}"));
        }
        let spec = ExpectedExternalCommand::exact("gh", argv);
        if fail { spec.error("dispatch failed") } else { spec.stdout("") }
    }

    fn workflow_calls(observations: &[ObservedExternalCommand]) -> Vec<String> {
        observations
            .iter()
            .filter(|observed| observed.argv.first().map(String::as_str) == Some("workflow"))
            .map(|observed| observed.argv.join(" "))
            .collect()
    }

    struct DispatchScenario<'a> {
        pr_json: &'a str,
        fail_pr_view: bool,
        fail_workflow: Option<&'a str>,
        move_after_call: Option<(u32, &'a str)>,
    }

    fn run_scenario(
        scenario: DispatchScenario<'_>,
    ) -> Result<(Output, Vec<String>), Box<dyn std::error::Error>> {
        let step = dispatch_step()?;
        let order = dispatch_order(&step.run)?;
        let mut fixture = ExternalCommandFixture::new();
        let identity_ok =
            !scenario.fail_pr_view && scenario.move_after_call.is_none_or(|(call, _)| call > 1);
        let initial_head_ok = scenario.pr_json.contains(HEAD_SHA);
        if scenario.fail_pr_view {
            fixture = fixture.expect(pr_view(scenario.pr_json, 1, true));
        } else if let Some((after_call, after_json)) = scenario.move_after_call {
            let first = after_call.saturating_sub(1);
            if first > 0 {
                fixture = fixture.expect(pr_view(scenario.pr_json, first as usize, false));
            }
            fixture = fixture.expect(pr_view(after_json, 4, false));
        } else {
            fixture = fixture.expect(pr_view(scenario.pr_json, 4, false));
        }
        let skip_ci = scenario.fail_pr_view
            || !initial_head_ok
            || scenario.move_after_call.is_some_and(|(call, _)| call == 2);
        for workflow in &order {
            let is_ci = workflow == "ci.yml";
            if is_ci && skip_ci {
                continue;
            }
            let fail = scenario.fail_workflow == Some(workflow.as_str());
            fixture = fixture.expect(workflow_run(
                workflow,
                fail,
                is_ci && identity_ok && initial_head_ok,
            ));
        }
        let env = HermeticCommandEnv::install("gh", &fixture)?;
        let output = env.run_bash(
            &step.run,
            &[
                ("BRANCH", DISPATCH_BRANCH),
                ("PR_NUMBER", PR_NUMBER),
                ("CREATED_HEAD_SHA", HEAD_SHA),
            ],
        )?;
        if env.sentinel_hit() {
            return Err("live gh fallback reached the PATH sentinel".into());
        }
        Ok((output, workflow_calls(&env.observations()?)))
    }

    fn pr_json(base: &str, head: &str) -> String {
        format!("{{\"baseRefOid\":\"{base}\",\"headRefOid\":\"{head}\"}}")
    }

    fn expected_calls(order: &[String]) -> Vec<String> {
        order
            .iter()
            .map(|workflow| {
                if workflow == "ci.yml" {
                    format!(
                        "workflow run {workflow} --ref {DISPATCH_BRANCH} -f base_sha={BASE_SHA} -f head_sha={HEAD_SHA}"
                    )
                } else {
                    format!("workflow run {workflow} --ref {DISPATCH_BRANCH}")
                }
            })
            .collect()
    }

    /// A healthy run dispatches ci.yml with the resolved PR identity and the
    /// remaining workflows bare, in loop order (#15100).
    #[test]
    fn test_all_success_dispatch_passes_subject_identity() -> Result<(), Box<dyn std::error::Error>>
    {
        let step = dispatch_step()?;
        let order = dispatch_order(&step.run)?;
        let (output, calls) = run_scenario(DispatchScenario {
            pr_json: &pr_json(BASE_SHA, HEAD_SHA),
            fail_pr_view: false,
            fail_workflow: None,
            move_after_call: None,
        })?;
        assert!(
            output.status.success(),
            "all-success dispatch run failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(calls, expected_calls(&order), "ci.yml must receive the exact PR identity");
        Ok(())
    }

    /// A failed identity lookup must fail the step, skip the ci.yml
    /// dispatch, and still attempt the other workflows (#15100).
    #[test]
    fn test_failed_identity_lookup_skips_ci_dispatch() -> Result<(), Box<dyn std::error::Error>> {
        let step = dispatch_step()?;
        let order = dispatch_order(&step.run)?;
        let (output, calls) = run_scenario(DispatchScenario {
            pr_json: &pr_json(BASE_SHA, HEAD_SHA),
            fail_pr_view: true,
            fail_workflow: None,
            move_after_call: None,
        })?;
        assert!(!output.status.success(), "a failed identity lookup must fail the step");
        assert!(
            !calls.iter().any(|call| call.contains("ci.yml")),
            "ci.yml must not be dispatched without a resolved subject: {calls:?}"
        );
        let bare: Vec<String> = order
            .iter()
            .filter(|workflow| *workflow != "ci.yml")
            .map(|workflow| format!("workflow run {workflow} --ref {DISPATCH_BRANCH}"))
            .collect();
        assert_eq!(calls, bare, "the other workflows must still be attempted");
        Ok(())
    }

    /// A head that moved between creation and dispatch must fail the step
    /// and never attach CI to the new, unproven subject (#15100).
    #[test]
    fn test_moved_head_is_refused() -> Result<(), Box<dyn std::error::Error>> {
        let step = dispatch_step()?;
        let order = dispatch_order(&step.run)?;
        let moved = "fedcba9876543210fedcba9876543210fedcba98";
        let (output, calls) = run_scenario(DispatchScenario {
            pr_json: &pr_json(BASE_SHA, moved),
            fail_pr_view: false,
            fail_workflow: None,
            move_after_call: None,
        })?;
        assert!(!output.status.success(), "a moved head must fail the step");
        assert!(
            !calls.iter().any(|call| call.contains("ci.yml")),
            "ci.yml must not be dispatched for a moved subject: {calls:?}"
        );
        let bare: Vec<String> = order
            .iter()
            .filter(|workflow| *workflow != "ci.yml")
            .map(|workflow| format!("workflow run {workflow} --ref {DISPATCH_BRANCH}"))
            .collect();
        assert_eq!(calls, bare, "the other workflows must still be attempted");
        Ok(())
    }

    /// A failed dispatch must fail the step without skipping later
    /// workflows (#11731).
    #[test]
    fn test_failed_dispatch_does_not_strand_later_workflows()
    -> Result<(), Box<dyn std::error::Error>> {
        let step = dispatch_step()?;
        let order = dispatch_order(&step.run)?;
        let first = order.first().ok_or("dispatch set must not be empty")?.clone();
        let (output, calls) = run_scenario(DispatchScenario {
            pr_json: &pr_json(BASE_SHA, HEAD_SHA),
            fail_pr_view: false,
            fail_workflow: Some(&first),
            move_after_call: None,
        })?;
        assert!(!output.status.success(), "a failed dispatch must fail the step");
        assert_eq!(calls, expected_calls(&order), "every workflow must still be attempted");
        Ok(())
    }

    /// A head that moves after the initial lookup but before the ci.yml
    /// dispatch must be caught by the per-dispatch re-resolution: ci.yml is
    /// skipped and the step fails rather than proving the moved subject
    /// (#15100 TOCTOU guard). Lookup order in the step is: initial resolve
    /// (1), ci.yml pre-dispatch re-resolve (2), post-loop re-resolve (3).
    #[test]
    fn test_head_moving_before_ci_dispatch_is_caught() -> Result<(), Box<dyn std::error::Error>> {
        let step = dispatch_step()?;
        let order = dispatch_order(&step.run)?;
        let moved = "fedcba9876543210fedcba9876543210fedcba98";
        let after = pr_json(BASE_SHA, moved);
        let (output, calls) = run_scenario(DispatchScenario {
            pr_json: &pr_json(BASE_SHA, HEAD_SHA),
            fail_pr_view: false,
            fail_workflow: None,
            move_after_call: Some((2, after.as_str())),
        })?;
        assert!(!output.status.success(), "a head moving mid-step must fail the step");
        assert!(
            !calls.iter().any(|call| call.contains("ci.yml")),
            "ci.yml must not be dispatched once the subject moved: {calls:?}"
        );
        let bare: Vec<String> = order
            .iter()
            .filter(|workflow| *workflow != "ci.yml")
            .map(|workflow| format!("workflow run {workflow} --ref {DISPATCH_BRANCH}"))
            .collect();
        assert_eq!(calls, bare, "the other workflows must still be attempted");
        Ok(())
    }

    /// A head that moves after the ci.yml dispatch is caught by the
    /// post-loop re-resolution: the dispatch already landed on the
    /// then-verified subject, but the step must fail loudly because later
    /// checks may name different subjects (#15100 TOCTOU guard).
    #[test]
    fn test_head_moving_after_ci_dispatch_fails_the_step() -> Result<(), Box<dyn std::error::Error>>
    {
        let step = dispatch_step()?;
        let order = dispatch_order(&step.run)?;
        let moved = "fedcba9876543210fedcba9876543210fedcba98";
        let after = pr_json(BASE_SHA, moved);
        let (output, calls) = run_scenario(DispatchScenario {
            pr_json: &pr_json(BASE_SHA, HEAD_SHA),
            fail_pr_view: false,
            fail_workflow: None,
            move_after_call: Some((3, after.as_str())),
        })?;
        assert!(!output.status.success(), "a post-dispatch head move must fail the step");
        assert_eq!(
            calls,
            expected_calls(&order),
            "the ci.yml dispatch already landed on the verified subject"
        );
        Ok(())
    }
}
