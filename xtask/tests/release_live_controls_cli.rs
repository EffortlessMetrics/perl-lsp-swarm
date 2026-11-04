//! CLI integration tests for `cargo xtask release-live-controls` (#9403, #16073).
//!
//! The library-level coverage in `tests/release_live_controls.rs` exercises
//! `observe()` and `run()` through the API surface, but the subcommand
//! itself must resolve through the CLI as well. The pre-merge review of
//! #14689 caught the original wiring loss precisely because no test ran the
//! built binary; this file restores that discriminator so a future conflict
//! cannot unwire the subcommand silently.
//!
//! Tests never read the network: every invocation prepends a stub `gh` to
//! the child's `PATH` so the production `SystemCommands` surface can never
//! reach a real (possibly authenticated) GitHub API, and the observer fails
//! closed to `NOT_PROVEN` for every response the stub gives. The exit code
//! is fixed at `NOT_PROVEN_EXIT_CODE` so a shell caller can tell "the
//! observer ran" from "the observer was never invoked".

use assert_cmd::Command;
use color_eyre::eyre::{Result, eyre};

const NOT_PROVEN_EXIT_CODE: i32 = 3;

/// A directory whose `gh` always fails, to prepend to the child's `PATH`.
///
/// On Unix the stub is a shell script exiting 1. On Windows `Command::new`
/// resolves only `.exe`, so the stub is a copy of the console host: whatever
/// it does with `api --method GET …` arguments, it cannot reach GitHub, and
/// the observer reports every non-JSON outcome as `NOT_PROVEN` rather than
/// guessing. Either way the child never inherits an authenticated `gh`.
fn failing_gh_stub_dir() -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;
        let script = dir.path().join("gh");
        let mut file = std::fs::File::create(&script)?;
        file.write_all(
            b"#!/bin/sh\necho 'stub gh: CLI tests never read the network' >&2\nexit 1\n",
        )?;
        drop(file);
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(windows)]
    {
        let host = std::env::var_os("COMSPEC")
            .ok_or_else(|| eyre!("COMSPEC is not set; cannot stage a stub gh.exe"))?;
        std::fs::copy(host, dir.path().join("gh.exe"))?;
    }
    Ok(dir)
}

/// The real `xtask` binary, isolated from any authenticated host `gh`.
///
/// The stub directory is prepended to `PATH`, and token variables are
/// removed so a stub that did reach the network still could not authenticate.
/// The caller must keep the returned `TempDir` alive while the command runs.
fn isolated_xtask_command() -> Result<(Command, tempfile::TempDir)> {
    let stub_dir = failing_gh_stub_dir()?;
    let path = std::env::join_paths(
        std::iter::once(stub_dir.path().to_path_buf()).chain(
            std::env::var_os("PATH")
                .map(|unparsed| std::env::split_paths(&unparsed).collect::<Vec<_>>())
                .into_iter()
                .flatten(),
        ),
    )?;
    let mut command = Command::cargo_bin("xtask")?;
    command.env_remove("GH_TOKEN").env_remove("GITHUB_TOKEN").env("PATH", path);
    Ok((command, stub_dir))
}

#[test]
fn top_level_help_lists_release_live_controls() -> Result<()> {
    let output = Command::cargo_bin("xtask")?.arg("--help").output()?;
    if !output.status.success() {
        return Err(eyre!("xtask --help should exit 0"));
    }
    let stdout = String::from_utf8(output.stdout)?;
    if !stdout.contains("release-live-controls") {
        return Err(eyre!("top-level help should mention release-live-controls; got: {stdout}"));
    }
    Ok(())
}

#[test]
fn subcommand_help_describes_required_flags() -> Result<()> {
    let output = Command::cargo_bin("xtask")?.args(["release-live-controls", "--help"]).output()?;
    if !output.status.success() {
        return Err(eyre!("subcommand --help should exit 0"));
    }
    let stdout = String::from_utf8(output.stdout)?;
    for flag in ["--repo-root", "--repository", "--branch", "--out", "--json"] {
        if !stdout.contains(flag) {
            return Err(eyre!("subcommand --help should mention {flag}; got: {stdout}"));
        }
    }
    Ok(())
}

#[test]
fn subcommand_resolves_and_exits_not_proven_without_credentials() -> Result<()> {
    // The stubbed `gh` fails every read, so the observer must reach its
    // fail-closed boundary and report NOT_PROVEN — not be missing entirely
    // from the CLI dispatch, and not touch a real authenticated `gh`.
    let (mut command, _stub_dir) = isolated_xtask_command()?;
    let output =
        command.args(["release-live-controls", "--repository", "octocat/Hello-World"]).output()?;
    if output.status.code() != Some(NOT_PROVEN_EXIT_CODE) {
        return Err(eyre!(
            "release-live-controls without credentials must exit {NOT_PROVEN_EXIT_CODE}; got {:?}\nstdout: {}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(())
}

#[test]
fn subcommand_json_output_names_configured_repository() -> Result<()> {
    // The JSON form must include the explicit override the caller passed on
    // the command line — repository and branch both — so the discriminator
    // proves the CLI argument parser reached `ObserveOptions` instead of
    // being short-circuited or dropping a non-default branch.
    let (mut command, _stub_dir) = isolated_xtask_command()?;
    let output = command
        .args([
            "release-live-controls",
            "--repository",
            "octocat/Hello-World",
            "--branch",
            "release-candidate",
            "--json",
        ])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("octocat/Hello-World") {
        return Err(eyre!("--repository override must reach the receipt; got: {stdout}"));
    }
    if !stdout.contains("\"branch\": \"release-candidate\"") {
        return Err(eyre!(
            "a non-default --branch override must reach the receipt verbatim; got: {stdout}"
        ));
    }
    if !stdout.contains("\"verdict\"") {
        return Err(eyre!("--json output must include the verdict field; got: {stdout}"));
    }
    Ok(())
}

#[test]
fn subcommand_repo_root_dot_resolves_workspace_identity() -> Result<()> {
    // The default `--repo-root .` must resolve to the workspace root and
    // read `policy/product-identity.toml`, not the shell's cwd. The child is
    // launched from a bare temporary directory that carries no policy file,
    // so a regression that forwards `.` unresolved would find nothing there
    // and fail, rather than passing by accident of the test runner's cwd.
    // The committed identity pins
    // `public_repository = "EffortlessMetrics/perl-lsp"` and
    // `development_repository = "EffortlessMetrics/perl-lsp-swarm"`, so a
    // successful `.` resolution surfaces `EffortlessMetrics/perl-lsp` in the
    // receipt.
    let (mut command, _stub_dir) = isolated_xtask_command()?;
    let bare_cwd = tempfile::tempdir()?;
    let output =
        command.current_dir(bare_cwd.path()).args(["release-live-controls", "--json"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("EffortlessMetrics/perl-lsp") {
        return Err(eyre!(
            "default --repo-root . must surface the committed product-identity repository; got: {stdout}"
        ));
    }
    Ok(())
}
