//! Contract tests for ready-for-review RIPR workflow routing.

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, anyhow, bail};
use serde_yaml_ng::Value;
use zip::{ZipWriter, write::SimpleFileOptions};

#[path = "support/workflow_bash.rs"]
mod workflow_bash;

use workflow_bash::bash_executable;

fn project_root() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("CARGO_MANIFEST_DIR has no parent"))?
        .to_path_buf())
}

fn require_real_jq() -> Result<()> {
    let output = Command::new("jq")
        .arg("--version")
        .output()
        .context("checking for the real jq executable")?;
    if !output.status.success() {
        bail!("the RIPR workflow contract tests require a working jq executable");
    }
    Ok(())
}

fn evaluate_run_block() -> Result<String> {
    workflow_run_block("ripr", "Evaluate routed result")
}

fn evaluate_gh_token_binding() -> Result<String> {
    workflow_step_value("ripr", "Evaluate routed result")?
        .get("env")
        .and_then(|env| env.get("GH_TOKEN"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("Evaluate routed result GH_TOKEN binding is missing"))
}

fn retry_gh_token_binding() -> Result<String> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr-infra-retry.yml"))?;
    let yaml: Value = serde_yaml_ng::from_str(&workflow)?;
    yaml.get("jobs")
        .and_then(|jobs| jobs.get("retry-on-eviction"))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("name").and_then(Value::as_str)
                    == Some("Retry once when the gate classified the failure as infra-no-proof")
            })
        })
        .and_then(|step| step.get("env"))
        .and_then(|env| env.get("GH_TOKEN"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("retry workflow GH_TOKEN binding is missing"))
}

fn workflow_run_block(job_name: &str, step_name: &str) -> Result<String> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let yaml: Value = serde_yaml_ng::from_str(&workflow)?;
    yaml.get("jobs")
        .and_then(|jobs| jobs.get(job_name))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| step.get("name").and_then(Value::as_str) == Some(step_name))
        })
        .and_then(|step| step.get("run"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{job_name} {step_name} run block is missing"))
}

fn workflow_job_if(job_name: &str) -> Result<String> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let yaml: Value = serde_yaml_ng::from_str(&workflow)?;
    yaml.get("jobs")
        .and_then(|jobs| jobs.get(job_name))
        .and_then(|job| job.get("if"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{job_name} if expression is missing"))
}

fn workflow_step_value(job_name: &str, step_name: &str) -> Result<Value> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let yaml: Value = serde_yaml_ng::from_str(&workflow)?;
    yaml.get("jobs")
        .and_then(|jobs| jobs.get(job_name))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| step.get("name").and_then(Value::as_str) == Some(step_name))
        })
        .cloned()
        .ok_or_else(|| anyhow!("{job_name} step {step_name} is missing"))
}

#[derive(Clone, Copy)]
struct GateRoute<'a> {
    router_target: &'a str,
    cx53_result: &'a str,
    cx43_result: &'a str,
    github_result: &'a str,
    fallback_result: &'a str,
}

impl<'a> GateRoute<'a> {
    fn github_failure() -> Self {
        Self {
            router_target: "github",
            cx53_result: "skipped",
            cx43_result: "skipped",
            github_result: "failure",
            fallback_result: "skipped",
        }
    }
}

fn gate_lane_identity(route: GateRoute<'_>) -> (&'static str, &'static str) {
    if route.fallback_result == "failure" || route.fallback_result == "cancelled" {
        return ("ripr+ (Disk-Full Fallback)", "88001");
    }
    match route.router_target {
        "cx53" => ("ripr+ on CX53", "53001"),
        "cx43" => ("ripr+ on CX43", "43001"),
        "github" => ("ripr+ on GitHub Hosted", "97001"),
        _ => ("unknown", "0"),
    }
}

fn run_gate_with_fake_gh(
    log: Option<&str>,
    lookup_failures: u32,
    fetch_failures: u32,
    route: GateRoute<'_>,
) -> Result<(std::process::Output, String, Option<String>)> {
    run_gate_with_fake_gh_logs(
        log,
        "##[error]The runner has received a shutdown signal.\n",
        lookup_failures,
        fetch_failures,
        route,
        GhApiControl::Valid,
        Some("gate-token"),
    )
}

#[derive(Clone, Copy)]
enum GhApiControl {
    Valid,
    WrongRepository,
    WrongRun,
    WrongLogJob,
    MissingToken,
}

fn run_gate_with_fake_gh_logs(
    log: Option<&str>,
    partial_log: &str,
    lookup_failures: u32,
    fetch_failures: u32,
    route: GateRoute<'_>,
    api_control: GhApiControl,
    token: Option<&str>,
) -> Result<(std::process::Output, String, Option<String>)> {
    let root = project_root()?;
    let sandbox = tempfile::tempdir().context("creating gate workflow sandbox")?;
    let classifier_dir = sandbox.path().join("scripts/ci");
    fs::create_dir_all(&classifier_dir)?;
    fs::copy(
        root.join("scripts/ci/classify-ripr-lane-termination"),
        classifier_dir.join("classify-ripr-lane-termination"),
    )?;
    let summary = sandbox.path().join("summary.md");
    let lookup_calls = sandbox.path().join("job-lookup-calls");
    let fetch_calls = sandbox.path().join("log-fetch-calls");
    let fetch_urls = sandbox.path().join("log-fetch-urls");
    let jobs_response = sandbox.path().join("jobs.json");
    let classification = sandbox.path().join("ripr-gate-classification.env");
    require_real_jq()?;
    if evaluate_gh_token_binding()? != "${{ github.token }}" {
        bail!("Evaluate routed result must bind GH_TOKEN to github.token");
    }
    let mut run = evaluate_run_block()?;
    match api_control {
        GhApiControl::Valid | GhApiControl::MissingToken => {}
        GhApiControl::WrongRepository => {
            run = run.replace("repos/${GITHUB_REPOSITORY}/", "repos/Other/repository/");
        }
        GhApiControl::WrongRun => {
            run = run.replace("actions/runs/${GITHUB_RUN_ID}/jobs", "actions/runs/9999/jobs");
        }
        GhApiControl::WrongLogJob => {
            run = run.replace("actions/jobs/${lane_job_id}/logs", "actions/jobs/99999/logs");
        }
    }
    let (job_name, job_id) = gate_lane_identity(route);
    let fake = r#"
sleep() { :; }
gh() {
  [ "$1" = "api" ] || return 1
  [ -n "${GH_TOKEN:-}" ] || return 1
  [ "$GH_TOKEN" = "${GITHUB_TOKEN:-}" ] || return 1
  shift
  local url="$1"
  shift
  local jq_selector=""
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--jq" ]; then
      jq_selector="$2"
      shift 2
    else
      shift
    fi
  done
  local expected_jobs_url="repos/${FAKE_REPOSITORY}/actions/runs/${FAKE_RUN_ID}/jobs?per_page=100"
  local expected_log_url="repos/${FAKE_REPOSITORY}/actions/jobs/${FAKE_JOB_ID}/logs"
  case "$url" in
    "$expected_jobs_url")
      local count=0
      if [ -f "$FAKE_LOOKUP_CALLS" ]; then count=$(cat "$FAKE_LOOKUP_CALLS"); fi
      printf '%s' "$((count + 1))" > "$FAKE_LOOKUP_CALLS"
      if [ "$count" -lt "$FAKE_LOOKUP_FAILURES" ]; then return 1; fi
      if [ "$jq_selector" != ".jobs[] | select(.name == \"$FAKE_JOB_NAME\") | .id" ]; then
        printf 'unexpected job selector: %s\n' "$jq_selector" >&2
        return 1
      fi
      # A stale job appears first in the response. Let the real jq executable
      # evaluate the workflow's selector against this JSON; a canned ID or
      # discarded response would make the wrong-job negative control vacuous.
      printf '{"jobs":[{"name":"ripr+ stale job","id":11111},{"name":"%s","id":%s}]}\n' "$FAKE_JOB_NAME" "$FAKE_JOB_ID" > "$FAKE_JOBS_RESPONSE"
      jq "$jq_selector" "$FAKE_JOBS_RESPONSE"
      return $?
      ;;
    "$expected_log_url")
      local requested_job_id="${url%/logs}"
      requested_job_id="${requested_job_id##*/}"
      printf '%s\n' "$requested_job_id" >> "$FAKE_FETCH_URLS"
      if [ "$requested_job_id" != "$FAKE_JOB_ID" ]; then
        # A stale or hard-coded job ID must not look like the selected lane's
        # teardown log. Returning a genuine gap makes wrong-job selection
        # observable through the real classifier.
        printf '%s' 'quality gate failed; see receipt from stale job\n'
        return 0
      fi
      local count=0
      if [ -f "$FAKE_FETCH_CALLS" ]; then count=$(cat "$FAKE_FETCH_CALLS"); fi
      printf '%s' "$((count + 1))" > "$FAKE_FETCH_CALLS"
      if [ "$count" -lt "$FAKE_FETCH_FAILURES" ] || [ "$FAKE_FETCH" = "fail" ]; then
        printf '%s' "$FAKE_PARTIAL_LOG"
        return 1
      fi
      printf '%s' "$FAKE_LOG"
      return
      ;;
    *)
      printf 'unexpected gh API URL: %s\n' "$url" >&2
      return 1
      ;;
  esac
  return 1
}
"#;
    let script = format!("{fake}\n{run}");
    let mut child = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-s"])
        .current_dir(sandbox.path())
        .env("ROUTE_RESULT", "success")
        .env("ROUTER_TARGET", route.router_target)
        .env("ROUTER_REASON", "test")
        .env("CX53_RESULT", route.cx53_result)
        .env("CX43_RESULT", route.cx43_result)
        .env("GITHUB_RESULT", route.github_result)
        .env("FALLBACK_RESULT", route.fallback_result)
        .env("GITHUB_REPOSITORY", "EffortlessMetrics/perl-lsp-swarm")
        .env("GITHUB_RUN_ID", "4242")
        .env("GITHUB_SHA", "0123456789abcdef0123456789abcdef01234567")
        .env("GITHUB_TOKEN", "gate-token")
        .env("GH_TOKEN", token.unwrap_or(""))
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("FAKE_FETCH", if log.is_some() { "success" } else { "fail" })
        .env("FAKE_LOOKUP_FAILURES", lookup_failures.to_string())
        .env("FAKE_FETCH_FAILURES", fetch_failures.to_string())
        .env("FAKE_LOG", log.unwrap_or(""))
        .env("FAKE_PARTIAL_LOG", partial_log)
        .env("FAKE_LOOKUP_CALLS", &lookup_calls)
        .env("FAKE_FETCH_CALLS", &fetch_calls)
        .env("FAKE_FETCH_URLS", &fetch_urls)
        .env("FAKE_JOBS_RESPONSE", &jobs_response)
        .env("FAKE_REPOSITORY", "EffortlessMetrics/perl-lsp-swarm")
        .env("FAKE_RUN_ID", "4242")
        .env("FAKE_JOB_NAME", job_name)
        .env("FAKE_JOB_ID", job_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("executing the real ripr gate run block")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("bash stdin is unavailable"))?
        .write_all(script.as_bytes())?;
    let output = child.wait_with_output().context("waiting for the real ripr gate run block")?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let lookup_text = match fs::read_to_string(&lookup_calls) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("reading fake job lookup count"),
    };
    let fetch_text = match fs::read_to_string(&fetch_calls) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("reading fake log fetch count"),
    };
    let fetch_urls_text = match fs::read_to_string(&fetch_urls) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("reading fake log fetch URLs"),
    };
    let classification_text = match fs::read_to_string(&classification) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    Ok((
        output,
        format!(
            "{combined}\nlookup={lookup_text:?}\nfetch={fetch_text:?}\nfetch_urls={fetch_urls_text:?}"
        ),
        classification_text,
    ))
}

