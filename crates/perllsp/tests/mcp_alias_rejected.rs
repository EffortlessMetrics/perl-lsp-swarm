#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use std::process::Command;

fn run_perllsp(args: &[&str]) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_perllsp")).args(args).output()?;
    Ok(output)
}

#[test]
fn retired_mcp_alias_exits_without_starting_lsp() -> Result<(), Box<dyn std::error::Error>> {
    let output = run_perllsp(&["--mcp"])?;

    if output.status.success() {
        return Err("--mcp unexpectedly succeeded".into());
    }
    if !output.stdout.is_empty() {
        return Err(format!("protocol stdout was not empty: {:?}", output.stdout).into());
    }

    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("`--mcp` is not an LSP transport alias."), "{stderr}");
    assert!(stderr.contains("Use `perllsp --stdio` for LSP."), "{stderr}");
    assert!(
        stderr.contains("Use `perllsp mcp --stdio` only when the native MCP adapter is available."),
        "{stderr}"
    );
    assert!(!stderr.contains("Content-Length"), "LSP framing leaked into rejection: {stderr}");
    Ok(())
}

#[test]
fn canonical_mcp_subcommand_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    let output = run_perllsp(&["mcp", "--stdio"])?;

    if output.status.success() {
        return Err("reserved MCP command unexpectedly succeeded".into());
    }
    if !output.stdout.is_empty() {
        return Err(format!("protocol stdout was not empty: {:?}", output.stdout).into());
    }

    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("is reserved for the native MCP adapter"),
        "missing native-adapter boundary: {stderr}"
    );
    assert!(stderr.contains("No MCP server was started."), "missing fail-closed result: {stderr}");
    assert!(!stderr.contains("Content-Length"), "LSP framing leaked into rejection: {stderr}");
    Ok(())
}

#[test]
fn reserved_mcp_help_is_protocol_clean() -> Result<(), Box<dyn std::error::Error>> {
    let output = run_perllsp(&["mcp", "--help"])?;

    if !output.status.success() {
        return Err(format!("MCP help failed with status {:?}", output.status.code()).into());
    }
    if !output.stderr.is_empty() {
        return Err(format!("MCP help wrote stderr: {:?}", output.stderr).into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("Usage: perllsp mcp --stdio [--workspace <ROOT>]"), "{stdout}");
    assert!(stdout.contains("native MCP adapter is not available"), "{stdout}");
    assert!(stdout.contains("never starts the LSP runtime"), "{stdout}");
    assert!(!stdout.contains("Content-Length"), "protocol framing leaked into help: {stdout}");
    Ok(())
}

#[test]
fn bare_mcp_subcommand_is_rejected_at_process_level() -> Result<(), Box<dyn std::error::Error>> {
    let output = run_perllsp(&["mcp"])?;

    if output.status.success() {
        return Err("bare `perllsp mcp` unexpectedly succeeded".into());
    }
    if !output.stdout.is_empty() {
        return Err(format!("protocol stdout was not empty: {:?}", output.stdout).into());
    }

    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("error: `perllsp mcp` requires the explicit `--stdio` transport"),
        "missing exact rejection reason: {stderr}"
    );
    assert!(
        stderr.contains("Usage: perllsp mcp --stdio [--workspace <ROOT>]"),
        "missing usage after rejection: {stderr}"
    );
    assert!(!stderr.contains("Content-Length"), "LSP framing leaked into rejection: {stderr}");
    Ok(())
}
