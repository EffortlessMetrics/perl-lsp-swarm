//! Wave H Collapse: Workspace-Level Verification Tests
//!
//! Verifies build artifacts and workspace structure after collapse.
//! These tests are integration-style and verify the workspace as a whole.
//!
//! Run with: `cargo test -p perl-dap --test wave_h_workspace_verification`

use std::process::Command;

#[test]
fn test_perl_lsp_can_build_with_new_imports() -> Result<(), Box<dyn std::error::Error>> {
    // Verify that perl-lsp crate builds successfully with the new import paths
    // It should depend on perl_dap instead of perl_dap_platform

    let output = Command::new("cargo")
        .args(["build", "-p", "perl-lsp-rs", "--message-format=short"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "perl-lsp-rs build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(())
}

#[test]
fn test_rs_core_config_can_build_after_absorption() -> Result<(), Box<dyn std::error::Error>> {
    // Verify that the current home for the absorbed perl-lsp-config module
    // builds successfully with the new import paths.

    let output = Command::new("cargo")
        .args(["build", "-p", "perl-lsp-rs-core", "--message-format=short"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "perl-lsp-rs-core build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(())
}

#[test]
fn test_executable_binary_builds_successfully() -> Result<(), Box<dyn std::error::Error>> {
    // Verify that the perl-dap binary itself builds successfully
    // with the new module structure

    let output =
        Command::new("cargo").args(["build", "-p", "perl-dap", "--bin", "perl-dap"]).output()?;
    if !output.status.success() {
        return Err(format!(
            "perl-dap binary build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(())
}

#[test]
fn test_clippy_has_no_warnings_in_new_modules() -> Result<(), Box<dyn std::error::Error>> {
    // Verify that the new module code doesn't introduce clippy warnings.
    // This test MUST fail on clippy warnings — a silent-pass version that
    // only logs was identified as a false-confidence pattern (#954).
    //
    // Temporarily allows wildcard_imports warnings until Phase 2 of #2333
    // (crate-by-crate wildcard fix) reaches perl-dap's 19 use super::* sites.

    let output = Command::new("cargo")
        .args([
            "clippy",
            "-p",
            "perl-dap",
            "--lib",
            "--",
            "-D",
            "warnings",
            "-A",
            "clippy::wildcard_imports",
        ])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "clippy warnings detected in perl-dap lib (run: cargo clippy -p perl-dap --lib -- -D warnings):\n{stderr}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn test_formatting_is_correct() -> Result<(), Box<dyn std::error::Error>> {
    // Verify code formatting is consistent

    let output = Command::new("cargo").args(["fmt", "-p", "perl-dap", "--", "--check"]).output()?;
    if !output.status.success() {
        return Err(format!(
            "code formatting issues found:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}
