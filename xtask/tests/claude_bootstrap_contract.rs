use anyhow::{ensure, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below the repository root")
}

fn installer_source() -> Result<String> {
    Ok(fs::read_to_string(repository_root()?.join("scripts/install.sh"))?)
}

#[test]
fn posix_bootstrap_is_one_option_over_the_canonical_installer() -> Result<()> {
    let source = installer_source()?;
    ensure!(source.contains("--with-claude"), "installer does not expose --with-claude");
    ensure!(
        source.contains("\"$_bin\" setup claude"),
        "bootstrap must invoke the Rust-owned `perllsp setup claude` surface"
    );
    ensure!(
        !source.contains("claude plugin marketplace") && !source.contains("claude plugin install"),
        "shell installer must not grow a second Claude marketplace/plugin state machine"
    );
    Ok(())
}

#[test]
fn bootstrap_runs_only_after_binary_install_and_verification() -> Result<()> {
    let source = installer_source()?;
    let install = source.find("    install_binaries\n").context("install_binaries call missing")?;
    let verify = source.find("    verify_install\n").context("verify_install call missing")?;
    let path = source.find("    check_path\n").context("check_path call missing")?;
    let claude = source
        .find("    configure_claude || _combined_exit=$?\n")
        .context("Claude composition call missing")?;

    ensure!(install < verify, "verification moved before binary installation");
    ensure!(verify < path, "PATH observation moved before binary verification");
    ensure!(
        path < claude,
        "Claude reconciliation must run after binary install/verification/PATH observation"
    );
    Ok(())
}

#[test]
fn bootstrap_does_not_manufacture_fresh_process_path_proof() -> Result<()> {
    let source = installer_source()?;
    let start = source.find("configure_claude() {").context("configure_claude function missing")?;
    let end = source[start..]
        .find("# ── Main")
        .map(|offset| start + offset)
        .context("configure_claude function boundary missing")?;
    let function = &source[start..end];

    ensure!(
        !function.contains("export PATH=") && !function.contains("PATH=\""),
        "Claude bootstrap must not inject PATH and call that fresh-process proof"
    );
    ensure!(
        function.contains("absolute path") && function.contains("#7832/#7746"),
        "absolute-path bootstrap invocation must retain the explicit PATH-proof boundary"
    );
    Ok(())
}

#[test]
fn claude_failure_preserves_the_installed_binary_and_returns_stage_result() -> Result<()> {
    let source = installer_source()?;
    let start = source.find("configure_claude() {").context("configure_claude function missing")?;
    let end = source[start..]
        .find("# ── Main")
        .map(|offset| start + offset)
        .context("configure_claude function boundary missing")?;
    let function = &source[start..end];

    ensure!(function.contains("CLAUDE_SETUP_RESULT=\"complete\""));
    ensure!(function.contains("CLAUDE_SETUP_RESULT=\"action_required\""));
    ensure!(function.contains("CLAUDE_SETUP_RESULT=\"failed\""));
    ensure!(
        function.contains("the binary has been preserved"),
        "partial Claude setup must explicitly preserve a healthy binary"
    );
    ensure!(
        !function.contains("rm \"$INSTALL_DIR/$BIN_NAME\"")
            && !function.contains("rm -f \"$INSTALL_DIR/$BIN_NAME\""),
        "Claude-stage failure must not roll back the verified binary"
    );
    ensure!(
        function.contains("return \"$_status\""),
        "combined result must retain Claude-stage exit status"
    );
    Ok(())
}
