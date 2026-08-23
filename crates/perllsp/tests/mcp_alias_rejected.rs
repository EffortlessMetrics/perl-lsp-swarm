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