fn runner_response(fixture: &str) -> Result<&'static str> {
    match fixture {
        "ready" => Ok(
            r#"{"runners":[{"id":53001,"name":"cx53-ready","status":"online","busy":false,"labels":[{"name":"EM-CI"},{"name":"CX53"},{"name":"rust-small"},{"name":"trusted-pr"}]},{"id":43001,"name":"cx43-ready","status":"online","busy":false,"labels":[{"name":"em-ci"},{"name":"cx43"},{"name":"rust-small"},{"name":"trusted-pr"}]},{"id":53002,"name":"cx53-busy","status":"online","busy":true,"labels":[{"name":"em-ci"},{"name":"cx53"},{"name":"rust-small"},{"name":"trusted-pr"}]},{"id":53003,"name":"cx53-missing-label","status":"online","busy":false,"labels":[{"name":"em-ci"},{"name":"cx53"},{"name":"rust-small"}]},{"id":53004,"name":"cx53-offline","status":"offline","busy":false,"labels":[{"name":"em-ci"},{"name":"cx53"},{"name":"rust-small"},{"name":"trusted-pr"}]}]}"#,
        ),
        "busy-only" => Ok(
            r#"{"runners":[{"id":53002,"name":"cx53-busy","status":"online","busy":true,"labels":[{"name":"em-ci"},{"name":"cx53"},{"name":"rust-small"},{"name":"trusted-pr"}]}]}"#,
        ),
        "missing-label-only" => Ok(
            r#"{"runners":[{"id":53003,"name":"cx53-missing-label","status":"online","busy":false,"labels":[{"name":"em-ci"},{"name":"cx53"},{"name":"rust-small"}]}]}"#,
        ),
        _ => bail!("unknown runner fixture: {fixture}"),
    }
}

fn artifact_response_with_id(fixture: &str, artifact_id: &str) -> String {
    match fixture {
        "missing" => {
            r#"{"total_count":1,"artifacts":[{"id":99000,"name":"ripr-pr-evidence"}]}"#.to_owned()
        }
        _ => format!(
            r#"{{"total_count":2,"artifacts":[{{"id":99000,"name":"ripr-pr-evidence"}},{{"id":{artifact_id},"name":"ripr-gate-classification"}}]}}"#
        ),
    }
}

fn run_router_case(
    is_fork_pr: &str,
    gh_token: &str,
    runner_fixture: &str,
    curl_status: &str,
) -> Result<(std::process::Output, String)> {
    let sandbox = tempfile::tempdir().context("creating router sandbox")?;
    let output_file = sandbox.path().join("router-output");
    let summary = sandbox.path().join("summary.md");
    let curl_request = sandbox.path().join("curl-request");
    let run = workflow_run_block("route-ripr", "Decide target runner")?;
    let runner_json = runner_response(runner_fixture)?;
    require_real_jq()?;
    let fake = r#"
curl() {
  local output=""
  local status_format=""
  local endpoint=""
  local accept=""
  local authorization=""
  local api_version=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -sS) shift ;;
      -w) status_format="$2"; shift 2 ;;
      -o) output="$2"; shift 2 ;;
      -H)
        case "$2" in
          Accept:*) accept="$2" ;;
          Authorization:*) authorization="$2" ;;
          X-GitHub-Api-Version:*) api_version="$2" ;;
          *) printf 'unexpected curl header: %s\n' "$2" >&2; return 1 ;;
        esac
        shift 2
        ;;
      https://*) endpoint="$1"; shift ;;
      *) printf 'unexpected curl argument: %s\n' "$1" >&2; return 1 ;;
    esac
  done
  [ "$status_format" = "%{http_code}" ] || return 1
  [ "$output" != "" ] || return 1
  [ "$endpoint" = "https://api.github.com/orgs/$ORG/actions/runners?per_page=100" ] || {
    printf 'unexpected curl endpoint: %s\n' "$endpoint" >&2
    return 1
  }
  [ "$accept" = "Accept: application/vnd.github+json" ] || return 1
  [ "$authorization" = "Authorization: Bearer $GH_TOKEN" ] || return 1
  [ -n "$GH_TOKEN" ] || return 1
  [ "$api_version" = "X-GitHub-Api-Version: 2022-11-28" ] || return 1
  printf 'endpoint=%s\naccept=%s\nauthorization=%s\napi_version=%s\noutput=%s\n' \
    "$endpoint" "$accept" "$authorization" "$api_version" "$output" > "$FAKE_CURL_REQUEST"
  printf '%s\n' "$FAKE_RUNNERS_JSON" > "$output"
  printf '%s' "$FAKE_CURL_STATUS"
}
"#;
    let script = format!("{fake}\n{run}");
    let mut child = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-s"])
        .current_dir(sandbox.path())
        .env("IS_FORK_PR", is_fork_pr)
        .env("PR_AUTHOR_LOGIN", "human")
        .env("PR_AUTHOR_TYPE", "User")
        .env("GH_TOKEN", gh_token)
        .env("ORG", "EffortlessMetrics")
        .env("FAKE_RUNNER_FIXTURE", runner_fixture)
        .env("FAKE_RUNNERS_JSON", runner_json)
        .env("FAKE_CURL_STATUS", curl_status)
        .env("FAKE_CURL_REQUEST", &curl_request)
        .env("GITHUB_OUTPUT", &output_file)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("executing the real ripr router run block")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("router bash stdin is unavailable"))?
        .write_all(script.as_bytes())?;
    let output = child.wait_with_output().context("waiting for router run block")?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let route_output = fs::read_to_string(&output_file)
        .with_context(|| format!("reading router output {}", output_file.display()))?;
    let curl_request_text = fs::read_to_string(&curl_request)
        .with_context(|| format!("reading fake curl request {}", curl_request.display()))?;
    Ok((output, format!("{combined}\n{route_output}\n{curl_request_text}")))
}

