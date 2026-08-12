//! Policy and shell-semantic guards for the UX receipt workflows (#6027, #7522).
//!
//! Both workflows execute the checked-out candidate and publish evidence about it.
//! These tests therefore pin the exact subject binding, verified publication path,
//! unavailable-harness behavior, and the `bash -e -o pipefail` status-capture seam.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value as JsonValue;
use serde_yaml_ng::Value;
use tempfile::TempDir;

const IMMUTABLE_SUBJECT: &str =
    "github.event_name == 'pull_request' && github.event.pull_request.head.sha || github.sha";
const FIXED_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const WORKFLOWS: [(&str, &str); 2] =
    [("ux-regression-gate.yml", "ux-regression-gate"), ("ci.yml", "ux-tests")];

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn workflow(file: &str) -> Result<Value> {
    let path = project_root().join(".github/workflows").join(file);
    let content = fs::read_to_string(&path)?;
    Ok(serde_yaml_ng::from_str(&content)?)
}

fn jobs(workflow: &Value) -> Result<&serde_yaml_ng::Mapping> {
    workflow
        .get("jobs")
        .and_then(Value::as_mapping)
        .ok_or_else(|| anyhow!("workflow must declare jobs"))
}

fn steps<'a>(workflow: &'a Value, job_name: &str) -> Result<&'a Vec<Value>> {
    jobs(workflow)?
        .get(Value::String(job_name.into()))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("job `{job_name}` must declare steps"))
}

fn named_step<'a>(steps: &'a [Value], name: &str) -> Result<&'a Value> {
    steps
        .iter()
        .find(|step| step.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| anyhow!("step `{name}` must exist"))
}

fn run_step<'a>(steps: &'a [Value], name: &str) -> Result<&'a str> {
    named_step(steps, name)?
        .get("run")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("step `{name}` must have a run block"))
}

fn bash_executable() -> PathBuf {
    if let Some(path) = env::var_os("BASH") {
        return path.into();
    }
    #[cfg(windows)]
    {
        let git_bash = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
        if git_bash.is_file() {
            return git_bash;
        }
    }
    PathBuf::from("bash")
}

fn execute_run_block(run: &str, command_status: i32, tee_status: i32) -> Result<(TempDir, Output)> {
    let temp = tempfile::tempdir()?;
    let fake_command = if command_status == 0 {
        "printf '%s\\n' 'running 1 test' 'test result: ok. 1 passed; 0 failed'"
    } else {
        "printf '%s\\n' 'running 1 test' \
            'test ux_scenario_01_startup::starts ... FAILED' \
            'test result: FAILED. 0 passed; 1 failed'"
    };
    let script = format!(
        "just() {{ {fake_command}; return \"$FAKE_UX_STATUS\"; }}\n\
         tee() {{ command tee \"$@\"; local status=$?; \
                 if [ \"$FAKE_TEE_STATUS\" -ne 0 ]; then return \"$FAKE_TEE_STATUS\"; fi; \
                 return \"$status\"; }}\n\
         {run}"
    );
    let output = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c", &script])
        .env("FAKE_UX_STATUS", command_status.to_string())
        .env("FAKE_TEE_STATUS", tee_status.to_string())
        .current_dir(temp.path())
        .output()
        .context("executing workflow run block with Actions bash semantics")?;
    Ok((temp, output))
}

