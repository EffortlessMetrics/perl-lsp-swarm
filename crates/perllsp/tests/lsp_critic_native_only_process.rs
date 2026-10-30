//! Public-process regression for the native-only Critic boundary (#6969/#9072).
//!
//! A failing external analyzer can be hidden behind `analyzerUsed = "native"`.
//! Observe a real PATH trap's side effect, not only that response label. Keep
//! useful native unopened-file analysis as a positive control. This is Unix
//! Cargo-exact process proof, not installed-editor or complete migration proof.
#![cfg(unix)]
#![deny(clippy::map_err_ignore)]

#[path = "support/real_process.rs"]
mod real_process;

use anyhow::{Context, Result, ensure};
use real_process::RealProcessClient;
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;
use url::Url;

const SOURCE: &str = "my $c = '1';\neval $c;\nprint $c;\n";

struct CriticTrap {
    _directory: TempDir,
    marker: PathBuf,
    path_env: String,
}

impl CriticTrap {
    fn new(exit_code: i32) -> Result<Self> {
        let directory = tempfile::tempdir().context("create Critic PATH trap")?;
        let executable = directory.path().join("perlcritic");
        let marker = directory.path().join("invoked.log");
        // Embed the marker path: an external runtime may sanitize its child
        // environment, so an inherited marker variable would be a false-pass risk.
        let quoted_marker = format!(
            "'{}'",
            marker.to_str().context("trap marker path is not UTF-8")?.replace('\'', "'\\''")
        );
        // Write under a scratch name and rename into place: executing a
        // file another thread still holds open for writing (including via
        // an inherited fork descriptor) fails with ETXTBSY on Linux, and
        // these tests run in parallel in one process.
        let scratch = directory.path().join("perlcritic.write");
        fs::write(
            &scratch,
            format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> {quoted_marker}\nexit {exit_code}\n"),
        )
        .context("write executable Critic trap")?;
        fs::set_permissions(&scratch, fs::Permissions::from_mode(0o755))
            .context("make Critic trap executable")?;
        fs::rename(&scratch, &executable).context("publish executable Critic trap")?;

        // Prove the instrument really executes and records an invocation.
        // A missing interpreter, noexec mount or failed write is not a pass.
        let status = Command::new(&executable)
            .arg("--instrument-control")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("execute Critic trap positive control")?;
        ensure!(status.code() == Some(exit_code), "trap control exited {status}");
        ensure!(
            fs::read_to_string(&marker).context("read trap positive control")?
                == "--instrument-control\n",
            "trap did not record its positive-control invocation"
        );
        fs::remove_file(&marker).context("reset only the owned trap marker")?;

        // Preserve host discovery utilities while shadowing every ambient
        // perlcritic with the trap. This is a hostile-present PATH case, not
        // a claim that the host has no Perl installation.
        let inherited = std::env::var_os("PATH").context("host PATH is unavailable")?;
        let path_env = std::env::join_paths(
            std::iter::once(directory.path().to_path_buf())
                .chain(std::env::split_paths(&inherited)),
        )
        .context("construct trap-prefixed child PATH")?
        .into_string()
        .map_err(|path| anyhow::anyhow!("test child PATH is not UTF-8: {path:?}"))?;
        Ok(Self { _directory: directory, marker, path_env })
    }

    fn spawn(&self) -> Result<RealProcessClient> {
        RealProcessClient::spawn_exact_with_env(&[("PATH", self.path_env.as_str())])
    }

    fn assert_not_invoked(&self, engine: &str, response: &Value) -> Result<()> {
        match fs::read_to_string(&self.marker) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Ok(invocations) => anyhow::bail!(
                "critic.engine={engine} invoked external perlcritic: {invocations:?}; \
                 command response={response}"
            ),
            Err(error) => Err(error).context("inspect external Critic invocation marker"),
        }
    }
}

fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|uri| uri.to_string())
        .map_err(|()| anyhow::anyhow!("cannot make file URI for {}", path.display()))
}

fn request(client: &mut RealProcessClient, id: &str, method: &str, params: Value) -> Result<Value> {
    client.request(json!(id), method, params, Duration::from_secs(15))
}

fn successful_result(response: &Value) -> Result<&Value> {
    ensure!(response.get("error").is_none(), "unexpected JSON-RPC error: {response}");
    response.get("result").with_context(|| format!("missing result: {response}"))
}