fn run_preflight_case(
    image_present: bool,
    disk_guard_fails: bool,
) -> Result<(std::process::Output, String)> {
    let sandbox = tempfile::tempdir().context("creating preflight sandbox")?;
    let output_file = sandbox.path().join("preflight-output");
    let summary = sandbox.path().join("summary.md");
    let scratch = sandbox.path().join("scratch");
    let cache = sandbox.path().join("cache");
    fs::create_dir_all(&scratch)?;
    fs::create_dir_all(&cache)?;
    let run = workflow_run_block("ripr-cx53", "Preflight disk")?;
    let fake = r#"
docker() {
  if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
    [ "$FAKE_IMAGE_PRESENT" = "true" ]
  else
    return 0
  fi
}
ci-disk-guard() {
  [ "$FAKE_DISK_GUARD" = "fail" ] && return 1
  return 0
}
"#;
    let script = format!("{fake}\n{run}");
    let mut child = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-s"])
        .current_dir(sandbox.path())
        .env("FAKE_IMAGE_PRESENT", image_present.to_string())
        .env("FAKE_DISK_GUARD", if disk_guard_fails { "fail" } else { "pass" })
        .env("SCCACHE_DIR", cache.join("sccache"))
        .env("TMPDIR", scratch.join("tmp"))
        .env("CARGO_TARGET_DIR", scratch.join("target"))
        .env("CARGO_HOME", cache.join("cargo-home"))
        .env("GITHUB_OUTPUT", &output_file)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("executing the real ripr preflight run block")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("preflight bash stdin is unavailable"))?
        .write_all(script.as_bytes())?;
    let output = child.wait_with_output().context("waiting for preflight run block")?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let preflight_output = fs::read_to_string(&output_file)
        .with_context(|| format!("reading preflight output {}", output_file.display()))?;
    Ok((output, format!("{combined}\n{preflight_output}")))
}

fn run_fallback_condition(
    router_target: &str,
    cx53_result: &str,
    cx43_result: &str,
    cx53_preflight_ok: &str,
) -> Result<(std::process::Output, String)> {
    let sandbox = tempfile::tempdir().context("creating fallback-condition sandbox")?;
    let summary = sandbox.path().join("summary.md");
    let condition = workflow_job_if("ripr-fallback")?
        .replace("always()", "true")
        .replace("needs.route-ripr.result", "\"$ROUTE_RESULT\"")
        .replace("needs.route-ripr.outputs.target", "\"$ROUTER_TARGET\"")
        .replace("needs.ripr-cx53.result", "\"$CX53_RESULT\"")
        .replace("needs.ripr-cx53.outputs.preflight_ok", "\"$CX53_PREFLIGHT_OK\"")
        .replace("needs.ripr-cx43.result", "\"$CX43_RESULT\"")
        .replace("needs.ripr-cx43.outputs.preflight_ok", "\"$CX43_PREFLIGHT_OK\"");
    let run = workflow_run_block("ripr-fallback", "Annotate failover reason")?
        .replace("${{ needs.route-ripr.outputs.target }}", router_target);
    let script = format!(
        "if [[ {condition} ]]; then\n  fallback_started=true\n{run}\nelse\n  fallback_started=false\nfi\necho \"fallback_started=$fallback_started\""
    );
    let mut child = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-s"])
        .current_dir(sandbox.path())
        .env("ROUTE_RESULT", "success")
        .env("ROUTER_TARGET", router_target)
        .env("CX53_RESULT", cx53_result)
        .env("CX43_RESULT", cx43_result)
        .env("CX53_PREFLIGHT_OK", cx53_preflight_ok)
        .env("CX43_PREFLIGHT_OK", "")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("executing the real ripr fallback condition and first step")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("fallback condition bash stdin is unavailable"))?
        .write_all(script.as_bytes())?;
    let output = child.wait_with_output().context("waiting for fallback condition")?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let summary_text = match fs::read_to_string(&summary) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("reading fallback summary"),
    };
    Ok((output, format!("{combined}\n{summary_text}")))
}

fn run_fallback_path(quality_gate_fails: bool) -> Result<(std::process::Output, String)> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let yaml: Value = serde_yaml_ng::from_str(&workflow)?;
    let steps = yaml
        .get("jobs")
        .and_then(|jobs| jobs.get("ripr-fallback"))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("ripr-fallback steps are missing"))?;
    let step_names = steps
        .iter()
        .filter_map(|step| step.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let checkout_index = step_names
        .iter()
        .position(|name| *name == "Checkout")
        .ok_or_else(|| anyhow!("ripr-fallback checkout step is missing"))?;
    let evidence_index = step_names
        .iter()
        .position(|name| *name == "Generate PR evidence")
        .ok_or_else(|| anyhow!("ripr-fallback evidence step is missing"))?;
    let quality_index = step_names
        .iter()
        .position(|name| *name == "Enforce new RIPR gap quality gate")
        .ok_or_else(|| anyhow!("ripr-fallback quality-gate step is missing"))?;
    if checkout_index >= evidence_index || evidence_index >= quality_index {
        bail!("ripr-fallback must checkout before evidence and quality-gate steps");
    }

    let checkout = workflow_step_value("ripr-fallback", "Checkout")?;
    if checkout.get("uses").and_then(Value::as_str)
        != Some("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1")
        || checkout.get("with").and_then(|with| with.get("fetch-depth")).and_then(Value::as_i64)
            != Some(0)
    {
        bail!("ripr-fallback checkout must use the pinned action with fetch-depth: 0");
    }

    let upload = workflow_step_value("ripr-fallback", "Upload ripr PR evidence")?;
    let upload_paths = upload
        .get("with")
        .and_then(|with| with.get("path"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("ripr-fallback evidence upload paths are missing"))?;
    for path in [
        "target/ripr/pr/**",
        "target/ripr/review/**",
        "target/receipts/quality/ripr-plus.json",
        "target/receipts/quality/quality-gate-ripr.json",
    ] {
        if !upload_paths.contains(path) {
            bail!("ripr-fallback upload must include {path}");
        }
    }

    let sandbox = tempfile::tempdir().context("creating fallback path sandbox")?;
    let calls = sandbox.path().join("fallback-calls");
    let checkout_marker = sandbox.path().join("checkout-marker");
    let github_env = sandbox.path().join("github-env");
    let summary = sandbox.path().join("summary.md");
    let annotation = workflow_run_block("ripr-fallback", "Annotate failover reason")?
        .replace("${{ needs.route-ripr.outputs.target }}", "cx53");
    let normalize = workflow_run_block("ripr-fallback", "Normalize base ref")?;
    let executable_steps = [
        "Toolchain (rust-toolchain.toml pins 1.95.0)",
        "Install ripr",
        "Run ripr doctor",
        "Generate PR evidence",
        "Generate repo-wide RIPR+ baseline receipt",
        "Record exact RIPR badge producer",
        "Generate review guidance",
        "Generate impacted evidence",
        "Generate PR evidence summary",
        "Generate warning annotations",
        "Validate PR evidence contracts",
        "Enforce new RIPR gap quality gate",
        "Append PR evidence summary",
    ];
    let mut script = String::from(
        r#"set -euo pipefail
checkout_action() {
  printf 'uses=%s fetch-depth=%s\n' "$FAKE_CHECKOUT_USES" "$FAKE_CHECKOUT_DEPTH" > "$FAKE_CHECKOUT_MARKER"
  mkdir -p checkout
}
rustc() {
  printf 'rustc %s\n' "$*" >> "$FAKE_CALLS"
}
ripr() {
  printf 'ripr %s\n' "$*" >> "$FAKE_CALLS"
}
git() {
  [ "$1" = "rev-parse" ] && [ "$2" = "HEAD" ] || return 1
  printf '%s\n' "$FAKE_HEAD"
}
cargo() {
  printf 'cargo %s\n' "$*" >> "$FAKE_CALLS"
  [ "$1" = "xtask" ] || return 0
  case "$2" in
    ripr-pr)
      mkdir -p target/ripr/pr
      printf '{"kind":"ripr-pr"}\n' > target/ripr/pr/repo-exposure.json
      ;;
    ripr-plus)
      mkdir -p target/receipts/quality
      printf '{"kind":"ripr-plus"}\n' > target/receipts/quality/ripr-plus.json
      ;;
    ripr-review-comments)
      mkdir -p target/ripr/review
      printf '{"kind":"review"}\n' > target/ripr/review/comments.json
      ;;
    impacted-evidence)
      mkdir -p target/xtask/impacted-evidence
      printf '{"kind":"impacted"}\n' > target/xtask/impacted-evidence/receipt.json
      ;;
    ripr-pr-summary)
      mkdir -p target/ripr/pr
      printf 'fallback PR evidence summary\n' > target/ripr/pr/summary.md
      ;;
    ripr-annotations)
      mkdir -p target/ripr/review
      printf 'fallback warning annotation\n' > target/ripr/review/annotations.txt
      ;;
    quality-gate)
      if [ "${FAKE_QUALITY_GATE:-pass}" = "fail" ]; then
        return 1
      fi
      mkdir -p target/receipts/quality
      printf '{"kind":"quality-gate"}\n' > target/receipts/quality/quality-gate-ripr.json
      printf 'fallback quality gate summary\n' > target/receipts/quality/quality-gate-ripr.md
      ;;
  esac
}
checkout_action
"#,
    );
    script.push_str(&annotation);
    script.push_str("\n");
    script.push_str(&normalize);
    // GITHUB_ENV is applied between Actions steps. The harness runs the real
    // run blocks in one shell, so apply the same step boundary explicitly.
    script.push_str("\nBASE_REF=main\n");
    for step_name in executable_steps {
        script.push_str("\n");
        script.push_str(&workflow_run_block("ripr-fallback", step_name)?);
        script.push_str("\n");
    }

    let mut child = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-s"])
        .current_dir(sandbox.path())
        .env("BASE_REF", "refs/heads/main")
        .env("PR_HEAD_SHA", "0123456789abcdef0123456789abcdef01234567")
        .env("PR_LABELS", "ci")
        .env("RIPR_VERSION", "0.9.0")
        .env("GITHUB_ENV", &github_env)
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("FAKE_CALLS", &calls)
        .env("FAKE_CHECKOUT_MARKER", &checkout_marker)
        .env(
            "FAKE_CHECKOUT_USES",
            checkout
                .get("uses")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("fallback checkout action is missing"))?,
        )
        .env("FAKE_CHECKOUT_DEPTH", "0")
        .env("FAKE_QUALITY_GATE", if quality_gate_fails { "fail" } else { "pass" })
        .env("FAKE_HEAD", "0123456789abcdef0123456789abcdef01234567")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("executing the real ripr fallback run blocks")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("fallback bash stdin is unavailable"))?
        .write_all(script.as_bytes())?;
    let output = child.wait_with_output().context("waiting for fallback run blocks")?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let checkout_text = fs::read_to_string(&checkout_marker)?;
    let calls_text = fs::read_to_string(&calls)?;
    let summary_text = fs::read_to_string(&summary)?;
    let env_text = fs::read_to_string(&github_env)?;
    Ok((
        output,
        format!(
            "{combined}\ncheckout={checkout_text}calls={calls_text}summary={summary_text}env={env_text}"
        ),
    ))
}

