//! Execute the real result evaluator: draft skipping must not manufacture proof.
use serde_yaml_ng::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
#[path = "workflow_bash.rs"]
mod workflow_bash;
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn run(script: &str, draft: bool, route: &str, producer: &str) -> TestResult<(bool, String)> {
    let sandbox = tempfile::tempdir()?;
    // A failed producer remains blocking without using live GitHub services.
    // Retrieval is not under test here; existing RIPR contracts own that seam.
    let bounded = format!("timeout() {{ return 1; }}\nsleep() {{ :; }}\n{script}");
    let deadline = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 300;
    let mut child = Command::new(workflow_bash::bash_executable())
        .args(["--noprofile", "--norc", "-s"])
        .current_dir(sandbox.path())
        .env("IS_DRAFT_PR", if draft { "true" } else { "false" })
        .env("ROUTE_RESULT", route)
        .env("ROUTER_TARGET", "github")
        .env("ROUTER_REASON", "controlled-fixture")
        .env("ROUTER_ERROR", "")
        .env("ROUTER_FALLBACK_ALLOWED", "false")
        .env("CX53_RESULT", "skipped")
        .env("CX43_RESULT", "skipped")
        .env("GITHUB_RESULT", producer)
        .env("FALLBACK_RESULT", "skipped")
        .env("GITHUB_STEP_SUMMARY", "summary.md")
        .env("GITHUB_REPOSITORY", "controlled/fixture")
        .env("GITHUB_RUN_ID", "1")
        .env("GITHUB_SHA", "0123456789abcdef0123456789abcdef01234567")
        .env("GH_TOKEN", "")
        .env("RIPR_GATE_DEADLINE_EPOCH", deadline.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().ok_or("evaluator stdin unavailable")?.write_all(bounded.as_bytes())?;
    let output = child.wait_with_output()?;
    let transcript = format!(
        "{}{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        fs::read_to_string(sandbox.path().join("summary.md"))?
    );
    Ok((output.status.success(), transcript))
}

pub fn check_contract(path: &Path, job: &str, token: &str) -> TestResult<()> {
    let yaml: Value = serde_yaml_ng::from_str(&fs::read_to_string(path)?)?;
    let step = yaml
        .get("jobs")
        .and_then(|v| v.get(job))
        .and_then(|v| v.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("name").and_then(Value::as_str) == Some("Evaluate routed result")
            })
        })
        .ok_or("result evaluator missing")?;
    let expected =
        "${{ github.event_name == 'pull_request' && github.event.pull_request.draft == true }}";
    if step.get("env").and_then(|v| v.get("IS_DRAFT_PR")).and_then(Value::as_str) != Some(expected)
    {
        return Err("evaluator must bind an explicit draft pull-request boolean".into());
    }
    let script = step.get("run").and_then(Value::as_str).ok_or("run block missing")?;
    let (passed, output) = run(script, true, "skipped", "skipped")?;
    if passed || !output.contains(token) || !output.contains("NOT_PROVEN") {
        return Err(format!("draft skip must be distinct non-proof: {output}").into());
    }
    // Same SHA, ready producer: success must pass; failure/cancellation must not.
    for (producer, expected_pass) in [("success", true), ("failure", false), ("cancelled", false)] {
        let (passed, output) = run(script, false, "success", producer)?;
        if passed != expected_pass || output.contains(token) {
            return Err(format!("ready producer {producer} misclassified: {output}").into());
        }
    }
    // Scheduled/manual seed events bind the explicit boolean false. Keep their
    // existing skipped-router result; this is not a claim of analysis proof.
    let (passed, output) = run(script, false, "skipped", "skipped")?;
    if !passed || output.contains(token) {
        return Err(format!("non-draft skip behavior changed: {output}").into());
    }
    let anchor = "if [ \"${IS_DRAFT_PR:-false}\" = \"true\" ]; then";
    let start = script.find(anchor).ok_or("draft decision missing")?;
    let suffix = script.get(start..).ok_or("invalid draft decision offset")?;
    let stop = suffix.find("\n  fi").ok_or("draft decision terminator missing")?;
    let branch = suffix.get(..stop).ok_or("invalid draft decision bounds")?;
    if branch.matches("exit 1").count() != 1 {
        return Err("draft refusal must have one nonpassing exit".into());
    }
    let mutant = script.replacen(branch, &branch.replacen("exit 1", "exit 0", 1), 1);
    let (passed, output) = run(&mutant, true, "skipped", "skipped")?;
    if !passed || !output.contains(token) {
        return Err(format!("draft-success mutant did not expose the regression: {output}").into());
    }
    Ok(())
}
