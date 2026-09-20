//! CLI integration tests for `cargo xtask release-live-controls` (#9403, #16073).
//!
//! The library-level coverage in `tests/release_live_controls.rs` exercises
//! `observe()` and `run()` through the API surface, but the subcommand
//! itself must resolve through the CLI as well. The pre-merge review of
//! #14689 caught the original wiring loss precisely because no test ran the
//! built binary; this file restores that discriminator so a future conflict
//! cannot unwire the subcommand silently.
//!
//! Tests never read the network: the observer fails closed to `NOT_PROVEN`
//! whenever GitHub credentials are missing, and the exit code is fixed at
//! `NOT_PROVEN_EXIT_CODE` so a shell caller can tell "the observer ran" from
//! "the observer was never invoked".

use assert_cmd::Command;
use color_eyre::eyre::Result;

const NOT_PROVEN_EXIT_CODE: i32 = 3;

#[test]
fn top_level_help_lists_release_live_controls() -> Result<()> {
    let output = Command::cargo_bin("xtask")?.arg("--help").output()?;
    assert!(output.status.success(), "xtask --help should exit 0");
    let stdout = String::from_utf8(output.stdout)?;
    assert!(
        stdout.contains("release-live-controls"),
        "top-level help should mention release-live-controls; got: {stdout}"
    );
    Ok(())
}

#[test]
fn subcommand_help_describes_required_flags() -> Result<()> {
    let output = Command::cargo_bin("xtask")?.args(["release-live-controls", "--help"]).output()?;
    assert!(output.status.success(), "subcommand --help should exit 0");
    let stdout = String::from_utf8(output.stdout)?;
    for flag in ["--repo-root", "--repository", "--branch", "--out", "--json"] {
        assert!(stdout.contains(flag), "subcommand --help should mention {flag}; got: {stdout}");
    }
    Ok(())
}

#[test]
fn subcommand_resolves_and_exits_not_proven_without_credentials() -> Result<()> {
    // The workspace does not expose `GH_TOKEN` to xtask test runs by default,
    // so the observer must reach the network boundary and report NOT_PROVEN,
    // not be missing entirely from the CLI dispatch.
    let output = Command::cargo_bin("xtask")?
        .args(["release-live-controls", "--repository", "octocat/Hello-World"])
        .output()?;
    assert_eq!(
        output.status.code(),
        Some(NOT_PROVEN_EXIT_CODE),
        "release-live-controls without credentials must exit {NOT_PROVEN_EXIT_CODE}; \
         got {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    Ok(())
}

#[test]
fn subcommand_json_output_names_configured_repository() -> Result<()> {
    // The JSON form must include the explicit override the caller passed on
    // the command line; the discriminator that proves the CLI argument parser
    // reached `ObserveOptions.repositories` instead of being short-circuited.
    let output = Command::cargo_bin("xtask")?
        .args([
            "release-live-controls",
            "--repository",
            "octocat/Hello-World",
            "--branch",
            "main",
            "--json",
        ])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("octocat/Hello-World"),
        "--repository override must reach the receipt; got: {stdout}"
    );
    assert!(
        stdout.contains("\"verdict\""),
        "--json output must include the verdict field; got: {stdout}"
    );
    Ok(())
}

#[test]
fn subcommand_repo_root_dot_resolves_workspace_identity() -> Result<()> {
    // The default `--repo-root .` must resolve to the workspace root and
    // read `policy/product-identity.toml`, not the shell's cwd. The committed
    // identity pins `public_repository = "EffortlessMetrics/perl-lsp"` and
    // `development_repository = "EffortlessMetrics/perl-lsp-swarm"`, so a
    // successful `.` resolution surfaces `EffortlessMetrics/perl-lsp` in the
    // receipt.
    let output = Command::cargo_bin("xtask")?.args(["release-live-controls", "--json"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("EffortlessMetrics/perl-lsp"),
        "default --repo-root . must surface the committed product-identity repository; \
         got: {stdout}"
    );
    Ok(())
}