fn retry_run_block() -> Result<String> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr-infra-retry.yml"))?;
    let yaml: Value = serde_yaml_ng::from_str(&workflow)?;
    yaml.get("jobs")
        .and_then(|jobs| jobs.get("retry-on-eviction"))
        .and_then(|job| job.get("steps"))
        .and_then(Value::as_sequence)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("name").and_then(Value::as_str)
                    == Some("Retry once when the gate classified the failure as infra-no-proof")
            })
        })
        .and_then(|step| step.get("run"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("retry workflow run block is missing"))
}

fn write_retry_artifact(path: &std::path::Path, artifact_mode: &str) -> Result<()> {
    if artifact_mode == "corrupt" {
        fs::write(path, b"not a ZIP archive")?;
        return Ok(());
    }

    let file = fs::File::create(path)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    match artifact_mode {
        "missing-file" => {
            archive.start_file("unrelated.txt", options)?;
            archive.write_all(b"not the classification file\n")?;
        }
        "malformed" => {
            archive.start_file("ripr-gate-classification.env", options)?;
            archive.write_all(b"classification=infra-no-proof\nrun_id=not-a-number\n")?;
        }
        "valid" | "download-failure" | "missing" => {
            archive.start_file("ripr-gate-classification.env", options)?;
            archive.write_all(
                b"classification=infra-no-proof\nlane_name=ripr+ on GitHub Hosted\nlane_job_id=97001\nhead_sha=0123456789abcdef0123456789abcdef01234567\nrouter_target=github\nrun_id=4242\n",
            )?;
        }
        _ => bail!("unknown artifact fixture: {artifact_mode}"),
    }
    archive.finish()?;
    Ok(())
}

fn run_retry_case(
    artifact_mode: &str,
    run_attempt: &str,
) -> Result<(std::process::Output, String, bool)> {
    run_retry_case_with(
        artifact_mode,
        run_attempt,
        "99001",
        Some("retry-token"),
        "retry-token",
        false,
    )
}

fn run_retry_case_with(
    artifact_mode: &str,
    run_attempt: &str,
    artifact_id: &str,
    gh_token: Option<&str>,
    expected_token: &str,
    hardcode_artifact_id: bool,
) -> Result<(std::process::Output, String, bool)> {
    let sandbox = tempfile::tempdir().context("creating retry sandbox")?;
    let summary = sandbox.path().join("summary.md");
    let post_called = sandbox.path().join("retry-post-called");
    let download_called = sandbox.path().join("artifact-download-called");
    let api_calls = sandbox.path().join("api-calls");
    let zip_payload = sandbox.path().join("classification-payload.zip");
    let artifact_response_file = sandbox.path().join("artifacts.json");
    write_retry_artifact(&zip_payload, artifact_mode)?;
    let mut run = retry_run_block()?;
    if hardcode_artifact_id {
        let production_url = "actions/artifacts/${artifact_id}/zip";
        let replacement_count = run.matches(production_url).count();
        if replacement_count != 1 {
            bail!(
                "hardcoded-artifact negative control expected one production artifact ID binding, found {replacement_count}"
            );
        }
        run = run.replace(production_url, "actions/artifacts/99001/zip");
    }
    require_real_jq()?;
    let fake = r#"
gh() {
  [ "$1" = "api" ] || return 1
  [ -n "${GH_TOKEN:-}" ] || return 1
  [ "$GH_TOKEN" = "${GITHUB_TOKEN:-}" ] || return 1
  shift
  local method="GET"
  if [ "$1" = "-X" ]; then
    method="$2"
    shift 2
  fi
  local url="$1"
  shift
  local jq_selector=""
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--jq" ]; then
      jq_selector="$2"
      shift 2
    else
      shift
    fi
  done
  local expected_artifacts_url="repos/${FAKE_REPOSITORY}/actions/runs/${FAKE_RUN_ID}/artifacts?per_page=100"
  local expected_artifact_zip_url="repos/${FAKE_REPOSITORY}/actions/artifacts/${FAKE_ARTIFACT_ID}/zip"
  local expected_run_url="repos/${FAKE_REPOSITORY}/actions/runs/${FAKE_RUN_ID}"
  local expected_rerun_url="repos/${FAKE_REPOSITORY}/actions/runs/${FAKE_RUN_ID}/rerun-failed-jobs"
  printf '%s %s\n' "$method" "$url" >> "$FAKE_API_CALLS"
  if [ "$url" = "$expected_artifacts_url" ]; then
      [ "$method" = "GET" ] || return 1
      if [ "$jq_selector" != '.artifacts[] | select(.name == "ripr-gate-classification") | .id' ]; then
        printf 'unexpected artifact selector: %s\n' "$jq_selector" >&2
        return 1
      fi
      printf '%s\n' "$FAKE_ARTIFACTS_JSON" > "$FAKE_ARTIFACT_RESPONSE"
      jq "$jq_selector" "$FAKE_ARTIFACT_RESPONSE"
  elif [ "$url" = "$expected_artifact_zip_url" ]; then
      [ "$method" = "GET" ] || return 1
      : > "$FAKE_DOWNLOAD_CALLED"
      if [ "$FAKE_ARTIFACT_MODE" = "download-failure" ]; then
        return 1
      fi
      cat "$FAKE_ZIP_PAYLOAD"
  elif [ "$url" = "$expected_run_url" ]; then
      [ "$method" = "GET" ] || return 1
      if [ "$jq_selector" != '"\(.run_attempt) \(.status)"' ]; then
        printf 'unexpected run selector: %s\n' "$jq_selector" >&2
        return 1
      fi
      printf '%s completed\n' "$FAKE_LIVE_ATTEMPT"
  elif [ "$url" = "$expected_rerun_url" ]; then
      [ "$method" = "POST" ] || return 1
      : > "$FAKE_POST_CALLED"
  else
      printf 'unexpected gh API URL: %s\n' "$url" >&2
      return 1
  fi
}
"#;
    let script = format!("{fake}\n{run}");
    let mut child = Command::new(bash_executable())
        .args(["--noprofile", "--norc", "-s"])
        .current_dir(sandbox.path())
        .env("GITHUB_REPOSITORY", "EffortlessMetrics/perl-lsp-swarm")
        .env("GITHUB_TOKEN", expected_token)
        .env("GH_TOKEN", gh_token.unwrap_or(""))
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("RUN_ID", "4242")
        .env("RUN_ATTEMPT", run_attempt)
        .env("HEAD_SHA", "0123456789abcdef0123456789abcdef01234567")
        .env("FAKE_ARTIFACT_MODE", artifact_mode)
        .env("FAKE_ARTIFACTS_JSON", artifact_response_with_id(artifact_mode, artifact_id))
        .env("FAKE_ARTIFACT_RESPONSE", &artifact_response_file)
        .env("FAKE_DOWNLOAD_CALLED", &download_called)
        .env("FAKE_API_CALLS", &api_calls)
        .env("FAKE_ZIP_PAYLOAD", &zip_payload)
        .env("FAKE_LIVE_ATTEMPT", "1")
        .env("FAKE_POST_CALLED", &post_called)
        .env("FAKE_REPOSITORY", "EffortlessMetrics/perl-lsp-swarm")
        .env("FAKE_RUN_ID", "4242")
        .env("FAKE_ARTIFACT_ID", artifact_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("executing the real ripr retry run block")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("retry bash stdin is unavailable"))?
        .write_all(script.as_bytes())?;
    let output = child.wait_with_output().context("waiting for retry run block")?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let downloaded = download_called.is_file();
    let posted = post_called.is_file();
    let api_calls_text = match fs::read_to_string(&api_calls) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("reading fake retry API calls"),
    };
    Ok((
        output,
        format!("{combined}\ndownload_called={downloaded}\napi_calls={api_calls_text}"),
        posted,
    ))
}