fn run_unopened(engine: &str, exit_code: i32, require_native_success: bool) -> Result<()> {
    let trap = CriticTrap::new(exit_code)?;
    let workspace = tempfile::tempdir().context("create unopened Critic workspace")?;
    fs::write(
        workspace.path().join(".perl-lsp.toml"),
        format!(
            "[diagnostics]\nperlcritic = true\nperlcritic_severity = 1\n\n\
             [critic]\nengine = \"{engine}\"\nprofile = \"recommended\"\n"
        ),
    )
    .context("write project Critic settings")?;
    let target = workspace.path().join("unopened.pl");
    fs::write(&target, SOURCE).context("write unopened native-positive source")?;
    let root_uri = file_uri(workspace.path())?;
    let target_uri = file_uri(&target)?;
    let mut client = trap.spawn()?;
    let initialized = request(
        &mut client,
        "initialize",
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": "critic-fixture" }],
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] }
            }
        }),
    )?;
    ensure!(
        successful_result(&initialized)?.get("capabilities").is_some(),
        "initialize omitted capabilities: {initialized}"
    );
    client.notify("initialized", json!({}))?;

    // Deliberately never send didOpen: #17715's open-buffer interception must
    // not mask the residual provider path this regression is meant to reach.
    let response = request(
        &mut client,
        "critic-unopened",
        "workspace/executeCommand",
        json!({ "command": "perl.runCritic", "arguments": [target_uri] }),
    )?;

    let shutdown = request(&mut client, "shutdown", "shutdown", Value::Null)?;
    ensure!(successful_result(&shutdown)?.is_null(), "shutdown must return null: {shutdown}");
    client.notify("exit", Value::Null)?;
    let status = client.wait_for_exit(Duration::from_secs(15))?;
    ensure!(status.success(), "candidate exit={status}; stderr={}", client.stderr_tail());
    client.assert_transport_clean()?;

    // Inspect after the server is settled; a swallowed external failure must
    // fail this assertion even when the response reports a native fallback.
    trap.assert_not_invoked(engine, &response)?;
    if require_native_success {
        let result = successful_result(&response)?;
        ensure!(result.get("status") == Some(&json!("success")), "native run failed: {result}");
        ensure!(result.get("analyzerUsed") == Some(&json!("native")), "wrong analyzer: {result}");
        let violations = result
            .get("violations")
            .and_then(Value::as_array)
            .context("native success omitted violations")?;
        ensure!(
            violations.iter().any(|finding| {
                finding
                    .get("policy")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id.starts_with("native."))
            }),
            "native positive control produced no native finding: {result}"
        );
        ensure!(
            result.get("violationCount").and_then(Value::as_u64) == Some(violations.len() as u64),
            "native finding count disagrees with payload: {result}"
        );
    }
    Ok(())
}

// Separate tests keep a successful external invocation from preventing the
// failing-tool/fallback cases from running in the same regression target.
#[test]
fn native_unopened_analysis_ignores_successful_external_tool() -> Result<()> {
    run_unopened("native", 0, true)
}

#[test]
fn native_unopened_analysis_ignores_failing_external_tool() -> Result<()> {
    run_unopened("native", 73, true)
}

// These assert process containment only. Migration messaging, normalized-result
// parity and validated disk identity remain separate obligations; an arbitrary
// refusal is not proof of them.
#[test]
fn legacy_project_engine_cannot_invoke_successful_external_tool() -> Result<()> {
    run_unopened("legacy", 0, false)
}

#[test]
fn legacy_project_engine_cannot_hide_failed_external_tool_behind_native_fallback() -> Result<()> {
    run_unopened("legacy", 73, false)
}

#[test]
fn external_project_engine_cannot_invoke_successful_external_tool() -> Result<()> {
    run_unopened("external", 0, false)
}

#[test]
fn external_project_engine_cannot_hide_failed_external_tool_behind_native_fallback() -> Result<()> {
    run_unopened("external", 73, false)
}

#[test]
fn perlcritic_project_engine_cannot_invoke_successful_external_tool() -> Result<()> {
    run_unopened("perlcritic", 0, false)
}

#[test]
fn perlcritic_project_engine_cannot_hide_failed_external_tool_behind_native_fallback() -> Result<()>
{
    run_unopened("perlcritic", 73, false)
}