fn assert_exit(output: &Output, expected: i32, context: &str) -> Result<()> {
    if output.status.code() != Some(expected) {
        bail!(
            "{context}: expected exit {expected}, got {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn receipt_path(root: &Path, extension: &str) -> PathBuf {
    root.join(format!("target/receipts/ux-regression.{extension}"))
}

#[test]
fn failing_command_is_captured_and_emits_exact_subject_receipt_in_both_workflows() -> Result<()> {
    for (file, job) in WORKFLOWS {
        let wf = workflow(file)?;
        let run = run_step(steps(&wf, job)?, "Run UX regression tests")?;
        let (temp, output) = execute_run_block(run, 23, 0)?;
        assert_exit(&output, 23, file)?;

        let log = fs::read_to_string(receipt_path(temp.path(), "log"))?;
        assert!(log.contains("ux_scenario_01_startup::starts ... FAILED"), "{file}");
        assert_eq!(fs::read_to_string(receipt_path(temp.path(), "exit"))?, "23\n", "{file}");

        let receipt = receipt_path(temp.path(), "json");
        let receipt_output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args([
                "ux-regression-receipt",
                "--input",
                receipt_path(temp.path(), "log").to_str().ok_or_else(|| anyhow!("log path"))?,
                "--receipt",
                receipt.to_str().ok_or_else(|| anyhow!("receipt path"))?,
                "--exit-status-file",
                receipt_path(temp.path(), "exit").to_str().ok_or_else(|| anyhow!("exit path"))?,
                "--sha",
                FIXED_SHA,
            ])
            .output()?;
        assert_exit(&receipt_output, 0, "ux-regression-receipt")?;
        let payload: JsonValue = serde_json::from_str(&fs::read_to_string(receipt)?)?;
        assert_eq!(payload["schema_version"], 1, "{file}");
        assert_eq!(payload["sha"], FIXED_SHA, "{file}");
        assert_eq!(payload["result"], "fail", "{file}");
        assert_eq!(payload["blocking"], true, "{file}");
    }
    Ok(())
}

#[test]
fn passing_command_preserves_zero_status_in_both_workflows() -> Result<()> {
    for (file, job) in WORKFLOWS {
        let wf = workflow(file)?;
        let run = run_step(steps(&wf, job)?, "Run UX regression tests")?;
        let (temp, output) = execute_run_block(run, 0, 0)?;
        assert_exit(&output, 0, file)?;
        assert_eq!(fs::read_to_string(receipt_path(temp.path(), "exit"))?, "0\n", "{file}");
    }
    Ok(())
}

#[test]
fn tee_failure_is_instrumentation_failure_without_losing_command_status() -> Result<()> {
    for (file, job) in WORKFLOWS {
        let wf = workflow(file)?;
        let run = run_step(steps(&wf, job)?, "Run UX regression tests")?;
        let (temp, output) = execute_run_block(run, 0, 74)?;
        assert_exit(&output, 74, file)?;
        assert_eq!(fs::read_to_string(receipt_path(temp.path(), "exit"))?, "0\n", "{file}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("tee failed"),
            "{file} must identify tee failure separately from the UX command"
        );
    }
    Ok(())
}

#[test]
fn tested_sha_cannot_be_rebound_by_candidate_code() -> Result<()> {
    for (file, selected_job) in WORKFLOWS {
        let wf = workflow(file)?;
        if wf.get("env").and_then(|env| env.get("TESTED_SHA")).is_some() {
            bail!("{file} declares workflow-level rebindable TESTED_SHA");
        }

        let mut checked = 0usize;
        for (job_name, job) in jobs(&wf)? {
            if job.get("env").and_then(|env| env.get("TESTED_SHA")).is_some() {
                bail!("{file} job {job_name:?} declares rebindable TESTED_SHA");
            }
            let Some(job_steps) = job.get("steps").and_then(Value::as_sequence) else {
                continue;
            };
            for step in job_steps {
                let run = step.get("run").and_then(Value::as_str).unwrap_or_default();
                let with = step.get("with").map(|w| format!("{w:?}")).unwrap_or_default();
                if with.contains("env.TESTED_SHA") {
                    bail!(
                        "{file} job {job_name:?} reads rebindable env.TESTED_SHA in action input"
                    );
                }
                if !run.contains("TESTED_SHA") {
                    continue;
                }
                let bound = step
                    .get("env")
                    .and_then(|env| env.get("TESTED_SHA"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("{file} job {job_name:?} uses unbound TESTED_SHA"))?;
                assert_eq!(bound, format!("${{{{ {IMMUTABLE_SUBJECT} }}}}"), "{file}");
                checked += 1;
            }
        }
        assert!(checked >= 2, "{file} must bind emitter and verifier subjects");

        let checkout = steps(&wf, selected_job)?
            .iter()
            .find(|step| {
                step.get("uses")
                    .and_then(Value::as_str)
                    .is_some_and(|uses| uses.starts_with("actions/checkout@"))
            })
            .ok_or_else(|| anyhow!("{file} job {selected_job} must checkout its tested subject"))?;
        let checkout_ref = checkout
            .get("with")
            .and_then(|with| with.get("ref"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{file} job {selected_job} checkout must declare ref"))?;
        assert_eq!(checkout_ref, format!("${{{{ {IMMUTABLE_SUBJECT} }}}}"), "{file}");
    }
    Ok(())
}

#[test]
fn exact_subject_uploads_require_verification_and_failures_remain_diagnostic() -> Result<()> {
    for (file, job) in WORKFLOWS {
        let wf = workflow(file)?;
        let job_steps = steps(&wf, job)?;
        let verifier = job_steps
            .iter()
            .find(|step| {
                step.get("run")
                    .and_then(Value::as_str)
                    .is_some_and(|run| run.contains("UX receipt subject"))
            })
            .ok_or_else(|| anyhow!("{file} must verify receipt subject"))?;
        let verifier_id = verifier
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{file} verifier needs id"))?;

        let evidence = job_steps
            .iter()
            .find(|step| {
                step.get("with")
                    .and_then(|with| with.get("name"))
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.contains("receipt-${{"))
            })
            .ok_or_else(|| anyhow!("{file} exact-subject evidence upload must exist"))?;
        let condition = evidence.get("if").and_then(Value::as_str).unwrap_or_default();
        assert!(condition.contains(&format!("steps.{verifier_id}.outcome == 'success'")), "{file}");
        let name = evidence
            .get("with")
            .and_then(|with| with.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(name.contains(IMMUTABLE_SUBJECT), "{file}: {name}");

        let diagnostics = job_steps
            .iter()
            .find(|step| {
                step.get("with")
                    .and_then(|with| with.get("name"))
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.contains("unverified-${{"))
            })
            .ok_or_else(|| anyhow!("{file} unverified diagnostic upload must exist"))?;
        let diagnostic_name = diagnostics
            .get("with")
            .and_then(|with| with.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(!diagnostic_name.contains(IMMUTABLE_SUBJECT), "{file}");
    }
    Ok(())
}

#[test]
fn unavailable_harness_has_no_executed_test_receipt() -> Result<()> {
    let wf = workflow("ux-regression-gate.yml")?;
    let job_steps = steps(&wf, "ux-regression-gate")?;
    for name in [
        "Run UX regression tests",
        "Emit structured UX regression receipt",
        "Verify receipt subject identity",
        "Upload UX regression evidence",
    ] {
        let condition = named_step(job_steps, name)?
            .get("if")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{name} needs an availability condition"))?;
        assert!(condition.contains("harness_available == 'true'"), "{name}: {condition}");
    }
    Ok(())
}

#[test]
fn final_jobs_fail_after_captured_ux_failure() -> Result<()> {
    for (file, job) in WORKFLOWS {
        let wf = workflow(file)?;
        let job_steps = steps(&wf, job)?;
        let final_failure = named_step(job_steps, "Fail UX regression gate on test failures")?;
        let condition = final_failure.get("if").and_then(Value::as_str).unwrap_or_default();
        assert!(condition.contains("steps.ux_tests.outcome == 'failure'"), "{file}");
        assert!(
            final_failure
                .get("run")
                .and_then(Value::as_str)
                .is_some_and(|run| run.trim() == "exit 1"),
            "{file}"
        );
    }
    Ok(())
}