#[test]
fn ripr_workflow_runs_on_ready_for_review_without_path_filter()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let xtask_main = fs::read_to_string(root.join("xtask/src/main.rs"))?;

    assert!(workflow.contains("pull_request:"), "ripr.yml must run from pull_request events");
    assert!(
        workflow.contains("cancel-in-progress: false"),
        "RIPR evidence runs must queue newer heads instead of cancelling active analysis"
    );
    assert!(
        !workflow.contains("cancel-in-progress: ${{ github.event_name == 'pull_request' && github.event.action == 'synchronize' }}"),
        "RIPR must not turn normal PR synchronization into a cancelled no-verdict"
    );
    assert!(
        workflow.contains("types: [opened, synchronize, reopened, ready_for_review]"),
        "ripr.yml must rerun when a draft PR becomes ready for review because the job skips draft PRs"
    );
    assert!(
        !workflow.contains("\n    paths:"),
        "ripr.yml must not path-filter the ready-for-review proof run"
    );
    assert!(
        workflow.contains("if: github.event.pull_request.draft != true"),
        "ripr.yml may skip draft PRs while they are still draft"
    );
    let gate_step = workflow_step(&workflow, "Enforce new RIPR gap quality gate")
        .ok_or("missing RIPR gate step")?;
    assert!(
        !gate_step.contains("continue-on-error: true"),
        "RIPR workflow is now promoted past PR1 routing-only mode and must block new-gap failures"
    );
    assert!(
        workflow.contains("cargo xtask ripr-pr --base") && workflow.contains("target/ripr/pr/**"),
        "ripr.yml must produce and upload diff-scoped RIPR PR receipts"
    );
    assert!(
        workflow.contains("PR_HEAD_SHA: ${{ github.event_name == 'pull_request' && github.event.pull_request.head.sha || '' }}")
            && workflow.matches("--pr-head \"$PR_HEAD_SHA\"").count() >= 8,
        "every receipt generation/check route must carry the PR head separately from evaluated HEAD"
    );
    assert!(
        workflow.contains("cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json")
            && workflow.contains(
                "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check"
            )
            && workflow.contains("target/receipts/quality/ripr-plus.json"),
        "ripr.yml must generate, check, and upload the repo-wide RIPR+ receipt"
    );
    assert!(
        xtask_main.contains("RiprPlus")
            && xtask_main.contains("ripr_plus(&root, &receipt, &suppressions, check)")
            && xtask_main.contains("default_value = \"policy/ripr-suppressions.toml\""),
        "PR1 workflow must not call a missing `cargo xtask ripr-plus` command"
    );
    assert!(
        workflow.contains("cargo xtask ripr-review-comments --base")
            && workflow.contains("target/ripr/review/**"),
        "ripr.yml must generate and upload review-guidance receipts"
    );
    let validate_step = workflow_step(&workflow, "Validate PR evidence contracts")
        .ok_or("missing validate step")?;
    for check_command in [
        "cargo xtask ripr-pr --base",
        "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check",
        "cargo xtask ripr-review-comments --base",
    ] {
        assert!(
            validate_step.contains(check_command) && validate_step.contains("--check"),
            "PR1 validate step must check `{check_command}`"
        );
    }
    let upload_step =
        workflow_step(&workflow, "Upload ripr PR evidence").ok_or("missing upload step")?;
    for artifact_path in [
        "target/ripr/pr/**",
        "target/ripr/review/**",
        "target/xtask/impacted-evidence/**",
        "target/receipts/quality/ripr-plus.json",
    ] {
        assert!(
            upload_step.contains(artifact_path),
            "PR1 upload step must include `{artifact_path}`"
        );
    }
    assert!(
        upload_step.contains("if-no-files-found: error"),
        "RIPR proof artifacts are required after PR8"
    );
    let summary_step =
        workflow_step(&workflow, "Append PR evidence summary").ok_or("missing summary step")?;
    assert!(
        summary_step.contains("if: always()") && summary_step.contains("target/ripr/pr/summary.md"),
        "RIPR summary step must publish PR evidence even when earlier receipt steps fail"
    );

    Ok(())
}

#[test]
fn ripr_self_hosted_preflight_falls_back_when_required_image_is_missing() -> Result<()> {
    let root = project_root()?;
    let workflow = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;

    assert_eq!(
        workflow
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter(|line| {
                line.trim_start().starts_with("if ! docker image inspect em-ci-rust:1.95")
            })
            .count(),
        2,
        "CX53 and CX43 preflight must both check the required Docker image before running ripr"
    );
    assert!(
        workflow.contains("Required Docker image em-ci-rust:1.95 is missing on CX53")
            && workflow.contains("Required Docker image em-ci-rust:1.95 is missing on CX43"),
        "missing self-hosted Rust image must be reported as preflight failure"
    );
    assert!(
        workflow.contains(
            "needs.route-ripr.outputs.target == 'cx53' && needs.ripr-cx53.result == 'failure' && needs.ripr-cx53.outputs.preflight_ok == 'false'",
        )
            && workflow.contains(
                "needs.route-ripr.outputs.target == 'cx43' && needs.ripr-cx43.result == 'failure' && needs.ripr-cx43.outputs.preflight_ok == 'false'",
            ),
        "only a failed self-hosted preflight with preflight_ok=false must route the run to the GitHub-hosted fallback"
    );

    let (self_hosted_route, self_hosted_output) =
        run_router_case("false", "runner-token", "ready", "200")?;
    if !self_hosted_route.status.success()
        || !self_hosted_output.contains("target=cx53")
        || !self_hosted_output.contains("reason=cx53_idle")
        || !self_hosted_output.contains("idle_cx53=1")
        || !self_hosted_output.contains("idle_cx43=1")
        || !self_hosted_output.contains(
            "endpoint=https://api.github.com/orgs/EffortlessMetrics/actions/runners?per_page=100",
        )
        || !self_hosted_output.contains("accept=Accept: application/vnd.github+json")
        || !self_hosted_output.contains("authorization=Authorization: Bearer runner-token")
        || !self_hosted_output.contains("api_version=X-GitHub-Api-Version: 2022-11-28")
    {
        bail!(
            "router must select the eligible runner from a realistic response through its real run block:\n{self_hosted_output}"
        );
    }
    let (busy_route, busy_output) = run_router_case("false", "runner-token", "busy-only", "200")?;
    if !busy_route.status.success()
        || !busy_output.contains("target=github")
        || !busy_output.contains("reason=no_idle_runner")
        || !busy_output.contains("idle_cx53=0")
    {
        bail!("a busy runner must not be routed to self-hosted:\n{busy_output}");
    }
    let (missing_label_route, missing_label_output) =
        run_router_case("false", "runner-token", "missing-label-only", "200")?;
    if !missing_label_route.status.success()
        || !missing_label_output.contains("target=github")
        || !missing_label_output.contains("reason=no_idle_runner")
        || !missing_label_output.contains("idle_cx53=0")
    {
        bail!(
            "a runner missing a required label must not be routed to self-hosted:\n{missing_label_output}"
        );
    }
    let (hosted_route, hosted_output) = run_router_case("false", "runner-token", "ready", "503")?;
    if !hosted_route.status.success()
        || !hosted_output.contains("target=github")
        || !hosted_output.contains("reason=runner_api_failed")
    {
        bail!(
            "router must select GitHub-hosted when no self-hosted runner is idle:\n{hosted_output}"
        );
    }
    let (missing_image, missing_image_output) = run_preflight_case(false, false)?;
    if missing_image.status.success()
        || !missing_image_output
            .contains("Required Docker image em-ci-rust:1.95 is missing on CX53")
        || !missing_image_output.contains("preflight_ok=false")
    {
        bail!(
            "missing Docker image must fail the real preflight and record false:\n{missing_image_output}"
        );
    }
    let missing_image_preflight = if missing_image_output.contains("preflight_ok=false") {
        "false"
    } else {
        bail!("missing-image preflight did not produce the expected false output")
    };
    let (fallback_started, fallback_output) =
        run_fallback_condition("cx53", "failure", "skipped", missing_image_preflight)?;
    if !fallback_started.status.success()
        || !fallback_output.contains("fallback_started=true")
        || !fallback_output.contains("### ripr Disk-Full Failover")
    {
        bail!(
            "the production ripr-fallback condition must start the fallback after preflight_ok=false:\n{fallback_output}"
        );
    }
    let (fallback_path, fallback_path_output) = run_fallback_path(false)?;
    if !fallback_path.status.success()
        || !fallback_path_output.contains(
            "checkout=uses=actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 fetch-depth=0",
        )
        || !fallback_path_output.contains(
            "cargo xtask ripr-pr --base origin/main --head HEAD --pr-head 0123456789abcdef0123456789abcdef01234567",
        )
        || !fallback_path_output.contains(
            "cargo xtask quality-gate --mode enforce-new-ripr --ripr-receipt target/receipts/quality/ripr-plus.json",
        )
        || !fallback_path_output.contains("fallback PR evidence summary")
        || !fallback_path_output.contains("fallback quality gate summary")
        || !fallback_path_output.contains("fallback warning annotation")
        || !fallback_path_output.contains("env=BASE_REF=main")
    {
        bail!(
            "the fallback must execute its checkout boundary, evidence generation, and quality-gate wiring:\n{fallback_path_output}"
        );
    }
    let (preflight_passed, preflight_passed_output) =
        run_fallback_condition("cx53", "failure", "skipped", "true")?;
    if !preflight_passed.status.success()
        || !preflight_passed_output.contains("fallback_started=false")
        || preflight_passed_output.contains("### ripr Disk-Full Failover")
    {
        bail!("a successful preflight must not start ripr-fallback:\n{preflight_passed_output}");
    }
    let (primary_succeeded, primary_succeeded_output) =
        run_fallback_condition("cx53", "success", "skipped", "false")?;
    if !primary_succeeded.status.success()
        || !primary_succeeded_output.contains("fallback_started=false")
        || primary_succeeded_output.contains("### ripr Disk-Full Failover")
    {
        bail!(
            "a successful primary lane must not start ripr-fallback even when its preflight output is false:\n{primary_succeeded_output}"
        );
    }
    let (ready_image, ready_image_output) = run_preflight_case(true, false)?;
    if !ready_image.status.success() || !ready_image_output.contains("preflight_ok=true") {
        bail!(
            "available Docker image and disk guards must pass the real preflight:\n{ready_image_output}"
        );
    }
    let (disk_guard_failure, disk_guard_output) = run_preflight_case(true, true)?;
    if disk_guard_failure.status.success()
        || !disk_guard_output.contains("preflight_ok=false")
        || !disk_guard_output.contains("Self-hosted preflight failed on CX53")
    {
        bail!(
            "a failing disk guard must fail preflight and record false for fallback routing:\n{disk_guard_output}"
        );
    }

    Ok(())
}

