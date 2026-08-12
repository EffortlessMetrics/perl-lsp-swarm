//! Governed Changie tool resolution and dry-render execution.

use super::config::AQUA_CONFIG_PATH;
use color_eyre::eyre::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Output};

const DRY_RUN_VERSION: &str = "v0.0.0-precommit-render";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RenderOutcome {
    Passed,
    Rejected(Vec<String>),
}

#[derive(Debug, Clone, Copy)]
enum ChangieRunner {
    Aqua,
    Direct,
}

pub(super) fn render_with_changie(
    workspace: &Path,
    projects: &[String],
) -> Result<RenderOutcome> {
    let runner = select_changie_runner(workspace)?;
    let mut rejected = Vec::new();
    for project in projects {
        let output = changie_command(workspace, runner)
            .args([
                "batch",
                DRY_RUN_VERSION,
                "--project",
                project,
                "--dry-run",
                "--keep",
            ])
            .output()
            .with_context(|| format!("failed to run Changie for project `{project}`"))?;
        if !output.status.success() {
            if output.status.code().is_none() {
                return Err(color_eyre::eyre::eyre!(
                    "Changie for project `{project}` terminated without an exit code: {}",
                    command_diagnostic(&output)
                ));
            }
            rejected.push(format!(
                "project `{project}` exited {:?}: {}",
                output.status.code(),
                command_diagnostic(&output)
            ));
        }
    }

    if rejected.is_empty() {
        Ok(RenderOutcome::Passed)
    } else {
        Ok(RenderOutcome::Rejected(rejected))
    }
}

fn select_changie_runner(workspace: &Path) -> Result<ChangieRunner> {
    let aqua_probe = changie_command(workspace, ChangieRunner::Aqua)
        .arg("--version")
        .output();
    if aqua_probe
        .as_ref()
        .is_ok_and(|output| output.status.success())
    {
        return Ok(ChangieRunner::Aqua);
    }

    let direct_probe = changie_command(workspace, ChangieRunner::Direct)
        .arg("--version")
        .output();
    if direct_probe
        .as_ref()
        .is_ok_and(|output| output.status.success())
    {
        return Ok(ChangieRunner::Direct);
    }

    let aqua = probe_diagnostic("aqua", aqua_probe);
    let direct = probe_diagnostic("direct", direct_probe);
    bail!(
        "Changie is unavailable from both governed tool paths ({aqua}; {direct}); run \
         `bash scripts/tools/aqua-doctor.sh` or enter the Nix development shell"
    )
}

fn changie_command(workspace: &Path, runner: ChangieRunner) -> Command {
    let mut command = match runner {
        ChangieRunner::Aqua => {
            let mut command = Command::new("aqua");
            command.args(["exec", "--", "changie"]);
            command
        }
        ChangieRunner::Direct => Command::new("changie"),
    };
    command.current_dir(workspace);
    command.env("AQUA_CONFIG", workspace.join(AQUA_CONFIG_PATH));
    command.env("AQUA_DISABLE_LAZY_INSTALL", "true");
    for variable in [
        "AQUA_GLOBAL_CONFIG",
        "AQUA_DISABLE_POLICY",
        "AQUA_DISABLE_COSIGN",
        "AQUA_DISABLE_SLSA",
        "AQUA_DISABLE_GITHUB_ARTIFACT_ATTESTATION",
    ] {
        command.env_remove(variable);
    }
    command
}

fn probe_diagnostic(label: &str, result: std::io::Result<Output>) -> String {
    match result {
        Ok(output) => format!(
            "{label} probe exited {:?}: {}",
            output.status.code(),
            command_diagnostic(&output)
        ),
        Err(err) => format!("{label} probe could not start: {err}"),
    }
}

fn command_diagnostic(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stderr.trim().is_empty() {
        limit_output(stdout.as_ref())
    } else {
        limit_output(stderr.as_ref())
    }
}

fn limit_output(raw: &str) -> String {
    let mut chars = raw.trim().chars();
    let mut limited: String = chars.by_ref().take(2_000).collect();
    if chars.next().is_some() {
        limited.push('…');
    }
    if limited.is_empty() {
        "no diagnostic output".to_string()
    } else {
        limited
    }
}