#[test]
fn ripr_docs_describe_unfiltered_ready_for_review_receipt_routing()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let docs = fs::read_to_string(root.join("docs/ci/ripr.md"))?;
    let posture = section_block(&docs, "## Current routing posture")
        .ok_or("docs/ci/ripr.md is missing the current routing posture section")?;
    let when_it_runs = section_block(&docs, "## When it runs")
        .ok_or("docs/ci/ripr.md is missing the When it runs section")?;
    let behavior = section_block(&docs, "## Behavior")
        .ok_or("docs/ci/ripr.md is missing the Behavior section")?;

    assert!(
        posture.contains("blocks PRs that introduce")
            && posture.contains("Repo-wide")
            && posture.contains("RIPR+ total zero remains a burn-down target"),
        "RIPR docs must describe the promoted new-gap blocking posture without final total-zero enforcement"
    );
    assert!(
        when_it_runs.contains("Every PR targeting `master` or `main`"),
        "RIPR docs must describe the workflow as an every-PR proof run"
    );
    assert!(
        when_it_runs.contains("No path filter is applied")
            && when_it_runs.contains("docs-only")
            && when_it_runs.contains("policy-only")
            && when_it_runs.contains("workflow-only"),
        "RIPR docs must make docs/policy/workflow-only PR coverage explicit"
    );
    assert!(
        when_it_runs.contains("ready_for_review"),
        "RIPR docs must say draft PRs run the workflow when they become ready"
    );
    assert!(
        behavior.contains("target/ripr/pr/")
            && behavior.contains("target/receipts/quality/ripr-plus.json")
            && behavior.contains("target/ripr/review/"),
        "RIPR docs must name diff-scoped, repo-wide, and review-guidance receipts"
    );
    for forbidden in ["Blocks merges", "quality-gate --mode enforce "] {
        assert!(
            !docs.contains(forbidden),
            "RIPR docs must not carry final quality-gate text `{forbidden}`"
        );
    }

    Ok(())
}

#[test]
fn ripr_docs_use_direct_local_proof_commands() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let docs = fs::read_to_string(root.join("docs/ci/ripr.md"))?;
    let block = fenced_block_after(&docs, "## Running locally")
        .ok_or("docs/ci/ripr.md is missing the Running locally command block")?;
    let commands = block.lines().filter(|line| !line.trim().is_empty()).collect::<Vec<_>>();
    assert!(!commands.is_empty(), "RIPR local proof block must list commands");
    for command in &commands {
        let direct = command.starts_with("cargo install ripr ")
            || command.starts_with("cargo xtask ")
            || *command == "ripr doctor";
        assert!(direct, "RIPR local proof command must be directly executable: {command}");
        assert_ne!(
            command.split_whitespace().next(),
            Some("rtk"),
            "RIPR local proof command must not use the retired RTK wrapper: {command}"
        );
        assert!(
            !command.contains("quality-gate --mode enforce "),
            "RIPR local proof commands must not run final enforcement before burn-down: {command}"
        );
    }
    for required in [
        "cargo xtask ripr-pr --base origin/HEAD --head HEAD",
        "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json",
        "cargo xtask ripr-review-comments --base origin/HEAD --head HEAD",
        "cargo xtask quality-gate --mode enforce-new-ripr",
        "cargo xtask ripr-pr --base origin/HEAD --head HEAD --check",
        "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check",
        "cargo xtask ripr-review-comments --base origin/HEAD --head HEAD --check",
        "cargo xtask quality-gate --mode enforce-new-ripr",
    ] {
        assert!(
            commands.iter().any(|command| command.contains(required)),
            "RIPR local proof block must include `{required}`"
        );
    }

    Ok(())
}

#[test]
fn ripr_infra_retry_is_bounded_and_gate_classified() -> Result<()> {
    let root = project_root()?;
    let gate = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let retry = fs::read_to_string(root.join(".github/workflows/ripr-infra-retry.yml"))?;

    // #6807 slice 2: the gate remains the single eviction classifier and
    // hands its verdict to the retry workflow strictly as data.
    let evaluate_step = workflow_step(&gate, "Evaluate routed result")
        .ok_or_else(|| anyhow!("missing evaluate step"))?;
    assert!(
        evaluate_step.contains("classification=infra-no-proof")
            && evaluate_step.contains("> ripr-gate-classification.env")
            && evaluate_step.contains("head_sha=${GITHUB_SHA}")
            && evaluate_step.contains("run_id=${GITHUB_RUN_ID}"),
        "the ripr gate must emit its infra-no-proof verdict as a data file for the retry workflow"
    );
    let upload_step = workflow_step(&gate, "Upload gate classification")
        .ok_or_else(|| anyhow!("missing classification upload"))?;
    assert!(
        upload_step.contains("if: failure()")
            && upload_step.contains("name: ripr-gate-classification")
            && upload_step.contains("if-no-files-found: ignore"),
        "genuine ripr failures produce no classification file, so the upload must tolerate its absence"
    );

    // The retry workflow fires on completed failing ripr runs only.
    assert!(
        retry.contains("workflow_run:")
            && retry.contains("workflows: [ripr]")
            && retry.contains("types: [completed]")
            && retry.contains("github.event.workflow_run.conclusion == 'failure'"),
        "ripr-infra-retry must trigger on completed failing ripr runs"
    );
    // Bound: exactly one automatic retry; attempt 2+ takes the manual path.
    assert!(
        retry.contains("[ \"${RUN_ATTEMPT}\" != \"1\" ]"),
        "ripr-infra-retry must bound the automatic retry to run attempt 1"
    );
    // The verdict is consumed strictly as data: exact-line grep, no source,
    // and the rerun target is the event-provided run id.
    assert!(
        retry.contains("grep -qx 'classification=infra-no-proof'"),
        "ripr-infra-retry must match the classification line exactly"
    );
    assert!(
        !retry.contains("actions/checkout"),
        "ripr-infra-retry runs with actions:write on the default branch and must never check out candidate code"
    );
    assert!(
        retry.contains("RUN_ID: ${{ github.event.workflow_run.id }}")
            && retry.contains("actions/runs/${RUN_ID}/rerun-failed-jobs"),
        "ripr-infra-retry must rerun failed jobs of the event run id, not an artifact-provided id"
    );
    // Artifact/run coherence is proven by the recorded run id, not by head
    // SHA: for pull_request runs the gate's GITHUB_SHA is the evaluated
    // refs/pull/<n>/merge commit while workflow_run.head_sha is the PR branch
    // tip, so a head comparison would skip every genuine PR eviction.
    assert!(
        retry.contains("[ \"${gate_run_id}\" != \"${RUN_ID}\" ]"),
        "ripr-infra-retry must verify the classification run id matches the event run"
    );
    assert_eq!(
        retry_gh_token_binding()?,
        "${{ github.token }}",
        "ripr-infra-retry must bind GH_TOKEN to the production workflow token"
    );
    assert!(
        !retry.contains("[ \"${gate_head}\" != \"${HEAD_SHA}\" ]"),
        "ripr-infra-retry must not gate the retry on a head-SHA comparison (merge ref vs branch tip)"
    );

    let (missing, missing_output, missing_posted) = run_retry_case("missing", "1")?;
    if !missing.status.success()
        || !missing_output.contains("no ripr-gate-classification artifact")
        || missing_posted
    {
        bail!(
            "missing classification artifact must skip retry without arming it:\n{missing_output}"
        );
    }
    let (download_failure, download_failure_output, download_failure_posted) =
        run_retry_case("download-failure", "1")?;
    if download_failure.status.success()
        || download_failure_posted
        || !download_failure_output.contains("download_called=true")
    {
        bail!(
            "classification artifact download failure must remain an explicit failed consumer path:\n{download_failure_output}"
        );
    }
    let (corrupt, corrupt_output, corrupt_posted) = run_retry_case("corrupt", "1")?;
    if corrupt.status.success()
        || corrupt_posted
        || !corrupt_output.contains("download_called=true")
    {
        bail!("a corrupt downloaded ZIP must fail before retrying:\n{corrupt_output}");
    }
    let (missing_file, missing_file_output, missing_file_posted) =
        run_retry_case("missing-file", "1")?;
    if !missing_file.status.success()
        || !missing_file_output.contains("did not contain ripr-gate-classification.env")
        || !missing_file_output.contains("download_called=true")
        || missing_file_posted
    {
        bail!("artifact without its classification file must skip retry:\n{missing_file_output}");
    }
    let (malformed, malformed_output, malformed_posted) = run_retry_case("malformed", "1")?;
    if !malformed.status.success()
        || !malformed_output
            .contains("classification run id (invalid) does not match event run 4242")
        || !malformed_output.contains("download_called=true")
        || malformed_posted
    {
        bail!("malformed classification data must fail closed before retry:\n{malformed_output}");
    }
    let (valid, valid_output, valid_posted) = run_retry_case("valid", "1")?;
    if !valid.status.success()
        || !valid_output.contains("re-queued failed jobs of run 4242")
        || !valid_output.contains("download_called=true")
        || !valid_posted
    {
        bail!("valid classification data must reach the bounded rerun API:\n{valid_output}");
    }
    for expected_call in [
        "GET repos/EffortlessMetrics/perl-lsp-swarm/actions/runs/4242/artifacts?per_page=100",
        "GET repos/EffortlessMetrics/perl-lsp-swarm/actions/artifacts/99001/zip",
        "GET repos/EffortlessMetrics/perl-lsp-swarm/actions/runs/4242",
        "POST repos/EffortlessMetrics/perl-lsp-swarm/actions/runs/4242/rerun-failed-jobs",
    ] {
        if !valid_output.contains(expected_call) {
            bail!(
                "valid retry fixture did not replace and exercise production API call `{expected_call}`:\n{valid_output}"
            );
        }
    }

    let (hardcoded_id, hardcoded_id_output, hardcoded_id_posted) =
        run_retry_case_with("valid", "1", "99002", Some("retry-token"), "retry-token", true)?;
    if hardcoded_id.status.success()
        || hardcoded_id_posted
        || !hardcoded_id_output.contains("unexpected gh API URL")
    {
        bail!(
            "a hardcoded fixture artifact ID must fail against the exact production URL binding:\n{hardcoded_id_output}"
        );
    }
    let (arbitrary_token, arbitrary_token_output, arbitrary_token_posted) =
        run_retry_case_with("valid", "1", "99001", Some("arbitrary-token"), "retry-token", false)?;
    if arbitrary_token.status.success()
        || arbitrary_token_posted
        || arbitrary_token_output.contains("re-queued failed jobs")
    {
        bail!(
            "a nonempty arbitrary token must not satisfy the production token binding:\n{arbitrary_token_output}"
        );
    }
    let (exhausted, exhausted_output, exhausted_posted) = run_retry_case("valid", "2")?;
    if !exhausted.status.success()
        || !exhausted_output.contains("RIPR_GATE_VERDICT=not-proven-infra-retry-exhausted")
        || exhausted_posted
    {
        bail!("attempt two must take the loud manual NOT_PROVEN path:\n{exhausted_output}");
    }

    Ok(())
}

fn fenced_block_after<'a>(content: &'a str, heading: &str) -> Option<&'a str> {
    let start = content.find(heading)?;
    let rest = &content[start..];
    let fence_start = rest.find("```bash")? + "```bash".len();
    let after_start = &rest[fence_start..];
    let body_start = after_start.strip_prefix('\n').unwrap_or(after_start);
    let fence_end = body_start.find("```")?;
    Some(&body_start[..fence_end])
}

fn section_block<'a>(content: &'a str, heading: &str) -> Option<&'a str> {
    let start = content.find(heading)?;
    let rest = &content[start..];
    let next = rest
        .lines()
        .skip(1)
        .scan(heading.len() + 1, |offset, line| {
            let current = *offset;
            *offset += line.len() + 1;
            Some((current, line))
        })
        .find_map(
            |(offset, line)| {
                if line.starts_with("## ") && line != heading { Some(offset) } else { None }
            },
        )
        .unwrap_or(rest.len());
    Some(&rest[..next])
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
fn ripr_infra_classifier_is_shared_tested_and_boundary_documented()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root()?;
    let gate = fs::read_to_string(root.join(".github/workflows/ripr.yml"))?;
    let retry = fs::read_to_string(root.join(".github/workflows/ripr-infra-retry.yml"))?;
    let classifier = fs::read_to_string(root.join("scripts/ci/classify-ripr-lane-termination"))?;
    let self_test =
        fs::read_to_string(root.join("scripts/tests/test-classify-ripr-lane-termination.sh"))?;
    let whitelist = fs::read_to_string(root.join("policy/ci-lane-whitelist.toml"))?;

    // #12563 complement to #12771: the gate must run the ONE classifier from
    // the tested script instead of an inline grep twin, and must echo its
    // machine-checkable verdict into the run log for auditability.
    assert!(
        gate.contains("bash scripts/ci/classify-ripr-lane-termination"),
        "gate must classify lane termination through the shared tested script"
    );
    assert!(
        !gate.contains("eviction_matches=$(grep -cE"),
        "inline grep classification twins the tested classifier and must not return"
    );
    for verdict in [
        "RIPR_GATE_VERDICT=infra-no-proof",
        "RIPR_GATE_VERDICT=ripr-failure",
        "RIPR_GATE_VERDICT=cancelled-no-verdict",
        "RIPR_GATE_VERDICT=neutral-router-skipped",
        "RIPR_GATE_VERDICT=router-not-success",
        "RIPR_GATE_VERDICT=success",
    ] {
        assert!(
            gate.contains(verdict),
            "every gate outcome must emit a distinctive machine-checkable verdict token"
        );
    }
    assert!(
        gate.contains("RIPR_GATE_DECISION boundary="),
        "each infra-no-proof application must document its decision boundary in the run log"
    );

    // Classifier boundary: precedence between genuine reds and teardown
    // evidence is encoded in code order, not prose.
    let gap_rule =
        classifier.find("gap_hits").ok_or("classifier must evaluate genuine gap receipts first")?;
    let infra_class = classifier
        .find("\"infra-no-proof\"")
        .ok_or("classifier must assign the infra-no-proof class")?;
    assert!(
        gap_rule < infra_class,
        "genuine gap receipt evaluation must precede any infra classification"
    );
    for marker in [
        "The runner has received a shutdown signal",
        "Process completed with exit code 143.",
        "The operation was canceled",
        "quality gate failed; see receipt",
    ] {
        assert!(
            classifier.contains(marker),
            "classifier must pin its exact evidence markers: {marker}"
        );
    }

    // Responder: the exhausted retry bound surfaces NOT_PROVEN loudly rather
    // than as a silent notice.
    assert!(
        retry.contains("RIPR_GATE_VERDICT=not-proven-infra-retry-exhausted"),
        "attempt >= 2 must surface NOT_PROVEN with a machine-checkable verdict token"
    );
    assert!(
        retry.contains("[ \"${RUN_ATTEMPT}\" != \"1\" ]"),
        "the single automatic retry stays bounded to attempt 1"
    );

    // The fixture suite pins the discriminator: real failure with teardown
    // noise present still classifies ripr-failure.
    assert!(
        self_test.contains("DISCRIMINATOR: genuine gap receipt outranks later teardown marker"),
        "self-test must prove real failures are never classified infra"
    );
    assert!(
        self_test.contains("empty log fails closed to ripr-failure"),
        "self-test must prove absent evidence fails closed"
    );

    // Lane hygiene: the privileged responder must carry a whitelist entry.
    assert!(
        whitelist.contains("workflow = \".github/workflows/ripr-infra-retry.yml\""),
        "ripr-infra-retry must be governed by a ci-lane-whitelist entry"
    );

    Ok(())
}

#[test]
fn ripr_gate_retrieval_reaches_classifier_and_failed_fetch_fails_closed() -> Result<()> {
    let evicted_log = concat!(
        "##[error]The runner has received a shutdown signal.\n",
        "##[error]The operation was canceled.\n"
    );
    let (success, success_output, classification) =
        run_gate_with_fake_gh(Some(evicted_log), 0, 0, GateRoute::github_failure())?;
    if success.status.success() {
        bail!("a classified lane failure must keep the gate red");
    }
    if !success_output.contains("classification=infra-no-proof")
        || !success_output.contains("RIPR_GATE_VERDICT=infra-no-proof")
    {
        bail!(
            "successful log retrieval must reach the shared classifier and gate verdict:\n{success_output}"
        );
    }
    let classification =
        classification.ok_or_else(|| anyhow!("infra classification artifact is missing"))?;
    if !classification.contains("classification=infra-no-proof")
        || !classification.contains("run_id=4242")
    {
        bail!(
            "infra classification artifact must be written by the actual gate block:\n{classification}"
        );
    }

    let genuine_failure_log = concat!(
        "quality gate failed; see receipt target/receipts/quality/quality-gate-ripr.json\n",
        "##[error]The runner has received a shutdown signal.\n"
    );
    let (genuine, genuine_output, genuine_classification) =
        run_gate_with_fake_gh(Some(genuine_failure_log), 0, 0, GateRoute::github_failure())?;
    if genuine.status.success()
        || !genuine_output.contains("classification=ripr-failure")
        || !genuine_output.contains("RIPR_GATE_VERDICT=ripr-failure")
    {
        bail!(
            "a retrieved genuine gap must remain fail-closed even with teardown noise:\n{genuine_output}"
        );
    }
    if genuine_classification.is_some() {
        bail!(
            "a genuine ripr failure must not create an infra retry artifact:\n{genuine_classification:?}"
        );
    }

    let (failed, failed_output, no_classification) =
        run_gate_with_fake_gh(None, 0, 0, GateRoute::github_failure())?;
    if failed.status.success() {
        bail!("an unretrievable lane log must keep the gate red");
    }
    if !failed_output.contains("classification=ripr-failure")
        || !failed_output.contains("was not retrievable after 5 attempts")
        || !failed_output.contains("RIPR_GATE_VERDICT=ripr-failure")
        || failed_output.contains("verdict=infra-no-proof")
    {
        bail!(
            "failed retrieval must emit an explicit fail-closed classification and warning:\n{failed_output}"
        );
    }
    if no_classification.is_some() {
        bail!("failed retrieval must not create an infra retry artifact:\n{no_classification:?}");
    }
    if !failed_output.contains("lookup=Some(\"1\")\nfetch=Some(\"5\")") {
        bail!("log retrieval retry must be bounded at five attempts:\n{failed_output}");
    }

    let (retried, retried_output, retried_classification) =
        run_gate_with_fake_gh(Some(evicted_log), 1, 1, GateRoute::github_failure())?;
    if retried.status.success()
        || !retried_output.contains("classification=infra-no-proof")
        || !retried_output.contains("lookup=Some(\"2\")\nfetch=Some(\"2\")")
    {
        bail!(
            "transient lookup and log-fetch failures must recover before classification:\n{retried_output}"
        );
    }
    if !retried_classification
        .ok_or_else(|| anyhow!("retried classification artifact is missing"))?
        .contains("classification=infra-no-proof")
    {
        bail!("retried successful evidence must produce an infra classification artifact");
    }

    let partial_teardown = "##[error]The runner has received a shutdown signal.\n";
    let genuine_after_partial =
        "quality gate failed; see receipt target/receipts/quality/quality-gate-ripr.json\n";
    let (reused, reused_output, reused_classification) = run_gate_with_fake_gh_logs(
        Some(genuine_after_partial),
        partial_teardown,
        0,
        1,
        GateRoute::github_failure(),
        GhApiControl::Valid,
        Some("gate-token"),
    )?;
    if reused.status.success()
        || !reused_output.contains("classification=ripr-failure")
        || !reused_output.contains("gap_receipt_matches=1")
        || !reused_output.contains("shutdown_signal_matches=0")
        || !reused_output.contains("log_lines_scanned=1")
        || !reused_output.contains("fetch=Some(\"2\")")
        || !reused_output.contains("fetch_urls=Some(\"97001\\n97001\\n\")")
        || reused_classification.is_some()
    {
        bail!(
            "a failed partial teardown fetch followed by a distinct genuine-gap log must truncate and reclassify the buffer:\n{reused_output}"
        );
    }

    for (route_name, route) in [
        (
            "cx53",
            GateRoute {
                router_target: "cx53",
                cx53_result: "failure",
                cx43_result: "skipped",
                github_result: "skipped",
                fallback_result: "skipped",
            },
        ),
        (
            "cx43",
            GateRoute {
                router_target: "cx43",
                cx53_result: "skipped",
                cx43_result: "failure",
                github_result: "skipped",
                fallback_result: "skipped",
            },
        ),
    ] {
        let expected_job_id = if route_name == "cx53" { "53001" } else { "43001" };
        let (self_hosted, output, artifact) =
            run_gate_with_fake_gh(Some(evicted_log), 0, 0, route)?;
        let artifact = artifact.ok_or_else(|| anyhow!("{route_name} classification is missing"))?;
        if self_hosted.status.success()
            || !output.contains("classification=infra-no-proof")
            || !output.contains("RIPR_GATE_VERDICT=infra-no-proof")
            || !output.contains("lookup=Some(\"1\")\nfetch=Some(\"1\")")
            || !artifact.contains(&format!("lane_name=ripr+ on {}", route_name.to_uppercase()))
            || !artifact.contains(&format!("lane_job_id={expected_job_id}"))
        {
            bail!(
                "{route_name} runner failure must select its job, classify its downloaded log, and publish the retry artifact:\n{output}\n{artifact}"
            );
        }
    }

    let (fallback_execution, fallback_execution_output) = run_fallback_path(false)?;
    if !fallback_execution.status.success()
        || !fallback_execution_output.contains("cargo xtask quality-gate")
    {
        bail!(
            "fallback success result must come from the executed checkout/evidence/quality-gate path:\n{fallback_execution_output}"
        );
    }
    let fallback_result = if fallback_execution.status.success() { "success" } else { "failure" };
    let fallback_route = GateRoute {
        router_target: "cx53",
        cx53_result: "failure",
        cx43_result: "skipped",
        github_result: "skipped",
        fallback_result,
    };
    let (fallback, fallback_output, fallback_artifact) =
        run_gate_with_fake_gh(None, 0, 0, fallback_route)?;
    if !fallback.status.success()
        || !fallback_output.contains("CX53 disk preflight failed")
        || !fallback_output.contains("lookup=Some(\"1\")\nfetch=Some(\"5\")")
        || fallback_artifact.is_some()
    {
        bail!(
            "successful disk-full fallback must preserve primary fail-closed retrieval and then take the fallback route without a retry artifact:\n{fallback_output}"
        );
    }

    let (fallback_failure_execution, fallback_failure_execution_output) = run_fallback_path(true)?;
    if fallback_failure_execution.status.success()
        || !fallback_failure_execution_output.contains("cargo xtask quality-gate")
    {
        bail!(
            "fallback failure result must come from the executed quality-gate path:\n{fallback_failure_execution_output}"
        );
    }
    let fallback_failure_result =
        if fallback_failure_execution.status.success() { "success" } else { "failure" };
    let fallback_failure_route = GateRoute {
        router_target: "cx53",
        cx53_result: "failure",
        cx43_result: "skipped",
        github_result: "skipped",
        fallback_result: fallback_failure_result,
    };
    let (fallback_failure, fallback_failure_output, fallback_failure_artifact) =
        run_gate_with_fake_gh(Some(evicted_log), 0, 0, fallback_failure_route)?;
    let fallback_failure_artifact = fallback_failure_artifact
        .ok_or_else(|| anyhow!("fallback failure classification artifact is missing"))?;
    if fallback_failure.status.success()
        || !fallback_failure_output.contains("RIPR_GATE_VERDICT=infra-no-proof")
        || !fallback_failure_output.contains("fetch_urls=Some(\"88001\\n\")")
        || !fallback_failure_artifact.contains("lane_name=ripr+ (Disk-Full Fallback)")
        || !fallback_failure_artifact.contains("lane_job_id=88001")
    {
        bail!(
            "fallback failure must select and classify the fallback job rather than a stale primary job:\n{fallback_failure_output}\n{fallback_failure_artifact}"
        );
    }

    for (name, control, token) in [
        ("wrong repository", GhApiControl::WrongRepository, Some("gate-token")),
        ("wrong run", GhApiControl::WrongRun, Some("gate-token")),
        ("wrong log job", GhApiControl::WrongLogJob, Some("gate-token")),
        ("missing token", GhApiControl::MissingToken, None),
    ] {
        let (negative, output, artifact) = run_gate_with_fake_gh_logs(
            Some(evicted_log),
            "##[error]The runner has received a shutdown signal.\n",
            0,
            0,
            GateRoute::github_failure(),
            control,
            token,
        )?;
        if negative.status.success() || artifact.is_some() {
            bail!(
                "{name} API/auth negative control must fail closed without retry artifact:\n{output}"
            );
        }
        if !output.contains("RIPR_GATE_VERDICT=ripr-failure") {
            bail!("{name} negative control did not reach the fail-closed gate verdict:\n{output}");
        }
    }

    Ok(())
}
